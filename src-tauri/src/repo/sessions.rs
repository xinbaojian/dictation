use crate::domain::sampler::Scope;
use crate::error::{AppError, AppResult};
use crate::repo::{settings, word_progress};
use rand::seq::SliceRandom;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    #[serde(default)]
    pub group_ids: Vec<i64>,
    #[serde(default)]
    pub group_names: Vec<String>,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub limit: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionItemDto {
    pub id: i64,
    pub seq: i64,
    pub word_id: i64,
    pub text: String,
    pub skip_marked: bool,
    pub result: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: i64,
    pub mode: String,
    pub status: String,
    pub total: i64,
    pub wrong_total: i64,
    pub current_seq: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub group_label: String,
    pub scope: String,
    pub items: Vec<SessionItemDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: i64,
    pub mode: String,
    pub status: String,
    pub total: i64,
    pub wrong_total: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub group_label: String,
    pub scope: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub wrong: i64,
    pub correct: i64,
    /// 本次批改后移出错题本的词数
    pub graduated: i64,
}

pub fn group_label(config: &SessionConfig) -> String {
    if config.scope == Scope::Mistake.as_str() {
        return "错题本".to_string();
    }
    if config.group_names.is_empty() {
        return "全部词语".to_string();
    }
    config.group_names.join("、")
}

fn parse_config(raw: &str) -> SessionConfig {
    serde_json::from_str(raw).unwrap_or(SessionConfig {
        group_ids: vec![],
        group_names: vec![],
        scope: "all".into(),
        limit: 0,
    })
}

pub fn create(
    conn: &Connection,
    group_ids: &[i64],
    scope: Scope,
    limit: i64,
    shuffle: bool,
) -> AppResult<i64> {
    let mut candidates = super::super::domain::sampler::collect(conn, group_ids, scope)?;
    if candidates.is_empty() {
        return Err(AppError::msg(scope_empty_hint(scope)));
    }
    if shuffle {
        let mut rng = rand::thread_rng();
        candidates.shuffle(&mut rng);
    }
    if limit > 0 {
        candidates.truncate(limit as usize);
    }
    let word_ids: Vec<i64> = candidates.iter().map(|c| c.word_id).collect();

    let group_names = load_group_names(conn, group_ids)?;
    let config = SessionConfig {
        group_ids: group_ids.to_vec(),
        group_names,
        scope: scope.as_str().to_string(),
        limit,
    };
    let mode = if scope == Scope::Mistake { "mistake" } else { "group" };
    insert(conn, mode, &config, &word_ids)
}

fn scope_empty_hint(scope: Scope) -> &'static str {
    match scope {
        Scope::Mistake => "错题本里还没有词，先做一次默写并批改出错的词",
        Scope::Unlearned => "所选分组里的词都已掌握，可以选「全部词」再复习一遍",
        Scope::All => "所选分组还没有词条，先去词语表添加",
    }
}

fn load_group_names(conn: &Connection, group_ids: &[i64]) -> AppResult<Vec<String>> {
    if group_ids.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for id in group_ids {
        let name: Option<String> = conn
            .query_row("SELECT name FROM groups WHERE id = ?1", params![id], |r| r.get(0))
            .ok();
        if let Some(name) = name {
            out.push(name);
        }
    }
    Ok(out)
}

fn insert(conn: &Connection, mode: &str, config: &SessionConfig, word_ids: &[i64]) -> AppResult<i64> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO sessions (mode, status, config_json, total, started_at)
         VALUES (?1, 'running', ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![mode, serde_json::to_string(config)?, word_ids.len() as i64],
    )?;
    let session_id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO session_items (session_id, word_id, seq, created_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )?;
        for (index, word_id) in word_ids.iter().enumerate() {
            stmt.execute(params![session_id, word_id, (index + 1) as i64])?;
        }
    }
    tx.commit()?;
    Ok(session_id)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<SessionDto> {
    let (mode, status, total, wrong_total, current_seq, started_at, finished_at, config_json) = conn
        .query_row(
            "SELECT mode, status, total, wrong_total, current_seq, started_at, finished_at, config_json
             FROM sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::msg("默写记录不存在"),
            other => AppError::Db(other),
        })?;

    let config = parse_config(&config_json);
    let mut stmt = conn.prepare(
        "SELECT i.id, i.seq, i.word_id, w.text, i.skip_marked, i.result
         FROM session_items i
         JOIN words w ON w.id = i.word_id
         WHERE i.session_id = ?1
         ORDER BY i.seq ASC",
    )?;
    let items = stmt
        .query_map(params![id], |r| {
            Ok(SessionItemDto {
                id: r.get(0)?,
                seq: r.get(1)?,
                word_id: r.get(2)?,
                text: r.get(3)?,
                skip_marked: r.get::<_, i64>(4)? == 1,
                result: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SessionDto {
        id,
        mode,
        status,
        total,
        wrong_total,
        current_seq,
        started_at,
        finished_at,
        group_label: group_label(&config),
        scope: config.scope,
        items,
    })
}

pub fn active(conn: &Connection) -> AppResult<Option<SessionDto>> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM sessions WHERE status IN ('running','reviewing') ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    match id {
        Some(id) => Ok(Some(get(conn, id)?)),
        None => Ok(None),
    }
}

/// 记录进度：current_seq 表示已推进到的位置；skip_item_id 不为空表示这个词孩子说不会
pub fn update_progress(
    conn: &Connection,
    session_id: i64,
    current_seq: i64,
    skip_item_id: Option<i64>,
) -> AppResult<()> {
    if let Some(item_id) = skip_item_id {
        conn.execute(
            "UPDATE session_items SET skip_marked = 1 WHERE id = ?1 AND session_id = ?2",
            params![item_id, session_id],
        )?;
    }
    conn.execute(
        "UPDATE sessions SET current_seq = ?1 WHERE id = ?2 AND status = 'running'",
        params![current_seq, session_id],
    )?;
    Ok(())
}

pub fn finish_quiz(conn: &Connection, session_id: i64) -> AppResult<SessionDto> {
    let changed = conn.execute(
        "UPDATE sessions SET status = 'reviewing' WHERE id = ?1 AND status = 'running'",
        params![session_id],
    )?;
    if changed == 0 {
        return Err(AppError::msg("这次默写已经结束，无法再次提交"));
    }
    get(conn, session_id)
}

pub fn abandon(conn: &Connection, session_id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET status = 'abandoned', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?1 AND status IN ('running','reviewing')",
        params![session_id],
    )?;
    Ok(())
}

/// 批改提交：写入每题对错、推进词条状态、关闭会话
pub fn submit_review(conn: &Connection, session_id: i64, wrong_item_ids: &[i64]) -> AppResult<ReviewResult> {
    let session = get(conn, session_id)?;
    if session.status != "reviewing" {
        return Err(AppError::msg("这次默写不在批改状态"));
    }
    let wrong_set: HashSet<i64> = wrong_item_ids.iter().copied().collect();
    let streak = settings::load(conn)?.streak_to_master;
    let mut graduated = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE session_items SET result = ?1 WHERE id = ?2")?;
        for item in &session.items {
            let wrong = wrong_set.contains(&item.id);
            stmt.execute(params![if wrong { "wrong" } else { "correct" }, item.id])?;
            if wrong {
                word_progress::mark_wrong(&tx, item.word_id)?;
            } else if word_progress::mark_correct(&tx, item.word_id, streak)? {
                graduated += 1;
            }
        }
    }
    let wrong_total = wrong_set.len() as i64;
    tx.execute(
        "UPDATE sessions SET status = 'finished', wrong_total = ?1,
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?2",
        params![wrong_total, session_id],
    )?;
    tx.commit()?;

    Ok(ReviewResult {
        wrong: wrong_total,
        correct: session.total - wrong_total,
        graduated,
    })
}

/// 用某次默写里的错词重新开一场
pub fn retry_wrong_words(conn: &Connection, session_id: i64) -> AppResult<i64> {
    let source = get(conn, session_id)?;
    let mut seen = HashSet::new();
    let word_ids: Vec<i64> = source
        .items
        .iter()
        .filter(|i| i.result == "wrong")
        .filter(|i| seen.insert(i.word_id))
        .map(|i| i.word_id)
        .collect();
    if word_ids.is_empty() {
        return Err(AppError::msg("这次默写没有错词"));
    }
    let config = SessionConfig {
        group_ids: vec![],
        group_names: vec![],
        scope: Scope::Mistake.as_str().to_string(),
        limit: word_ids.len() as i64,
    };
    insert(conn, "mistake", &config, &word_ids)
}

pub fn list(conn: &Connection, limit: i64) -> AppResult<Vec<SessionSummaryDto>> {
    let mut stmt = conn.prepare(
        "SELECT id, mode, status, total, wrong_total, started_at, finished_at, config_json
         FROM sessions
         WHERE status IN ('finished','reviewing','abandoned')
         ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, mode, status, total, wrong_total, started_at, finished_at, config_json) = row?;
        let config = parse_config(&config_json);
        out.push(SessionSummaryDto {
            id,
            mode,
            status,
            total,
            wrong_total,
            started_at,
            finished_at,
            group_label: group_label(&config),
            scope: config.scope,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::domain::sampler::Scope;
    use crate::repo::{groups, words};

    fn seed() -> (Connection, i64) {
        let conn = db::open_memory();
        let g = groups::create(&conn, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }
        (conn, g.id)
    }

    #[test]
    fn create_writes_items_in_order() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 2, false).unwrap();
        let s = get(&conn, id).unwrap();
        assert_eq!(s.total, 2);
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].seq, 1);
        assert_eq!(s.items[1].seq, 2);
        assert_eq!(s.status, "running");
        assert_eq!(s.group_label, "第一单元");
    }

    #[test]
    fn create_rejects_empty_pool() {
        let conn = db::open_memory();
        let g = groups::create(&conn, "1", "空分组").unwrap();
        let err = create(&conn, &[g.id], Scope::All, 0, false).unwrap_err();
        assert!(err.to_string().contains("还没有词条"));
    }

    #[test]
    fn submit_review_updates_words_and_session() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();

        let r = submit_review(&conn, id, &[s.items[0].id]).unwrap();
        assert_eq!(r.wrong, 1);
        assert_eq!(r.correct, 2);

        let after = get(&conn, id).unwrap();
        assert_eq!(after.status, "finished");
        assert_eq!(after.wrong_total, 1);
        assert_eq!(after.items[0].result, "wrong");
        assert_eq!(after.items[1].result, "correct");

        let mistakes = words::list_mistakes(&conn, None).unwrap();
        assert_eq!(mistakes.len(), 1);
        assert_eq!(mistakes[0].wrong_count, 1);
    }

    #[test]
    fn cannot_submit_twice() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 1, false).unwrap();
        finish_quiz(&conn, id).unwrap();
        submit_review(&conn, id, &[]).unwrap();
        assert!(submit_review(&conn, id, &[]).is_err());
    }

    #[test]
    fn progress_and_skip_are_persisted() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false).unwrap();
        let s = get(&conn, id).unwrap();
        update_progress(&conn, id, 2, Some(s.items[1].id)).unwrap();

        let after = get(&conn, id).unwrap();
        assert_eq!(after.current_seq, 2);
        assert!(after.items[1].skip_marked);
        assert!(!after.items[0].skip_marked);
    }

    #[test]
    fn active_returns_latest_unfinished() {
        let (conn, gid) = seed();
        assert!(active(&conn).unwrap().is_none());
        let id = create(&conn, &[gid], Scope::All, 1, false).unwrap();
        assert_eq!(active(&conn).unwrap().unwrap().id, id);
        abandon(&conn, id).unwrap();
        assert!(active(&conn).unwrap().is_none());
    }

    #[test]
    fn retry_builds_session_from_wrong_words() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();
        submit_review(&conn, id, &[s.items[0].id, s.items[1].id]).unwrap();

        let new_id = retry_wrong_words(&conn, id).unwrap();
        let retried = get(&conn, new_id).unwrap();
        assert_eq!(retried.total, 2);
        assert_eq!(retried.mode, "mistake");
        assert_eq!(retried.group_label, "错题本");
    }
}
