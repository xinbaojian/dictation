use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 迁移列表：版本号必须从 1 开始连续递增，发布后不可修改历史脚本
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_init.sql")),
    (2, include_str!("../../migrations/0002_session_progress.sql")),
    (3, include_str!("../../migrations/0003_wrong_chars.sql")),
    (4, include_str!("../../migrations/0004_group_tree.sql")),
    (5, include_str!("../../migrations/0005_word_wrong_chars.sql")),
    (6, include_str!("../../migrations/0006_ai_cards.sql")),
];

/// 当前程序认识的最大 user_version；导入同步快照时用来自拒来自更新版本的数据。
pub fn latest_user_version() -> i64 {
    MIGRATIONS.last().map(|(v, _)| *v).unwrap_or(0)
}

pub fn init(app: &AppHandle) -> AppResult<(Connection, PathBuf)> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::msg(format!("无法获取用户数据目录：{e}")))?;
    let dir = resolve_dir(&app_data_dir);
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("dictation.db");
    let marker = marker_path(&dir);

    // 上次同步下载的快照在这时落地：必须在 Connection::open 之前替换文件，
    // 否则会动到正在被 Mutex<Connection> 持有的库。
    // 每次启动都无条件调用（不只在 pending.db 存在时）：无 pending 时它还要负责
    // 补提交上次「库已替换但状态没写完」的 .sync-pending-state.json，并清掉过期错误文件。
    // apply_pending 自己会写 .sync-last-error.txt；失败不阻塞启动。
    let _ = crate::repo::sync::apply_pending(&dir, &db_path);

    if !db_path.exists() {
        // 库被删掉了但 WAL 还留着时，SQLite 可能把旧页恢复出来，先清掉
        remove_sidecars(&db_path);
        // 迁移来源见 migration_sources：① 旧 bundle identifier 的用户数据目录；
        // ② 当前用户数据目录（老版本 → 便携目录）；③（macOS）旧版放在 .app 包内的库。
        // 按序尝试，找到第一个存在的就搬过来，且只搬一次（marker 兜底）。
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        for src in migration_sources(&app_data_dir, exe_dir.as_deref()) {
            if should_adopt(&db_path, &marker, &src) {
                adopt_legacy(&src, &db_path)?;
                if let Some(from_dir) = src.parent() {
                    adopt_backups(from_dir, &dir);
                }
                break;
            }
        }
    }

    let conn = Connection::open(&db_path)?;
    // 迁移会加列、进而改变同步用的内容哈希；迁移前先记一份，迁移后好把同步基准一起挪过去
    let needs_migration = {
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
        current < latest_user_version()
    };
    let hash_before = if needs_migration {
        crate::repo::sync::content_hash(&conn).ok()
    } else {
        None
    };
    prepare(&conn)?;
    if let Some(before) = hash_before {
        crate::repo::sync::rebase_baseline_after_migration(&dir, &before, &conn);
    }
    // 记录「这个目录已经初始化过」：以后用户再删库，不会又把旧位置的数据搬回来
    let _ = std::fs::write(&marker, b"");
    Ok((conn, dir))
}

/// 旧版 bundle identifier。Tauri 直接拿 identifier 当 app_data_dir 的最后一级目录，
/// 改 identifier 会换数据目录；这里保留旧名，用于把老用户的库一次性搬过来。
const LEGACY_IDENTIFIER: &str = "com.dictation.app";

/// app_data_dir = <平台数据根>/<identifier>，把末段换成旧 identifier 即可定位老目录。
/// 这样不用引入 dirs 之类的依赖，macOS / Windows / Linux 都适用。
fn legacy_identifier_dir(app_data_dir: &Path) -> Option<PathBuf> {
    app_data_dir
        .parent()
        .map(|root| root.join(LEGACY_IDENTIFIER))
}

/// 迁移来源按「离当前版本越近越优先」排列。
fn migration_sources(app_data_dir: &Path, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    if let Some(old) = legacy_identifier_dir(app_data_dir) {
        if old.as_path() != app_data_dir {
            sources.push(old.join("dictation.db"));
        }
    }
    sources.push(app_data_dir.join("dictation.db"));
    if let Some(dir) = exe_dir {
        sources.push(dir.join("dictation.db"));
    }
    sources
}

/// 备份文件也跟着搬：只补新目录里缺的，绝不覆盖；失败不影响主库启动。
fn adopt_backups(from_dir: &Path, to_dir: &Path) {
    if from_dir == to_dir {
        return;
    }
    let Ok(entries) = std::fs::read_dir(from_dir.join("backups")) else {
        return;
    };
    let dst = to_dir.join("backups");
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let target = dst.join(entry.file_name());
        if target.exists() {
            continue;
        }
        if std::fs::create_dir_all(&dst).is_err() {
            return;
        }
        let _ = std::fs::copy(entry.path(), target);
    }
}

fn marker_path(dir: &Path) -> PathBuf {
    dir.join(".dictation-initialized")
}

/// 只有「便携目录里还没有库」且「从未初始化过」时才去旧位置找数据
fn should_adopt(db: &Path, marker: &Path, legacy: &Path) -> bool {
    !db.exists() && !marker.exists() && legacy.exists()
}

fn remove_sidecars(db: &Path) {
    for suffix in ["-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{}", db.display(), suffix));
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// 数据目录优先用「程序同级目录」，做到绿色便携：拷走整个文件夹就能换电脑。
/// 目录不可写（例如装在 Program Files）时回落到用户数据目录。
///
/// macOS 例外：.app 包在更新时会被整体替换，绝不能把数据放在包内的可执行文件
/// 旁边（Dictation.app/Contents/MacOS），否则一升级数据就没了。macOS 一律用
/// 用户数据目录（~/Library/Application Support/com.dictation.desktop）。
/// 注意 identifier 改名会换这个目录，旧库靠 init 里的迁移源搬过来。
fn resolve_dir(fallback: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = std::fs::create_dir_all(fallback);
        fallback.to_path_buf()
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                if is_writable(dir) {
                    return dir.to_path_buf();
                }
            }
        }
        let _ = std::fs::create_dir_all(fallback);
        fallback.to_path_buf()
    }
}

#[cfg(not(target_os = "macos"))]
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".dictation-write-test");
    match std::fs::write(&probe, b"1") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 老版本把库放在用户数据目录，首次在便携目录启动时把库（含 WAL）整体搬过来
fn adopt_legacy(from: &Path, to: &Path) -> AppResult<()> {
    if !from.exists() || to.exists() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    for suffix in ["-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{}", from.display(), suffix));
        let dst = PathBuf::from(format!("{}{}", to.display(), suffix));
        if src.exists() {
            let _ = std::fs::copy(src, dst);
        }
    }
    Ok(())
}

fn prepare(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(conn)
}

fn migrate(conn: &Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (version, sql) in MIGRATIONS {
        if *version > current {
            conn.execute_batch(sql)?;
            conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn open_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open memory db");
    prepare(&conn).expect("migrate memory db");
    conn
}

/// 仅测试可见：把库只迁移到指定版本（可以比 latest 旧）。
/// 同步的兼容逻辑（内容哈希按库自身 schema 自适应、迁移后重定位同步基准）必须用
/// 「真·旧 schema」才测得出来——只把 user_version 数字改小、schema 仍是最新版，
/// 是测不出回归的（见 repo/sync.rs 的旧快照测试）。生产路径永远由 `init` 迁到最新版。
#[cfg(test)]
pub(crate) fn migrate_to(conn: &Connection, target: i64) -> AppResult<()> {
    // 与生产 migrate 同样的「只跑比 current 新的」判定，只是多一个 target 上限：
    // 已在 uv=5 的库迁到 6 时只补 0006，不会把 0001-0005 重跑一遍
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (version, sql) in MIGRATIONS {
        if *version > target || *version <= current {
            continue;
        }
        conn.execute_batch(sql)?;
        conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-db-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // is_writable 只在非 macOS 编译，测试必须跟着加同样的 cfg
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn writable_dir_is_used() {
        let dir = temp("writable");
        assert!(is_writable(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_always_uses_app_data_dir() {
        let fallback = temp("macos-data");
        assert_eq!(resolve_dir(&fallback), fallback);
        let _ = std::fs::remove_dir_all(&fallback);
    }

    #[test]
    fn legacy_db_is_adopted_once() {
        let dir = temp("adopt");
        let from = dir.join("dictation.db");
        let to = dir.join("portable.db");
        std::fs::write(&from, b"old").unwrap();
        std::fs::write(dir.join("dictation.db-wal"), b"wal").unwrap();

        adopt_legacy(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"old");
        assert!(dir.join("portable.db-wal").exists());

        // 目标已存在时不再覆盖
        std::fs::write(&from, b"new").unwrap();
        adopt_legacy(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"old");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_missing_is_noop() {
        let dir = temp("missing");
        adopt_legacy(&dir.join("nope.db"), &dir.join("target.db")).unwrap();
        assert!(!dir.join("target.db").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_only_when_never_initialized() {
        let dir = temp("adopt-rule");
        let db = dir.join("dictation.db");
        let marker = marker_path(&dir);
        let legacy = dir.join("legacy.db");

        std::fs::write(&legacy, b"x").unwrap();
        assert!(should_adopt(&db, &marker, &legacy), "首次启动应搬旧库");

        // 初始化过（marker 存在）但库被删了 —— 不应再从旧位置搬回来
        std::fs::write(&marker, b"").unwrap();
        assert!(!should_adopt(&db, &marker, &legacy), "删库后不应复活旧数据");

        // 库已存在时也不搬
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(&db, b"y").unwrap();
        assert!(!should_adopt(&db, &marker, &legacy));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_sidecars_are_removed() {
        let dir = temp("sidecar");
        let db = dir.join("dictation.db");
        let wal = dir.join("dictation.db-wal");
        let shm = dir.join("dictation.db-shm");
        std::fs::write(&wal, b"wal").unwrap();
        std::fs::write(&shm, b"shm").unwrap();

        remove_sidecars(&db);
        assert!(!wal.exists() && !shm.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_identifier_dir_is_a_sibling_of_app_data_dir() {
        let app_data = temp("legacy-dir").join("com.dictation.desktop");
        assert_eq!(
            legacy_identifier_dir(&app_data).unwrap(),
            app_data.parent().unwrap().join(LEGACY_IDENTIFIER)
        );
    }

    #[test]
    fn migration_sources_puts_old_identifier_dir_first() {
        let root = temp("sources");
        let app_data = root.join("com.dictation.desktop");
        let old = root.join(LEGACY_IDENTIFIER);
        let exe = root.join("MacOS");
        assert_eq!(
            migration_sources(&app_data, Some(&exe)),
            vec![
                old.join("dictation.db"),
                app_data.join("dictation.db"),
                exe.join("dictation.db"),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn backups_from_old_dir_are_copied_without_overwriting() {
        let root = temp("backups-adopt");
        let old = root.join(LEGACY_IDENTIFIER);
        let new = root.join("com.dictation.desktop");
        std::fs::create_dir_all(old.join("backups")).unwrap();
        std::fs::create_dir_all(new.join("backups")).unwrap();
        std::fs::write(old.join("backups/same.json"), b"old").unwrap();
        std::fs::write(old.join("backups/only-old.json"), b"old-only").unwrap();
        std::fs::write(new.join("backups/same.json"), b"new").unwrap();

        adopt_backups(&old, &new);

        assert_eq!(
            std::fs::read(new.join("backups/same.json")).unwrap(),
            b"new"
        );
        assert_eq!(
            std::fs::read(new.join("backups/only-old.json")).unwrap(),
            b"old-only"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
