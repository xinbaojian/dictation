use crate::domain::ai_card::CardContent;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCardDto {
    pub id: i64,
    pub word_id: i64,
    pub kind: String,
    pub content: CardContent,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 生成或重新生成一张卡：同 (word_id, kind) 覆盖（UPSERT，连点两次生成后写者胜）
pub fn upsert(
    conn: &Connection,
    word_id: i64,
    kind: &str,
    content: &CardContent,
    model: &str,
) -> AppResult<AiCardDto> {
    let raw = serde_json::to_string(content)?;
    conn.execute(
        "INSERT INTO ai_cards (word_id, kind, content_json, model,
                               created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(word_id, kind) DO UPDATE SET
           content_json = excluded.content_json,
           model = excluded.model,
           updated_at = excluded.updated_at",
        params![word_id, kind, raw, model],
    )?;
    get(conn, word_id, kind)?.ok_or_else(|| AppError::msg("讲解卡保存失败，请重试"))
}

pub fn get(conn: &Connection, word_id: i64, kind: &str) -> AppResult<Option<AiCardDto>> {
    let row = conn
        .query_row(
            "SELECT id, word_id, kind, content_json, model, created_at, updated_at
             FROM ai_cards WHERE word_id = ?1 AND kind = ?2",
            params![word_id, kind],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((id, word_id, kind, raw, model, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let content: CardContent = serde_json::from_str(&raw)?;
    Ok(Some(AiCardDto { id, word_id, kind, content, model, created_at, updated_at }))
}

/// 词文本被编辑时清卡（由 words::update 调用）；词整删由外键级联兜底
pub fn delete_for_word(conn: &Connection, word_id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM ai_cards WHERE word_id = ?1", params![word_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        let g = crate::repo::groups::create(&conn, None, "1", "第一单元").unwrap();
        (conn, g.id)
    }

    fn sample(mnemonic: &str) -> CardContent {
        CardContent {
            decomposition: vec!["王".into(), "白".into(), "石".into()],
            mnemonic: mnemonic.into(),
            confusables: vec![crate::domain::ai_card::ConfusableHint {
                char: "壁".into(),
                hint: "下面是土，和墙有关".into(),
            }],
            sentences: vec!["湖水碧绿得像一块玉石。".into()],
        }
    }

    #[test]
    fn upsert_roundtrip_and_overwrite() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();

        let first = upsert(&conn, w.id, "mnemonic", &sample("老王爷坐在白石头上"), "test-model")
            .unwrap();
        assert_eq!(first.content.mnemonic, "老王爷坐在白石头上");
        assert_eq!(first.word_id, w.id);

        let second =
            upsert(&conn, w.id, "mnemonic", &sample("换一句口诀"), "test-model-2").unwrap();
        assert_eq!(second.id, first.id, "UPSERT 不换行");
        assert_eq!(second.content.mnemonic, "换一句口诀");
        assert_eq!(second.model, "test-model-2");
    }

    #[test]
    fn get_missing_returns_none() {
        let (conn, _gid) = setup();
        assert!(get(&conn, 999, "mnemonic").unwrap().is_none());
    }

    #[test]
    fn word_delete_cascades() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();
        upsert(&conn, w.id, "mnemonic", &sample("口诀"), "m").unwrap();
        crate::repo::words::delete(&conn, w.id).unwrap();
        assert!(get(&conn, w.id, "mnemonic").unwrap().is_none());
    }

    #[test]
    fn delete_for_word_removes_all_kinds() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();
        upsert(&conn, w.id, "mnemonic", &sample("口诀"), "m").unwrap();
        delete_for_word(&conn, w.id).unwrap();
        assert!(get(&conn, w.id, "mnemonic").unwrap().is_none());
    }
}
