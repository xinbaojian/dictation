use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 迁移列表：版本号必须从 1 开始连续递增，发布后不可修改历史脚本
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_init.sql")),
    (2, include_str!("../../migrations/0002_session_progress.sql")),
];

pub fn init(app: &AppHandle) -> AppResult<(Connection, PathBuf)> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::msg(format!("无法获取数据目录：{e}")))?;
    std::fs::create_dir_all(&dir)?;
    let conn = Connection::open(dir.join("dictation.db"))?;
    prepare(&conn)?;
    Ok((conn, dir))
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
