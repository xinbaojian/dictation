use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub name: String,
    /// 层级，根为 0
    pub depth: i64,
    /// 完整路径，如 "二年级上册 / 第一单元"
    pub path: String,
    /// 路径分段，如 ["二年级上册", "第一单元"]
    pub path_segments: Vec<String>,
    pub sort_order: i64,
    /// 本节点直属词条数
    pub word_count: i64,
    /// 含所有子分组
    pub total_word_count: i64,
    pub learning_count: i64,
    pub total_learning_count: i64,
    pub has_children: bool,
    pub last_practiced_at: Option<String>,
}

struct Node {
    id: i64,
    parent_id: Option<i64>,
    code: String,
    name: String,
    sort_order: i64,
}

#[derive(Default, Clone)]
struct Stat {
    words: i64,
    learning: i64,
    last: Option<String>,
}

const MAX_DEPTH: usize = 32;

pub fn list(conn: &Connection) -> AppResult<Vec<GroupDto>> {
    let mut nodes: HashMap<i64, Node> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT id, parent_id, code, name, sort_order FROM groups WHERE is_archived = 0")?;
        let rows = stmt.query_map([], |r| {
            Ok(Node {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                code: r.get(2)?,
                name: r.get(3)?,
                sort_order: r.get(4)?,
            })
        })?;
        for row in rows {
            let node = row?;
            nodes.insert(node.id, node);
        }
    }

    // 每个节点直属词条的统计
    let mut stats: HashMap<i64, Stat> = HashMap::new();
    let mut children_count: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT group_id, COUNT(*),
                    SUM(CASE WHEN status = 'learning' THEN 1 ELSE 0 END),
                    MAX(last_practiced_at)
             FROM words GROUP BY group_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (id, words, learning, last) = row?;
            stats.insert(id, Stat { words, learning, last });
        }
    }
    for node in nodes.values() {
        if let Some(parent) = node.parent_id {
            *children_count.entry(parent).or_insert(0) += 1;
        }
    }

    // 计算深度、路径与排序键（祖先链上的 sort_order 组成 DFS 顺序）
    struct Item {
        key: Vec<(i64, i64)>,
        path: String,
        segments: Vec<String>,
        depth: i64,
        node: Node,
    }
    let mut items: Vec<Item> = Vec::new();
    for node in nodes.values() {
        let mut chain: Vec<i64> = vec![node.id];
        let mut cursor = node.parent_id;
        let mut guard = 0;
        while let Some(parent_id) = cursor {
            guard += 1;
            if guard > MAX_DEPTH {
                break;
            }
            match nodes.get(&parent_id) {
                Some(parent) => {
                    chain.push(parent.id);
                    cursor = parent.parent_id;
                }
                None => break,
            }
        }
        chain.reverse();
        let key: Vec<(i64, i64)> = chain
            .iter()
            .map(|id| {
                let n = &nodes[id];
                (n.sort_order, n.id)
            })
            .collect();
        let segments: Vec<String> = chain.iter().map(|id| nodes[id].name.clone()).collect();
        let path = segments.join(" / ");
        items.push(Item {
            key,
            path,
            segments,
            depth: (chain.len() as i64) - 1,
            node: Node {
                id: node.id,
                parent_id: node.parent_id,
                code: node.code.clone(),
                name: node.name.clone(),
                sort_order: node.sort_order,
            },
        });
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));

    // 自底向上累加子分组的统计
    let mut totals: HashMap<i64, Stat> = HashMap::new();
    for item in items.iter().rev() {
        let mut acc = stats.get(&item.node.id).cloned().unwrap_or_default();
        for (child_id, child_total) in totals.iter() {
            let is_child = nodes
                .get(child_id)
                .map(|c| c.parent_id == Some(item.node.id))
                .unwrap_or(false);
            if is_child {
                acc.words += child_total.words;
                acc.learning += child_total.learning;
                acc.last = match (acc.last.take(), child_total.last.clone()) {
                    (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
                    (None, b) => b,
                    (a, None) => a,
                };
            }
        }
        totals.insert(item.node.id, acc);
    }

    Ok(items
        .into_iter()
        .map(|item| {
            let own = stats.get(&item.node.id).cloned().unwrap_or_default();
            let total = totals.get(&item.node.id).cloned().unwrap_or_default();
            GroupDto {
                id: item.node.id,
                parent_id: item.node.parent_id,
                code: item.node.code,
                name: item.node.name,
                depth: item.depth,
                path: item.path,
                path_segments: item.segments,
                sort_order: item.node.sort_order,
                word_count: own.words,
                total_word_count: total.words,
                learning_count: own.learning,
                total_learning_count: total.learning,
                has_children: children_count.get(&item.node.id).copied().unwrap_or(0) > 0,
                last_practiced_at: total.last,
            }
        })
        .collect())
}

pub fn get(conn: &Connection, id: i64) -> AppResult<GroupDto> {
    list(conn)?
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| AppError::msg("分组不存在或已被删除"))
}

fn next_sort_order(conn: &Connection, parent_id: Option<i64>) -> AppResult<i64> {
    let value: i64 = match parent_id {
        Some(parent) => conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM groups WHERE parent_id = ?1",
            params![parent],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM groups WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(value)
}

pub fn create(
    conn: &Connection,
    parent_id: Option<i64>,
    code: &str,
    name: &str,
) -> AppResult<GroupDto> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::msg("分组名称不能为空"));
    }
    if let Some(parent) = parent_id {
        let exists: i64 =
            conn.query_row("SELECT COUNT(*) FROM groups WHERE id = ?1", params![parent], |r| r.get(0))?;
        if exists == 0 {
            return Err(AppError::msg("上级分组不存在或已被删除"));
        }
    }
    let sort_order = next_sort_order(conn, parent_id)?;
    let code = match code.trim() {
        "" => (sort_order / 10).to_string(),
        c => c.to_string(),
    };
    conn.execute(
        "INSERT INTO groups (parent_id, code, name, sort_order, created_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![parent_id, code, name, sort_order],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, code: &str, name: &str) -> AppResult<GroupDto> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::msg("分组名称不能为空"));
    }
    let changed = conn.execute(
        "UPDATE groups SET code = ?1, name = ?2 WHERE id = ?3",
        params![code.trim(), name, id],
    )?;
    if changed == 0 {
        return Err(AppError::msg("分组不存在或已被删除"));
    }
    get(conn, id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub groups: i64,
    pub words: i64,
}

/// 连带子孙分组一起删除，结果供前端提示
pub fn delete(conn: &Connection, id: i64) -> AppResult<DeleteResult> {
    let mut subtree: Vec<i64> = vec![id];
    let mut cursor = 0;
    while cursor < subtree.len() {
        let current = subtree[cursor];
        cursor += 1;
        let mut stmt = conn.prepare("SELECT id FROM groups WHERE parent_id = ?1")?;
        let rows = stmt.query_map(params![current], |r| r.get::<_, i64>(0))?;
        for row in rows {
            let child = row?;
            if !subtree.contains(&child) {
                subtree.push(child);
            }
        }
    }
    if subtree.len() == 1 {
        let exists: i64 =
            conn.query_row("SELECT COUNT(*) FROM groups WHERE id = ?1", params![id], |r| r.get(0))?;
        if exists == 0 {
            return Err(AppError::msg("分组不存在或已被删除"));
        }
    }

    let placeholders = subtree.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT COUNT(*) FROM words WHERE group_id IN ({placeholders})");
    let word_count: i64 =
        conn.query_row(&sql, rusqlite::params_from_iter(subtree.iter()), |r| r.get(0))?;

    conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
    Ok(DeleteResult {
        groups: (subtree.len() as i64) - 1,
        words: word_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn builds_tree_paths_and_totals() {
        let conn = db::open_memory();
        let book = create(&conn, None, "1", "二年级上册").unwrap();
        let unit1 = create(&conn, Some(book.id), "1", "第一单元").unwrap();
        let unit2 = create(&conn, Some(book.id), "2", "第二单元").unwrap();
        crate::repo::words::add(&conn, unit1.id, "春天", None).unwrap();
        crate::repo::words::add(&conn, unit2.id, "夏天", None).unwrap();

        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "二年级上册");
        assert_eq!(list[0].depth, 0);
        assert_eq!(list[0].word_count, 0);
        assert_eq!(list[0].total_word_count, 2);
        assert!(list[0].has_children);
        assert_eq!(list[1].path, "二年级上册 / 第一单元");
        assert_eq!(list[1].path_segments, vec!["二年级上册", "第一单元"]);
        assert_eq!(list[1].depth, 1);
        assert_eq!(list[1].word_count, 1);
        assert!(!list[1].has_children);
    }

    #[test]
    fn delete_removes_subtree_and_words() {
        let conn = db::open_memory();
        let book = create(&conn, None, "1", "二年级上册").unwrap();
        let unit = create(&conn, Some(book.id), "1", "第一单元").unwrap();
        crate::repo::words::add(&conn, unit.id, "春天", None).unwrap();

        let r = delete(&conn, book.id).unwrap();
        assert_eq!(r.groups, 1);
        assert_eq!(r.words, 1);
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn empty_name_rejected() {
        let conn = db::open_memory();
        assert!(create(&conn, None, "1", "   ").is_err());
    }

    #[test]
    fn child_of_missing_parent_rejected() {
        let conn = db::open_memory();
        assert!(create(&conn, Some(9999), "1", "第一单元").is_err());
    }
}
