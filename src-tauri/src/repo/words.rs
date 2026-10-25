use crate::domain::text::normalize_word;
use crate::error::{is_unique_violation, AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordDto {
    pub id: i64,
    pub group_id: i64,
    pub text: String,
    pub note: Option<String>,
    pub status: String,
    pub wrong_count: i64,
    pub correct_streak: i64,
    pub last_practiced_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub inserted: i64,
    pub skipped: i64,
}

const SELECT_ONE: &str = "
SELECT id, group_id, text, note, status, wrong_count, correct_streak, last_practiced_at
FROM words WHERE id = ?1";

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WordDto> {
    Ok(WordDto {
        id: r.get(0)?,
        group_id: r.get(1)?,
        text: r.get(2)?,
        note: r.get(3)?,
        status: r.get(4)?,
        wrong_count: r.get(5)?,
        correct_streak: r.get(6)?,
        last_practiced_at: r.get(7)?,
    })
}

pub fn list(conn: &Connection, group_id: i64) -> AppResult<Vec<WordDto>> {
    let mut stmt = conn.prepare(
        "SELECT id, group_id, text, note, status, wrong_count, correct_streak, last_practiced_at
         FROM words WHERE group_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![group_id], map_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<WordDto> {
    conn.query_row(SELECT_ONE, params![id], map_row)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::msg("词条不存在或已被删除"),
            other => AppError::Db(other),
        })
}

pub fn add(conn: &Connection, group_id: i64, text: &str, note: Option<&str>) -> AppResult<WordDto> {
    let text = normalize_word(text);
    if text.is_empty() {
        return Err(AppError::msg("词语不能为空"));
    }
    let note = note.map(str::trim).filter(|s| !s.is_empty());
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM words WHERE group_id = ?1 AND text = ?2",
            params![group_id, text],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_some() {
        return Err(AppError::msg(format!("该分组中已有「{text}」")));
    }
    conn.execute(
        "INSERT INTO words (group_id, text, note, created_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![group_id, text, note],
    )?;
    get(conn, conn.last_insert_rowid())
}

/// 批量导入：已存在的词条跳过而不报错，返回实际写入与跳过数量
pub fn import(conn: &Connection, group_id: i64, texts: &[String]) -> AppResult<ImportResult> {
    let tx = conn.unchecked_transaction()?;
    let mut inserted = 0;
    let mut skipped = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO words (group_id, text, created_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )?;
        for raw in texts {
            let text = normalize_word(raw);
            if text.is_empty() {
                skipped += 1;
                continue;
            }
            let n = stmt.execute(params![group_id, text])?;
            if n == 1 {
                inserted += 1;
            } else {
                skipped += 1;
            }
        }
    }
    tx.commit()?;
    Ok(ImportResult { inserted, skipped })
}

pub fn update(conn: &Connection, id: i64, text: &str, note: Option<&str>) -> AppResult<WordDto> {
    let text = normalize_word(text);
    if text.is_empty() {
        return Err(AppError::msg("词语不能为空"));
    }
    let current = get(conn, id)?;
    let note = note.map(str::trim).filter(|s| !s.is_empty());
    let result = conn.execute(
        "UPDATE words SET text = ?1, note = ?2 WHERE id = ?3",
        params![text, note, id],
    );
    if let Err(e) = result {
        if is_unique_violation(&e) {
            return Err(AppError::msg(format!(
                "「{}」所在分组中已有「{text}」",
                current.text
            )));
        }
        return Err(AppError::Db(e));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM words WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::msg("词条不存在或已被删除"));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MistakeDto {
    pub id: i64,
    pub text: String,
    pub group_id: i64,
    pub group_code: String,
    pub group_name: String,
    pub wrong_count: i64,
    pub correct_streak: i64,
    pub last_practiced_at: Option<String>,
}

/// 错题本：状态为 learning 的词，按错误次数降序
pub fn list_mistakes(conn: &Connection, group_id: Option<i64>) -> AppResult<Vec<MistakeDto>> {
    let mut stmt = conn.prepare(
        "SELECT w.id, w.text, w.group_id, g.code, g.name,
                w.wrong_count, w.correct_streak, w.last_practiced_at
         FROM words w
         JOIN groups g ON g.id = w.group_id
         WHERE w.status = 'learning' AND (?1 IS NULL OR w.group_id = ?1)
         ORDER BY w.wrong_count DESC, w.last_practiced_at DESC, w.id ASC",
    )?;
    let rows = stmt.query_map(params![group_id], |r| {
        Ok(MistakeDto {
            id: r.get(0)?,
            text: r.get(1)?,
            group_id: r.get(2)?,
            group_code: r.get(3)?,
            group_name: r.get(4)?,
            wrong_count: r.get(5)?,
            correct_streak: r.get(6)?,
            last_practiced_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        let g = crate::repo::groups::create(&conn, "1", "第一单元").unwrap();
        (conn, g.id)
    }

    #[test]
    fn add_trimmed_word() {
        let (conn, gid) = setup();
        let w = add(&conn, gid, "  春天  ", None).unwrap();
        assert_eq!(w.text, "春天");
        assert_eq!(w.status, "new");
        assert_eq!(list(&conn, gid).unwrap().len(), 1);
    }

    #[test]
    fn duplicate_rejected_with_friendly_message() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let err = add(&conn, gid, "春天", None).unwrap_err();
        assert!(err.to_string().contains("已有「春天」"));
    }

    #[test]
    fn import_skips_existing() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let r = import(&conn, gid, &["春天".into(), "夏天".into(), "夏天".into()]).unwrap();
        assert_eq!(r.inserted, 1);
        assert_eq!(r.skipped, 2);
        assert_eq!(list(&conn, gid).unwrap().len(), 2);
    }

    #[test]
    fn update_to_existing_word_fails() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let b = add(&conn, gid, "夏天", None).unwrap();
        assert!(update(&conn, b.id, "春天", None).is_err());
    }

    #[test]
    fn delete_word() {
        let (conn, gid) = setup();
        let w = add(&conn, gid, "春天", None).unwrap();
        delete(&conn, w.id).unwrap();
        assert!(list(&conn, gid).unwrap().is_empty());
    }
}
