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
];

pub fn init(app: &AppHandle) -> AppResult<(Connection, PathBuf)> {
    let legacy_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::msg(format!("无法获取用户数据目录：{e}")))?;
    let dir = resolve_dir(&legacy_dir);
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("dictation.db");
    let marker = marker_path(&dir);

    if !db_path.exists() {
        // 库被删掉了但 WAL 还留着时，SQLite 可能把旧页恢复出来，先清掉
        remove_sidecars(&db_path);
        // 迁移来源：① 旧版用户数据目录；②（macOS）旧版放在 .app 包内的库。
        // 按序尝试，找到第一个存在的就搬过来。
        let mut sources: Vec<PathBuf> = vec![legacy_dir.join("dictation.db")];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                sources.push(exe_dir.join("dictation.db"));
            }
        }
        for src in sources {
            if should_adopt(&db_path, &marker, &src) {
                adopt_legacy(&src, &db_path)?;
                break;
            }
        }
    }

    let conn = Connection::open(&db_path)?;
    prepare(&conn)?;
    // 记录「这个目录已经初始化过」：以后用户再删库，不会又把旧位置的数据搬回来
    let _ = std::fs::write(&marker, b"");
    Ok((conn, dir))
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
/// 用户数据目录（~/Library/Application Support/com.dictation.app）。
fn resolve_dir(fallback: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = std::fs::create_dir_all(fallback);
        return fallback.to_path_buf();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-db-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writable_dir_is_used() {
        let dir = temp("writable");
        assert!(is_writable(&dir));
        let _ = std::fs::remove_dir_all(&dir);
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
}
