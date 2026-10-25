// 本模块由 M3（批改页 / 错题本）接入：状态推进与测试先行，届时命令层直接调用即可
#![allow(dead_code)]

use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};

/// 连续答对多少次算「学会了」——全局配置项，V1 固定 2
pub const STREAK_TO_MASTER: i64 = 2;

const TOUCH: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

/// 批改为错 / 默写中点了「不会」：进入错题本，连续答对计数归零
pub fn mark_wrong(conn: &Connection, word_id: i64) -> AppResult<()> {
    conn.execute(
        &format!(
            "UPDATE words
             SET status = 'learning',
                 wrong_count = wrong_count + 1,
                 correct_streak = 0,
                 last_practiced_at = {TOUCH}
             WHERE id = ?1"
        ),
        params![word_id],
    )?;
    Ok(())
}

pub fn is_mastered(conn: &Connection, word_id: i64) -> AppResult<bool> {
    let status: Option<String> = conn
        .query_row("SELECT status FROM words WHERE id = ?1", params![word_id], |r| r.get(0))
        .optional()?;
    Ok(status.as_deref() == Some("mastered"))
}

/// 批改为对：新词直接掌握；错题本内的词连续答对达到次数后出本。
/// 返回 true 表示这次批改让词条从错题本「出本」了（用于统计）。
pub fn mark_correct(conn: &Connection, word_id: i64, streak_needed: i64) -> AppResult<bool> {
    let previous: Option<String> = conn
        .query_row("SELECT status FROM words WHERE id = ?1", params![word_id], |r| r.get(0))
        .optional()?;
    conn.execute(
        &format!(
            "UPDATE words
             SET correct_streak = correct_streak + 1,
                 status = CASE
                     WHEN status = 'new' THEN 'mastered'
                     WHEN status = 'learning' AND correct_streak + 1 >= ?2 THEN 'mastered'
                     ELSE status
                 END,
                 last_practiced_at = {TOUCH}
             WHERE id = ?1"
        ),
        params![word_id, streak_needed],
    )?;
    // 出本后把连对计数归零，避免残留进度影响下次入本
    conn.execute(
        "UPDATE words SET correct_streak = 0 WHERE id = ?1 AND status = 'mastered'",
        params![word_id],
    )?;
    Ok(previous.as_deref() == Some("learning") && is_mastered(conn, word_id)?)
}

/// 手动移出错题本 / 手动重新加入
pub fn set_mastered(conn: &Connection, word_id: i64, mastered: bool) -> AppResult<()> {
    let status = if mastered { "mastered" } else { "learning" };
    conn.execute(
        "UPDATE words SET status = ?1, correct_streak = 0 WHERE id = ?2",
        params![status, word_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn word(conn: &Connection) -> i64 {
        let g = crate::repo::groups::create(conn, None, "1", "第一单元").unwrap();
        crate::repo::words::add(conn, g.id, "春天", None).unwrap().id
    }

    fn status(conn: &Connection, id: i64) -> (String, i64, i64) {
        conn.query_row(
            "SELECT status, wrong_count, correct_streak FROM words WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    }

    #[test]
    fn wrong_moves_to_learning() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_wrong(&conn, id).unwrap();
        assert_eq!(status(&conn, id), ("learning".into(), 1, 0));
    }

    #[test]
    fn new_word_correct_becomes_mastered() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert_eq!(status(&conn, id).0, "mastered");
    }

    #[test]
    fn learning_word_needs_two_correct_to_graduate() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_wrong(&conn, id).unwrap();
        mark_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert_eq!(status(&conn, id), ("learning".into(), 1, 1));
        mark_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert_eq!(status(&conn, id), ("mastered".into(), 1, 0));
    }

    #[test]
    fn wrong_resets_streak() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_wrong(&conn, id).unwrap();
        mark_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        mark_wrong(&conn, id).unwrap();
        assert_eq!(status(&conn, id), ("learning".into(), 2, 0));
    }
}
