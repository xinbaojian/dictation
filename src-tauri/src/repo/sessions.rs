use crate::domain::sampler::{self, Candidate, Scope};
use crate::domain::text::{format_char_indices, parse_char_indices};
use crate::error::{AppError, AppResult};
use crate::repo::{settings, word_progress};
use rand::seq::SliceRandom;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    #[serde(default)]
    pub options: SampleOptions,
}

/// 抽词偏好：前端「抽词偏好」多选项，全不选 = 纯随机
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SampleOptions {
    /// 含历次标错的字优先
    pub weak_first: bool,
    /// 没默写过的新词优先
    pub new_first: bool,
    /// 最近一场写过的词排后
    pub last_back: bool,
    /// 同一批尽量不重复用字
    pub diverse: bool,
    /// 连字：下一个词优先和上一个词有相同的字
    pub chain: bool,
}

impl Default for SampleOptions {
    fn default() -> Self {
        Self { weak_first: true, new_first: true, last_back: true, diverse: true, chain: false }
    }
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
    /// 批改时标出的「不会写的字」下标（0-based），空数组表示整词算错
    pub wrong_chars: Vec<i64>,
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

/// 批改时提交的一项：item_id + 不会写的字下标（空 = 整词算错）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrongMark {
    pub item_id: i64,
    #[serde(default)]
    pub chars: Vec<i64>,
}

pub fn group_label(config: &SessionConfig) -> String {
    if config.scope == Scope::Mistake.as_str() {
        return "错题本".to_string();
    }
    match config.group_names.len() {
        0 => "全部词语".to_string(),
        1 => config.group_names[0].clone(),
        2 => config.group_names.join("、"),
        n => format!("{} 等 {} 组", config.group_names[0], n),
    }
}

fn parse_config(raw: &str) -> SessionConfig {
    serde_json::from_str(raw).unwrap_or(SessionConfig {
        group_ids: vec![],
        group_names: vec![],
        scope: "all".into(),
        limit: 0,
        options: SampleOptions::default(),
    })
}

/// 建会话：偏好全关时就是纯随机抽词
pub fn create(
    conn: &Connection,
    group_ids: &[i64],
    scope: Scope,
    limit: i64,
    shuffle: bool,
    options: SampleOptions,
) -> AppResult<i64> {
    let mut candidates = sampler::collect(conn, group_ids, scope)?;
    if candidates.is_empty() {
        return Err(AppError::msg(scope_empty_hint(scope)));
    }
    // 错题本模式不排除上次的错词：上次错的正是这次要练的
    let practiced = if scope == Scope::Mistake || !options.last_back {
        HashSet::new()
    } else {
        last_practiced_texts(conn)?
    };
    // 历次批改里点过「不会写」的字，用来把含这些字的词提前
    let weak = if options.weak_first {
        weak_chars(conn)?
    } else {
        HashSet::new()
    };
    if shuffle {
        let mut rng = rand::thread_rng();
        candidates = if options.chain {
            // 「连字」：优先让下一个词和上一个词有相同的字，整体顺序仍然随机
            sampler::chain_by_shared_char(candidates, &mut rng)
        } else {
            candidates.shuffle(&mut rng);
            candidates
        };
    }
    // 排序优先级：含写错过的字 > 新词 > 其余；同一档内保持打散后的顺序。
    // 上一场刚默写过的词只在本档内往后排，不从池子里删：删掉会让「题量 20」在 20 个词的
    // 分组里只抽出 10 个，看起来像题量没生效；词不够时宁可重复也要凑够用户选的题量。
    let candidates = prioritize(candidates, &weak, &practiced, &options);
    // 题量有限时按「尽量不重复用字」挑，顺序仍按洗牌/连字后的原次序
    let candidates = if options.diverse {
        pick_diverse(candidates, limit as usize, &weak, &options)
    } else if limit > 0 {
        candidates.into_iter().take(limit as usize).collect()
    } else {
        candidates
    };
    let word_ids: Vec<i64> = candidates.iter().map(|c| c.word_id).collect();

    let group_names = load_group_names(conn, group_ids)?;
    let config = SessionConfig {
        group_ids: group_ids.to_vec(),
        group_names,
        scope: scope.as_str().to_string(),
        limit,
        options,
    };
    let mode = if scope == Scope::Mistake { "mistake" } else { "group" };
    insert(conn, mode, &config, &word_ids)
}

/// 抽词排序：先按档位，再按「最近一场刚写过」。
///
/// 档位：0 = 含历次批改里标错的字，1 = 新词（没默写过），2 = 其余已练过的词。
/// 用稳定排序，所以同一档内保留入参顺序（洗过牌就是随机，没洗就是词表顺序），
/// 不会把随机性排没。只排序不过滤——过滤会让可抽数量凭空变少，用户选的题量就对不上了。
///
/// 「最近一场」匹配按**词语文本**而不是 word_id：同一个词写在两个分组里是两条记录，
/// 上一场抽到 A 组的「春天」，这一场抽到 B 组的「春天」其实还是同一个词。
fn prioritize(
    mut candidates: Vec<Candidate>,
    weak: &HashSet<char>,
    practiced: &HashSet<String>,
    options: &SampleOptions,
) -> Vec<Candidate> {
    candidates.sort_by_key(|c| (tier_of(c, weak, options), practiced.contains(&c.text)));
    candidates
}

/// 档位：0 = 含写错过的字，1 = 新词，2 = 其余已练过的词。
/// 对应偏好关掉时该档直接并入 2，等于不参与排序。
fn tier_of(c: &Candidate, weak: &HashSet<char>, options: &SampleOptions) -> u8 {
    if options.weak_first && c.text.chars().any(|ch| weak.contains(&ch)) {
        0
    } else if options.new_first && c.is_new {
        1
    } else {
        2
    }
}

/// 从已排好序的候选里挑 `limit` 个（0 = 不限），尽量不让同批词共用一个字。
///
/// 逐档取词：高优先级先占满名额，只有同一档里词数超过名额时才做取舍——
/// 先贪心挑「和已选词没有共同字」的，凑不满再按原顺序把同档剩下的补上。
/// 结果保持入参顺序，随机性和连字顺序都不会被挑词打乱。
fn pick_diverse(
    candidates: Vec<Candidate>,
    limit: usize,
    weak: &HashSet<char>,
    options: &SampleOptions,
) -> Vec<Candidate> {
    if limit == 0 || candidates.len() <= limit {
        return candidates;
    }
    let mut out = Vec::with_capacity(limit);
    let mut used: HashSet<char> = HashSet::new();
    let mut left = limit;
    let mut start = 0;
    while start < candidates.len() && left > 0 {
        let tier = tier_of(&candidates[start], weak, options);
        let mut end = start;
        while end < candidates.len() && tier_of(&candidates[end], weak, options) == tier {
            end += 1;
        }
        let take = left.min(end - start);
        out.extend(select_in_tier(&candidates[start..end], take, &mut used));
        left -= take;
        start = end;
    }
    out
}

/// 同一档内挑 `take` 个：先挑字不冲突的，不够再按原顺序补齐
fn select_in_tier(segment: &[Candidate], take: usize, used: &mut HashSet<char>) -> Vec<Candidate> {
    let mut taken = vec![false; segment.len()];
    // 本轮已经用掉的字（含前几档的），边挑边并，保证同批不撞字
    let mut seen = used.clone();
    let mut count = 0;
    for (i, c) in segment.iter().enumerate() {
        if count == take {
            break;
        }
        let chars = sampler::char_set(&c.text);
        if chars.is_disjoint(&seen) {
            seen.extend(chars);
            taken[i] = true;
            count += 1;
        }
    }
    if count < take {
        for (i, _) in segment.iter().enumerate() {
            if count == take {
                break;
            }
            if !taken[i] {
                taken[i] = true;
                count += 1;
            }
        }
    }
    // 选中的字全部计入已用：下一档挑词时也要避开
    for (i, c) in segment.iter().enumerate() {
        if taken[i] {
            used.extend(sampler::char_set(&c.text));
        }
    }
    segment
        .iter()
        .zip(taken)
        .filter(|(_, hit)| *hit)
        .map(|(c, _)| c.clone())
        .collect()
}

/// 历次批改里标过「不会写」的字（跨词聚合）。
///
/// 来源有两处：批改时点出的字（session_items）和手工加入错题本时标的字（words）。
/// 整词算错的条目 wrong_chars 为空，没有位置信息，跳过；
/// 词条后来被改短的话老下标会越界，直接丢掉，避免把别的字当成弱字。
fn weak_chars(conn: &Connection) -> AppResult<HashSet<char>> {
    let mut stmt = conn.prepare(
        "SELECT w.text, i.wrong_chars
         FROM session_items i
         JOIN words w ON w.id = i.word_id
         WHERE i.result = 'wrong' AND i.wrong_chars <> ''
         UNION ALL
         SELECT w.text, w.wrong_chars FROM words w WHERE w.wrong_chars <> ''",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashSet::new();
    for row in rows {
        let (text, raw) = row?;
        let chars: Vec<char> = text.chars().collect();
        for idx in parse_char_indices(&raw) {
            let Some(ch) = usize::try_from(idx).ok().and_then(|i| chars.get(i)) else {
                continue;
            };
            if !ch.is_whitespace() {
                out.insert(*ch);
            }
        }
    }
    Ok(out)
}

/// 最近一场（已批改 / 待批改 / 已放弃）默写出现过的词语文本
fn last_practiced_texts(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT w.text
         FROM session_items i
         JOIN words w ON w.id = i.word_id
         WHERE i.session_id = (
             SELECT id FROM sessions
             WHERE status IN ('finished','reviewing','abandoned')
             ORDER BY id DESC LIMIT 1
         )",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<HashSet<_>, _>>()?)
}

fn scope_empty_hint(scope: Scope) -> &'static str {
    match scope {
        Scope::Mistake => "错题本里还没有词，先做一次默写并批改出错的词",
        Scope::Unlearned => "所选分组里的词都已掌握，可以选「全部词」再复习一遍",
        Scope::All => "所选分组还没有词条，先去词语表添加",
    }
}

/// 记录完整路径（"二年级上册 / 第一单元"），历史里看得懂是哪一册哪一单元
fn load_group_names(conn: &Connection, group_ids: &[i64]) -> AppResult<Vec<String>> {
    if group_ids.is_empty() {
        return Ok(vec![]);
    }
    let all = super::groups::list(conn)?;
    let mut out = Vec::new();
    for id in group_ids {
        if let Some(group) = all.iter().find(|g| g.id == *id) {
            out.push(group.path.clone());
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
        "SELECT i.id, i.seq, i.word_id, w.text, i.skip_marked, i.result, i.wrong_chars
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
                wrong_chars: parse_char_indices(&r.get::<_, String>(6)?),
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

/// 删除一条默写记录：`session_items` 由外键级联删掉。
///
/// 只删记录本身，**不回滚词条进度**——错题本状态、连对次数是学习结果，
/// 不该因为「历史记录里这条不好看」而一起消失。
pub fn delete(conn: &Connection, session_id: i64) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
    if changed == 0 {
        return Err(AppError::msg("默写记录不存在或已被删除"));
    }
    Ok(())
}

/// 批改提交：写入每题对错、推进词条状态、关闭会话
pub fn submit_review(
    conn: &Connection,
    session_id: i64,
    wrong_items: &[WrongMark],
) -> AppResult<ReviewResult> {
    let session = get(conn, session_id)?;
    if session.status != "reviewing" {
        return Err(AppError::msg("这次默写不在批改状态"));
    }
    // item_id -> 不会写的字下标（空串 = 整词算错）
    let mut wrong_map: HashMap<i64, String> = HashMap::new();
    for mark in wrong_items {
        wrong_map.insert(mark.item_id, format_char_indices(&mark.chars));
    }
    let streak = settings::load(conn)?.streak_to_master;
    let mut graduated = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE session_items SET result = ?1, wrong_chars = ?2 WHERE id = ?3")?;
        for item in &session.items {
            match wrong_map.get(&item.id) {
                Some(chars) => {
                    stmt.execute(params!["wrong", chars, item.id])?;
                    word_progress::mark_wrong(&tx, item.word_id)?;
                }
                None => {
                    stmt.execute(params!["correct", "", item.id])?;
                    if word_progress::mark_correct(&tx, item.word_id, streak)? {
                        graduated += 1;
                    }
                }
            }
        }
    }
    // 只统计本次会话里真实存在的条目，避免客户端多传 id 影响计数
    let wrong_total = session.items.iter().filter(|i| wrong_map.contains_key(&i.id)).count() as i64;
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
        options: SampleOptions::default(),
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

    fn marks(ids: &[i64]) -> Vec<WrongMark> {
        ids.iter().map(|id| WrongMark { item_id: *id, chars: vec![] }).collect()
    }

    fn seed() -> (Connection, i64) {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }
        (conn, g.id)
    }

    #[test]
    fn create_writes_items_in_order() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 2, false, SampleOptions::default()).unwrap();
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
        let g = groups::create(&conn, None, "1", "空分组").unwrap();
        let err = create(&conn, &[g.id], Scope::All, 0, false, SampleOptions::default()).unwrap_err();
        assert!(err.to_string().contains("还没有词条"));
    }

    #[test]
    fn submit_review_updates_words_and_session() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();

        let r = submit_review(&conn, id, &marks(&[s.items[0].id])).unwrap();
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
    fn wrong_chars_are_stored_per_item() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();

        let target = s.items[0].id;
        submit_review(
            &conn,
            id,
            &[WrongMark { item_id: target, chars: vec![1, 0, 1] }],
        )
        .unwrap();

        let after = get(&conn, id).unwrap();
        assert_eq!(after.items[0].wrong_chars, vec![0, 1], "下标应去重且升序");
        assert!(after.items[1].wrong_chars.is_empty());
        assert_eq!(after.wrong_total, 1);
    }

    #[test]
    fn cannot_submit_twice() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 1, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, id).unwrap();
        submit_review(&conn, id, &[]).unwrap();
        assert!(submit_review(&conn, id, &[]).is_err());
    }

    #[test]
    fn progress_and_skip_are_persisted() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
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
        let id = create(&conn, &[gid], Scope::All, 1, false, SampleOptions::default()).unwrap();
        assert_eq!(active(&conn).unwrap().unwrap().id, id);
        abandon(&conn, id).unwrap();
        assert!(active(&conn).unwrap().is_none());
    }

    #[test]
    fn retry_builds_session_from_wrong_words() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();
        submit_review(&conn, id, &marks(&[s.items[0].id, s.items[1].id])).unwrap();

        let new_id = retry_wrong_words(&conn, id).unwrap();
        let retried = get(&conn, new_id).unwrap();
        assert_eq!(retried.total, 2);
        assert_eq!(retried.mode, "mistake");
        assert_eq!(retried.group_label, "错题本");
    }

    fn texts_of(conn: &Connection, session_id: i64) -> Vec<String> {
        get(conn, session_id)
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.text)
            .collect()
    }

    #[test]
    fn new_round_prefers_unpracticed_words() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天", "冬天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }

        let first = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        // 题量一样是 2，但这两题应该先给上一场没默写过的词
        let second = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        let rest = texts_of(&conn, second);
        assert_eq!(rest, vec!["秋天", "冬天"]);
    }
    #[test]
    fn new_words_come_before_mastered_ones() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }

        // 第一场把「春天」「夏天」练成熟词，「秋天」还是新词
        let first = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        let second = create(&conn, &[g.id], Scope::All, 1, false, SampleOptions::default()).unwrap();
        assert_eq!(texts_of(&conn, second), vec!["秋天"], "新词应优先于已会词");
    }

    #[test]
    fn words_with_weak_chars_are_sampled_first() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "天空", "月亮"] {
            words::add(&conn, g.id, w, None).unwrap();
        }

        // 批改时标出「春天」的「天」不会写
        let first = create(&conn, &[g.id], Scope::All, 0, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        let spring = get(&conn, first)
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.text == "春天")
            .unwrap()
            .id;
        submit_review(&conn, first, &[WrongMark { item_id: spring, chars: vec![1] }]).unwrap();

        // 下一场只抽 2 个：含「天」的两个词排在「月亮」前面
        let second = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        assert_eq!(texts_of(&conn, second), vec!["春天", "天空"]);
    }
    fn cand(text: &str, is_new: bool) -> Candidate {
        Candidate { word_id: 0, text: text.to_string(), group_id: 1, is_new }
    }

    fn texts_of_cands(picked: &[Candidate]) -> Vec<String> {
        picked.iter().map(|c| c.text.clone()).collect()
    }

    #[test]
    fn batch_prefers_distinct_chars() {
        let weak: HashSet<char> = HashSet::new();
        let list = vec![
            cand("春天", true),
            cand("天空", true),
            cand("月亮", true),
            cand("书包", true),
        ];
        // 天空和春天共用「天」，名额 3 时换成月亮、书包
        let picked = pick_diverse(list, 3, &weak, &SampleOptions::default());
        assert_eq!(texts_of_cands(&picked), vec!["春天", "月亮", "书包"]);
    }

    #[test]
    fn batch_falls_back_when_chars_run_out() {
        let weak: HashSet<char> = HashSet::new();
        let list = vec![cand("春天", true), cand("天空", true), cand("今天", true)];
        // 三个词都带「天」，第二轮按原顺序补，不能少抽
        let picked = pick_diverse(list, 2, &weak, &SampleOptions::default());
        assert_eq!(texts_of_cands(&picked), vec!["春天", "天空"]);
    }

    #[test]
    fn diversity_keeps_priority_tiers() {
        let weak: HashSet<char> = "天".chars().collect();
        // 已按档排好：两个含「天」的弱字词在前，两个无关的新词在后
        let list = vec![
            cand("春天", false),
            cand("天空", false),
            cand("月亮", true),
            cand("书包", true),
        ];
        let picked = pick_diverse(list, 3, &weak, &SampleOptions::default());
        assert_eq!(
            texts_of_cands(&picked),
            vec!["春天", "天空", "月亮"],
            "同档词不够时宁可重复字，也不能跳过弱字词去凑不重复"
        );
    }
    #[test]
    fn manual_char_marks_feed_weak_pool() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        words::add_mistake(&conn, g.id, "春天", &[1]).unwrap();

        let weak = weak_chars(&conn).unwrap();
        assert!(weak.contains(&'天'), "手工标的错字也要进弱字池");
    }

    #[test]
    fn no_limit_keeps_every_candidate() {
        let weak: HashSet<char> = HashSet::new();
        let list = vec![cand("春天", true), cand("天空", true)];
        assert_eq!(pick_diverse(list.clone(), 0, &weak, &SampleOptions::default()).len(), 2);
        assert_eq!(pick_diverse(list, 5, &weak, &SampleOptions::default()).len(), 2);
    }
    #[test]
    fn all_preferences_off_is_plain_random() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天", "冬天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }
        let first = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        let off = SampleOptions {
            weak_first: false,
            new_first: false,
            last_back: false,
            diverse: false,
            chain: false,
        };
        let second = create(&conn, &[g.id], Scope::All, 2, false, off).unwrap();
        // 偏好全关 + 不洗牌：原样取前两个，上一场刚写过也照样来
        assert_eq!(texts_of(&conn, second), vec!["春天", "夏天"]);
    }

    #[test]
    fn limit_is_not_shrunk_by_last_session() {
        let conn = db::open_memory();
        let g = groups::create(&conn, None, "1", "第一单元").unwrap();
        for w in ["春天", "夏天", "秋天", "冬天"] {
            words::add(&conn, g.id, w, None).unwrap();
        }

        let first = create(&conn, &[g.id], Scope::All, 2, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        // 选「全部」就是 4 个词，不能因为上一场写过两个就只剩两个
        let second = create(&conn, &[g.id], Scope::All, 0, false, SampleOptions::default()).unwrap();
        assert_eq!(get(&conn, second).unwrap().total, 4);
        // 选「3 词」也要抽满 3 个：先给没写过的两个，再补一个写过的
        let third = create(&conn, &[g.id], Scope::All, 3, false, SampleOptions::default()).unwrap();
        let texts = texts_of(&conn, third);
        assert_eq!(texts.len(), 3);
        assert!(texts[..2].iter().all(|t| *t == "秋天" || *t == "冬天"));
    }

    #[test]
    fn new_round_falls_back_when_everything_was_practiced() {
        let (conn, gid) = seed();
        let first = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        let second = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        assert_eq!(get(&conn, second).unwrap().total, 3, "全练过时回退成不排除，不能开不了局");
    }

    #[test]
    fn new_round_excludes_by_text_across_groups() {
        let conn = db::open_memory();
        let g1 = groups::create(&conn, None, "1", "第一单元").unwrap();
        let g2 = groups::create(&conn, None, "2", "第二单元").unwrap();
        words::add(&conn, g1.id, "春天", None).unwrap();
        words::add(&conn, g1.id, "夏天", None).unwrap();
        words::add(&conn, g2.id, "春天", None).unwrap();
        words::add(&conn, g2.id, "秋天", None).unwrap();

        // 第一场只抽 g1，两个词都默写过
        let first = create(&conn, &[g1.id], Scope::All, 0, false, SampleOptions::default()).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &[]).unwrap();

        // 第二场把两个组都选上、只抽 1 个：g2 的那条「春天」是同一个词，
        // 上一场写过就不该再抽到，应该给没写过的「秋天」
        let second = create(&conn, &[g1.id, g2.id], Scope::All, 1, false, SampleOptions::default()).unwrap();
        assert_eq!(texts_of(&conn, second), vec!["秋天"]);
    }

    #[test]
    fn mistake_mode_does_not_skip_last_session() {
        let (conn, gid) = seed();
        let first = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        let s = get(&conn, first).unwrap();
        finish_quiz(&conn, first).unwrap();
        submit_review(&conn, first, &marks(&[s.items[0].id])).unwrap();

        let second = create(&conn, &[], Scope::Mistake, 0, true, SampleOptions::default()).unwrap();
        let after = get(&conn, second).unwrap();
        assert_eq!(after.total, 1, "错题本里上次错的词不能被排除");
        assert_eq!(after.items[0].text, s.items[0].text);
    }

    #[test]
    fn chain_preference_keeps_every_word_once() {
        let (conn, gid) = seed();
        let options = SampleOptions { chain: true, ..Default::default() };

        let id = create(&conn, &[gid], Scope::All, 0, true, options).unwrap();
        let mut texts: Vec<String> = get(&conn, id).unwrap().items.into_iter().map(|i| i.text).collect();
        texts.sort();
        assert_eq!(texts, vec!["夏天", "春天", "秋天"], "连字只改顺序，不能漏词或重复");
    }

    #[test]
    fn default_preferences_match_ui() {
        let o = SampleOptions::default();
        assert!(o.weak_first && o.new_first && o.last_back && o.diverse);
        assert!(!o.chain, "连字默认不选");
    }

    #[test]
    fn delete_removes_session_and_items_only() {
        let (conn, gid) = seed();
        let id = create(&conn, &[gid], Scope::All, 0, false, SampleOptions::default()).unwrap();
        let s = get(&conn, id).unwrap();
        finish_quiz(&conn, id).unwrap();
        submit_review(&conn, id, &marks(&[s.items[0].id])).unwrap();
        assert_eq!(words::list_mistakes(&conn, None).unwrap().len(), 1);

        delete(&conn, id).unwrap();

        assert!(get(&conn, id).is_err(), "记录应该没了");
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_items WHERE session_id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 0, "session_items 应随记录级联删除");
        // 词条本身和它的错题状态不受影响
        assert_eq!(words::list(&conn, gid).unwrap().len(), 3);
        assert_eq!(words::list_mistakes(&conn, None).unwrap().len(), 1, "删记录不该把词移出错题本");
    }

    #[test]
    fn delete_missing_session_rejected() {
        let conn = db::open_memory();
        assert!(delete(&conn, 9999).is_err());
    }
}
