//! 自有 MySQL 快照同步。
//!
//! 核心约定：**本地 SQLite 永远是唯一主库**。云端只存一个整库快照 + 版本号，
//! 联网只是「上传/下载一个文件」，不做行级复制，因此我们不需要全局 ID、事件日志、
//! 墓碑或离线写队列。断网时所有功能照常可用。
//!
//! 换库走 `pending.db`：导入时不碰正在被 `Mutex<Connection>` 持有的库文件，
//! 由 `db::init` 在下次启动、打开连接之前完成替换。

use crate::error::{AppError, AppResult};
use mysql::prelude::Queryable;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CREDENTIALS_FILE: &str = ".sync-credentials.json";
const STATE_FILE: &str = ".sync-state.json";
const PENDING_FILE: &str = "pending.db";
const PENDING_STATE_FILE: &str = ".sync-pending-state.json";
const LAST_ERROR_FILE: &str = ".sync-last-error.txt";
const META_KEY: &str = "meta";
/// 参与内容哈希的业务表。`settings` 刻意排除：它只存界面偏好（字号 / 遮罩秒数），
/// 两台机器字号不同不该被算成「本机内容有改动」，否则每次拉取都会弹覆盖确认。
/// 快照本身仍然把 settings 表一起带走，只是它不参与差异判定。
/// `ai_cards`（迁移 0006 的讲解卡缓存）必须在这里：只生成/重新生成了卡片、词没变时，
/// 哈希也得变，否则推送会被判成「内容没变」而跳过上传，另一台机器永远拿不到缓存卡。
const TABLES: [&str; 5] = ["groups", "words", "sessions", "session_items", "ai_cards"];

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub fn credentials_path(dir: &Path) -> PathBuf {
    dir.join(CREDENTIALS_FILE)
}
pub fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}
pub fn pending_path(dir: &Path) -> PathBuf {
    dir.join(PENDING_FILE)
}
pub fn pending_state_path(dir: &Path) -> PathBuf {
    dir.join(PENDING_STATE_FILE)
}
pub fn last_error_path(dir: &Path) -> PathBuf {
    dir.join(LAST_ERROR_FILE)
}

// ---------------------------------------------------------------- 配置与凭据

/// 可以安全回传给前端的同步配置。**绝不包含密码**。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncConfigDto {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub device_name: String,
}

impl Default for SyncConfigDto {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 3306,
            database: String::new(),
            user: String::new(),
            device_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    config: SyncConfigDto,
    password: String,
}

/// 私密文件：Unix 下强制 600。密码不进 `settings` 表，也永不回传前端。
/// 同目录「临时文件 → fsync → rename」原子落盘：状态 / 凭据 / 待提交状态都是小 JSON，
/// 直接 `File::create` 时若中途崩溃会留下半截 JSON，正好掉进「状态文件损坏」的坑。
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = PathBuf::from(format!("{}.tmp-{}", path.display(), rand::random::<u32>()));
    let result = (|| -> AppResult<()> {
        let mut f = fs::File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn load_credentials(dir: &Path) -> AppResult<Option<Credentials>> {
    let path = credentials_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

/// `password = None` 表示沿用已保存的密码（前端密码框留空即不改）。
pub fn save_credentials(
    dir: &Path,
    config: &SyncConfigDto,
    password: Option<String>,
) -> AppResult<()> {
    // 旧凭据文件损坏时不阻塞修复：用户填了新密码就能整体覆盖它。
    // 没填密码时仍会落到下面的「请先填写密码」，不会静默写空密码。
    let existing = load_credentials(dir).unwrap_or(None);
    let password = match password {
        Some(p) if !p.is_empty() => p,
        _ => existing
            .as_ref()
            .map(|c| c.password.clone())
            .unwrap_or_default(),
    };
    // 只在「启用」时校验：启用后推送/拉取按钮会亮起来，半截配置必须在这里拦住，
    // 别等用户点下去才由 MysqlStore::new 报错；停用时允许先存半截（换机时常见）。
    if config.enabled {
        if config.port == 0 {
            return Err(AppError::msg("端口不合法，请填写 1~65535 之间的数字"));
        }
        if config.host.trim().is_empty()
            || config.database.trim().is_empty()
            || config.user.trim().is_empty()
        {
            return Err(AppError::msg("请先填写主机、数据库名和账号"));
        }
    }
    if password.is_empty() {
        return Err(AppError::msg("请先填写同步数据库的密码"));
    }
    let creds = Credentials {
        config: config.clone(),
        password,
    };
    write_private(
        &credentials_path(dir),
        serde_json::to_string_pretty(&creds)?.as_bytes(),
    )
}

// ---------------------------------------------------------------- 本机状态

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncState {
    pub device_id: String,
    /// 上次同步成功时本地库的内容哈希；用来判断「本地之后有没有改过」
    pub last_synced_hash: String,
    pub last_sync_at: String,
    pub last_sync_from: String,
    pub remote_version: u32,
    /// 只在 `.sync-pending-state.json` 里有意义：拉取那一刻本机的内容哈希。
    /// 启动替换前会重算本机哈希，与它不一致说明「点了拉取之后家长又默写过」，
    /// 此时绝不能拿旧快照覆盖新数据。
    pub pending_local_hash: String,
}

/// 程序升级跑迁移时只改 schema（加列），`content_hash` 会因为列数变化而整体变样。
/// 如果「迁移前的哈希」正好等于上次同步的基准，说明这段时间用户没动过数据，
/// 那就把基准更新成迁移后的哈希；否则升级后第一次拉取会被误判成「本机有改动」而弹覆盖确认。
/// 迁移前数据就变过（基准对不上）时不做任何事，真实的「本地改动」信号必须保住。
pub fn rebase_baseline_after_migration(dir: &Path, hash_before: &str, conn: &Connection) {
    let Ok(mut state) = load_state(dir) else {
        return;
    };
    if state.last_synced_hash.is_empty() || state.last_synced_hash != hash_before {
        return;
    }
    let Ok(after) = content_hash(conn) else {
        return;
    };
    if after == hash_before {
        return;
    }
    state.last_synced_hash = after;
    let _ = save_state(dir, &state);
}

pub fn load_state(dir: &Path) -> AppResult<SyncState> {
    let path = state_path(dir);
    if !path.exists() {
        return Ok(SyncState::default());
    }
    let raw = fs::read_to_string(&path)?;
    // 坏 JSON 不能静默重置：否则 remote_version 归零，每次推送都会误判冲突
    let state: SyncState = serde_json::from_str(&raw).map_err(|e| {
        AppError::msg(format!(
            "同步状态文件损坏（{e}），请检查 {}，不要直接删除",
            path.display()
        ))
    })?;
    Ok(state)
}

pub fn save_state(dir: &Path, state: &SyncState) -> AppResult<()> {
    write_private(
        &state_path(dir),
        serde_json::to_string_pretty(state)?.as_bytes(),
    )
}

fn new_device_id() -> String {
    format!(
        "{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

/// 每台设备一个稳定 ID；首次用到时生成并落盘。返回的状态里 `device_id` 一定非空。
/// **推送/拉取都要用它拿状态**：只调 `ensure_device_id` 再单独 `load_state`，
/// 会拿到 device_id 为空的那份旧状态，回写时把刚生成的 ID 抹掉。
/// 状态文件损坏时这里仍严格报错：绝不能带着错误状态往上走。
fn load_state_with_device(dir: &Path) -> AppResult<SyncState> {
    let mut state = load_state(dir)?;
    if state.device_id.is_empty() {
        state.device_id = new_device_id();
        save_state(dir, &state)?;
    }
    Ok(state)
}

// ---------------------------------------------------------------- 内容哈希

/// 对 `TABLES` 里的业务表逐行逐列算逻辑哈希。`SELECT *` 自动适配列变化，
/// 但**新增业务表必须同步加进 `TABLES`**，且必须是 `INTEGER PRIMARY KEY`（rowid 稳定）——
/// `VACUUM INTO` 会重排没有 INTEGER PRIMARY KEY 的表的 rowid，那样快照内哈希就与元数据对不上。刻意排除 `_sync_meta`（同步元数据）
/// 与 `settings`（界面偏好），因此同一份业务数据每次结果都相同。
/// 哈希按**被算库自身 schema** 自适应：`TABLES` 里有、但被算库（旧版本程序建的库 /
/// 云端旧快照）还没有的表，整张表连表名段一起从哈希输入中排除——这样新版对旧 schema
/// 算出的哈希与旧程序逐字节一致，旧快照才能通过 `validate_snapshot`，
/// 迁移前的 `hash_before` 才能命中旧同步基准（`rebase_baseline_after_migration` 才会生效）。
pub fn content_hash(conn: &Connection) -> AppResult<String> {
    // 刻意**不**把 `PRAGMA user_version` 混进来：升级程序跑迁移只改版本号、业务数据没变，
    // 混进来会让每次升级后的第一次拉取都被判成「本机有改动」而弹覆盖确认。
    // 版本兼容性由 `validate_snapshot` 的 `uv > latest` 检查单独负责。
    let mut hasher = Sha256::new();
    for table in TABLES {
        // 缺失的表整张（含「table:...;」段）跳过，绝不能只把报错吞成 0 行：
        // 表名段混进输入后算出的哈希就与旧程序的口径不等，旧快照照样永远过不了校验
        if !table_exists(conn, table)? {
            continue;
        }
        hasher.update(table.as_bytes());
        hasher.update(b":");
        hash_table(&mut hasher, conn, table)?;
        hasher.update(b";");
    }
    Ok(hex(&hasher.finalize()))
}

/// 被算库里有没有这张表（查 `sqlite_master`）。表名来自常量 `TABLES`，不是用户输入，无注入风险。
fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn hash_table(hasher: &mut Sha256, conn: &Connection, table: &str) -> AppResult<()> {
    let sql = format!("SELECT * FROM {table} ORDER BY rowid");
    let mut stmt = conn.prepare(&sql)?;
    let cols = stmt.column_count();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        for i in 0..cols {
            match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => hasher.update(b"N"),
                rusqlite::types::ValueRef::Integer(n) => hasher.update(format!("I{n}").as_bytes()),
                rusqlite::types::ValueRef::Real(f) => {
                    hasher.update(format!("F{}", f.to_bits()).as_bytes())
                }
                rusqlite::types::ValueRef::Text(t) => {
                    hasher.update(b"T");
                    hasher.update(t);
                }
                rusqlite::types::ValueRef::Blob(b) => {
                    // 同样带类型标记，否则 Blob 与相邻 Text 的字节流可能拼出相同的哈希输入
                    hasher.update(b"B");
                    hasher.update(b);
                }
            }
            hasher.update(b"\x1f");
        }
        hasher.update(b"\x1e");
    }
    Ok(())
}

/// 本机是否连一条词都没有：决定「从没同步过的新机器」拉取时要不要弹覆盖确认。
/// 只看业务数据，不看 settings：字号被改过不等于用户录过词。
fn library_is_empty(conn: &Connection) -> AppResult<bool> {
    for table in TABLES {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        if n > 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------- 快照

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    pub device_id: String,
    pub device_name: String,
    pub app_version: String,
    pub user_version: i64,
    pub exported_at: String,
    /// 源库的 content_hash；服务端 hash 列存同一个值
    pub content_hash: String,
}

pub fn new_snapshot_path(dir: &Path) -> PathBuf {
    dir.join(format!(
        ".snapshot-{}-{}.db",
        unix_secs(),
        rand::random::<u32>()
    ))
}

/// `VACUUM INTO` 产出已 checkpoint 的单文件（不带 -wal / -shm），
/// 再在副本里写入 `_sync_meta`。绝不能 `fs::copy` 正在运行的库。
pub fn create_snapshot(conn: &Connection, dir: &Path, meta: &SnapshotMeta) -> AppResult<PathBuf> {
    let path = new_snapshot_path(dir);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let result = (|| -> AppResult<()> {
        let escaped = path.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{escaped}';"))?;
        let snap = Connection::open(&path)?;
        // 拉过一次快照并替换后，主库本身就带 _sync_meta。用 IF NOT EXISTS +
        // OR REPLACE，保证「拉取 → 重启 → 再推送」闭环不会因表已存在而失败。
        snap.execute_batch(
            "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        snap.execute(
            "INSERT OR REPLACE INTO _sync_meta (key, value) VALUES (?1, ?2)",
            params![META_KEY, serde_json::to_string(meta)?],
        )?;
        drop(snap);
        Ok(())
    })();
    if result.is_err() {
        // VACUUM 已产出半成品时别留下整库垃圾
        let _ = fs::remove_file(&path);
    }
    result?;
    Ok(path)
}

pub fn read_snapshot_meta(path: &Path) -> AppResult<SnapshotMeta> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let raw: String = conn.query_row(
        "SELECT value FROM _sync_meta WHERE key = ?1",
        params![META_KEY],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&raw)?)
}

/// 导入前校验：完整性、迁移版本不能高于本程序、`_sync_meta` 必须存在。
pub fn validate_snapshot(path: &Path, max_user_version: i64) -> AppResult<SnapshotMeta> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(AppError::msg("同步快照已损坏，已取消导入"));
    }
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if uv > max_user_version {
        return Err(AppError::msg("快照来自更新版本的程序，请先升级后再同步"));
    }
    let meta = read_snapshot_meta(path)?;
    let actual = content_hash(&conn)?;
    if actual != meta.content_hash {
        return Err(AppError::msg("快照内容与元数据不一致，已取消导入"));
    }
    Ok(meta)
}

// ---------------------------------------------------------------- 云端存储

#[derive(Debug, Clone)]
pub struct RemoteSnapshot {
    pub version: u32,
    pub data: Vec<u8>,
    /// 云端行自带的 hash / app_version / user_version 列。**判定一律用校验过的快照内
    /// 元数据**（`validate_snapshot` 返回值），这些列只留作诊断——列被手工改过时
    /// 不能拿它们做判定，否则会造成永久拒绝或静默错判。
    #[allow(dead_code)]
    pub content_hash: String,
    #[allow(dead_code)]
    pub app_version: String,
    #[allow(dead_code)]
    pub user_version: i64,
}

/// `fetch` 查回来的原始行，字段顺序与下面的 SELECT 一致
/// （version, data, hash, app_version, user_version）
type RemoteRow = (u32, Vec<u8>, String, String, i64);

pub enum PushOutcome {
    Applied(u32),
    /// 云端版本已被别的设备推进；返回当前云端版本
    Conflict(u32),
}

pub trait SnapshotStore: Send + Sync {
    fn fetch(&self) -> AppResult<RemoteSnapshot>;
    fn push(&self, expected: u32, data: &[u8], meta: &SnapshotMeta) -> AppResult<PushOutcome>;
    fn server_version(&self) -> AppResult<String>;
}

pub struct MysqlStore {
    opts: mysql::Opts,
}

impl MysqlStore {
    pub fn new(cfg: &SyncConfigDto, password: &str) -> AppResult<Self> {
        if cfg.host.trim().is_empty()
            || cfg.database.trim().is_empty()
            || cfg.user.trim().is_empty()
        {
            return Err(AppError::msg("请先填写主机、数据库名和账号"));
        }
        if cfg.port == 0 {
            return Err(AppError::msg("端口不合法，请填写 1~65535 之间的数字"));
        }
        let opts: mysql::Opts = mysql::OptsBuilder::new()
            .ip_or_hostname(Some(cfg.host.trim().to_string()))
            .tcp_port(cfg.port)
            .user(Some(cfg.user.trim().to_string()))
            .pass(Some(password.to_string()))
            .db_name(Some(cfg.database.trim().to_string()))
            .tcp_connect_timeout(Some(Duration::from_secs(8)))
            .read_timeout(Some(Duration::from_secs(15)))
            .write_timeout(Some(Duration::from_secs(30)))
            .max_allowed_packet(Some(64 * 1024 * 1024))
            .into();
        Ok(Self { opts })
    }

    fn connect(&self) -> AppResult<mysql::Conn> {
        mysql::Conn::new(self.opts.clone())
            .map_err(|e| AppError::msg(format!("连接同步数据库失败：{e}")))
    }
}

impl SnapshotStore for MysqlStore {
    fn fetch(&self) -> AppResult<RemoteSnapshot> {
        let mut conn = self.connect()?;
        let row: Option<RemoteRow> = conn
            .query_first(
                "SELECT version, data, hash, app_version, user_version
                 FROM dictation_snapshot WHERE id = 1",
            )
            .map_err(|e| AppError::msg(format!("读取云端快照失败：{e}")))?;
        let (version, data, content_hash, app_version, user_version) =
            row.ok_or_else(|| AppError::msg("云端快照表还没初始化，请先在服务器建表"))?;
        Ok(RemoteSnapshot {
            version,
            data,
            content_hash,
            app_version,
            user_version,
        })
    }

    /// CAS 乐观锁：`affected_rows = 0` 说明另一台设备已经推进过版本，绝不静默覆盖。
    fn push(&self, expected: u32, data: &[u8], meta: &SnapshotMeta) -> AppResult<PushOutcome> {
        let mut conn = self.connect()?;
        conn.exec_drop(
            "UPDATE dictation_snapshot
             SET data = ?, version = version + 1, device_id = ?, device_name = ?,
                 hash = ?, app_version = ?, user_version = ?, exported_at = ?, updated_at = NOW()
             WHERE id = 1 AND version = ?",
            (
                data,
                meta.device_id.as_str(),
                meta.device_name.as_str(),
                meta.content_hash.as_str(),
                meta.app_version.as_str(),
                meta.user_version,
                meta.exported_at.as_str(),
                expected,
            ),
        )
        .map_err(|e| AppError::msg(format!("上传快照失败：{e}")))?;
        if conn.affected_rows() == 0 {
            // 0 行有两种可能：CAS 失败，或根本没有 id=1 占位行。
            // 后者必须单独报错，否则会骗用户「另一台设备已更新云端」，引导他去点强制覆盖。
            let current: Option<u32> = conn
                .query_first("SELECT version FROM dictation_snapshot WHERE id = 1")
                .map_err(|e| AppError::msg(format!("查询云端版本失败：{e}")))?;
            let current = current.ok_or_else(|| {
                AppError::msg(
                    "云端快照表里没有 id=1 的占位行，请先在服务器执行 wiki/sync.md 里的建表语句",
                )
            })?;
            return Ok(PushOutcome::Conflict(current));
        }
        Ok(PushOutcome::Applied(expected + 1))
    }

    fn server_version(&self) -> AppResult<String> {
        let mut conn = self.connect()?;
        conn.query_first("SELECT VERSION()")
            .map_err(|e| AppError::msg(format!("连接测试失败：{e}")))?
            .ok_or_else(|| AppError::msg("连接测试没有返回结果"))
    }
}

// ---------------------------------------------------------------- pending 替换

/// 同步相关留档统一放 `backups/sync/`。`backup::list` 只列 `backups/` 顶层的 `.json`
/// 导出备份，混在一起会让「重置同步状态」留下的损坏 JSON 出现在恢复列表里（点开必报格式错误），
/// 也让 `.db` 整库副本在没有入口的情况下无限堆积。
fn sync_backup_dir(dir: &Path) -> AppResult<PathBuf> {
    let d = crate::repo::backup::backup_dir(dir).join("sync");
    fs::create_dir_all(&d)?;
    Ok(d)
}

/// 本机库「读不出来」时的两种性质，决定要不要拿云端快照覆盖它。
#[derive(Debug, PartialEq, Eq)]
enum LocalDbFailure {
    /// 库本身坏了 / 根本不是数据库 / 空文件没有业务表：本机没有可用数据，
    /// 换库正是唯一的恢复手段。
    Corrupt,
    /// 暂时读不出来（被别的连接锁着、IO 瞬时错误）：数据还在，绝不能覆盖。
    Unusable,
}

/// 只能靠错误码判断，别用 `PRAGMA integrity_check` 反推：库被别的连接锁住时
/// integrity_check 同样返回 Err，那样会把「忙」误判成「坏」，跳过守卫直接覆盖本机数据。
fn classify_local_failure(e: &AppError) -> LocalDbFailure {
    let AppError::Db(rusqlite::Error::SqliteFailure(err, msg)) = e else {
        return LocalDbFailure::Unusable;
    };
    match err.code {
        rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
            LocalDbFailure::Corrupt
        }
        // SQLITE_ERROR 里最常见的是「没这张表」：新库 / 0 字节库，本来就没有数据可丢
        rusqlite::ErrorCode::Unknown => match msg {
            Some(m) if m.contains("no such table") => LocalDbFailure::Corrupt,
            _ => LocalDbFailure::Unusable,
        },
        // Busy / Locked / IO 错误等一律按「暂时读不出来」处理，保留 pending 下次再试
        _ => LocalDbFailure::Unusable,
    }
}

fn remove_sidecars(db: &Path) {
    for suffix in ["-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{}", db.display(), suffix));
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
}

/// 把下载好的快照写成 `pending.db`，先写临时文件（fsync）再原子改名。
pub fn write_pending(dir: &Path, bytes: &[u8]) -> AppResult<()> {
    // 临时名带随机后缀：与 write_private 对齐，并发调用时不会互相踩同一个文件
    let tmp = dir.join(format!(".{PENDING_FILE}.tmp-{}", rand::random::<u32>()));
    let result = (|| -> AppResult<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        // 与 write_private 一致：不 fsync 的话崩溃 / 断电可能留下截断的快照，
        // 下次启动校验失败，用户白拉一次
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, pending_path(dir))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// 上次替换已成功、但状态没来得及提交时补提交（自愈）。返回是否真的提交了。
///
/// 两个前提，缺一不可：
/// 1. **库确实已经被换掉了**：拿本机当前内容哈希与 `pending_local_hash`（拉取那一刻的样子）
///    比，如果一模一样，说明当时根本没替换成功（例如用户手工删了 `pending.db`），
///    此时提交就等于「库还是旧的却对齐了云端版本」，下一次推送会直接通过 CAS 静默覆盖云端。
/// 2. 主状态没有被别的成功推送推进到更高版本，也**没有损坏**（损坏时不自动覆盖，
///    交给用户按设置页提示处理）。
fn commit_pending_state(dir: &Path, db_path: &Path) -> bool {
    let ps = pending_state_path(dir);
    if !ps.exists() {
        return false;
    }
    let Ok(raw) = fs::read_to_string(&ps) else {
        return false;
    };
    let Ok(next) = serde_json::from_str::<SyncState>(&raw) else {
        return false;
    };
    // 交给一个统一出口：需要作废时把待提交状态挪进 backups/sync/ 并写清原因
    let reject = |reason: &str| {
        let archived = sync_backup_dir(dir).map(|d| {
            d.join(format!(
                "sync-pending-state-rejected-{}-{}.json",
                unix_secs(),
                rand::random::<u32>()
            ))
        });
        let moved = matches!(archived, Ok(path) if fs::rename(&ps, &path).is_ok());
        if !moved {
            let _ = fs::remove_file(&ps);
        }
        prune_sync_backups(dir, "sync-pending-state-rejected-");
        let _ = fs::write(last_error_path(dir), reason);
        false
    };
    // 先比版本：期间又成功推送过的话主状态已经更高，这份旧状态直接作废，不必碰数据库
    match load_state(dir) {
        Ok(current) if current.remote_version > next.remote_version => {
            let _ = fs::remove_file(&ps);
            return false;
        }
        Ok(_) => {}
        // 主状态损坏时不自动覆盖，交给用户按设置页提示处理
        Err(_) => return false,
    }
    // 「库到底有没有被换成这次拉的快照」不靠猜：快照生成时把内容哈希写进了 `_sync_meta`，
    // 替换成功后主库就带着它。只有主库的 marker 等于 `last_synced_hash` 才说明替换真的发生过。
    // 否则（待应用快照被手工删掉、库被删、或换成过别的版本）一律不提交——
    // 提交会让 remote_version 提前对齐云端，下一次推送就绕过 CAS 静默覆盖另一台设备的改动。
    // 两个条件缺一不可：
    // a) 主库 `_sync_meta` 记的正是这次拉取的快照（正面证据）；
    // b) 本机内容已经不是「拉取前那份」（否则说明库根本没换，只是 pending 被删了）。
    // 少了 b)，当上次应用的快照内容恰好等于这次拉取的内容时，会误判成「已替换」。
    // 主库读不出来时按错误码分流（与 apply_pending_inner 一致），绝不能一股脑当成「没替换过」：
    // - BUSY / LOCKED / IO 瞬时错误：数据还在，只是这次读不到——保留 pending_state，下次启动再试。
    //   若误作废，remote_version 永不对齐，之后每次推送都弹冲突，用户被训练成习惯性点「继续覆盖」。
    // - NOTADB / CORRUPT：替换成功的主库必然是那份完好快照，库坏了就不可能「已替换」，作废是对的。
    if !db_path.exists() {
        return reject(
            "同步状态里记着「已下载新快照」，但本机库并不存在（可能被手工删掉了），\
             已作废这次记录；请先确认本机数据，必要时重新拉取",
        );
    }
    let now = match Connection::open(db_path).map_err(AppError::from).and_then(|c| content_hash(&c)) {
        Ok(h) => h,
        Err(e) if classify_local_failure(&e) == LocalDbFailure::Unusable => {
            // 暂时读不出（杀毒扫描 / 备份软件占着锁）：这次先不动，下次启动再试
            return false;
        }
        Err(_) => {
            return reject(
                "同步状态里记着「已下载新快照」，但本机库并不是那份快照（可能已损坏或被手工替换），\
                 已作废这次记录；请先确认本机数据，必要时重新拉取",
            );
        }
    };
    let marker_match = match read_snapshot_meta(db_path) {
        Ok(m) => m.content_hash == next.last_synced_hash,
        // 与上面的哈希读取同一套分流：只是暂时读不出（撞上杀毒扫描窗口）就保留待提交状态，
        // 下次启动再试；真坏了才作废。
        Err(e) if classify_local_failure(&e) == LocalDbFailure::Unusable => return false,
        Err(_) => false,
    };
    let replaced =
        now != next.pending_local_hash && !next.last_synced_hash.is_empty() && marker_match;
    if !replaced {
        return reject(
            "同步状态里记着「已下载新快照」，但本机库并不是那份快照（快照可能被手工删掉了），\
             已作废这次记录；请先确认本机内容，必要时重新拉取",
        );
    }
    if save_state(dir, &next).is_ok() {
        let _ = fs::remove_file(&ps);
        // 自愈完成，之前那条「状态没写成」的提示就该消失了
        clear_last_error(dir);
        true
    } else {
        false
    }
}

/// 清掉上次进程被强杀 / 断电留下的临时文件。`.snapshot-*.db` 是整库大小，
/// `.tmp-*` 是写到一半的状态文件，留着只会在数据目录里越堆越多。
/// **只在启动早期调用**（`apply_pending` 里），此时不可能有同步任务在写这些文件。
fn cleanup_stale_temp(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".snapshot-") || name.contains(".tmp-") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// 同步留档最多留几份（按前缀分开算）。全是整库大小，无限堆会把便携版的数据目录撑爆。
const KEEP_SYNC_BACKUPS: usize = 5;

/// 按前缀裁剪 `backups/sync/`，只留文件名（含时间戳）最大的 N 份。
/// 文件名格式是 `<prefix>-<unix秒>-<随机>.db`，字典序即时间序。
fn prune_sync_backups(dir: &Path, prefix: &str) {
    let Ok(backups) = sync_backup_dir(dir) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&backups) else {
        return;
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(prefix))
                .unwrap_or(false)
        })
        .collect();
    if found.len() <= KEEP_SYNC_BACKUPS {
        return;
    }
    found.sort();
    let cut = found.len() - KEEP_SYNC_BACKUPS;
    for old in found.into_iter().take(cut) {
        let _ = fs::remove_file(old);
    }
}

fn clear_last_error(dir: &Path) {
    let _ = fs::remove_file(last_error_path(dir));
}

/// 用户主动放弃待应用快照 / 清掉上一次失败的提示，避免残留 pending 把同步永久锁死。
/// 主状态 `.sync-state.json` 绝不碰；孤儿 pending_state（库已替换、只差提交状态）要留着给启动补提交。
pub fn discard_pending(dir: &Path) -> AppResult<()> {
    let pending = pending_path(dir);
    if pending.exists() {
        // 删不掉（Windows 上常被杀软 / 索引服务占着）必须让用户知道，
        // 否则前端回报「已放弃」但状态里 pending 仍在，点几次都没反应
        fs::remove_file(&pending).map_err(|e| {
            AppError::msg(format!(
                "无法删除待应用快照（{e}）；请关闭其他程序后重试"
            ))
        })?;
        let _ = fs::remove_file(pending_state_path(dir));
    }
    clear_last_error(dir);
    Ok(())
}

/// 用户确认后隔离损坏的状态文件并重建：remote_version 归零，下一次推送/拉取会走确认流程，
/// 不会静默覆盖云端。旧文件不删除，留到 `backups/sync/` 供人工排查。
/// 未应用的 `pending.db` 也一起作废：它的配套状态已被隔离，没有基准就无从判断
/// 拉取之后本机有没有改过，留着只会在下次启动被拒。
pub fn repair_state(dir: &Path) -> AppResult<SyncStatus> {
    let path = state_path(dir);
    if !path.exists() || load_state(dir).is_ok() {
        return Err(AppError::msg("同步状态正常，无需重置"));
    }
    let archived_dir = sync_backup_dir(dir)?;
    let archived = archived_dir.join(format!(
        "sync-state-corrupt-{}-{}.json",
        unix_secs(),
        rand::random::<u32>()
    ));
    fs::rename(&path, archived)?;
    // 孤儿待提交状态也要一起隔离：否则下次启动 commit_pending_state 会把刚重置的
    // remote_version / device_id 又写回去，「重置后一定会先确认」的承诺就失效了。
    let ps = pending_state_path(dir);
    if ps.exists() {
        let target = archived_dir.join(format!(
            "sync-pending-state-orphan-{}-{}.json",
            unix_secs(),
            rand::random::<u32>()
        ));
        if fs::rename(&ps, target).is_err() {
            let _ = fs::remove_file(&ps);
        }
    }
    let pending = pending_path(dir);
    if pending.exists() {
        let target = archived_dir.join(format!(
            "pending-on-reset-{}-{}.db",
            unix_secs(),
            rand::random::<u32>()
        ));
        if fs::rename(&pending, target).is_err() {
            let _ = fs::remove_file(&pending);
        }
    }
    prune_sync_backups(dir, "pending-on-reset-");
    prune_sync_backups(dir, "sync-pending-state-orphan-");
    prune_sync_backups(dir, "sync-state-corrupt-");
    save_state(
        dir,
        &SyncState {
            device_id: new_device_id(),
            ..SyncState::default()
        },
    )?;
    clear_last_error(dir);
    status(dir)
}

/// 替换失败分两类：`retry = true` 表示主库还没被动过、pending 完好，留着下次启动再试；
/// `retry = false` 表示这份快照不可用（校验失败 / 守卫缺失 / 已替换但状态没写成），必须作废。
/// **一律 fail-closed**：拿不准就作废并让用户重新拉取，绝不在缺少守卫的情况下覆盖主库。
struct ApplyFailure {
    error: AppError,
    retry: bool,
}

impl ApplyFailure {
    fn fatal(e: impl Into<AppError>) -> Self {
        Self {
            error: e.into(),
            retry: false,
        }
    }
    fn retry(e: impl Into<AppError>) -> Self {
        Self {
            error: e.into(),
            retry: true,
        }
    }
}

/// 由 `db::init` 在打开连接之前调用：校验 → 完整备份 → 清 WAL → 替换 → 提交状态。
/// 瞬时失败（库被占用 / 备份写不进去）保留 pending；其它失败把 pending 挪进 `backups/sync/` 留档。
/// 失败原因写进 `.sync-last-error.txt` 并由 `status()` 回显，**不能在下次启动自动清掉**，
/// 否则用户会以为快照已经生效。
pub fn apply_pending(dir: &Path, db_path: &Path) -> AppResult<bool> {
    // 启动早期、任何同步任务都还没开始，顺手清掉上次崩溃 / 强杀留下的临时文件
    cleanup_stale_temp(dir);
    let pending = pending_path(dir);
    let err_file = last_error_path(dir);
    if !pending.exists() {
        commit_pending_state(dir, db_path);
        return Ok(false);
    }
    // 这次要重新尝试，先清掉上一次的痕迹；本次产生的说明（例如「库坏了、已用云端快照替换」）
    // 必须留到用户看见，所以成功分支**不能**再清一次。
    clear_last_error(dir);
    match apply_pending_inner(dir, db_path, &pending) {
        Ok(()) => Ok(true),
        Err(f) => {
            if f.retry {
                // 主库没动过、pending 完好：留着下次启动重试，别白扔一份已经下载好的快照
                let _ = fs::write(
                    &err_file,
                    format!("{}（已保留下载好的快照，下次启动会重试）", f.error),
                );
                return Err(f.error);
            }
            let had_pending = pending.exists();
            if had_pending {
                // 这份 pending 作废：挪到 backups/sync/ 留档供排查，挪不动就直接删
                let rejected = sync_backup_dir(dir).map(|d| {
                    d.join(format!(
                        "pending-rejected-{}-{}.db",
                        unix_secs(),
                        rand::random::<u32>()
                    ))
                });
                let moved = matches!(rejected, Ok(path) if fs::rename(&pending, &path).is_ok());
                if !moved {
                    let _ = fs::remove_file(&pending);
                }
                prune_sync_backups(dir, "pending-rejected-");
                // 配套状态一起删：留着会变成孤儿，下次启动 commit_pending_state
                // 会把 remote_version 提前对齐云端（后续推送就绕过 CAS 了）
                let _ = fs::remove_file(pending_state_path(dir));
            }
            // pending 已不存在 = 库替换成功，保留 pending_state 供下次启动补提交
            let note = if had_pending {
                "（本次下载的快照已作废，需要重新拉取）"
            } else {
                ""
            };
            let _ = fs::write(&err_file, format!("{}{}", f.error, note));
            Err(f.error)
        }
    }
}

fn apply_pending_inner(dir: &Path, db_path: &Path, pending: &Path) -> Result<(), ApplyFailure> {
    let meta =
        validate_snapshot(pending, crate::db::latest_user_version()).map_err(ApplyFailure::fatal)?;

    // 先解析待提交状态，避免「库已替换但状态没写」的半途失败。
    // 这套状态同时是本机数据的「基准」：缺了它就无从判断拉取之后本机有没有被改过，
    // 所以一律拒绝替换（早期版本这里是 fail-open，会让新写的进度被静默盖掉）。
    let ps = pending_state_path(dir);
    if !ps.exists() {
        return Err(ApplyFailure::fatal(AppError::msg(
            "待应用快照缺少配套的同步状态（写入可能中断），已作废；请重新拉取",
        )));
    }
    let raw = fs::read_to_string(&ps).map_err(ApplyFailure::fatal)?;
    let next_state: SyncState = serde_json::from_str(&raw)
        .map_err(|e| ApplyFailure::fatal(AppError::msg(format!("待应用状态文件损坏：{e}"))))?;
    if next_state.pending_local_hash.is_empty() {
        return Err(ApplyFailure::fatal(AppError::msg(
            "待应用快照没有记录拉取时的本机基准，无法确认是否会覆盖新数据，已作废；请重新拉取",
        )));
    }
    // 主状态必须能读：读不出来时若继续替换，末尾的 save_state 会把坏文件原地覆盖，
    // 既没留档也绕过了「损坏必须走重置」的约定。
    match load_state(dir) {
        Ok(current) if current.remote_version > next_state.remote_version => {
            return Err(ApplyFailure::fatal(AppError::msg(
                "本机之后已经推送过更新的版本，这份下载好的快照已过期，已作废；请重新拉取",
            )));
        }
        Ok(_) => {}
        Err(e) => {
            return Err(ApplyFailure::fatal(AppError::msg(format!(
                "同步状态文件损坏（{e}），本次先不替换。请在设置页点「重置同步状态」后再重新拉取"
            ))))
        }
    }
    // 快照与配套状态必须是同一次拉取产生的：并发拉取（切页回来再点一次）时两者可能来自
    // 不同版本，替换后状态会记成另一份快照的版本，之后一次推送就能静默覆盖云端。
    if meta.content_hash != next_state.last_synced_hash {
        return Err(ApplyFailure::fatal(AppError::msg(
            "待应用快照与配套状态不是同一次拉取（可能同时发起了两次拉取），已作废；请重新拉取",
        )));
    }

    // 整库替换会把 `settings` 一起换掉，但那是本机偏好（字号、遮罩秒数、错题本出本阈值），
    // 不该被另一台机器的设置悄悄覆盖：先记下来，替换完成后写回。
    let mut local_settings = None;
    // 本机库已经坏了（自己打不开 / 读不出来）时，换库正好是唯一的恢复手段：
    // 这种情况不能因为「算不出哈希」就把下载好的快照作废，否则「从云端恢复」永远走不通。
    let mut local_broken = false;
    if db_path.exists() {
        let old = Connection::open(db_path).map_err(ApplyFailure::retry)?;
        local_settings = crate::repo::settings::load(&old).ok();
        // 拉取之后本机又被改过（家长点了拉取却继续默写、或网络往返期间刚批改完一场）：
        // 绝不能用旧快照盖掉这些新数据。宁可作废这次 pending 让用户重来。
        match content_hash(&old) {
            Ok(now) if now != next_state.pending_local_hash => {
                return Err(ApplyFailure::fatal(AppError::msg(
                    "拉取之后本机数据又有改动，已取消应用这份快照（本机数据未动）。\
                     确认要用云端覆盖时，请重新点「拉取」并在确认框选择继续",
                )));
            }
            Ok(_) => {}
            Err(e) => {
                match classify_local_failure(&e) {
                    // 库真坏了：本机本来就没有可用数据，拿云端快照救回来
                    LocalDbFailure::Corrupt => {
                        local_broken = true;
                        local_settings = None;
                    }
                    // 只是暂时读不出来：保留 pending，下次启动再试
                    LocalDbFailure::Unusable => {
                        return Err(ApplyFailure::retry(AppError::msg(format!(
                            "暂时读不出本机数据（{e}），本次启动先不替换；请关闭其他程序后重启"
                        ))))
                    }
                }
            }
        }
        if !local_broken {
            // checkpoint 必须真的成功，否则 WAL 里已提交的事务会在删 sidecar 时丢失，
            // 而且只复制主 .db 的备份也不完整。
            let busy: i64 = old
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
                .map_err(ApplyFailure::retry)?;
            if busy != 0 {
                drop(old);
                return Err(ApplyFailure::retry(AppError::msg(
                    "数据库正被其他连接占用，本次启动先不替换；请关闭其他实例后重启",
                )));
            }
        }
        drop(old);
        // 坏库也留一份：用户可能还要从那堆字节里捞数据
        let backups = sync_backup_dir(dir).map_err(ApplyFailure::retry)?;
        let backup_path = backups.join(format!(
            "before-sync-{}-{}.db",
            unix_secs(),
            rand::random::<u32>()
        ));
        fs::copy(db_path, backup_path).map_err(ApplyFailure::retry)?;
        prune_sync_backups(dir, "before-sync-");
    }
    remove_sidecars(db_path);
    // 旧 WAL/SHM 没删掉就换库等于把旧事务带进新库（Windows 上可能被别的句柄占着）
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", db_path.display(), suffix));
        if sidecar.exists() {
            return Err(ApplyFailure::retry(AppError::msg(
                "无法清除数据库的 WAL 临时文件（可能被其他程序占用），本次启动先不替换；请关闭其他实例后重启",
            )));
        }
    }

    // 本机 settings（字号 / 遮罩 / 出本阈值）要留在本机：**先写进 pending.db 再 rename**，
    // 这样「换库」与「保留设置」是同一个原子动作。若替换后才写，进程在两步之间被强杀
    // 就会留下「库已是云端那份、设置却是另一台的」，而补提交状态时又会把提示清掉。
    if let Some(settings) = local_settings {
        let staged = Connection::open(pending).map_err(|e| {
            ApplyFailure::retry(AppError::msg(format!(
                "无法打开待应用快照以写回本机设置（{e}）"
            )))
        })?;
        crate::repo::settings::save(&staged, &settings).map_err(|e| {
            ApplyFailure::retry(AppError::msg(format!(
                "无法把本机设置写进待应用快照（{e}）"
            )))
        })?;
        drop(staged);
        // 单文件快照必须回到「无 journal / 无 wal」状态才能直接改名
        for suffix in ["-journal", "-wal", "-shm"] {
            let extra = PathBuf::from(format!("{}{}", pending.display(), suffix));
            if extra.exists() {
                fs::remove_file(&extra).map_err(ApplyFailure::retry)?;
            }
        }
    }

    if local_broken {
        // 坏库已经在 before-sync 备份里；这里直接覆盖（rename 覆盖是原子的），
        // 但要在设置页留个痕：本机数据是被云端快照替换掉的
        let _ = fs::write(
            last_error_path(dir),
            "本机数据库损坏，已用下载好的云端快照替换成功；旧库留在 backups/sync/ 下",
        );
    }

    // std::fs::rename 在 Windows 同样会覆盖已存在的目标文件，不要先删旧库
    fs::rename(pending, db_path).map_err(ApplyFailure::retry)?;

    // 写失败时保留 pending_state，下次启动由 commit_pending_state 补提交。
    // 注意此时库**已经替换成功**，错误文案不能说「快照没生效」。
    save_state(dir, &next_state).map_err(|e| {
        ApplyFailure::fatal(AppError::msg(format!(
            "快照已替换成功，但同步状态没写成（{e}）；下次启动会自动补提交，无需重新拉取"
        )))
    })?;
    let _ = fs::remove_file(&ps);
    Ok(())
}

// ---------------------------------------------------------------- 高层操作

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub configured: bool,
    pub enabled: bool,
    pub has_password: bool,
    pub config: SyncConfigDto,
    pub device_id: String,
    pub last_sync_at: String,
    pub last_sync_from: String,
    pub pending: bool,
    /// 状态/凭据文件损坏时的告警文案；非空表示推送与拉取已被暂停
    pub warning: String,
    /// 上次启动替换快照失败的原因；只做提示，不禁用同步，用户可手动清除
    pub last_error: String,
    /// `.sync-state.json` 是否损坏；为 true 时前端才显示「重置同步状态」
    pub state_corrupt: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    /// applied / conflict / unchanged / pending / empty
    pub kind: String,
    pub message: String,
    pub remote_version: u32,
    pub needs_confirm: bool,
}

fn sqlite_now(conn: &Connection) -> AppResult<String> {
    Ok(
        conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
            r.get(0)
        })?,
    )
}

fn ensure_no_pending(dir: &Path) -> AppResult<()> {
    if pending_path(dir).exists() {
        return Err(AppError::msg(
            "有下载好的快照待应用，请先重启应用完成替换，再同步",
        ));
    }
    Ok(())
}

fn require_creds(dir: &Path, require_enabled: bool) -> AppResult<(SyncConfigDto, String)> {
    let creds =
        load_credentials(dir)?.ok_or_else(|| AppError::msg("请先在设置里填写同步数据库信息"))?;
    if require_enabled && !creds.config.enabled {
        return Err(AppError::msg("跨设备同步尚未启用，请先在设置里打开开关"));
    }
    Ok((creds.config, creds.password))
}

/// 设置页要能打开，所以这里对损坏的本机文件降级展示，而不是把错误抛给整个页面。
/// 但**不重置、不改写**任何状态：`load_state` 严格报错，推送/拉取依旧会拦住用户，
/// 避免 remote_version 归零造成「每次推送都误判冲突」甚至被习惯性强制覆盖。
pub fn status(dir: &Path) -> AppResult<SyncStatus> {
    let mut warnings: Vec<String> = Vec::new();
    let creds = match load_credentials(dir) {
        Ok(c) => c,
        Err(e) => {
            warnings.push(format!("同步凭据文件损坏：{e}"));
            None
        }
    };
    let mut state_corrupt = false;
    // 只读：早前这里用 load_state_with_device 顺手补写 device_id，会让一个查询命令
    // 在只读/满盘目录上失败，还把「写不进去」谎报成「状态文件损坏」。
    // device_id 的生成交给真正要同步的 prepare_push / prepare_pull。
    let state = match load_state(dir) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(e.to_string());
            state_corrupt = true;
            SyncState::default()
        }
    };
    Ok(SyncStatus {
        configured: creds.is_some(),
        enabled: creds.as_ref().map(|c| c.config.enabled).unwrap_or(false),
        has_password: creds
            .as_ref()
            .map(|c| !c.password.is_empty())
            .unwrap_or(false),
        config: creds.map(|c| c.config).unwrap_or_default(),
        device_id: state.device_id.clone(),
        last_sync_at: state.last_sync_at,
        last_sync_from: state.last_sync_from,
        pending: pending_path(dir).exists(),
        warning: warnings.join("；"),
        // 替换失败必须让用户看见：页面会提示「上次下载的快照没生效」，并给清除入口
        last_error: fs::read_to_string(last_error_path(dir))
            .unwrap_or_default()
            .trim()
            .to_string(),
        state_corrupt,
    })
}

/// 测试连接。`config` 有值时直接用「表单里还没保存的配置」测，避免用户为了试一个端口
/// 反而把已经能用的凭据覆盖成错的；密码留空表示沿用已保存的密码。
pub fn test_connection(
    dir: &Path,
    config: Option<SyncConfigDto>,
    password: Option<String>,
) -> AppResult<String> {
    let (cfg, password) = match config {
        Some(cfg) => {
            let password = match password {
                Some(p) if !p.is_empty() => p,
                _ => load_credentials(dir)
                    .ok()
                    .flatten()
                    .map(|c| c.password)
                    .unwrap_or_default(),
            };
            (cfg, password)
        }
        None => require_creds(dir, false)?,
    };
    MysqlStore::new(&cfg, &password)?.server_version()
}

/// 命令层在持有 conn 时调用：算哈希、生成快照。之后网络 I/O 交给 spawn_blocking。
pub struct PushPlan {
    dir: PathBuf,
    cfg: SyncConfigDto,
    password: String,
    /// 快照临时文件；`PushPlan` 被丢弃时（构造 MysqlStore 失败、任务 panic 等）
    /// 由 `Drop` 负责删掉，绝不把整库垃圾留在数据目录里
    _snapshot: SnapshotFile,
    bytes: Vec<u8>,
    meta: SnapshotMeta,
    local_hash: String,
    /// 本机一条业务数据都没有：推送会把云端和另一台设备一起清空，必须先确认
    local_empty: bool,
    state: SyncState,
}

/// 快照临时文件的自清理句柄。字段不能是裸 `PathBuf`：那样 `PushPlan` 提前返回时
/// 文件会永久留在数据目录（用户点「打开数据目录」能看到一堆 `.snapshot-*.db`）。
struct SnapshotFile(PathBuf);

impl Drop for SnapshotFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn prepare_push(conn: &Connection, dir: &Path) -> AppResult<PushPlan> {
    // 本地库还是旧的，此时推送会绕过 CAS 覆盖云端；必须先重启完成替换
    ensure_no_pending(dir)?;
    let (cfg, password) = require_creds(dir, true)?;
    // 必须用它拿状态：device_id 在这里可能刚刚生成，回写时不能把它抹掉
    let state = load_state_with_device(dir)?;
    let device_id = state.device_id.clone();
    let local_hash = content_hash(conn)?;
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let meta = SnapshotMeta {
        device_id: device_id.clone(),
        device_name: if cfg.device_name.trim().is_empty() {
            device_id.chars().take(8).collect()
        } else {
            cfg.device_name.trim().to_string()
        },
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        user_version,
        exported_at: sqlite_now(conn)?,
        content_hash: local_hash.clone(),
    };
    let local_empty = library_is_empty(conn)?;
    let snapshot = SnapshotFile(create_snapshot(conn, dir, &meta)?);
    let bytes = fs::read(&snapshot.0)?;
    Ok(PushPlan {
        dir: dir.to_path_buf(),
        cfg,
        password,
        _snapshot: snapshot,
        bytes,
        meta,
        local_hash,
        local_empty,
        state,
    })
}

pub fn perform_push(plan: PushPlan, force: bool) -> AppResult<SyncOutcome> {
    let store = MysqlStore::new(&plan.cfg, &plan.password)?;
    perform_push_with(&store, plan, force)
}

/// 覆盖云端之前先把云端现有快照留一份。**所有会真正写云端的路径都要走它**：
/// CAS 冲突后的强制覆盖、以及「空库强推」——后者同样一次写入就抹掉云端全部数据，
/// 只是它不会触发 CAS 冲突分支，早期版本因此漏了存档。
fn archive_remote_snapshot(store: &dyn SnapshotStore, dir: &Path) -> AppResult<()> {
    let remote = store.fetch()?;
    if remote.data.is_empty() {
        return Ok(());
    }
    let backups = sync_backup_dir(dir)?;
    fs::write(
        backups.join(format!(
            "remote-before-overwrite-{}-{}.db",
            unix_secs(),
            rand::random::<u32>()
        )),
        &remote.data,
    )?;
    prune_sync_backups(dir, "remote-before-overwrite-");
    Ok(())
}

/// 真正的推送流程。拆出来是为了让单测能用假的 `SnapshotStore` 覆盖冲突 / 强制覆盖分支，
/// 不用连真实 MySQL。
fn perform_push_with(
    store: &dyn SnapshotStore,
    plan: PushPlan,
    force: bool,
) -> AppResult<SyncOutcome> {
    // 本机是空库（典型场景：按文档「删库重来」重建后直接点推送）：此时 remote_version
    // 仍等于云端版本，CAS 会放行，于是空快照静默覆盖云端，另一台设备下次拉取也被清空。
    // 拉取侧有同样的守卫，推送侧必须对称。
    if plan.local_empty && !force {
        let remote = store.fetch()?;
        if !remote.data.is_empty() {
            return Ok(SyncOutcome {
                kind: "conflict".into(),
                message: "本机词库是空的（可能刚删库重建），推送会用空库覆盖云端快照，\n另一台设备的词表和进度也会跟着清空。确认要继续覆盖吗？".into(),
                remote_version: remote.version,
                needs_confirm: true,
            });
        }
    }
    // 空库强推即使 CAS 成功也是「把云端整份抹掉」，确认后要先留档
    if plan.local_empty && force {
        archive_remote_snapshot(store, &plan.dir)?;
    }
    let pushed = store.push(plan.state.remote_version, &plan.bytes, &plan.meta);

    let version = match pushed? {
        PushOutcome::Applied(v) => v,
        PushOutcome::Conflict(current) => {
            if !force {
                return Ok(SyncOutcome {
                    kind: "conflict".into(),
                    message: format!(
                        "另一台设备已更新云端（版本 {current}）。继续推送会覆盖它的改动，是否继续？"
                    ),
                    remote_version: current,
                    needs_confirm: true,
                });
            }
            // 从进函数到现在云端又变了：先把「现在这份」再存一次，否则重推会覆盖掉
            // 期间到达的那一版而不留任何存档
            archive_remote_snapshot(store, &plan.dir)?;
            match store.push(current, &plan.bytes, &plan.meta)? {
                PushOutcome::Applied(v) => v,
                PushOutcome::Conflict(v) => {
                    return Err(AppError::msg(format!("云端版本刚刚又变了（{v}），请重试")))
                }
            }
        }
    };

    let mut state = plan.state;
    state.last_synced_hash = plan.local_hash;
    state.last_sync_at = plan.meta.exported_at.clone();
    state.last_sync_from = plan.meta.device_name.clone();
    state.remote_version = version;
    // 推送已经生效：状态写不进去时本地 remote_version 仍是旧值，下次推送会被 CAS 拒掉
    // 并谎报「另一台设备已更新云端」，所以这里必须把事实讲清楚。
    save_state(&plan.dir, &state).map_err(|e| {
        AppError::msg(format!(
            "已推送到云端（版本 {version}），但本机同步状态没写成（{e}）。\
             下次推送可能提示版本冲突，重新拉取一次即可对齐"
        ))
    })?;
    clear_last_error(&plan.dir);
    Ok(SyncOutcome {
        kind: "applied".into(),
        message: format!("已推送到云端（版本 {version}）"),
        remote_version: version,
        needs_confirm: false,
    })
}

pub struct PullPlan {
    dir: PathBuf,
    cfg: SyncConfigDto,
    password: String,
    local_hash: String,
    /// 本机一条业务数据都没有（新装的第二台）：首次拉取不该弹「覆盖本机」。
    local_empty: bool,
    state: SyncState,
}

pub fn prepare_pull(conn: &Connection, dir: &Path) -> AppResult<PullPlan> {
    ensure_no_pending(dir)?;
    let (cfg, password) = require_creds(dir, true)?;
    Ok(PullPlan {
        dir: dir.to_path_buf(),
        cfg,
        password,
        local_hash: content_hash(conn)?,
        local_empty: library_is_empty(conn)?,
        state: load_state_with_device(dir)?,
    })
}

/// 拉取前的判定结果。抽成纯函数是为了能脱离 MySQL 单测这三种分支。
#[derive(Debug, PartialEq, Eq)]
enum PullDecision {
    /// 两边内容一致，只对齐状态、不替换文件
    Unchanged,
    /// 本机与云端都偏离上次同步点，必须让用户确认
    Conflict,
    /// 可以安全替换：写 pending，等下次启动生效
    Apply,
}

/// `last_synced_hash` 为空表示「本机从没和云端对齐过」：此时不能一律当成
/// 「本地有改动」，否则新装的第二台第一次拉取就会弹「覆盖本机」的吓人提示。
/// 只有本机确实录过东西（非空库）才需要确认。
fn decide_pull(
    local_hash: &str,
    last_synced_hash: &str,
    local_empty: bool,
    remote_hash: &str,
    force: bool,
) -> PullDecision {
    let local_changed = if last_synced_hash.is_empty() {
        !local_empty
    } else {
        local_hash != last_synced_hash
    };
    if local_changed && remote_hash != local_hash && !force {
        return PullDecision::Conflict;
    }
    if local_hash == remote_hash {
        return PullDecision::Unchanged;
    }
    PullDecision::Apply
}

pub fn perform_pull(plan: PullPlan, force: bool) -> AppResult<SyncOutcome> {
    let store = MysqlStore::new(&plan.cfg, &plan.password)?;
    perform_pull_with(&store, plan, force)
}

/// 真正的拉取流程。同样抽出来给单测用假 store 覆盖「首次拉取 / 冲突 / 一致 / 写 pending」。
fn perform_pull_with(
    store: &dyn SnapshotStore,
    plan: PullPlan,
    force: bool,
) -> AppResult<SyncOutcome> {
    let remote = store.fetch()?;
    if remote.data.is_empty() {
        return Ok(SyncOutcome {
            kind: "empty".into(),
            message: "云端还没有快照，请先在第一台电脑上点「推送」".into(),
            remote_version: remote.version,
            needs_confirm: false,
        });
    }
    // 先落地临时文件做完整校验，坏数据绝不写进 pending.db
    let tmp = new_snapshot_path(&plan.dir);
    fs::write(&tmp, &remote.data)?;
    let meta = match validate_snapshot(&tmp, crate::db::latest_user_version()) {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    };
    let _ = fs::remove_file(&tmp);

    // 版本判定一律用**校验过的快照元数据**，不用云端表里的列（列可能被手工改过：
    // 行的 user_version 被改高会造成永久拒绝且升级也救不回来）。快照文件头本身的
    // `uv > latest` 由 validate_snapshot 内部兜底。
    let latest_uv = crate::db::latest_user_version();
    if meta.user_version > latest_uv {
        return Err(AppError::msg(format!(
            "云端快照由更新版本的程序生成（v{}，数据版本 {}），请先升级后再拉取",
            meta.app_version, meta.user_version
        )));
    }

    // 反向也要确认：旧版本程序生成的快照里没有新列（如 words.wrong_chars）的取值，
    // 拉下去迁移只会把新列补成默认值，等于把本机已有的那部分数据丢掉。
    // 判定用**校验过的快照元数据**，不用云端表里的列（列可能被手工改过）。
    if meta.user_version < latest_uv && !force {
        return Ok(SyncOutcome {
            kind: "conflict".into(),
            message: format!(
                "云端快照由旧版本程序生成（数据版本 {} < {}），里面没有新版本字段（例如错字标记）的取值。\n继续拉取会用旧快照覆盖本机，那部分数据会丢失。是否继续？",
                meta.user_version, latest_uv
            ),
            remote_version: remote.version,
            needs_confirm: true,
        });
    }

    // 用校验过的快照内哈希，不要用云端行里的 hash 列：列被手工改过时，
    // 拿列值做判定会把「内容其实不同」误判成 Unchanged，从而把状态对齐到该版本，
    // 下一次推送就恰好通过 CAS 静默覆盖另一台设备的快照。
    let remote_hash = meta.content_hash.clone();
    let decision = decide_pull(
        &plan.local_hash,
        &plan.state.last_synced_hash,
        plan.local_empty,
        &remote_hash,
        force,
    );
    if decision == PullDecision::Conflict {
        return Ok(SyncOutcome {
            kind: "conflict".into(),
            message: "本机在上次同步之后有改动，云端也有不同的快照。继续拉取会用云端覆盖本机（覆盖前会自动完整备份），是否继续？".into(),
            remote_version: remote.version,
            needs_confirm: true,
        });
    }

    // 拉取成功后本机应有的状态。Unchanged 与 Apply 都要用它，差别只在
    // 「直接提交」还是「随 pending 一起等重启」。
    let mut next_state = plan.state.clone();
    next_state.last_synced_hash = remote_hash.clone();
    next_state.last_sync_at = meta.exported_at.clone();
    next_state.last_sync_from = if meta.device_name.is_empty() {
        meta.device_id.clone()
    } else {
        meta.device_name.clone()
    };
    next_state.remote_version = remote.version;
    // 记住「拉取时本机的样子」，启动替换前会重算比对，防止覆盖拉取后新写的进度
    next_state.pending_local_hash = plan.local_hash.clone();

    if decision == PullDecision::Unchanged {
        // 内容一致也要对齐状态，否则 last_synced_hash 一直停在旧值，
        // 之后每次拉取都会被判成「本地有改动」，界面上还一直显示「还没同步过」
        save_state(&plan.dir, &next_state)?;
        clear_last_error(&plan.dir);
        return Ok(SyncOutcome {
            kind: "unchanged".into(),
            message: "本机内容与云端一致，无需替换".into(),
            remote_version: remote.version,
            needs_confirm: false,
        });
    }

    // 状态绝不能现在就提交：本地库仍是旧的，提前对齐 remote_version 会让
    // 「拉取后未重启就推送」恰好通过 CAS，静默覆盖云端另一台设备的改动。
    // 把待提交状态随 pending 一起落盘，由 apply_pending 成功后才写 .sync-state.json。
    //
    // 顺序也不能反：先 pending 再状态。若 pending 写失败而状态已落盘，下次启动
    // 没有 pending，`commit_pending_state` 会把 remote_version 提到云端版本，
    // 之后的推送就会「正常地」通过 CAS 覆盖掉另一台设备的改动。
    let pending_state_json = serde_json::to_string_pretty(&next_state)?;
    write_pending(&plan.dir, &remote.data)?;
    if let Err(e) = write_private(
        &pending_state_path(&plan.dir),
        pending_state_json.as_bytes(),
    ) {
        // 只有快照没有配套状态，宁可都不要：清掉 pending 让用户可以重试
        let _ = fs::remove_file(pending_path(&plan.dir));
        return Err(e);
    }
    clear_last_error(&plan.dir);
    Ok(SyncOutcome {
        kind: "pending".into(),
        message: "已下载云端快照，重启应用后生效；替换前会自动备份当前库".into(),
        remote_version: remote.version,
        needs_confirm: false,
    })
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::repo::{groups, words};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-sync-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn meta(hash: String) -> SnapshotMeta {
        SnapshotMeta {
            device_id: "dev-a".into(),
            device_name: "书房".into(),
            app_version: "0.2.1".into(),
            // 夹具跟随迁移版本：写死旧版本号会让 prepare_pull 的「旧快照要确认」分支误命中
            user_version: crate::db::latest_user_version(),
            exported_at: "2026-11-05T00:00:00Z".into(),
            content_hash: hash,
        }
    }

    #[test]
    fn content_hash_is_stable_and_changes_with_data() {
        let conn = db::open_memory();
        let a = content_hash(&conn).unwrap();
        assert_eq!(a, content_hash(&conn).unwrap(), "同样的数据哈希必须稳定");

        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let b = content_hash(&conn).unwrap();
        assert_ne!(a, b, "加词后哈希必须变化");
    }

    /// 讲解卡（迁移 0006）必须参与内容哈希：只生成卡片、词一个没动时哈希也要变，
    /// 否则推送会被判成「内容没变」而跳过上传，另一台机器永远拿不到缓存卡。
    #[test]
    fn content_hash_changes_when_ai_card_is_added() {
        let conn = db::open_memory();
        // 先建 group / word 满足 ai_cards.word_id 的外键
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        let w = words::add(&conn, g.id, "碧绿", None).unwrap();
        let before = content_hash(&conn).unwrap();

        crate::repo::ai_cards::upsert(
            &conn,
            w.id,
            "mnemonic",
            &crate::domain::ai_card::CardContent {
                decomposition: vec!["王".into(), "白".into(), "石".into()],
                mnemonic: "老王爷坐在白石头上".into(),
                confusables: vec![],
                sentences: vec!["湖水碧绿得像一块玉石。".into()],
            },
            "test-model",
        )
        .unwrap();

        assert_ne!(
            before,
            content_hash(&conn).unwrap(),
            "加讲解卡后哈希必须变化"
        );
    }

    /// 旧版本程序（TABLES 还只有 4 张表、user_version≤5）算内容哈希的口径。
    /// 行列编码直接复用生产 `hash_table`，只固定旧的表清单：新版程序对旧 schema 库
    /// 算出的哈希必须与它**逐字节一致**，否则旧快照永远过不了 validate_snapshot。
    fn legacy_four_table_hash(conn: &Connection) -> String {
        let mut hasher = Sha256::new();
        for table in ["groups", "words", "sessions", "session_items"] {
            hasher.update(table.as_bytes());
            hasher.update(b":");
            hash_table(&mut hasher, conn, table).unwrap();
            hasher.update(b";");
        }
        hex(&hasher.finalize())
    }

    /// 造一台「旧版本程序」的库：迁移只跑到 0005，库里绝无 ai_cards 表。
    fn old_schema_db(dir: &Path, tag: &str) -> Connection {
        let path = dir.join(format!("{tag}.db"));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .unwrap();
        db::migrate_to(&conn, 5).unwrap();
        conn
    }

    /// 真·旧 schema 快照（迁移只跑到 0005、绝无 ai_cards 表、元数据按旧 4 表口径写）：
    /// 已同步用户升级 / 换机后，新版程序第一次拉取遇到的就是这种文件。
    /// 内容哈希必须按被算库自身 schema 自适应（缺失的表整张连表名段一起跳过），
    /// validate_snapshot 才能通过、「旧版本快照需确认」分支才可达；
    /// 确认落地后再启动，迁移把库补到最新 schema 且业务数据完好。
    #[test]
    fn old_schema_snapshot_validates_confirms_and_migrates() {
        let dir = temp_dir("old-snap");
        // 1. 旧版本程序的库 + 少量业务数据
        let old = old_schema_db(&dir, "old-source");
        let g = groups::create(&old, None, "1", "第一单元").unwrap();
        words::add(&old, g.id, "春天", None).unwrap();

        // 2. 新版对旧 schema 库算的哈希必须与旧程序的 4 表口径逐字节一致
        let legacy_hash = legacy_four_table_hash(&old);
        assert_eq!(
            content_hash(&old).unwrap(),
            legacy_hash,
            "对旧 schema 库必须整表（含表名段）跳过缺失的表，与旧程序口径一致"
        );

        // 3. 按旧程序推快照的真实样子造云端文件：VACUUM INTO + 旧口径元数据
        let snap_path = dir.join("old-remote.db");
        let escaped = snap_path.to_string_lossy().replace('\'', "''");
        old.execute_batch(&format!("VACUUM INTO '{escaped}';"))
            .unwrap();
        {
            let snap = Connection::open(&snap_path).unwrap();
            snap.execute_batch(
                "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )
            .unwrap();
            let m = SnapshotMeta {
                user_version: 5,
                content_hash: legacy_hash.clone(),
                ..meta(String::new())
            };
            snap.execute(
                "INSERT OR REPLACE INTO _sync_meta (key, value) VALUES (?1, ?2)",
                params![META_KEY, serde_json::to_string(&m).unwrap()],
            )
            .unwrap();
        }

        // 4. 新版程序校验这份快照必须通过（修复前：no such table: ai_cards）
        let validated = validate_snapshot(&snap_path, crate::db::latest_user_version()).unwrap();
        assert_eq!(validated.user_version, 5);
        assert_eq!(validated.content_hash, legacy_hash);

        // 5. 拉取路径：命中「旧版本快照需确认」分支，确认前不写 pending
        save_test_creds(&dir);
        let remote = RemoteSnapshot {
            version: 4,
            data: fs::read(&snap_path).unwrap(),
            content_hash: legacy_hash.clone(),
            app_version: "0.2.1".into(),
            user_version: 5,
        };
        let store = FakeStore::new(remote);
        let local = db::open_memory(); // 换机场景：新机的空库
        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "conflict", "旧版本快照必须先确认");
        assert!(out.needs_confirm);
        assert!(!pending_path(&dir).exists());
        let plan = prepare_pull(&local, &dir).unwrap();
        assert_eq!(perform_pull_with(&store, plan, true).unwrap().kind, "pending");

        // 6. 重启落地：库被换成旧快照，再模拟下次启动的迁移 → 最新 schema + 数据完好
        let db_path = dir.join("dictation.db");
        assert!(apply_pending(&dir, &db_path).unwrap());
        let applied = Connection::open(&db_path).unwrap();
        db::migrate_to(&applied, crate::db::latest_user_version())
            .unwrap();
        let uv: i64 = applied
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            uv,
            crate::db::latest_user_version(),
            "落地后必须迁移到最新 schema"
        );
        let words_n: i64 = applied
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(words_n, 1, "业务数据完好");
        let has_ai_cards: i64 = applied
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'ai_cards'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_ai_cards, 1, "迁移 0006 必须补上 ai_cards 表");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 已同步用户升级程序：迁移前的 hash_before 必须在旧 schema 上算得出来（旧 4 表口径），
    /// `rebase_baseline_after_migration` 才能把基准挪到迁移后的哈希——否则升级后第一次
    /// 拉取会被误判「本机有改动」而弹覆盖确认。除 AppHandle 外复刻 `db::init` 的顺序。
    #[test]
    fn rebase_baseline_updates_when_migrating_old_synced_library() {
        let dir = temp_dir("rebase");
        // 旧库 + 业务数据：上次同步的基准是旧程序写下的旧 4 表口径哈希
        let conn = old_schema_db(&dir, "old-local");
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let hash_before = legacy_four_table_hash(&conn);
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-old".into(),
                last_synced_hash: hash_before.clone(),
                ..SyncState::default()
            },
        )
        .unwrap();

        // db::init 的顺序：迁移前对旧库算 hash_before
        //（修复前这里 no such table: ai_cards → .ok() 得 None → rebase 根本不执行）
        let before = content_hash(&conn).unwrap();
        assert_eq!(before, hash_before, "迁移前的哈希必须命中旧同步基准");
        db::migrate_to(&conn, crate::db::latest_user_version())
            .unwrap();
        rebase_baseline_after_migration(&dir, &before, &conn);

        let after = content_hash(&conn).unwrap();
        assert_ne!(after, before, "迁移补上 ai_cards 表后哈希必须变化");
        assert_eq!(
            load_state(&dir).unwrap().last_synced_hash,
            after,
            "rebase 必须把基准挪到迁移后的哈希，首次拉取才不会误判本机有改动"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_roundtrip_and_validation() {
        let dir = temp_dir("snapshot");
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let hash = content_hash(&conn).unwrap();

        let path = create_snapshot(&conn, &dir, &meta(hash.clone())).unwrap();
        assert!(path.exists());
        let m = validate_snapshot(&path, crate::db::latest_user_version()).unwrap();
        assert_eq!(m.content_hash, hash);
        assert_eq!(m.device_name, "书房");

        let snap = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let n: i64 = snap
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "快照必须含完整业务数据");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn credentials_are_private_and_password_not_in_config() {
        let dir = temp_dir("creds");
        let cfg = SyncConfigDto {
            host: "100.91.153.120".into(),
            database: "dictation_sync".into(),
            user: "dictation".into(),
            ..SyncConfigDto::default()
        };
        save_credentials(&dir, &cfg, Some("s3cret".into())).unwrap();

        let creds = load_credentials(&dir).unwrap().unwrap();
        assert_eq!(creds.password, "s3cret");
        // 断言真正回传给前端的那份结构（SyncStatus）里没有密码，而不是自己造一个不含密码的结构体
        let wire = serde_json::to_string(&status(&dir).unwrap()).unwrap();
        assert!(
            !wire.contains("s3cret"),
            "sync_status 的返回值里绝不能出现密码：{wire}"
        );
        assert!(status(&dir).unwrap().has_password);

        // 密码留空表示沿用
        save_credentials(&dir, &cfg, Some(String::new())).unwrap();
        assert_eq!(load_credentials(&dir).unwrap().unwrap().password, "s3cret");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(credentials_path(&dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn device_id_is_stable() {
        let dir = temp_dir("device");
        let a = load_state_with_device(&dir).unwrap().device_id;
        assert_eq!(a.len(), 32);
        assert_eq!(a, load_state_with_device(&dir).unwrap().device_id);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 写一份「拉取时暂存的状态」：既带云端快照的链哈希，也带本机基准哈希
    fn write_pending_state(dir: &Path, chain_hash: &str, local_hash: &str, version: u32) {
        let state = SyncState {
            device_id: "dev-local".into(),
            last_synced_hash: chain_hash.into(),
            remote_version: version,
            pending_local_hash: local_hash.into(),
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(dir),
            serde_json::to_string_pretty(&state).unwrap().as_bytes(),
        )
        .unwrap();
    }

    #[test]
    fn pending_is_applied_with_backup() {
        let dir = temp_dir("pending");
        let source = db::open_memory();
        let g = groups::create(&source, None, "1", "第一单元").unwrap();
        words::add(&source, g.id, "春天", None).unwrap();
        let hash = content_hash(&source).unwrap();
        let snap = create_snapshot(&source, &dir, &meta(hash.clone())).unwrap();

        // 本机库是一份真实业务库（有 groups/words），守卫才能算出哈希
        let local = db::open_memory();
        let lg = groups::create(&local, None, "9", "旧单元").unwrap();
        words::add(&local, lg.id, "夏天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        write_pending_state(&dir, &hash, &local_hash, 2);
        write_pending(&dir, &fs::read(&snap).unwrap()).unwrap();
        assert!(pending_path(&dir).exists());
        assert!(apply_pending(&dir, &db_path).unwrap());
        assert!(!pending_path(&dir).exists(), "应用后 pending 必须消失");

        let applied = Connection::open(&db_path).unwrap();
        let n: i64 = applied
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let backups: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("before-sync"))
            .collect();
        assert_eq!(backups.len(), 1, "替换前必须留一份完整备份");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_pending_is_rejected_without_blocking() {
        let dir = temp_dir("broken");
        let db_path = dir.join("dictation.db");
        write_pending(&dir, b"not a sqlite database").unwrap();
        assert!(apply_pending(&dir, &db_path).is_err());
        assert!(
            !pending_path(&dir).exists(),
            "坏快照要挪走，避免每次启动都失败"
        );
        assert!(dir.join(".sync-last-error.txt").exists(), "失败原因要落盘");
        let rejected: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("pending-rejected")
            })
            .collect();
        assert_eq!(
            rejected.len(),
            1,
            "坏快照要留档供排查（放 backups/sync/，不污染数据目录根）"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：拉取替换后的主库本身带 _sync_meta，必须还能再次导出快照
    #[test]
    fn snapshot_from_already_synced_library_can_be_exported_again() {
        let dir = temp_dir("resnapshot");
        let conn = db::open_memory();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO _sync_meta (key, value) VALUES ('meta', 'stale')",
            [],
        )
        .unwrap();

        let applied = create_snapshot(&conn, &dir, &meta("old-hash".into())).unwrap();
        let applied = Connection::open(&applied).unwrap();
        let hash = content_hash(&applied).unwrap();
        let second = create_snapshot(&applied, &dir, &meta(hash)).unwrap();
        assert_eq!(
            read_snapshot_meta(&second).unwrap().content_hash,
            content_hash(&applied).unwrap()
        );
        assert!(
            validate_snapshot(&second, crate::db::latest_user_version()).is_ok(),
            "_sync_meta 不能算进业务哈希"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_state_commits_only_after_replace() {
        let dir = temp_dir("pending-state");
        let source = db::open_memory();
        let g = groups::create(&source, None, "1", "第一单元").unwrap();
        words::add(&source, g.id, "春天", None).unwrap();
        let hash = content_hash(&source).unwrap();
        let snap = create_snapshot(&source, &dir, &meta(hash.clone())).unwrap();

        // 本机是一份真实业务库（守卫要能算出内容哈希）
        let local = db::open_memory();
        let lg = groups::create(&local, None, "9", "旧单元").unwrap();
        words::add(&local, lg.id, "夏天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();
        write_pending_state(&dir, &hash, &local_hash, 7);
        write_pending(&dir, &fs::read(&snap).unwrap()).unwrap();
        // 待提交状态里已经是 7，但主状态此刻必须还是 0（替换还没发生）
        let staged = fs::read_to_string(pending_state_path(&dir)).unwrap();
        assert!(
            staged.contains("\"remoteVersion\": 7"),
            "待提交状态里应该已经写好目标版本：{staged}"
        );
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "替换前不得提前提交状态"
        );

        assert!(apply_pending(&dir, &db_path).unwrap());
        assert_eq!(load_state(&dir).unwrap().remote_version, 7);
        assert!(!pending_state_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_blocks_further_sync_actions() {
        let dir = temp_dir("block-pending");
        save_test_creds(&dir);
        write_pending(&dir, b"placeholder").unwrap();
        // 走真实入口（prepare_push / prepare_pull），删掉里面的 ensure_no_pending 测试就会红
        let conn = db::open_memory();
        let push_err = prepare_push(&conn, &dir)
            .err()
            .expect("有 pending 时推送必须被拒绝")
            .to_string();
        assert!(push_err.contains("待应用"), "{push_err}");
        let pull_err = prepare_pull(&conn, &dir)
            .err()
            .expect("有 pending 时拉取必须被拒绝")
            .to_string();
        assert!(pull_err.contains("待应用"), "{pull_err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// fail-closed 回归：pending 存在但配套状态没了（写入中断 / 被手工删）时，
    /// 绝不能因为没有守卫就直接替换主库。
    #[test]
    fn pending_without_state_is_rejected_instead_of_replacing() {
        let dir = temp_dir("pending-no-state");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let before = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        let (remote, _) = fake_remote("no-state");
        write_pending(&dir, &remote.data).unwrap();
        // 故意不写 .sync-pending-state.json

        let err = apply_pending(&dir, &db_path).unwrap_err().to_string();
        assert!(err.contains("缺少配套的同步状态"), "{err}");
        assert!(!pending_path(&dir).exists(), "缺守卫的快照要作废");
        let after = content_hash(&Connection::open(&db_path).unwrap()).unwrap();
        assert_eq!(before, after, "主库一个字节都不能动");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 快照与配套状态来自不同次拉取（切页回来又点了一次）时不能替换：
    /// 否则状态会记成另一份快照的版本，之后一次推送就能静默覆盖云端。
    #[test]
    fn pending_and_state_from_different_pulls_are_rejected() {
        let dir = temp_dir("pull-mismatch");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        let (remote, _) = fake_remote("pull-mismatch");
        write_pending_state(&dir, "hash-of-another-pull", &local_hash, 2);
        write_pending(&dir, &remote.data).unwrap();

        let err = apply_pending(&dir, &db_path).unwrap_err().to_string();
        assert!(err.contains("不是同一次拉取"), "{err}");
        assert!(!pending_path(&dir).exists());
        assert!(!pending_state_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 主库是 WAL 模式、还有没 checkpoint 的数据时，替换前必须先 checkpoint 再备份，
    /// 否则旧库会丢数据、备份也不完整（旧测试用 delete 模式，这条分支根本没走过）。
    #[test]
    fn wal_db_is_checkpointed_before_replace() {
        let dir = temp_dir("wal-apply");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);

        // 关掉 autocheckpoint 写一条，让数据停在 -wal 里。
        // 连接必须保持打开：SQLite 在最后一个连接关闭时会自己 checkpoint 掉 WAL。
        let live = Connection::open(&db_path).unwrap();
        live.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;")
            .unwrap();
        words::add(&live, g.id, "夏天", None).unwrap();
        let local_hash = content_hash(&live).unwrap();
        assert!(
            dir.join("dictation.db-wal").exists(),
            "前置条件：数据要还在 WAL 里"
        );

        let (remote, chain_hash) = fake_remote("wal");
        write_pending_state(&dir, &chain_hash, &local_hash, 2);
        write_pending(&dir, &remote.data).unwrap();

        assert!(apply_pending(&dir, &db_path).unwrap());
        drop(live);
        assert!(
            !dir.join("dictation.db-wal").exists(),
            "替换前必须清掉旧 WAL，否则会污染新库"
        );
        // 备份里必须有那条还在 WAL 里的词，否则等于 checkpoint 没做
        let backup = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("before-sync"))
            .expect("必须留一份替换前备份");
        let backup = Connection::open(backup.path()).unwrap();
        let n: i64 = backup
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "WAL 里已提交的数据必须进备份");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 测试连接用「传进来的配置」而不是已保存的配置：
    /// 否则用户为了试一个错端口，会把已经能用的凭据覆盖掉。
    #[test]
    fn test_connection_prefers_passed_config() {
        let dir = temp_dir("test-conn");
        let good = SyncConfigDto {
            host: "100.91.153.120".into(),
            database: "dictation_sync".into(),
            user: "dictation".into(),
            ..SyncConfigDto::default()
        };
        save_credentials(&dir, &good, Some("pass".into())).unwrap();

        // 传一份空主机的配置：必须立刻在本地校验处失败，而不是去连已保存的那个地址
        let broken = SyncConfigDto {
            host: String::new(),
            ..good.clone()
        };
        let err = test_connection(&dir, Some(broken), Some("other".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("请先填写主机"), "{err}");
        // 已保存的凭据不能被这次测试改坏
        let saved = load_credentials(&dir).unwrap().unwrap();
        assert_eq!(saved.password, "pass");
        assert_eq!(saved.config.host, "100.91.153.120");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：库已替换但状态没写完时，下次启动（无 pending）必须补提交；
    /// 补提交成功后那条「状态没写成」的提示就该消失（问题已自愈）。
    #[test]
    fn startup_without_pending_commits_orphan_state() {
        let dir = temp_dir("orphan-state");
        // 主库确实已经是那份快照（_sync_meta 记着它的哈希），补提交才有依据
        let applied = db::open_memory();
        let g = groups::create(&applied, None, "1", "第一单元").unwrap();
        words::add(&applied, g.id, "春天", None).unwrap();
        let hash = content_hash(&applied).unwrap();
        let snap = create_snapshot(&applied, &dir, &meta(hash.clone())).unwrap();
        let db_path = dir.join("dictation.db");
        fs::rename(&snap, &db_path).unwrap();
        let db_before = fs::read(&db_path).unwrap();
        fs::write(last_error_path(&dir), "上次写状态失败").unwrap();
        let next = SyncState {
            device_id: "dev-fixed".into(),
            last_synced_hash: hash,
            remote_version: 9,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();

        assert!(
            !apply_pending(&dir, &db_path).unwrap(),
            "没有 pending 就不该动主库"
        );
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            9,
            "状态必须补提交"
        );
        assert_eq!(load_state(&dir).unwrap().device_id, "dev-fixed");
        assert!(!pending_state_path(&dir).exists());
        assert!(
            status(&dir).unwrap().last_error.is_empty(),
            "补提交成功后提示要清掉，别一直挂着一句已经过期的报错"
        );
        assert_eq!(fs::read(&db_path).unwrap(), db_before, "补提交不能改动主库");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 反向回归：「没有 pending」不等于「库已经替换过」。用户手工删掉 pending.db
    /// （或写入中断）时，绝不能把 remote_version 对齐到云端，否则下次推送会静默覆盖云端。
    #[test]
    fn orphan_state_is_not_committed_when_library_unchanged() {
        let dir = temp_dir("orphan-unchanged");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let unchanged_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        // 状态里说「已拉到云端版本 9」，但本机库的 _sync_meta 并不是那份快照
        let next = SyncState {
            remote_version: 9,
            last_synced_hash: "cloud-hash-not-in-this-db".into(),
            pending_local_hash: unchanged_hash,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();

        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "库没被替换，就绝不能提前对齐云端版本"
        );
        assert!(!pending_state_path(&dir).exists(), "这条错误记录要作废");
        let rejected: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sync-pending-state-rejected")
            })
            .collect();
        assert_eq!(rejected.len(), 1, "作废的记录要留档");
        assert!(
            !status(&dir).unwrap().last_error.is_empty(),
            "要让用户看到为什么没提交"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：期间又成功推送过时，补提交不能把主状态回退到拉取时的旧版本。
    #[test]
    fn orphan_state_never_rolls_back_newer_state() {
        let dir = temp_dir("orphan-newer");
        let db_path = dir.join("dictation.db");
        let current = SyncState {
            remote_version: 12,
            ..SyncState::default()
        };
        save_state(&dir, &current).unwrap();
        let stale = SyncState {
            remote_version: 9,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&stale).unwrap().as_bytes(),
        )
        .unwrap();

        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            12,
            "新状态不能被回退"
        );
        assert!(!pending_state_path(&dir).exists(), "过期待提交状态要作废");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_pending_clears_residue_without_touching_state() {
        let dir = temp_dir("discard");
        let state = SyncState {
            remote_version: 4,
            ..SyncState::default()
        };
        save_state(&dir, &state).unwrap();
        write_pending(&dir, b"placeholder").unwrap();
        write_private(&pending_state_path(&dir), b"{}").unwrap();
        fs::write(dir.join(".sync-last-error.txt"), "oops").unwrap();

        discard_pending(&dir).unwrap();

        assert!(!pending_path(&dir).exists());
        assert!(!pending_state_path(&dir).exists());
        assert!(!dir.join(".sync-last-error.txt").exists());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            4,
            "主状态不能被重置"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：坏状态文件不能让整个设置页打不开，但推送/拉取仍必须严格拦住。
    #[test]
    fn status_degrades_but_sync_actions_still_reject_corrupt_state() {
        let dir = temp_dir("corrupt-state");
        fs::write(state_path(&dir), b"{not json").unwrap();

        let s = status(&dir).unwrap();
        assert!(s.warning.contains("同步状态文件损坏"), "告警要能展示给用户");
        assert!(s.state_corrupt);
        assert!(load_state(&dir).is_err(), "解析必须严格报错");

        // 真正拦住用户的是入口命令，不是 load_state：给推送路径加 unwrap_or_default 兜底，
        // 下面两条断言必须红
        save_test_creds(&dir);
        let conn = db::open_memory();
        let push_err = prepare_push(&conn, &dir)
            .err()
            .expect("状态损坏时推送必须被拒绝")
            .to_string();
        assert!(push_err.contains("状态文件损坏"), "{push_err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_credentials_can_be_repaired_by_saving_again() {
        let dir = temp_dir("corrupt-creds");
        fs::write(credentials_path(&dir), b"{broken").unwrap();
        let cfg = SyncConfigDto {
            host: "100.91.153.120".into(),
            database: "dictation_sync".into(),
            user: "dictation".into(),
            ..SyncConfigDto::default()
        };

        save_credentials(&dir, &cfg, Some("new-pass".into())).unwrap();
        assert_eq!(
            load_credentials(&dir).unwrap().unwrap().password,
            "new-pass"
        );
        assert!(status(&dir).unwrap().warning.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mysql_store_rejects_zero_port() {
        let cfg = SyncConfigDto {
            host: "127.0.0.1".into(),
            database: "dictation_sync".into(),
            user: "dictation".into(),
            port: 0,
            ..SyncConfigDto::default()
        };
        assert!(MysqlStore::new(&cfg, "pass").is_err());
    }

    /// 回归：替换失败的原因必须回显给用户，且不能在下次启动被自动清掉。
    #[test]
    fn replacement_failure_is_visible_and_not_auto_cleared() {
        let dir = temp_dir("last-error");
        let db_path = dir.join("dictation.db");
        write_pending(&dir, b"not a sqlite database").unwrap();
        assert!(apply_pending(&dir, &db_path).is_err());

        let first = status(&dir).unwrap();
        assert!(!first.last_error.is_empty(), "失败原因要回显到设置页");
        assert!(first.warning.is_empty(), "诊断提示不该禁用同步");
        assert!(!first.pending);

        // 下次启动没有 pending，也不能悄悄把错误抹掉
        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert!(!status(&dir).unwrap().last_error.is_empty());

        discard_pending(&dir).unwrap();
        assert!(
            status(&dir).unwrap().last_error.is_empty(),
            "用户可手动清除"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：损坏的状态文件有受控恢复出口，旧文件留档而不是删掉。
    #[test]
    fn repair_state_quarantines_corrupt_file() {
        let dir = temp_dir("repair-state");
        fs::write(state_path(&dir), b"{not json").unwrap();
        assert!(status(&dir).unwrap().state_corrupt);

        let s = repair_state(&dir).unwrap();
        assert!(!s.state_corrupt);
        assert!(s.warning.is_empty());
        assert_eq!(load_state(&dir).unwrap().remote_version, 0);
        assert_eq!(load_state(&dir).unwrap().device_id.len(), 32);
        let quarantined: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sync-state-corrupt")
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "损坏文件要留档供排查");
        // 留档不能混进「数据与备份」列表：那里只列 backups/ 顶层的可导入 JSON
        assert!(
            crate::repo::backup::list(&dir).unwrap().is_empty(),
            "同步留档不能出现在可导入的备份列表里"
        );
        assert!(repair_state(&dir).is_err(), "状态正常时不能重复重置");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：无 pending 的孤儿待提交状态不能被「放弃快照」误删，启动还要能补提交。
    #[test]
    fn discard_pending_keeps_orphan_state_for_startup() {
        let dir = temp_dir("orphan-discard");
        // 库确实已经是那份快照（_sync_meta 记着它的哈希），补提交才有依据
        let applied = db::open_memory();
        let g = groups::create(&applied, None, "1", "第一单元").unwrap();
        words::add(&applied, g.id, "春天", None).unwrap();
        let hash = content_hash(&applied).unwrap();
        let snap = create_snapshot(&applied, &dir, &meta(hash.clone())).unwrap();
        let db_path = dir.join("dictation.db");
        fs::rename(&snap, &db_path).unwrap();
        let next = SyncState {
            device_id: "dev-fixed".into(),
            last_synced_hash: hash,
            remote_version: 6,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();

        discard_pending(&dir).unwrap();
        assert!(
            pending_state_path(&dir).exists(),
            "孤儿状态要留给启动补提交"
        );
        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert_eq!(load_state(&dir).unwrap().remote_version, 6);
        assert!(!pending_state_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 回归：私有文件原子写不留 .tmp 垃圾，内容仍完整。
    #[test]
    fn private_write_is_clean_and_complete() {
        let dir = temp_dir("atomic-write");
        let state = SyncState {
            last_sync_from: "书房电脑".into(),
            ..SyncState::default()
        };
        save_state(&dir, &state).unwrap();
        assert_eq!(load_state(&dir).unwrap().last_sync_from, "书房电脑");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件必须被改名或清理");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 界面偏好（字号 / 遮罩秒数）不参与内容哈希：否则两台机器字号不同，
    /// 每次拉取都会被判成「本机有改动」而弹覆盖确认。
    #[test]
    fn content_hash_ignores_ui_settings() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let before = content_hash(&conn).unwrap();

        crate::repo::settings::save(
            &conn,
            &crate::repo::settings::SettingsDto {
                word_font_size: 120,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(before, content_hash(&conn).unwrap(), "改字号不算内容改动");
    }

    #[test]
    fn decide_pull_covers_first_sync_conflict_and_unchanged() {
        // 新装第二台：从没同步过 + 空库 → 直接替换，不该弹确认
        assert_eq!(
            decide_pull("local", "", true, "remote", false),
            PullDecision::Apply
        );
        // 从没同步过但本机已经录过词 → 必须确认
        assert_eq!(
            decide_pull("local", "", false, "remote", false),
            PullDecision::Conflict
        );
        // 已同步过、之后本地没改 → 直接替换（云端更新）
        assert_eq!(
            decide_pull("local", "local", false, "remote", false),
            PullDecision::Apply
        );
        // 已同步过、本地改过、云端也不同 → 冲突
        assert_eq!(
            decide_pull("local2", "local", false, "remote", false),
            PullDecision::Conflict
        );
        // 用户确认后强制覆盖 → 放行
        assert_eq!(
            decide_pull("local2", "local", false, "remote", true),
            PullDecision::Apply
        );
        // 两边内容相同 → 只对齐状态
        assert_eq!(
            decide_pull("same", "other", false, "same", false),
            PullDecision::Unchanged
        );
    }

    fn save_test_creds(dir: &Path) {
        let cfg = SyncConfigDto {
            enabled: true,
            host: "127.0.0.1".into(),
            database: "dictation_sync".into(),
            user: "dictation".into(),
            ..Default::default()
        };
        save_credentials(dir, &cfg, Some("pass".into())).unwrap();
    }

    /// 首次推送必须把刚生成的 device_id 留在状态里（曾经的 bug：回写时被空串覆盖），
    /// 并且 `PushPlan` 被丢弃时（构造 `MysqlStore` 失败 / 任务 panic）不能把整库快照垃圾留在数据目录。
    #[test]
    fn push_plan_keeps_device_id_and_cleans_snapshot() {
        let dir = temp_dir("push-plan");
        // host 留空：prepare_push 能过，MysqlStore::new 必然失败
        save_test_creds(&dir);
        let conn = db::open_memory();

        let plan = prepare_push(&conn, &dir).unwrap();
        assert_eq!(
            load_state(&dir).unwrap().device_id.len(),
            32,
            "首次推送要落盘 device_id"
        );
        // 连接失败／任务 panic 时 plan 会被丢弃，快照必须跟着清掉（不能连网络，直接 drop）
        drop(plan);

        assert_eq!(
            load_state(&dir).unwrap().device_id.len(),
            32,
            "丢弃 plan 也不能把 device_id 抹掉"
        );
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".snapshot-"))
            .collect();
        assert!(leftovers.is_empty(), "失败路径要清掉快照临时文件");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 假云端：只为覆盖 CAS / 冲突判定，不碰网络。
    struct FakeStore {
        row: std::sync::Mutex<Option<RemoteSnapshot>>,
        /// 强推场景专用：从第 N 次 push 起，无论 expected 是多少都先把版本跳到该值
        /// 再返回 Conflict——模拟「重推窗口内另一台设备又推了一把」。
        /// None = 不注入。
        bump_from_nth_push: std::sync::Mutex<Option<(usize, u32)>>,
        push_count: std::sync::Mutex<usize>,
    }

    impl FakeStore {
        fn new(remote: RemoteSnapshot) -> Self {
            Self {
                row: std::sync::Mutex::new(Some(remote)),
                bump_from_nth_push: std::sync::Mutex::new(None),
                push_count: std::sync::Mutex::new(0),
            }
        }

        /// 让第 N 次 push（1 起）先经历一次「版本已被别人推进」
        fn bump_on_nth_push(&self, nth: usize, to: u32) {
            *self.bump_from_nth_push.lock().unwrap() = Some((nth, to));
        }
    }

    impl SnapshotStore for FakeStore {
        fn fetch(&self) -> AppResult<RemoteSnapshot> {
            Ok(self.row.lock().unwrap().clone().unwrap())
        }
        fn push(&self, expected: u32, data: &[u8], meta: &SnapshotMeta) -> AppResult<PushOutcome> {
            let mut guard = self.row.lock().unwrap();
            // 注意：std::sync::Mutex 不可重入，先显式拿 guard 再读/清，
            // 不能写成 if-let 直连 lock() 的写法——临时 guard 会活到 if-let 块结束，
            // 块内再 lock 就是死锁。
            {
                let mut count = self.push_count.lock().unwrap();
                *count += 1;
                let bump = self.bump_from_nth_push.lock().unwrap();
                if let Some((nth, to)) = *bump {
                    if *count >= nth {
                        let cur = guard.as_mut().unwrap();
                        cur.version = to;
                        drop(bump);
                        let mut cfg = self.bump_from_nth_push.lock().unwrap();
                        *cfg = None;
                        return Ok(PushOutcome::Conflict(to));
                    }
                }
            }
            let cur = guard.as_mut().unwrap();
            if cur.version != expected {
                return Ok(PushOutcome::Conflict(cur.version));
            }
            cur.version += 1;
            cur.data = data.to_vec();
            cur.content_hash = meta.content_hash.clone();
            Ok(PushOutcome::Applied(cur.version))
        }
        fn server_version(&self) -> AppResult<String> {
            Ok("8.0.43".into())
        }
    }

    /// 造一份「云端已有数据」的快照，并返回可直接塞进 FakeStore 的 RemoteSnapshot。
    fn fake_remote(tag: &str) -> (RemoteSnapshot, String) {
        fake_remote_with_uv(tag, crate::db::latest_user_version())
    }

    /// 造一份「云端快照」，`uv` 是快照元数据里声称的数据版本
    fn fake_remote_with_uv(tag: &str, uv: i64) -> (RemoteSnapshot, String) {
        let dir = temp_dir(&format!("remote-{tag}"));
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let hash = content_hash(&conn).unwrap();
        let snap = create_snapshot(
            &conn,
            &dir,
            &SnapshotMeta {
                user_version: uv,
                ..meta(hash.clone())
            },
        )
        .unwrap();
        let data = fs::read(&snap).unwrap();
        let _ = fs::remove_dir_all(&dir);
        (
            RemoteSnapshot {
                version: 3,
                data,
                content_hash: hash.clone(),
                app_version: "0.2.1".into(),
                user_version: crate::db::latest_user_version(),
            },
            hash,
        )
    }

    /// 新装第二台（空库、从没同步过）第一次拉取：不该弹「覆盖本机」，
    /// 但要真的写 pending + 待提交状态，并且重启前不得提前对齐版本。
    #[test]
    fn first_pull_on_fresh_device_applies_without_conflict() {
        let dir = temp_dir("pull-fresh");
        save_test_creds(&dir);
        let (remote, remote_hash) = fake_remote("fresh");
        let store = FakeStore::new(remote);
        let local = db::open_memory();

        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "pending", "新设备首次拉取不该要求确认");
        assert!(pending_path(&dir).exists());
        assert!(pending_state_path(&dir).exists());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "重启前绝不能提前对齐云端版本"
        );

        // 模拟下次启动：替换主库并提交状态
        let db_path = dir.join("dictation.db");
        assert!(apply_pending(&dir, &db_path).unwrap());
        let applied = load_state(&dir).unwrap();
        assert_eq!(applied.remote_version, 3);
        assert_eq!(applied.last_synced_hash, remote_hash);
        assert_eq!(applied.last_sync_from, "书房");
        assert!(!pending_state_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 本机录过词但从未同步 → 必须先确认；force 才写 pending。
    #[test]
    fn pull_on_unsynced_library_requires_confirm() {
        let dir = temp_dir("pull-conflict");
        save_test_creds(&dir);
        let (remote, _) = fake_remote("conflict");
        let store = FakeStore::new(remote);
        let local = db::open_memory();
        let g = groups::create(&local, None, "9", "本地单元").unwrap();
        words::add(&local, g.id, "秋天", None).unwrap();

        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "conflict");
        assert!(out.needs_confirm);
        assert!(!pending_path(&dir).exists(), "确认前不能动 pending");

        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, true).unwrap();
        assert_eq!(out.kind, "pending");
        assert!(pending_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 两边内容一致时只对齐状态：`last_synced_hash / last_sync_at` 必须补上，
    /// 否则之后每次拉取都会被当成「本地有改动」，界面还一直显示「还没同步过」。
    #[test]
    fn unchanged_pull_aligns_state_without_replace() {
        let dir = temp_dir("pull-unchanged");
        save_test_creds(&dir);
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let hash = content_hash(&local).unwrap();
        // 云端就是本机的内容（推送后再拉取的场景）
        let snap = create_snapshot(&local, &dir, &meta(hash.clone())).unwrap();
        let remote = RemoteSnapshot {
            version: 5,
            data: fs::read(&snap).unwrap(),
            content_hash: hash.clone(),
            app_version: "0.2.1".into(),
            user_version: crate::db::latest_user_version(),
        };
        let _ = fs::remove_file(&snap);
        let store = FakeStore::new(remote);

        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "unchanged");
        assert!(!pending_path(&dir).exists());
        let state = load_state(&dir).unwrap();
        assert_eq!(state.remote_version, 5);
        assert_eq!(
            state.last_synced_hash, hash,
            "必须记住基准，别再误判本地有改动"
        );
        assert!(!state.last_sync_at.is_empty());
        assert_eq!(state.last_sync_from, "书房");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 把一份内存库落到磁盘，当作「本机 dictation.db」
    fn file_db_from(dir: &Path, conn: &Connection) -> PathBuf {
        let hash = content_hash(conn).unwrap();
        let snap = create_snapshot(conn, dir, &meta(hash)).unwrap();
        let db_path = dir.join("dictation.db");
        fs::rename(&snap, &db_path).unwrap();
        db_path
    }

    /// 本机在拉取后没有改动 → 启动时正常替换。
    #[test]
    fn pending_is_applied_when_local_unchanged_since_pull() {
        let dir = temp_dir("apply-guard-ok");
        save_test_creds(&dir);
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        // 这台设备之前同步过、之后没改：基准哈希等于当前内容
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                last_synced_hash: content_hash(&local).unwrap(),
                ..SyncState::default()
            },
        )
        .unwrap();

        let (remote, _) = fake_remote("guard-ok");
        let store = FakeStore::new(remote);
        let plan = prepare_pull(&local, &dir).unwrap();
        assert_eq!(perform_pull_with(&store, plan, false).unwrap().kind, "pending");

        assert!(apply_pending(&dir, &db_path).unwrap(), "本机没变就该替换");
        assert_eq!(load_state(&dir).unwrap().remote_version, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 拉取之后家长又默写了一段 → 绝不能用旧快照盖掉新写的进度。
    #[test]
    fn pull_then_local_edit_cancels_apply() {
        let dir = temp_dir("apply-guard-edit");
        save_test_creds(&dir);
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                last_synced_hash: content_hash(&local).unwrap(),
                ..SyncState::default()
            },
        )
        .unwrap();

        let (remote, _) = fake_remote("guard-edit");
        let store = FakeStore::new(remote);
        let plan = prepare_pull(&local, &dir).unwrap();
        assert_eq!(perform_pull_with(&store, plan, false).unwrap().kind, "pending");

        // 拉取之后本机又加了一个词（真实场景：家长没重启就继续默写）
        {
            let live = Connection::open(&db_path).unwrap();
            let gid: i64 = live
                .query_row("SELECT id FROM groups LIMIT 1", [], |r| r.get(0))
                .unwrap();
            words::add(&live, gid, "秋天", None).unwrap();
        }

        let err = apply_pending(&dir, &db_path).unwrap_err().to_string();
        assert!(err.contains("又有改动"), "错误要讲清原因：{err}");
        assert!(
            !pending_path(&dir).exists(),
            "这份 pending 要作废，否则每次启动都失败"
        );
        let live = Connection::open(&db_path).unwrap();
        let n: i64 = live
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "本机新写的词必须还在");
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "被取消应用时不能提前对齐云端版本"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 重置状态时必须把孤儿待提交状态一起隔离，否则下次启动又把它写回去。
    #[test]
    fn repair_state_also_quarantines_orphan_pending_state() {
        let dir = temp_dir("repair-orphan");
        fs::write(state_path(&dir), b"{not json").unwrap();
        let orphan = SyncState {
            remote_version: 9,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&orphan).unwrap().as_bytes(),
        )
        .unwrap();

        let s = repair_state(&dir).unwrap();
        assert!(!s.state_corrupt);
        assert!(!pending_state_path(&dir).exists(), "孤儿状态必须一起隔离");
        assert_eq!(load_state(&dir).unwrap().remote_version, 0);

        let db_path = dir.join("dictation.db");
        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "下次启动不能把任务版本又补提交回来"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 补提交遇到坏库（NOTADB/CORRUPT）或主库不存在：作废待提交状态并写明原因。
    /// 与锁库分支对照：锁库保留重试，坏库/无库作废。
    #[test]
    fn corrupt_or_missing_library_rejects_pending_state() {
        // a) 主库文件不存在
        let dir = temp_dir("commit-nolib");
        let next = SyncState {
            remote_version: 9,
            last_synced_hash: "some-hash".into(),
            pending_local_hash: "base".into(),
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();
        assert!(!commit_pending_state(&dir, &dir.join("dictation.db")));
        assert!(!pending_state_path(&dir).exists(), "作废后文件要消失");
        let err = status(&dir).unwrap().last_error;
        assert!(err.contains("并不存在"), "文案要能指导排查：{err}");
        let _ = fs::remove_dir_all(&dir);

        // b) 主库是坏文件（不是数据库）
        let dir = temp_dir("commit-badlib");
        let db_path = dir.join("dictation.db");
        fs::write(&db_path, b"definitely not a sqlite file").unwrap();
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();
        assert!(!commit_pending_state(&dir, &db_path));
        assert!(!pending_state_path(&dir).exists());
        let err = status(&dir).unwrap().last_error;
        assert!(
            err.contains("已作废"),
            "坏库场景要让用户知道记录被作废了：{err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 「重置同步状态」的留档也要参与裁剪：连做多次只保留最近几份。
    #[test]
    fn corrupt_state_archives_are_pruned() {
        let dir = temp_dir("repair-prune");
        let backups = sync_backup_dir(&dir).unwrap();
        for i in 1..=7 {
            fs::write(
                backups.join(format!("sync-state-corrupt-{i}-1.json")),
                b"x",
            )
            .unwrap();
        }
        // repair_state 前置条件：状态文件确实损坏
        fs::write(state_path(&dir), "{ not json").unwrap();
        repair_state(&dir).unwrap();
        let left: Vec<_> = fs::read_dir(&backups)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("sync-state-corrupt-"))
            .collect();
        assert_eq!(
            left.len(),
            KEEP_SYNC_BACKUPS,
            "corrupt 留档也要裁剪：{left:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 补提交时主库只是被别的连接锁住（不是坏）：待提交状态必须保留，下次启动再试。
    /// 曾经的 bug：读不出哈希一律当「没替换过」作废，此后每次推送都弹冲突，
    /// 用户被训练成习惯性点「继续覆盖」，另一台设备的快照反而真的会被盖掉。
    #[test]
    fn locked_library_keeps_pending_state_for_retry() {
        let dir = temp_dir("commit-locked");
        // 主库确实已经是那份快照
        let applied = db::open_memory();
        let g = groups::create(&applied, None, "1", "第一单元").unwrap();
        words::add(&applied, g.id, "春天", None).unwrap();
        let hash = content_hash(&applied).unwrap();
        let snap = create_snapshot(&applied, &dir, &meta(hash.clone())).unwrap();
        let db_path = dir.join("dictation.db");
        fs::rename(&snap, &db_path).unwrap();

        let local_before = db::open_memory(); // 拉取前的旧库，内容哈希必然不同
        let base_hash = content_hash(&local_before).unwrap();
        let next = SyncState {
            remote_version: 9,
            last_synced_hash: hash,
            pending_local_hash: base_hash,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();

        // 拿一把排他锁模拟「主库被杀毒/备份软件占着」
        let holder = Connection::open(&db_path).unwrap();
        holder
            .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE;")
            .unwrap();
        assert!(
            !commit_pending_state(&dir, &db_path),
            "锁库时读不出哈希不能当成「没替换过」"
        );
        assert!(
            pending_state_path(&dir).exists(),
            "待提交状态必须保留，下次启动再试"
        );
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "版本号不能在锁库时被误判后提前对齐"
        );
        let _ = holder.execute_batch("ROLLBACK;");
        drop(holder);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 反例：库的 `_sync_meta` 确实记着这份快照，但本机内容和「拉取前」也一模一样
    /// （两次拉取内容相同的快照）→ 说明这次替换根本没落地，不能补提交。
    #[test]
    fn orphan_state_is_not_committed_when_content_matches_baseline() {
        let dir = temp_dir("orphan-same-content");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let hash = content_hash(&local).unwrap();
        let snap = create_snapshot(&local, &dir, &meta(hash.clone())).unwrap();
        let db_path = dir.join("dictation.db");
        fs::rename(&snap, &db_path).unwrap();

        // marker == last_synced_hash（看着像替换过），但内容 == pending_local_hash（其实没换）
        let next = SyncState {
            remote_version: 9,
            last_synced_hash: hash.clone(),
            pending_local_hash: hash,
            ..SyncState::default()
        };
        write_private(
            &pending_state_path(&dir),
            serde_json::to_string(&next).unwrap().as_bytes(),
        )
        .unwrap();

        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            0,
            "内容没变过就不能当成「已替换」而提前对齐云端版本"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 本机库已经坏了的时候，拉取的快照就是唯一的恢复手段：
    /// 不能因为「算不出本机哈希」就把快照作废。
    #[test]
    fn corrupt_local_db_is_recovered_from_pending() {
        let dir = temp_dir("recover-corrupt");
        let db_path = dir.join("dictation.db");
        fs::write(&db_path, b"this is definitely not a sqlite database").unwrap();

        let (remote, chain_hash) = fake_remote("recover");
        // 坏库算不出基准，给个占位值；关键是不能因此拒绝替换
        write_pending_state(&dir, &chain_hash, "unreadable", 2);
        write_pending(&dir, &remote.data).unwrap();

        assert!(
            apply_pending(&dir, &db_path).unwrap(),
            "库坏了必须能用云端快照救回来"
        );
        let recovered = Connection::open(&db_path).unwrap();
        let n: i64 = recovered
            .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "主库应该已经是云端快照的内容");
        // 坏库要留档，用户可能还想从那堆字节里捞东西
        let archived: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("before-sync"))
            .collect();
        assert_eq!(archived.len(), 1, "坏库也要留一份");
        let note = status(&dir).unwrap().last_error;
        assert!(
            note.contains("已用下载好的云端快照替换"),
            "替换原因必须留给用户看见：{note}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 「读不出来」必须按错误码区分：锁库 = 数据还在（只能重试），
    /// 不是数据库 / 表页损坏 / 空库没表 = 本机没有可用数据（可以拿云端快照救）。
    /// 用 integrity_check 反推会把「忙」误判成「坏」，那样会跳过守卫覆盖本机数据。
    #[test]
    fn local_db_failure_is_classified_by_error_code() {
        use rusqlite::ffi::Error as FfiError;
        let db_err = |code: rusqlite::ErrorCode, extended: i32, msg: &str| {
            AppError::Db(rusqlite::Error::SqliteFailure(
                FfiError { code, extended_code: extended },
                Some(msg.to_string()),
            ))
        };
        assert_eq!(
            classify_local_failure(&db_err(rusqlite::ErrorCode::DatabaseBusy, 5, "database is locked")),
            LocalDbFailure::Unusable
        );
        assert_eq!(
            classify_local_failure(&db_err(rusqlite::ErrorCode::DatabaseLocked, 6, "locked")),
            LocalDbFailure::Unusable
        );
        assert_eq!(
            classify_local_failure(&db_err(
                rusqlite::ErrorCode::NotADatabase,
                26,
                "file is not a database"
            )),
            LocalDbFailure::Corrupt
        );
        assert_eq!(
            classify_local_failure(&db_err(rusqlite::ErrorCode::DatabaseCorrupt, 11, "malformed")),
            LocalDbFailure::Corrupt
        );
        // 空库 / 新库：没有业务表，本机没有数据可丢
        assert_eq!(
            classify_local_failure(&db_err(rusqlite::ErrorCode::Unknown, 1, "no such table: groups")),
            LocalDbFailure::Corrupt
        );
        // 其它未知错误保守按「暂时读不出来」处理，保留 pending
        assert_eq!(
            classify_local_failure(&db_err(rusqlite::ErrorCode::Unknown, 1, "something else")),
            LocalDbFailure::Unusable
        );
        assert_eq!(
            classify_local_failure(&AppError::msg("任意其它错误")),
            LocalDbFailure::Unusable
        );
    }

    /// 升级跑迁移只会改 schema，不能让「本机有改动」误判；基准要跟着挪。
    #[test]
    fn migration_rebases_sync_baseline() {
        let dir = temp_dir("rebase");
        let old_conn = db::open_memory();
        let hash_before = content_hash(&old_conn).unwrap();
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                last_synced_hash: hash_before.clone(),
                ..SyncState::default()
            },
        )
        .unwrap();

        // 迁移后的库内容多了一行（模拟加列导致哈希变化）
        let new_conn = db::open_memory();
        let g = groups::create(&new_conn, None, "1", "第一单元").unwrap();
        words::add(&new_conn, g.id, "春天", None).unwrap();
        let hash_after = content_hash(&new_conn).unwrap();
        assert_ne!(hash_before, hash_after);

        rebase_baseline_after_migration(&dir, &hash_before, &new_conn);
        assert_eq!(
            load_state(&dir).unwrap().last_synced_hash,
            hash_after,
            "迁移后基准要跟着换，否则第一次拉取会被判成「本机有改动」"
        );

        // 反例：迁移前数据就变过（基准对不上），不能把真实改动洗掉
        let dir2 = temp_dir("rebase-keep");
        save_state(
            &dir2,
            &SyncState {
                device_id: "dev-local".into(),
                last_synced_hash: "another-hash".into(),
                ..SyncState::default()
            },
        )
        .unwrap();
        rebase_baseline_after_migration(&dir2, &hash_before, &new_conn);
        assert_eq!(load_state(&dir2).unwrap().last_synced_hash, "another-hash");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }

    /// 快照内容被改过（哈希对不上）时必须拒收。
    #[test]
    fn tampered_snapshot_is_rejected() {
        let dir = temp_dir("tampered");
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let hash = content_hash(&conn).unwrap();
        let snap = create_snapshot(&conn, &dir, &meta(hash)).unwrap();
        {
            let s = Connection::open(&snap).unwrap();
            s.execute("UPDATE words SET text = '夏天'", []).unwrap();
        }
        let err = validate_snapshot(&snap, crate::db::latest_user_version())
            .unwrap_err()
            .to_string();
        assert!(err.contains("不一致"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 原子写失败时不能留下半截临时文件。
    #[test]
    fn private_write_failure_leaves_no_temp() {
        let dir = temp_dir("atomic-fail");
        let target = dir.join("state.json");
        fs::create_dir_all(&target).unwrap(); // 目标是个目录 → rename 必然失败
        assert!(write_private(&target, b"{}").is_err());
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "失败后要清掉临时文件");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 瞬时失败（备份目录写不进去）必须保留 pending，让下次启动重试；
    /// 早期实现会把这种情况也当成「快照坏了」直接作废。
    #[test]
    fn retryable_failure_keeps_pending_for_next_start() {
        let dir = temp_dir("retry-apply");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();
        let before = fs::read(&db_path).unwrap();

        // 让 backups/sync 变成一个普通文件：建目录必然失败
        let backups = crate::repo::backup::backup_dir(&dir);
        fs::create_dir_all(&backups).unwrap();
        fs::write(backups.join("sync"), b"not a dir").unwrap();

        let (remote, chain_hash) = fake_remote("retry");
        write_pending_state(&dir, &chain_hash, &local_hash, 2);
        write_pending(&dir, &remote.data).unwrap();

        let err = apply_pending(&dir, &db_path).unwrap_err().to_string();
        assert!(!err.is_empty());
        assert!(pending_path(&dir).exists(), "瞬时失败不能把快照扔掉");
        assert!(pending_state_path(&dir).exists(), "配套状态也要留着");
        assert_eq!(fs::read(&db_path).unwrap(), before, "主库不能被动过");
        let last_error = status(&dir).unwrap().last_error;
        assert!(last_error.contains("重试"), "要告诉用户下次启动会重试：{last_error}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 崩溃/强杀留下的临时文件要在启动时清掉（快照是整库大小，会越堆越多）
    #[test]
    fn startup_cleans_stale_temp_files() {
        let dir = temp_dir("stale-temp");
        let db_path = dir.join("dictation.db");
        let snapshot = dir.join(".snapshot-123-456.db");
        let tmp_state = dir.join(".sync-state.json.tmp-789");
        fs::write(&snapshot, b"junk").unwrap();
        fs::write(&tmp_state, b"junk").unwrap();
        // 正常文件不能被误删
        let keep = dir.join(".sync-state.json");
        fs::write(&keep, b"{}").unwrap();

        assert!(!apply_pending(&dir, &db_path).unwrap());
        assert!(!snapshot.exists(), "整库大小的快照垃圾要清掉");
        assert!(!tmp_state.exists(), "半截状态文件要清掉");
        assert!(keep.exists(), "正式的状态文件不能动");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 整库替换会把 settings 一起换掉，但那是本机偏好（字号 / 出本阈值），必须保留。
    #[test]
    fn local_settings_survive_pull_replace() {
        let dir = temp_dir("keep-settings");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        crate::repo::settings::save(
            &local,
            &crate::repo::settings::SettingsDto {
                word_font_size: 128,
                streak_to_master: 3,
                mask_seconds: 8,
                auto_check_updates: false,
                stroke_auto_speak: true,
            },
        )
        .unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        // 云端那份用的是默认设置
        let (remote, chain_hash) = fake_remote("settings");
        write_pending_state(&dir, &chain_hash, &local_hash, 2);
        write_pending(&dir, &remote.data).unwrap();

        assert!(apply_pending(&dir, &db_path).unwrap());
        let applied = Connection::open(&db_path).unwrap();
        let s = crate::repo::settings::load(&applied).unwrap();
        assert_eq!(s.word_font_size, 128, "字号是本机偏好，不能被云端覆盖");
        assert_eq!(s.streak_to_master, 3, "出本阈值也不能被云端覆盖");
        assert_eq!(s.mask_seconds, 8);
        assert!(!s.auto_check_updates, "启动自动检查开关同样是本机偏好");
        assert!(s.stroke_auto_speak, "笔画复习自动朗读开关同样是本机偏好");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `validate_snapshot` 的版本检查要单独被测到：构造一个哈希一致但 user_version 更高的快照，
    /// 否则删掉版本检查、靠哈希不一致也能让「拒收」成立，测试就成了摆设。
    #[test]
    fn newer_user_version_is_rejected_by_its_own_check() {
        let dir = temp_dir("newer-exact");
        let conn = db::open_memory();
        let hash = content_hash(&conn).unwrap();
        let snap = create_snapshot(&conn, &dir, &meta(hash)).unwrap();
        {
            let s = Connection::open(&snap).unwrap();
            s.execute_batch("PRAGMA user_version = 99;").unwrap();
            // 抬高版本后重算哈希并写回元数据，只留版本检查这一条拦截
            let bumped_hash = content_hash(&s).unwrap();
            let mut m = meta(bumped_hash);
            m.user_version = 99;
            s.execute(
                "INSERT OR REPLACE INTO _sync_meta (key, value) VALUES (?1, ?2)",
                params![META_KEY, serde_json::to_string(&m).unwrap()],
            )
            .unwrap();
        }
        let err = validate_snapshot(&snap, crate::db::latest_user_version())
            .unwrap_err()
            .to_string();
        assert!(err.contains("请先升级"), "应命中版本检查：{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 云端快照来自更高版本程序时，拉取要在连网后立刻拒绝。
    #[test]
    fn pull_rejects_snapshot_from_newer_app() {
        let dir = temp_dir("pull-newer");
        save_test_creds(&dir);
        // 快照元数据本身声称 user_version=99（真实场景：新版程序生成的快照）。
        // 只改云端行的列不再触发拒绝——判定用的就是快照元数据，不是行里的列。
        let (remote, _) = fake_remote_with_uv("newer", 99);
        let store = FakeStore::new(remote);
        let local = db::open_memory();

        let plan = prepare_pull(&local, &dir).unwrap();
        let err = perform_pull_with(&store, plan, false).unwrap_err().to_string();
        // 展示的版本取自快照元数据（生成它的程序版本），不是云端行里的列
        assert!(err.contains("更新版本"), "{err}");
        assert!(err.contains("数据版本 99"), "{err}");
        assert!(!pending_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 强推的重推窗口内云端又变了（二次 Conflict）：必须报错请重试，且云端数据不能被动。
    /// 这是防「重推覆盖期间到达的新版本」的最后防线，此前无测试覆盖。
    #[test]
    fn second_conflict_during_force_push_errors_out() {
        let dir = temp_dir("push-double-conflict");
        save_test_creds(&dir);
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();

        // 本机记录版本 3，云端实际 5（另一台设备已推进）→ 第一次 push 冲突
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                remote_version: 3,
                ..SyncState::default()
            },
        )
        .unwrap();
        let (mut remote, _) = fake_remote("double-conflict");
        remote.version = 5;
        let store = FakeStore::new(remote);
        let remote_bytes = store.fetch().unwrap().data.clone();

        // 用户确认强推；重推前另一台设备又推了（版本 5 → 6）
        store.bump_on_nth_push(2, 6);
        let plan = prepare_push(&conn, &dir).unwrap();
        let err = perform_push_with(&store, plan, true).unwrap_err().to_string();
        assert!(err.contains("刚刚又变了"), "要提示重试而不是覆盖：{err}");
        // 云端仍是重推前那份数据（版本 6 对应的内容），本机快照绝不能落上去
        let now = store.fetch().unwrap();
        assert_eq!(now.version, 6);
        assert_eq!(
            now.data, remote_bytes,
            "云端数据必须还是「期间到达的那份」，不能被本机快照覆盖"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 空库推送（删库重建后忘了先拉取）必须确认：否则云端和另一台设备会被一起清空。
    #[test]
    fn push_from_empty_library_needs_confirm() {
        let dir = temp_dir("push-empty");
        save_test_creds(&dir);
        let conn = db::open_memory(); // 空库
        // 本机记录的云端版本与云端一致 → CAS 会放行，正是危险之处
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                remote_version: 3,
                ..SyncState::default()
            },
        )
        .unwrap();
        let (remote, _) = fake_remote("push-empty");
        let store = FakeStore::new(remote);

        let plan = prepare_push(&conn, &dir).unwrap();
        let out = perform_push_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "conflict", "空库推送必须先确认");
        assert!(out.needs_confirm);
        assert_eq!(
            store.fetch().unwrap().version,
            3,
            "没确认之前云端一个字节都不能动"
        );
        assert!(!store.fetch().unwrap().data.is_empty());

        // 用户确认（确实想把云端清空）后才真正覆盖，且覆盖前必须把云端快照留档
        let plan = prepare_push(&conn, &dir).unwrap();
        assert_eq!(perform_push_with(&store, plan, true).unwrap().kind, "applied");
        assert_eq!(store.fetch().unwrap().version, 4);
        let archived: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("remote-before-overwrite")
            })
            .collect();
        assert_eq!(archived.len(), 1, "空库覆盖云端前也得留一份云端快照");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 云端快照来自更旧版本的程序：新列（如错字标记）会被迁移补成默认值，必须先确认。
    #[test]
    fn pull_from_older_app_needs_confirm() {
        let dir = temp_dir("pull-older");
        save_test_creds(&dir);
        let (mut remote, _) = fake_remote_with_uv("older", 1);
        remote.user_version = 1;
        remote.app_version = "0.1.0".into();
        let store = FakeStore::new(remote);
        let local = db::open_memory();

        let plan = prepare_pull(&local, &dir).unwrap();
        let out = perform_pull_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "conflict", "旧版本快照要确认后才覆盖本机");
        assert!(out.needs_confirm);
        assert!(!pending_path(&dir).exists());

        // 用户确认后才写 pending
        let plan = prepare_pull(&local, &dir).unwrap();
        assert_eq!(perform_pull_with(&store, plan, true).unwrap().kind, "pending");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 本机之后又推送过更高版本时，重启落地这份旧快照会把本机数据回退，必须作废。
    #[test]
    fn expired_pending_is_rejected_when_local_pushed_later() {
        let dir = temp_dir("pending-expired");
        let local = db::open_memory();
        let g = groups::create(&local, None, "1", "第一单元").unwrap();
        words::add(&local, g.id, "春天", None).unwrap();
        let db_path = file_db_from(&dir, &local);
        let local_hash = content_hash(&Connection::open(&db_path).unwrap()).unwrap();

        let (remote, chain_hash) = fake_remote("expired");
        write_pending_state(&dir, &chain_hash, &local_hash, 2);
        write_pending(&dir, &remote.data).unwrap();
        // 拉取之后本机又成功推送过，主状态版本更高
        save_state(
            &dir,
            &SyncState {
                device_id: "dev-local".into(),
                remote_version: 9,
                ..SyncState::default()
            },
        )
        .unwrap();

        let err = apply_pending(&dir, &db_path).unwrap_err().to_string();
        assert!(err.contains("已过期"), "{err}");
        assert!(!pending_path(&dir).exists());
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            9,
            "更高的版本不能被旧快照回退"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 留档只保留最近若干份，避免每次拉取都堆一整库。
    #[test]
    fn sync_backups_are_pruned() {
        let dir = temp_dir("prune");
        let backups = sync_backup_dir(&dir).unwrap();
        for i in 1..=7 {
            fs::write(backups.join(format!("before-sync-{i}-1.db")), b"x").unwrap();
        }
        // 另一种前缀不受影响
        fs::write(backups.join("remote-before-overwrite-1-1.db"), b"x").unwrap();

        prune_sync_backups(&dir, "before-sync-");
        let mut left: Vec<String> = fs::read_dir(&backups)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("before-sync-"))
            .collect();
        left.sort();
        assert_eq!(left.len(), KEEP_SYNC_BACKUPS);
        assert_eq!(left.first().unwrap(), "before-sync-3-1.db", "应删最旧的");
        assert!(backups.join("remote-before-overwrite-1-1.db").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 推送时云端版本已被推进：不确认就不能覆盖；强制覆盖前必须把云端快照留档。
    #[test]
    fn push_conflict_needs_confirm_and_force_archives_remote() {
        let dir = temp_dir("push-conflict");
        save_test_creds(&dir);
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();

        // 本机记录的云端版本是 3，但云端已经走到 7
        let state = SyncState {
            device_id: "dev-local".into(),
            remote_version: 3,
            ..SyncState::default()
        };
        save_state(&dir, &state).unwrap();

        let (remote, _) = fake_remote("push-conflict");
        let mut remote = remote;
        remote.version = 7;
        let store = FakeStore::new(remote);

        let plan = prepare_push(&conn, &dir).unwrap();
        let out = perform_push_with(&store, plan, false).unwrap();
        assert_eq!(out.kind, "conflict");
        assert!(out.needs_confirm);
        assert_eq!(out.remote_version, 7);
        assert_eq!(
            load_state(&dir).unwrap().remote_version,
            3,
            "未确认前状态不变"
        );

        let plan = prepare_push(&conn, &dir).unwrap();
        let out = perform_push_with(&store, plan, true).unwrap();
        assert_eq!(out.kind, "applied");
        assert_eq!(load_state(&dir).unwrap().remote_version, 8);
        let archived: Vec<_> = fs::read_dir(sync_backup_dir(&dir).unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("remote-before-overwrite")
            })
            .collect();
        assert_eq!(archived.len(), 1, "强制覆盖前必须留一份云端快照");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 真实服务器集成测试，默认忽略。凭据只从环境变量取，不写进仓库。
    /// 跑完会把云端行恢复成空快照 + version 0。
    #[test]
    #[ignore = "需要真实 MySQL；设置 DICTATION_SYNC_* 环境变量后手动运行"]
    fn live_mysql_roundtrip() {
        let password =
            std::env::var("DICTATION_SYNC_PASSWORD").expect("请设置 DICTATION_SYNC_PASSWORD");
        let cfg = SyncConfigDto {
            enabled: true,
            host: std::env::var("DICTATION_SYNC_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("DICTATION_SYNC_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3306),
            database: std::env::var("DICTATION_SYNC_DB")
                .unwrap_or_else(|_| "dictation_sync".into()),
            user: std::env::var("DICTATION_SYNC_USER").unwrap_or_else(|_| "dictation".into()),
            device_name: "集成测试".into(),
        };
        let store = MysqlStore::new(&cfg, &password).unwrap();
        assert!(
            store.server_version().unwrap().starts_with('8'),
            "服务端应为 MySQL 8"
        );

        let dir = temp_dir("live");
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add(&conn, g.id, "春天", None).unwrap();
        let hash = content_hash(&conn).unwrap();
        let m = meta(hash.clone());
        let snap = create_snapshot(&conn, &dir, &m).unwrap();
        let bytes = fs::read(&snap).unwrap();

        let base = store.fetch().unwrap().version;
        match store.push(base, &bytes, &m).unwrap() {
            PushOutcome::Applied(v) => assert_eq!(v, base + 1),
            PushOutcome::Conflict(_) => panic!("首次推送不该冲突"),
        }
        let got = store.fetch().unwrap();
        assert_eq!(got.data, bytes, "云端读回的快照必须与上传一致");
        assert_eq!(got.content_hash, hash);

        match store.push(base, &bytes, &m).unwrap() {
            PushOutcome::Conflict(v) => assert_eq!(v, base + 1, "过期版本必须被 CAS 拒绝"),
            PushOutcome::Applied(_) => panic!("过期版本不该推送成功"),
        }

        // 恢复现场：把云端行还原成空快照
        let mut c = mysql::Conn::new(store.opts.clone()).unwrap();
        c.query_drop(
            "UPDATE dictation_snapshot SET data = X'', version = 0, device_id = '',
             device_name = '', hash = '', app_version = '', user_version = 0,
             exported_at = '' WHERE id = 1",
        )
        .unwrap();
        assert_eq!(store.fetch().unwrap().version, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
