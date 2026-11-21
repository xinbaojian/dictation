//! 口算上机做题：一张卷（`math_sessions`）+ 每题作答（`math_items`）。
//!
//! 两条约定：
//! 1. **开卷即快照**：30 道题在 `create` 时就写进 `math_items`，之后读卷、续做、回看
//!    都不再跑出题引擎——以后改算法也不会让历史卷变样。
//! 2. **判题只做整数比较**：正确答案就是被留空的那个数（`domain::math` 出卷时已保证唯一解），
//!    不需要二次求值。判题放在这一层，前端不重复实现。
//!
//! 与 `sessions` / `session_items` 无关：后者的 `word_id` 是 `words` 的外键，
//! 口算题没有对应词条（见迁移 0008 的注释）。

use crate::domain::math::{MathQuestion, MathSheet, Op};
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// 一道题的作答视图。`correct` 只在交卷后由前端展示（做题时不回传对错）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathItemDto {
    pub seq: i64,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    /// 第一步中间结果 a op1 b
    pub t: i32,
    /// 得数
    pub d: i32,
    pub op1: String,
    pub op2: String,
    /// 留空位置 1-4
    pub blank_pos: u8,
    /// NULL = 没做；"" = 跳过了（记错）；其余是原始输入
    pub input: Option<String>,
    pub correct: Option<bool>,
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathPracticeDto {
    pub id: i64,
    pub seed: i64,
    pub total: i64,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub correct_total: Option<i64>,
    pub duration_ms: Option<i64>,
    /// 非空 = 错题重做卷（值为来源卷 id）；重做卷不参与「全对最快」纪录
    pub redo_of: Option<i64>,
    pub items: Vec<MathItemDto>,
}

/// 交卷后的成绩单
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathResultDto {
    pub id: i64,
    pub total: i64,
    pub correct_total: i64,
    pub duration_ms: i64,
    /// 错题题号（1 起，含未答）
    pub wrong_seqs: Vec<i64>,
    /// 本题量的历史「全对最快」用时（含本次）；没有全对过则为 null
    pub best_ms: Option<i64>,
    /// 本次是否刷新了最快纪录
    pub new_best: bool,
    /// 非空 = 错题重做卷（值为来源卷 id）；重做卷不参与「全对最快」纪录
    pub redo_of: Option<i64>,
    pub items: Vec<MathItemDto>,
}

/// 记录页的一行（摘要，不带 30 条题目）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathSummaryDto {
    pub id: i64,
    pub seed: i64,
    pub total: i64,
    pub answered: i64,
    pub correct_total: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// 非空 = 错题重做卷（值为来源卷 id）；重做卷不参与「全对最快」纪录
    pub redo_of: Option<i64>,
    /// 同为该题量的全对最快（并列也算）
    pub is_best: bool,
}

/// 「全对最快」按题量分组：10 题和 30 题的用时不可比，不能混成一条
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathBestDto {
    pub total: i64,
    pub best_ms: i64,
    pub achieved_at: String,
}

/// 交一题的回执：只回进度，不回对错（「全场结束后统一公布」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathAnswerAck {
    pub answered: i64,
    pub total: i64,
}

fn missing() -> AppError {
    AppError::msg("这次口算记录不存在了，请重新开一张卷")
}

fn status_of(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row("SELECT status FROM math_sessions WHERE id = ?1", params![id], |r| r.get(0))
        .optional()?
        .ok_or_else(missing)
}

/// 开卷：写入会话头 + 30 条题目快照（一个事务，要么全成要么全不成）
pub fn create(
    conn: &Connection,
    seed: u32,
    config_json: &str,
    sheet: &MathSheet,
) -> AppResult<MathPracticeDto> {
    insert_session(conn, seed, config_json, sheet, None)
}

/// `create` 的实现：`redo_of = Some(来源卷)` 时开出的是错题重做卷
fn insert_session(
    conn: &Connection,
    seed: u32,
    config_json: &str,
    sheet: &MathSheet,
    redo_of: Option<i64>,
) -> AppResult<MathPracticeDto> {
    if sheet.questions.is_empty() || sheet.questions.len() != sheet.blanks.len() {
        return Err(AppError::msg("题目与留空位置数量不一致，请重新出题"));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO math_sessions (seed, config_json, total, status, started_at, redo_of)
         VALUES (?1, ?2, ?3, 'running', strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?4)",
        params![seed as i64, config_json, sheet.questions.len() as i64, redo_of],
    )?;
    let id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO math_items (session_id, seq, a, b, c, d, t, op1, op2, blank_pos)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for (i, (q, &blank)) in sheet.questions.iter().zip(sheet.blanks.iter()).enumerate() {
            stmt.execute(params![
                id,
                i as i64,
                q.a,
                q.b,
                q.c,
                q.d,
                q.t,
                q.op1.as_str(),
                q.op2.as_str(),
                blank
            ])?;
        }
    }
    tx.commit()?;
    get(conn, id)
}

/// 读题目快照。`reveal=false`（做题中）时把 `correct` 抹掉——判题结果只在交卷后下发，
/// 否则「做题时不回传对错」只是前端不渲染而已（ADR-23 的红线）。
fn list_items(conn: &Connection, session_id: i64, reveal: bool) -> AppResult<Vec<MathItemDto>> {
    let mut stmt = conn.prepare(
        "SELECT seq, a, b, c, d, t, op1, op2, blank_pos, input, correct, elapsed_ms
         FROM math_items WHERE session_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map(params![session_id], |r| {
            Ok(MathItemDto {
                seq: r.get(0)?,
                a: r.get(1)?,
                b: r.get(2)?,
                c: r.get(3)?,
                d: r.get(4)?,
                t: r.get(5)?,
                op1: r.get(6)?,
                op2: r.get(7)?,
                blank_pos: r.get(8)?,
                input: r.get(9)?,
                correct: r.get::<_, Option<i64>>(10)?.map(|v| v != 0),
                elapsed_ms: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if reveal {
        rows
    } else {
        rows.into_iter().map(|it| MathItemDto { correct: None, ..it }).collect()
    })
}

pub fn get(conn: &Connection, id: i64) -> AppResult<MathPracticeDto> {
    let head = conn
        .query_row(
            "SELECT seed, total, status, started_at, finished_at, correct_total, duration_ms, redo_of
             FROM math_sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(missing)?;
    let (seed, total, status, started_at, finished_at, correct_total, duration_ms, redo_of) = head;
    // 做题中不下发对错（ADR-23）：判题结果只在交卷后的成绩单里出现
    let reveal = status == "finished";
    Ok(MathPracticeDto {
        id,
        seed,
        total,
        status,
        started_at,
        finished_at,
        correct_total,
        duration_ms,
        redo_of,
        items: list_items(conn, id, reveal)?,
    })
}

/// 最近一张没做完的卷（「继续上次」用）；没有则 None
pub fn latest_running(conn: &Connection) -> AppResult<Option<MathPracticeDto>> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM math_sessions WHERE status = 'running'
             ORDER BY started_at DESC, id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => Ok(Some(get(conn, id)?)),
        None => Ok(None),
    }
}

/// 交一题：判题并落库。允许改答案（同一 seq 覆盖），交卷后不再接受作答。
/// 空输入视为「跳过」，记错但保留 input = ''，成绩页能区分「没做」和「空着交了」。
pub fn answer(
    conn: &Connection,
    id: i64,
    seq: i64,
    input: &str,
    elapsed_ms: Option<i64>,
) -> AppResult<MathAnswerAck> {
    if status_of(conn, id)? != "running" {
        return Err(AppError::msg("这张卷已经交卷了，不能再改答案"));
    }
    let raw = input.trim();
    if !raw.is_empty() && raw.parse::<i32>().is_err() {
        return Err(AppError::msg("只能填数字"));
    }
    let (a, b, c, d, blank): (i32, i32, i32, i32, u8) = conn
        .query_row(
            "SELECT a, b, c, d, blank_pos FROM math_items WHERE session_id = ?1 AND seq = ?2",
            params![id, seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::msg("这道题不在本次卷子里"))?;
    let expected = crate::domain::math::value_at([a, b, c, d], blank);
    let correct = raw.parse::<i32>().map(|v| v == expected).unwrap_or(false);
    // 用时夹在合理区间：Windows 校准时钟回拨时前端会算出差值负数，落库后会把「全对最快」永久钉死
    let elapsed = elapsed_ms.map(|v| v.clamp(0, 3_600_000));
    conn.execute(
        // elapsed_ms 取「已有值与本次的较大者」：回头逐题按回车不能把用时清空重记，
        // 否则检查得越仔细、记录越快（最快纪录就废了）
        "UPDATE math_items
         SET input = ?3, correct = ?4,
             elapsed_ms = MAX(COALESCE(elapsed_ms, 0), COALESCE(?5, 0)),
             answered_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE session_id = ?1 AND seq = ?2",
        params![id, seq, raw, if correct { 1 } else { 0 }, elapsed],
    )?;
    progress(conn, id)
}

/// 作答进度（已答 / 总题数），做题页顶部的进度条用它
pub fn progress(conn: &Connection, id: i64) -> AppResult<MathAnswerAck> {
    let (answered, total): (i64, i64) = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM math_items WHERE session_id = ?1 AND input IS NOT NULL), total
         FROM math_sessions WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(MathAnswerAck { answered, total })
}

/// 交卷：汇总对错与用时（未答一律算错），幂等——重复调用得到同一份成绩单
pub fn finish(conn: &Connection, id: i64) -> AppResult<MathResultDto> {
    let status = status_of(conn, id)?;
    if status == "abandoned" {
        return Err(AppError::msg("这次口算已经放弃了，成绩不作数"));
    }
    let total: i64 = conn.query_row("SELECT total FROM math_sessions WHERE id = ?1", params![id], |r| {
        r.get(0)
    })?;
    // 重做卷只做错题、题量又可能与正式卷相同，全对再快也不能算「全对最快」纪录
    let redo_of: Option<i64> =
        conn.query_row("SELECT redo_of FROM math_sessions WHERE id = ?1", params![id], |r| {
            r.get(0)
        })?;
    // 必须在写回本次成绩之前取「旧纪录」，否则本次会把自己算进去
    let before_best = best_ms(conn, total)?;
    // 改状态 + 汇总 + 写回必须原子：中途被杀会留下 status='finished' 但 correct_total 为空的卷，
    // 记录页显示「已交卷」而成绩单是 0 分，且首次破纪录的徽章永久丢失
    let tx = conn.unchecked_transaction()?;
    if status == "running" {
        tx.execute(
            "UPDATE math_sessions
             SET status = 'finished', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![id],
        )?;
    }
    let (total, correct_total, duration_ms): (i64, i64, i64) = tx.query_row(
        "SELECT (SELECT COUNT(*) FROM math_items WHERE session_id = ?1),
                (SELECT COALESCE(SUM(CASE WHEN correct = 1 THEN 1 ELSE 0 END), 0)
                   FROM math_items WHERE session_id = ?1),
                (SELECT COALESCE(SUM(elapsed_ms), 0) FROM math_items WHERE session_id = ?1)",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    tx.execute(
        "UPDATE math_sessions SET correct_total = ?2, duration_ms = ?3 WHERE id = ?1",
        params![id, correct_total, duration_ms],
    )?;
    tx.commit()?;
    let mut out = result_of(conn, id)?;
    // 只有全对才叫「最快纪录」；重做卷不参与（同题量只做错题，用时与正式卷不可比）；
    // 重复交卷时 before_best 已含本次，自然不会再判新纪录
    out.new_best = redo_of.is_none()
        && correct_total == total
        && match before_best {
            Some(b) => duration_ms < b,
            None => true,
        };
    Ok(out)
}

/// 本题量的历史全对最快（分钟级的比较，忽略未答完/没交卷的卷；错题重做卷不参与）
fn best_ms(conn: &Connection, total: i64) -> AppResult<Option<i64>> {
    let v = conn.query_row(
        "SELECT MIN(duration_ms) FROM math_sessions
         WHERE status = 'finished' AND total = ?1
           AND correct_total = total AND duration_ms IS NOT NULL
           AND redo_of IS NULL",
        params![total],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    Ok(v)
}

/// 成绩单（只读）：记录页回看用，不做任何写入
pub fn result_of(conn: &Connection, id: i64) -> AppResult<MathResultDto> {
    let (total, correct_total, duration_ms, status, redo_of): (
        i64,
        Option<i64>,
        Option<i64>,
        String,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT total, correct_total, duration_ms, status, redo_of FROM math_sessions WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or_else(missing)?;
    if status != "finished" {
        return Err(AppError::msg("这次口算还没交卷，成绩还没出来"));
    }
    let items = list_items(conn, id, true)?;
    let wrong_seqs = items
        .iter()
        .filter(|it| it.correct != Some(true))
        .map(|it| it.seq + 1)
        .collect();
    Ok(MathResultDto {
        id,
        total,
        correct_total: correct_total.unwrap_or(0),
        duration_ms: duration_ms.unwrap_or(0),
        wrong_seqs,
        best_ms: best_ms(conn, total)?,
        new_best: false,
        redo_of,
        items,
    })
}

/// 记录列表：不含已放弃的卷；未交卷的也在里面（显示成「未交卷」可以接着做）
pub fn list(conn: &Connection, limit: i64) -> AppResult<Vec<MathSummaryDto>> {
    let limit = limit.clamp(1, 200);
    let mut stmt = conn.prepare(
        "SELECT s.id, s.seed, s.total,
                (SELECT COUNT(*) FROM math_items i
                  WHERE i.session_id = s.id AND i.input IS NOT NULL),
                s.correct_total, s.duration_ms, s.status, s.started_at, s.finished_at, s.redo_of,
                COALESCE((s.status = 'finished' AND s.correct_total = s.total
                 AND s.duration_ms IS NOT NULL AND s.redo_of IS NULL
                 AND s.duration_ms = (SELECT MIN(m.duration_ms) FROM math_sessions m
                      WHERE m.status = 'finished' AND m.total = s.total
                        AND m.correct_total = m.total AND m.duration_ms IS NOT NULL
                        AND m.redo_of IS NULL)), 0)
         FROM math_sessions s
         WHERE s.status <> 'abandoned'
         -- 按「最后一次动过」排序：三天前开的卷今天才交，得出现在列表最上面
         ORDER BY COALESCE(s.finished_at, s.started_at) DESC, s.id DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(MathSummaryDto {
                id: r.get(0)?,
                seed: r.get(1)?,
                total: r.get(2)?,
                answered: r.get(3)?,
                correct_total: r.get(4)?,
                duration_ms: r.get(5)?,
                status: r.get(6)?,
                started_at: r.get(7)?,
                finished_at: r.get(8)?,
                redo_of: r.get(9)?,
                is_best: r.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 各题量的全对最快（并列最快取最近一次；错题重做卷不参与）
pub fn best_times(conn: &Connection) -> AppResult<Vec<MathBestDto>> {
    let mut stmt = conn.prepare(
        "SELECT total, best_ms, achieved_at FROM (
           SELECT s.total,
                  s.duration_ms AS best_ms,
                  COALESCE(s.finished_at, s.started_at) AS achieved_at,
                  ROW_NUMBER() OVER (
                    PARTITION BY s.total
                    ORDER BY s.duration_ms ASC, COALESCE(s.finished_at, s.started_at) DESC, s.id DESC
                  ) AS rn
           FROM math_sessions s
           WHERE s.status = 'finished' AND s.correct_total = s.total
             AND s.duration_ms IS NOT NULL AND s.redo_of IS NULL
         ) WHERE rn = 1 ORDER BY total",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MathBestDto { total: r.get(0)?, best_ms: r.get(1)?, achieved_at: r.get(2)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 删一条做题记录（题目随外键级联删掉）
pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    let n = conn.execute("DELETE FROM math_sessions WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(missing());
    }
    Ok(())
}

/// 放弃这次做题（做题页「放弃这次」与「换新卷」时调用）；已结束的卷保持原状
pub fn abandon(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE math_sessions
         SET status = 'abandoned', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?1 AND status = 'running'",
        params![id],
    )?;
    Ok(())
}

/// 库里存的运算符单字符还原成 `Op`（与 `Op::as_str` 互逆；CHECK 约束保证只有这四个）
fn op_of(raw: &str) -> AppResult<Op> {
    match raw {
        "m" => Ok(Op::Mul),
        "d" => Ok(Op::Div),
        "a" => Ok(Op::Add),
        "s" => Ok(Op::Sub),
        _ => Err(AppError::msg("这张卷里的运算符数据坏了，没法重做")),
    }
}

/// 重做错题：把一张**已交卷**卷子里的错题（含没做/空着的）复制成一张新的 running 卷，
/// 题号从 1 重新编、作答与用时清零，做完单独出成绩。来源卷原样保留在记录里。
/// 新卷 `redo_of` 指回来源卷（config_json 里也记一份 redoOf 便于追溯）；
/// 重做卷不参与「全对最快」纪录。重做卷交卷后如果还有错题，可以继续再重做。
pub fn redo_wrong(conn: &Connection, id: i64) -> AppResult<MathPracticeDto> {
    let head = conn
        .query_row(
            "SELECT seed, config_json, status FROM math_sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(missing)?;
    let (seed, config_json, status) = head;
    if status != "finished" {
        return Err(AppError::msg("这张卷还没交卷，先交卷才能重做错题"));
    }
    // 错题 = 没做（NULL）与做错（0）的题，按原题号顺序复制
    let wrong = {
        let mut stmt = conn.prepare(
            "SELECT a, b, c, d, t, op1, op2, blank_pos FROM math_items
             WHERE session_id = ?1 AND (correct IS NULL OR correct = 0)
             ORDER BY seq",
        )?;
        let rows = stmt
            .query_map(params![id], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, i32>(2)?,
                    r.get::<_, i32>(3)?,
                    r.get::<_, i32>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, u8>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    if wrong.is_empty() {
        return Err(AppError::msg("这张卷没有错题，不用重做"));
    }
    let mut questions = Vec::with_capacity(wrong.len());
    let mut blanks = Vec::with_capacity(wrong.len());
    for (a, b, c, d, t, op1, op2, blank) in wrong {
        questions.push(MathQuestion { a, b, c, t, d, op1: op_of(&op1)?, op2: op_of(&op2)? });
        blanks.push(blank);
    }
    // 条件快照沿用来源卷的，再记一笔 redoOf；旧数据 config_json 坏了（非法 JSON、
    // 或合法但不是对象——数组/数字等塞不下 redoOf 键）也不挡重做，退回空对象
    let mut cfg: serde_json::Value =
        serde_json::from_str(&config_json).unwrap_or_else(|_| serde_json::json!({}));
    if !cfg.is_object() {
        cfg = serde_json::json!({});
    }
    cfg["redoOf"] = serde_json::json!(id);
    insert_session(conn, seed as u32, &cfg.to_string(), &MathSheet { questions, blanks }, Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::domain::math::{self, BlankSel, MathRanges, Op1Sel, Op2Sel};

    const SEED: u32 = 20261120;

    fn sheet() -> MathSheet {
        math::generate_count(
            SEED,
            BlankSel::Random,
            Op1Sel::Fixed(math::Op::Mul),
            Op2Sel::Random,
            &MathRanges::default(),
            math::DEFAULT_COUNT,
        )
        .expect("默认条件应能出卷")
    }

    fn answer_of(s: &MathSheet, i: usize) -> i32 {
        let q = &s.questions[i];
        math::value_at([q.a, q.b, q.c, q.d], s.blanks[i])
    }

    /// 按卷面把答案全填上；wrong 表示前几道故意填错
    fn answer_all(conn: &Connection, s: &MathSheet, id: i64, ms: i64, wrong: usize) {
        for i in 0..s.questions.len() {
            let mut v = answer_of(s, i);
            if i < wrong {
                v += 1;
            }
            answer(conn, id, i as i64, &v.to_string(), Some(ms)).unwrap();
        }
    }

    #[test]
    fn create_snapshots_all_questions() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, r#"{"pos":"random"}"#, &s).unwrap();
        assert_eq!(p.status, "running");
        assert_eq!(p.total, 30);
        assert_eq!(p.items.len(), 30);
        assert_eq!(p.items[0].blank_pos, s.blanks[0]);
        assert_eq!(p.items[0].a, s.questions[0].a);
        assert!(p.items.iter().all(|it| it.input.is_none() && it.correct.is_none()));
        assert_eq!(p.correct_total, None);
    }

    /// 直接读库：`get` 在做题中会抹掉 correct（ADR-23），要验判题结果就读原始行
    fn stored(conn: &Connection, id: i64, seq: i64) -> (String, i64, i64) {
        conn.query_row(
            "SELECT input, correct, elapsed_ms FROM math_items WHERE session_id = ?1 AND seq = ?2",
            params![id, seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    }

    #[test]
    fn answer_judges_and_allows_overwrite() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();

        let right = answer(&conn, p.id, 0, &answer_of(&s, 0).to_string(), Some(1200)).unwrap();
        assert_eq!(right.answered, 1);
        let (input, correct, elapsed) = stored(&conn, p.id, 0);
        assert_eq!(input, answer_of(&s, 0).to_string());
        assert_eq!(correct, 1, "答对要落 1");
        assert_eq!(elapsed, 1200);

        // 同一题改答案：覆盖而不是新增，且用时只增不减
        let wrong_input = (answer_of(&s, 0) + 1).to_string();
        answer(&conn, p.id, 0, &wrong_input, Some(800)).unwrap();
        let (input, correct, elapsed) = stored(&conn, p.id, 0);
        assert_eq!(input, wrong_input);
        assert_eq!(correct, 0);
        assert_eq!(elapsed, 1200, "改答案不能把用时改小（否则最快纪录可刷）");
        assert_eq!(get(&conn, p.id).unwrap().items.len(), 30);
    }

    #[test]
    fn elapsed_is_clamped_to_sane_range() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        // 系统时钟回拨会让前端算出负值；落库后会把「全对最快」永久钉死
        answer(&conn, p.id, 0, "1", Some(-5)).unwrap();
        assert_eq!(stored(&conn, p.id, 0).2, 0);
        // 异常大的值也要挡住（否则 SUM 溢出、纪录无人能破）
        answer(&conn, p.id, 1, "1", Some(9_999_999_999)).unwrap();
        assert_eq!(stored(&conn, p.id, 1).2, 3_600_000);
    }

    /// 做题中拿到的卷不携带对错：判题结果只在交卷后的成绩单里出现
    #[test]
    fn running_sheet_hides_verdicts() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        answer(&conn, p.id, 0, &answer_of(&s, 0).to_string(), Some(1000)).unwrap();

        let running = get(&conn, p.id).unwrap();
        assert!(
            running.items.iter().all(|it| it.correct.is_none()),
            "做题中不该回传 correct"
        );
        // 落库本身仍然是判过的（交卷后才公布）
        finish(&conn, p.id).unwrap();
        let done = get(&conn, p.id).unwrap();
        assert_eq!(done.items[0].correct, Some(true), "交卷后要能读到对错");
    }

    #[test]
    fn skip_is_recorded_as_wrong() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        let ack = answer(&conn, p.id, 3, "   ", None).unwrap();
        assert_eq!(ack.answered, 1);
        let (input, correct, _) = stored(&conn, p.id, 3);
        assert_eq!(input, "", "跳过要留空串，与「没做」区分");
        assert_eq!(correct, 0);
    }

    #[test]
    fn rejects_non_numeric_and_foreign_seq() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        assert!(answer(&conn, p.id, 0, "12a", None).is_err());
        assert!(answer(&conn, p.id, 99, "1", None).is_err());
        assert!(answer(&conn, 9999, 0, "1", None).is_err());
    }

    #[test]
    fn finish_aggregates_and_counts_unanswered_as_wrong() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        for i in 0..10 {
            answer(&conn, p.id, i as i64, &answer_of(&s, i).to_string(), Some(1000)).unwrap();
        }
        for i in 10..12 {
            answer(&conn, p.id, i as i64, &(answer_of(&s, i) + 1).to_string(), Some(500)).unwrap();
        }
        let r = finish(&conn, p.id).unwrap();
        assert_eq!(r.total, 30);
        assert_eq!(r.correct_total, 10);
        assert_eq!(r.duration_ms, 10 * 1000 + 2 * 500);
        assert_eq!(r.wrong_seqs.len(), 20, "2 道答错 + 18 道未答，全算错");
        assert!(!r.wrong_seqs.contains(&1));

        // 幂等：再交一次得到同样成绩
        let again = finish(&conn, p.id).unwrap();
        assert_eq!(again.correct_total, 10);
        assert_eq!(again.wrong_seqs, r.wrong_seqs);
        // 交卷后不接受作答
        assert!(answer(&conn, p.id, 20, "1", None).is_err());
    }

    #[test]
    fn latest_running_tracks_unfinished_sheet() {
        let conn = db::open_memory();
        let s = sheet();
        assert!(latest_running(&conn).unwrap().is_none());
        let first = create(&conn, SEED, "{}", &s).unwrap();
        let second = create(&conn, SEED + 1, "{}", &s).unwrap();
        assert_eq!(latest_running(&conn).unwrap().unwrap().id, second.id);

        abandon(&conn, second.id).unwrap();
        assert_eq!(latest_running(&conn).unwrap().unwrap().id, first.id);
        abandon(&conn, first.id).unwrap();
        assert!(latest_running(&conn).unwrap().is_none());
        // 放弃后不能再交卷
        assert!(finish(&conn, first.id).is_err());
    }

    #[test]
    fn best_time_tracks_fastest_perfect_sheet() {
        let conn = db::open_memory();
        let s = sheet();

        let p1 = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, p1.id, 1000, 0);
        let r1 = finish(&conn, p1.id).unwrap();
        assert!(r1.new_best, "第一次全对就是纪录");
        assert_eq!(r1.best_ms, Some(30_000));

        // 更慢的一次：不刷新
        let p2 = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, p2.id, 1200, 0);
        let r2 = finish(&conn, p2.id).unwrap();
        assert!(!r2.new_best);
        assert_eq!(r2.best_ms, Some(30_000));

        // 更快的一次：刷新
        let p3 = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, p3.id, 500, 0);
        let r3 = finish(&conn, p3.id).unwrap();
        assert!(r3.new_best);
        assert_eq!(r3.best_ms, Some(15_000));

        // 重复交卷不能再声称新纪录（before_best 此时已含自己）
        let again = finish(&conn, p3.id).unwrap();
        assert!(!again.new_best, "同一张卷重复交卷不该再报新纪录");
        assert_eq!(again.best_ms, Some(15_000));
        assert_eq!(again.duration_ms, r3.duration_ms);

        // 有错的不参与纪录
        let p4 = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, p4.id, 100, 1);
        let r4 = finish(&conn, p4.id).unwrap();
        assert!(!r4.new_best);
        assert_eq!(r4.best_ms, Some(15_000));
    }

    #[test]
    fn best_times_are_grouped_by_question_count() {
        let conn = db::open_memory();
        let r = MathRanges::default();
        let s30 = sheet();
        let s10 = math::generate_count(
            SEED,
            BlankSel::Two,
            Op1Sel::Fixed(math::Op::Mul),
            Op2Sel::Random,
            &r,
            10,
        )
        .expect("10 题应能出卷");

        let a = create(&conn, SEED, "{}", &s30).unwrap();
        answer_all(&conn, &s30, a.id, 1000, 0);
        finish(&conn, a.id).unwrap();
        let b = create(&conn, SEED, "{}", &s10).unwrap();
        answer_all(&conn, &s10, b.id, 2000, 0);
        finish(&conn, b.id).unwrap();

        let bests = best_times(&conn).unwrap();
        assert_eq!(bests.len(), 2, "10 题和 30 题的纪录不能混成一条");
        assert_eq!(bests[0].total, 10);
        assert_eq!(bests[0].best_ms, 20_000);
        assert_eq!(bests[1].total, 30);
        assert_eq!(bests[1].best_ms, 30_000);
    }

    #[test]
    fn list_reports_summaries_and_best_flag() {
        let conn = db::open_memory();
        let s = sheet();
        let slow = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, slow.id, 1000, 0);
        finish(&conn, slow.id).unwrap();
        let fast = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, fast.id, 400, 0);
        finish(&conn, fast.id).unwrap();

        let rows = list(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, fast.id, "按开始时间倒序");
        assert!(rows[0].is_best);
        assert!(!rows[1].is_best);
        assert_eq!(rows[0].answered, 30);
        assert_eq!(rows[0].correct_total, Some(30));
        assert_eq!(rows[0].duration_ms, Some(12_000));

        // 放弃的卷不进记录
        let dropped = create(&conn, SEED, "{}", &s).unwrap();
        abandon(&conn, dropped.id).unwrap();
        assert_eq!(list(&conn, 10).unwrap().len(), 2);
    }

    #[test]
    fn result_of_is_read_only_and_requires_finished() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        assert!(result_of(&conn, p.id).is_err(), "没交卷不该有成绩单");
        answer_all(&conn, &s, p.id, 1000, 0);
        finish(&conn, p.id).unwrap();
        let r = result_of(&conn, p.id).unwrap();
        assert_eq!(r.correct_total, 30);
        assert!(!r.new_best, "只读接口不声称纪录");
        assert_eq!(r.best_ms, Some(30_000));
    }

    #[test]
    fn delete_removes_sheet_and_items() {
        let conn = db::open_memory();
        let p = create(&conn, SEED, "{}", &sheet()).unwrap();
        delete(&conn, p.id).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM math_items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "题目要随卷级联删除");
        assert!(get(&conn, p.id).is_err());
        assert!(delete(&conn, p.id).is_err());
    }

    #[test]
    fn redo_wrong_copies_wrong_items_into_new_sheet() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        // 前 3 道做错、接着 5 道做对、其余 22 道没做
        for i in 0..3 {
            answer(&conn, p.id, i as i64, &(answer_of(&s, i) + 1).to_string(), Some(500)).unwrap();
        }
        for i in 3..8 {
            answer(&conn, p.id, i as i64, &answer_of(&s, i).to_string(), Some(500)).unwrap();
        }
        finish(&conn, p.id).unwrap();

        let redo = redo_wrong(&conn, p.id).unwrap();
        assert_eq!(redo.status, "running");
        assert_eq!(redo.redo_of, Some(p.id));
        assert_eq!(redo.total, 25, "3 道做错 + 22 道没做，全算错题");
        assert!(redo.items.iter().all(|it| it.input.is_none() && it.correct.is_none()));
        let expected: Vec<usize> = (0..3).chain(8..30).collect();
        for (k, it) in redo.items.iter().enumerate() {
            let src = &s.questions[expected[k]];
            assert_eq!(it.seq, k as i64, "重做卷题号从 0 重新编");
            assert_eq!((it.a, it.b, it.c, it.d), (src.a, src.b, src.c, src.d));
            assert_eq!(it.blank_pos, s.blanks[expected[k]]);
        }

        // 来源卷原样保留，重做不覆盖它
        let src = get(&conn, p.id).unwrap();
        assert_eq!(src.status, "finished");
        assert_eq!(src.items.len(), 30);

        // 重做卷走正常做题/交卷流程，做完有自己的成绩
        for it in &redo.items {
            let v = math::value_at([it.a, it.b, it.c, it.d], it.blank_pos);
            answer(&conn, redo.id, it.seq, &v.to_string(), Some(400)).unwrap();
        }
        let r = finish(&conn, redo.id).unwrap();
        assert_eq!(r.correct_total, 25);
        assert_eq!(r.redo_of, Some(p.id));
        // 全对之后没有错题可重做
        assert!(redo_wrong(&conn, redo.id).is_err());
    }

    #[test]
    fn redo_wrong_requires_finished_sheet_with_wrong_items() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        assert!(redo_wrong(&conn, p.id).is_err(), "没交卷的卷没有判题结果，不能重做");
        answer_all(&conn, &s, p.id, 1000, 0);
        finish(&conn, p.id).unwrap();
        assert!(redo_wrong(&conn, p.id).is_err(), "全对的卷没有错题");
        assert!(redo_wrong(&conn, 9999).is_err());
    }

    #[test]
    fn redo_wrong_tolerates_broken_config_json() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        answer_all(&conn, &s, p.id, 1000, 3);
        finish(&conn, p.id).unwrap();
        // 同步/外部工具可能写进坏数据：config_json 是合法 JSON 但不是对象时，
        // serde_json 的下标赋值会 panic——重做要退回空对象继续开卷
        conn.execute(
            "UPDATE math_sessions SET config_json = '[1,2]' WHERE id = ?1",
            params![p.id],
        )
        .unwrap();
        let redo = redo_wrong(&conn, p.id).unwrap();
        assert_eq!(redo.total, 3, "3 道做错，其余都对");
        assert_eq!(redo.redo_of, Some(p.id));
    }

    #[test]
    fn redo_sheets_do_not_claim_best_records() {
        let conn = db::open_memory();
        let s5 = math::generate_count(
            SEED,
            BlankSel::Two,
            Op1Sel::Fixed(math::Op::Mul),
            Op2Sel::Random,
            &MathRanges::default(),
            5,
        )
        .expect("5 题应能出卷");

        // 正式 5 题卷：全对 2000ms/题 → 「5 道全对最快」= 10s
        let base = create(&conn, SEED, "{}", &s5).unwrap();
        answer_all(&conn, &s5, base.id, 2000, 0);
        assert!(finish(&conn, base.id).unwrap().new_best);

        // 另一张 5 题卷全错 → 重做卷同样 5 道、全对更快，但不能抢纪录
        let bad = create(&conn, SEED + 1, "{}", &s5).unwrap();
        answer_all(&conn, &s5, bad.id, 100, 5);
        finish(&conn, bad.id).unwrap();
        let redo = redo_wrong(&conn, bad.id).unwrap();
        assert_eq!(redo.total, 5);
        for it in &redo.items {
            let v = math::value_at([it.a, it.b, it.c, it.d], it.blank_pos);
            answer(&conn, redo.id, it.seq, &v.to_string(), Some(100)).unwrap();
        }
        let rr = finish(&conn, redo.id).unwrap();
        assert_eq!(rr.correct_total, 5);
        assert!(!rr.new_best, "重做卷只做错题，全对再快也不算新纪录");
        assert_eq!(rr.best_ms, Some(10_000), "5 道的纪录仍是正式卷那次");

        let b5 = best_times(&conn).unwrap().into_iter().find(|b| b.total == 5).unwrap();
        assert_eq!(b5.best_ms, 10_000);

        // 记录页：重做卷标出 redo_of，不带「最快」；正式卷正常挂「最快」
        let rows = list(&conn, 10).unwrap();
        let redo_row = rows.iter().find(|x| x.id == redo.id).unwrap();
        assert_eq!(redo_row.redo_of, Some(bad.id));
        assert!(!redo_row.is_best);
        assert!(rows.iter().find(|x| x.id == base.id).unwrap().is_best);

        // 删掉来源卷：重做卷保留，标记回落成普通卷（迁移 0009 的 ON DELETE SET NULL）
        delete(&conn, bad.id).unwrap();
        let kept = get(&conn, redo.id).unwrap();
        assert_eq!(kept.status, "finished");
        assert_eq!(kept.redo_of, None);
    }

    #[test]
    fn redo_can_be_redone_until_no_wrong_left() {
        let conn = db::open_memory();
        let s = sheet();
        let p = create(&conn, SEED, "{}", &s).unwrap();
        // 只做前 2 道：一道对、一道错，其余没做 → 错题 29 道
        answer(&conn, p.id, 0, &answer_of(&s, 0).to_string(), Some(100)).unwrap();
        answer(&conn, p.id, 1, &(answer_of(&s, 1) + 1).to_string(), Some(100)).unwrap();
        finish(&conn, p.id).unwrap();

        let r1 = redo_wrong(&conn, p.id).unwrap();
        assert_eq!(r1.total, 29);
        // 重做时只把第 1 道（原卷做错的那道）做对，其余空着交卷（全算错）
        let first = &r1.items[0];
        let v = math::value_at([first.a, first.b, first.c, first.d], first.blank_pos);
        answer(&conn, r1.id, 0, &v.to_string(), Some(100)).unwrap();
        finish(&conn, r1.id).unwrap();

        let r2 = redo_wrong(&conn, r1.id).unwrap();
        assert_eq!(r2.total, 28, "重做卷还能再重做，错题越做越少");
        assert_eq!(r2.redo_of, Some(r1.id));
        assert_eq!((r2.items[0].a, r2.items[0].d), (r1.items[1].a, r1.items[1].d),
            "再重做的是重做卷里空着交的第一道，做对的那道（r1 的第 0 题）不再出现");
    }
}
