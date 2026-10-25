use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub sort_order: i64,
    pub word_count: i64,
    pub learning_count: i64,
    pub last_practiced_at: Option<String>,
}

const SELECT_ONE: &str = "
SELECT g.id, g.code, g.name, g.sort_order,
       (SELECT COUNT(*) FROM words w WHERE w.group_id = g.id),
       (SELECT COUNT(*) FROM words w WHERE w.group_id = g.id AND w.status = 'learning'),
       (SELECT MAX(w.last_practiced_at) FROM words w WHERE w.group_id = g.id)
FROM groups g
WHERE g.id = ?1";

pub fn list(conn: &Connection) -> AppResult<Vec<GroupDto>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.code, g.name, g.sort_order,
                (SELECT COUNT(*) FROM words w WHERE w.group_id = g.id),
                (SELECT COUNT(*) FROM words w WHERE w.group_id = g.id AND w.status = 'learning'),
                (SELECT MAX(w.last_practiced_at) FROM words w WHERE w.group_id = g.id)
         FROM groups g
         WHERE g.is_archived = 0
         ORDER BY g.sort_order ASC, g.id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(GroupDto {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            sort_order: r.get(3)?,
            word_count: r.get(4)?,
            learning_count: r.get(5)?,
            last_practiced_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<GroupDto> {
    conn.query_row(SELECT_ONE, params![id], |r| {
        Ok(GroupDto {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            sort_order: r.get(3)?,
            word_count: r.get(4)?,
            learning_count: r.get(5)?,
            last_practiced_at: r.get(6)?,
        })
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => AppError::msg("分组不存在或已被删除"),
        other => AppError::Db(other),
    })
}

pub fn create(conn: &Connection, code: &str, name: &str) -> AppResult<GroupDto> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::msg("分组名称不能为空"));
    }
    let sort_order: i64 =
        conn.query_row("SELECT COALESCE(MAX(sort_order), 0) + 10 FROM groups", [], |r| r.get(0))?;
    let code = match code.trim() {
        "" => (sort_order / 10).to_string(),
        c => c.to_string(),
    };
    conn.execute(
        "INSERT INTO groups (code, name, sort_order, created_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![code, name, sort_order],
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

/// 返回被一并删除的词条数，供前端提示
pub fn delete(conn: &Connection, id: i64) -> AppResult<i64> {
    let word_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM words WHERE group_id = ?1", params![id], |r| r.get(0))?;
    let changed = conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::msg("分组不存在或已被删除"));
    }
    Ok(word_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn create_and_list_in_order() {
        let conn = db::open_memory();
        create(&conn, "", "第一单元").unwrap();
        create(&conn, "2", "第二单元").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].code, "1");
        assert_eq!(list[1].name, "第二单元");
        assert_eq!(list[0].word_count, 0);
    }

    #[test]
    fn delete_cascades_words() {
        let conn = db::open_memory();
        let g = create(&conn, "1", "第一单元").unwrap();
        crate::repo::words::add(&conn, g.id, "春天", None).unwrap();
        assert_eq!(delete(&conn, g.id).unwrap(), 1);
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn empty_name_rejected() {
        let conn = db::open_memory();
        assert!(create(&conn, "1", "   ").is_err());
    }
}
