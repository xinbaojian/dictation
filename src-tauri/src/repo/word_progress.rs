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

/// 修改批改结果：把一次「批改为错」改成「批改为对」（家长批改手误的补救）。
///
/// 语义是**补偿差量**而不是精确回滚：抹掉那次错（`wrong_count-1`，下限 0），
/// 再把这次视为答对（复用 `mark_correct` 的推进与出本逻辑）。
/// 已清零的连对计数无法恢复到冤枉前的精确值——补偿为「+1」，即孩子可能需要
/// 多答对一次才出本；这是「改对」可解释的代价（见 wiki ADR-25）。
/// 返回 true 表示这次修改让词条从错题本出本。
pub fn revise_wrong_to_correct(
    conn: &Connection,
    word_id: i64,
    streak_needed: i64,
) -> AppResult<bool> {
    conn.execute(
        "UPDATE words SET wrong_count = max(0, wrong_count - 1) WHERE id = ?1",
        params![word_id],
    )?;
    mark_correct(conn, word_id, streak_needed)
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

    #[test]
    fn revise_wrong_to_correct_compensates_and_can_graduate() {
        let conn = db::open_memory();
        let id = word(&conn);
        // 错一次后又答对一次：本该再对一次就出本（needed = 2）
        mark_wrong(&conn, id).unwrap();
        mark_correct(&conn, id, STREAK_TO_MASTER).unwrap();

        // 家长发现最早那次其实是对的：把那场的结果改成对
        // ——抹掉那次错（wrong_count 回 0），这次修改本身按答对推进（streak 1→2 达标出本）
        let graduated = revise_wrong_to_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert!(graduated, "连对达标就该出本");
        assert_eq!(status(&conn, id), ("mastered".into(), 0, 0));
    }

    #[test]
    fn revise_wrong_to_correct_keeps_learning_when_streak_short() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_wrong(&conn, id).unwrap();
        // 没有别的答对记录：改成对后 streak 只到 1，还没达标
        let graduated = revise_wrong_to_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert!(!graduated);
        assert_eq!(status(&conn, id), ("learning".into(), 0, 1));
    }

    #[test]
    fn revise_wrong_to_correct_never_goes_negative() {
        let conn = db::open_memory();
        let id = word(&conn);
        mark_wrong(&conn, id).unwrap();
        // 防御性边界：wrong_count 已被清成 0（历史数据/外部改库）时也不能减成负数
        conn.execute("UPDATE words SET wrong_count = 0 WHERE id = ?1", params![id]).unwrap();
        revise_wrong_to_correct(&conn, id, STREAK_TO_MASTER).unwrap();
        assert_eq!(status(&conn, id).1, 0);
    }
}
