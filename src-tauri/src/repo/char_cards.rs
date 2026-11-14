use crate::domain::char_card::CharCardContent;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharCardDto {
    pub char: String,
    pub content: CharCardContent,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 生成或重新生成一张字卡：同字覆盖（UPSERT，连点两次生成后写者胜）
pub fn upsert(
    conn: &Connection,
    ch: &str,
    content: &CharCardContent,
    model: &str,
) -> AppResult<CharCardDto> {
    let raw = serde_json::to_string(content)?;
    conn.execute(
        "INSERT INTO char_cards (char, content_json, model,
                                 created_at, updated_at)
         VALUES (?1, ?2, ?3,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(char) DO UPDATE SET
           content_json = excluded.content_json,
           model = excluded.model,
           updated_at = excluded.updated_at",
        params![ch, raw, model],
    )?;
    get(conn, ch)?.ok_or_else(|| AppError::msg("字卡保存失败，请重试"))
}

pub fn get(conn: &Connection, ch: &str) -> AppResult<Option<CharCardDto>> {
    let row = conn
        .query_row(
            "SELECT char, content_json, model, created_at, updated_at
             FROM char_cards WHERE char = ?1",
            params![ch],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((ch, raw, model, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let content: CharCardContent = serde_json::from_str(&raw)?;
    Ok(Some(CharCardDto { char: ch, content, model, created_at, updated_at }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn sample(meaning: &str) -> CharCardContent {
        CharCardContent {
            meaning: meaning.into(),
            mnemonic: "王爷家的白玉石".into(),
            words: vec!["碧绿".into(), "碧玉".into()],
            sentences: vec!["湖水碧绿。".into()],
        }
    }

    #[test]
    fn user_version_is_7_after_migrations() {
        let conn = db::open_memory();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 7, "0007 迁移后 user_version 必须是 7");
    }

    #[test]
    fn upsert_roundtrip_and_overwrite() {
        let conn = db::open_memory();
        let first = upsert(&conn, "碧", &sample("青绿色的玉石"), "m1").unwrap();
        assert_eq!(first.char, "碧");
        assert_eq!(first.content.meaning, "青绿色的玉石");
        assert_eq!(first.model, "m1");

        let second = upsert(&conn, "碧", &sample("换一句"), "m2").unwrap();
        assert_eq!(second.content.meaning, "换一句");
        assert_eq!(second.model, "m2");
        assert_eq!(second.created_at, first.created_at, "覆盖不换创建时间");
    }

    #[test]
    fn get_missing_returns_none() {
        let conn = db::open_memory();
        assert!(get(&conn, "没有").unwrap().is_none());
    }
}
