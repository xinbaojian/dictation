use crate::error::{AppError, AppResult};
use rand::seq::SliceRandom;
use rand::Rng;
use rusqlite::Connection;
use std::collections::HashSet;

/// 抽题范围
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 全部词
    All,
    /// 未掌握（新词 + 错题）
    Unlearned,
    /// 错题本
    Mistake,
}

impl Scope {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "all" => Ok(Scope::All),
            "unlearned" => Ok(Scope::Unlearned),
            "mistake" => Ok(Scope::Mistake),
            other => Err(AppError::msg(format!("未知的抽题范围：{other}"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::All => "all",
            Scope::Unlearned => "unlearned",
            Scope::Mistake => "mistake",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word_id: i64,
    pub text: String,
    pub group_id: i64,
}

/// 候选词集合。
///
/// - group_ids 为空表示全部分组（错题本模式常用）
/// - 跨组按词语文本去重，保留分组顺序靠前的一条，避免一场默写重复写同一个词
/// 选了父分组就等于选了它下面所有子分组
pub fn expand_group_ids(conn: &Connection, group_ids: &[i64]) -> AppResult<Vec<i64>> {
    if group_ids.is_empty() {
        return Ok(vec![]);
    }
    let mut parents: Vec<(i64, Option<i64>)> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id, parent_id FROM groups")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))?;
        for row in rows {
            parents.push(row?);
        }
    }
    let mut out: Vec<i64> = group_ids.to_vec();
    let mut cursor = 0;
    while cursor < out.len() {
        let current = out[cursor];
        cursor += 1;
        for (id, parent) in &parents {
            if *parent == Some(current) && !out.contains(id) {
                out.push(*id);
            }
        }
    }
    Ok(out)
}

pub fn collect(conn: &Connection, group_ids: &[i64], scope: Scope) -> AppResult<Vec<Candidate>> {
    let group_ids = expand_group_ids(conn, group_ids)?;
    let group_ids = group_ids.as_slice();
    let mut stmt = conn.prepare(
        "SELECT w.id, w.text, w.group_id, w.status
         FROM words w
         JOIN groups g ON g.id = w.group_id
         WHERE g.is_archived = 0
         ORDER BY g.sort_order ASC, g.id ASC, w.id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let (word_id, text, group_id, status) = row?;
        if !group_ids.is_empty() && !group_ids.contains(&group_id) {
            continue;
        }
        let in_scope = match scope {
            Scope::All => true,
            Scope::Unlearned => status != "mastered",
            Scope::Mistake => status == "learning",
        };
        if !in_scope {
            continue;
        }
        if !seen.insert(text.clone()) {
            continue;
        }
        out.push(Candidate { word_id, text, group_id });
    }
    Ok(out)
}

/// 词语里的字集合，用于「连字」判断（空白不算字）
fn char_set(text: &str) -> HashSet<char> {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 连字排序：先随机打散，再从随机起点出发，每一步优先挑「和上一个词有相同字」的候选。
///
/// 没有任何候选共享字时退化成随机挑一个，所以结果始终是入参的一个排列——
/// 不漏词、不重复。对应设置项「下一个词优先和上一个词有相同的字」。
pub fn chain_by_shared_char(mut words: Vec<Candidate>, rng: &mut impl Rng) -> Vec<Candidate> {
    words.shuffle(rng);
    let mut remaining = words;
    // 字集合与 remaining 同下标，swap_remove 时一起换，避免每步重复建集合
    let mut sets: Vec<HashSet<char>> = remaining.iter().map(|c| char_set(&c.text)).collect();
    let mut out: Vec<Candidate> = Vec::with_capacity(remaining.len());
    let mut last: Option<HashSet<char>> = None;
    while !remaining.is_empty() {
        let picked = match &last {
            Some(set) => {
                let shared: Vec<usize> = sets
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.iter().any(|ch| set.contains(ch)))
                    .map(|(i, _)| i)
                    .collect();
                if shared.is_empty() {
                    rng.gen_range(0..remaining.len())
                } else {
                    shared[rng.gen_range(0..shared.len())]
                }
            }
            None => rng.gen_range(0..remaining.len()),
        };
        let next = remaining.swap_remove(picked);
        last = Some(sets.swap_remove(picked));
        out.push(next);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::repo::{groups, words};

    fn seed() -> (Connection, i64, i64) {
        let conn = db::open_memory();
        let g1 = groups::create(&conn, None, "1", "第一单元").unwrap();
        let g2 = groups::create(&conn, None, "2", "第二单元").unwrap();
        words::add(&conn, g1.id, "春天", None).unwrap();
        words::add(&conn, g1.id, "夏天", None).unwrap();
        words::add(&conn, g2.id, "春天", None).unwrap();
        words::add(&conn, g2.id, "秋天", None).unwrap();
        (conn, g1.id, g2.id)
    }

    #[test]
    fn dedupes_same_word_across_groups() {
        let (conn, g1, g2) = seed();
        let list = collect(&conn, &[g1, g2], Scope::All).unwrap();
        assert_eq!(list.iter().filter(|c| c.text == "春天").count(), 1);
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn filters_by_group() {
        let (conn, g1, _) = seed();
        let list = collect(&conn, &[g1], Scope::All).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn empty_group_filter_means_all_groups() {
        let (conn, _, _) = seed();
        assert_eq!(collect(&conn, &[], Scope::All).unwrap().len(), 3);
    }

    #[test]
    fn scope_filters_by_status() {
        let (conn, g1, g2) = seed();
        let id = collect(&conn, &[g1], Scope::All).unwrap()[0].word_id;
        crate::repo::word_progress::mark_wrong(&conn, id).unwrap();

        assert_eq!(collect(&conn, &[g1, g2], Scope::Mistake).unwrap().len(), 1);
        assert_eq!(collect(&conn, &[g1, g2], Scope::Unlearned).unwrap().len(), 3);

        // 跨组去重后仍有一个重复词没被抽到，这里直接把所有词都置为已掌握
        let ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM words").unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0)).unwrap();
            rows.collect::<Result<Vec<_>, _>>().unwrap()
        };
        for id in ids {
            crate::repo::word_progress::mark_correct(&conn, id, 1).unwrap();
        }
        assert_eq!(collect(&conn, &[g1, g2], Scope::Unlearned).unwrap().len(), 0);
        assert_eq!(collect(&conn, &[g1, g2], Scope::All).unwrap().len(), 3);
    }

    #[test]
    fn selecting_parent_includes_children() {
        let conn = db::open_memory();
        let book = groups::create(&conn, None, "1", "二年级上册").unwrap();
        let unit1 = groups::create(&conn, Some(book.id), "1", "第一单元").unwrap();
        let unit2 = groups::create(&conn, Some(book.id), "2", "第二单元").unwrap();
        crate::repo::words::add(&conn, unit1.id, "春天", None).unwrap();
        crate::repo::words::add(&conn, unit2.id, "夏天", None).unwrap();

        let list = collect(&conn, &[book.id], Scope::All).unwrap();
        assert_eq!(list.len(), 2);

        let only_unit1 = collect(&conn, &[unit1.id], Scope::All).unwrap();
        assert_eq!(only_unit1.len(), 1);
    }

    #[test]
    fn invalid_scope_rejected() {
        assert!(Scope::parse("nope").is_err());
    }

    fn cand(text: &str, id: i64) -> Candidate {
        Candidate { word_id: id, text: text.to_string(), group_id: 1 }
    }

    /// 4 个词共享「天」，1 个孤立词。连字后 4 个共享词必须连续，
    /// 孤立词只能被挤到开头或结尾（与种子无关，可放心断言）
    #[test]
    fn chain_keeps_shared_words_together() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        for seed in 0..20 {
            let words = vec![
                cand("春天", 1),
                cand("月亮", 2),
                cand("秋天", 3),
                cand("夏天", 4),
                cand("冬天", 5),
            ];
            let mut rng = StdRng::seed_from_u64(seed);
            let out = chain_by_shared_char(words, &mut rng);

            assert_eq!(out.len(), 5, "不能漏词");
            let isolated = out.iter().position(|c| c.text == "月亮").unwrap();
            assert!(
                isolated == 0 || isolated == 4,
                "孤立词应被挤到首尾，实际第 {isolated} 位：{:?}",
                out.iter().map(|c| &c.text).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn chain_without_shared_chars_is_still_a_permutation() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        let words = vec![cand("大海", 1), cand("月亮", 2), cand("苹果", 3)];
        let mut rng = StdRng::seed_from_u64(42);
        let out = chain_by_shared_char(words, &mut rng);
        let mut texts: Vec<&str> = out.iter().map(|c| c.text.as_str()).collect();
        texts.sort_unstable();
        assert_eq!(texts, vec!["大海", "月亮", "苹果"]);
    }

    #[test]
    fn chain_handles_zero_and_one() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        let mut rng = StdRng::seed_from_u64(1);
        assert!(chain_by_shared_char(vec![], &mut rng).is_empty());
        let one = chain_by_shared_char(vec![cand("春天", 1)], &mut rng);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].text, "春天");
    }
}
