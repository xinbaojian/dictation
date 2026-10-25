use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const VERSION: i32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupFile {
    version: i32,
    exported_at: String,
    groups: Vec<BackupGroup>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupGroup {
    code: String,
    name: String,
    words: Vec<BackupWord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupWord {
    text: String,
    note: Option<String>,
    status: String,
    wrong_count: i64,
    correct_streak: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub groups_added: i64,
    pub groups_merged: i64,
    pub words_added: i64,
    pub words_skipped: i64,
}

pub fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

pub fn create(conn: &Connection, data_dir: &Path) -> AppResult<String> {
    let exported_at: String =
        conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| r.get(0))?;
    let stamp: String =
        conn.query_row("SELECT strftime('%Y%m%d-%H%M%S','now')", [], |r| r.get(0))?;

    let mut groups = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id, code, name FROM groups ORDER BY sort_order, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (group_id, code, name) = row?;
            let mut wstmt = conn.prepare(
                "SELECT text, note, status, wrong_count, correct_streak
                 FROM words WHERE group_id = ?1 ORDER BY id",
            )?;
            let words = wstmt
                .query_map(params![group_id], |r| {
                    Ok(BackupWord {
                        text: r.get(0)?,
                        note: r.get(1)?,
                        status: r.get(2)?,
                        wrong_count: r.get(3)?,
                        correct_streak: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            groups.push(BackupGroup { code, name, words });
        }
    }

    let payload = BackupFile {
        version: VERSION,
        exported_at,
        groups,
    };
    let dir = backup_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("backup-{stamp}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&payload)?)?;
    Ok(path.to_string_lossy().to_string())
}

pub fn list(data_dir: &Path) -> AppResult<Vec<BackupInfo>> {
    let dir = backup_dir(data_dir);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        out.push(BackupInfo {
            name: entry.file_name().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            size: entry.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// 非破坏性合并：同编号同名分组视为同一个分组，已存在的词条一律跳过，不覆盖现有进度
pub fn import(conn: &Connection, path: &str) -> AppResult<ImportSummary> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| AppError::msg(format!("读取备份文件失败：{e}")))?;
    let payload: BackupFile =
        serde_json::from_str(&raw).map_err(|e| AppError::msg(format!("备份文件格式不正确：{e}")))?;
    if payload.version > VERSION {
        return Err(AppError::msg("备份文件来自更新的版本，请升级程序后再导入"));
    }

    let mut summary = ImportSummary {
        groups_added: 0,
        groups_merged: 0,
        words_added: 0,
        words_skipped: 0,
    };
    let tx = conn.unchecked_transaction()?;
    for group in payload.groups {
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM groups WHERE code = ?1 AND name = ?2",
                params![group.code, group.name],
                |r| r.get(0),
            )
            .ok();
        let group_id = match existing {
            Some(id) => {
                summary.groups_merged += 1;
                id
            }
            None => {
                let sort_order: i64 = tx.query_row(
                    "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM groups",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT INTO groups (code, name, sort_order, created_at)
                     VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                    params![group.code, group.name, sort_order],
                )?;
                summary.groups_added += 1;
                tx.last_insert_rowid()
            }
        };
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO words
               (group_id, text, note, status, wrong_count, correct_streak, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )?;
        for word in group.words {
            let n = stmt.execute(params![
                group_id,
                word.text,
                word.note,
                word.status,
                word.wrong_count,
                word.correct_streak
            ])?;
            if n == 1 {
                summary.words_added += 1;
            } else {
                summary.words_skipped += 1;
            }
        }
    }
    tx.commit()?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::repo::{groups, words};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn export_then_import_roundtrip() {
        let dir = temp_dir("backup");
        let source = db::open_memory();
        let g = groups::create(&source, "1", "第一单元").unwrap();
        words::add(&source, g.id, "春天", Some("造句用")).unwrap();
        words::add(&source, g.id, "夏天", None).unwrap();
        crate::repo::word_progress::mark_wrong(&source, words::list(&source, g.id).unwrap()[0].id)
            .unwrap();

        let path = create(&source, &dir).unwrap();
        assert!(Path::new(&path).exists());
        assert_eq!(list(&dir).unwrap().len(), 1);

        let target = db::open_memory();
        let summary = import(&target, &path).unwrap();
        assert_eq!(summary.groups_added, 1);
        assert_eq!(summary.words_added, 2);
        assert_eq!(summary.words_skipped, 0);

        let gs = groups::list(&target).unwrap();
        assert_eq!(gs.len(), 1);
        assert_eq!(gs[0].word_count, 2);
        // 进度随备份一起恢复
        assert_eq!(gs[0].learning_count, 1);

        // 重复导入不重复写入
        let again = import(&target, &path).unwrap();
        assert_eq!(again.groups_merged, 1);
        assert_eq!(again.words_added, 0);
        assert_eq!(again.words_skipped, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_rejects_broken_file() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not json").unwrap();
        let conn = db::open_memory();
        assert!(import(&conn, path.to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
