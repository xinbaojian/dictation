use crate::domain::math::{self, BlankSel, MathRanges, Op1Sel, Op2Sel};
use crate::error::{AppError, AppResult};
use crate::repo::math_sessions::{
    self, MathAnswerAck, MathBestDto, MathPracticeDto, MathResultDto, MathSummaryDto,
};
use crate::AppState;
use tauri::State;

/// 题量校验：家长界面只给 10/15/20/30，这里兜住越界值并给中文提示
fn count_of(raw: Option<usize>) -> AppResult<usize> {
    let n = raw.unwrap_or(math::DEFAULT_COUNT);
    if !(math::COUNT_MIN..=math::COUNT_MAX).contains(&n) {
        return Err(AppError::msg(format!(
            "题量需在 {} 到 {} 道之间",
            math::COUNT_MIN,
            math::COUNT_MAX
        )));
    }
    Ok(n)
}

/// 出一张口算卷（默认 30 题 + 每题留空位置）。
/// pos: "1"|"2"|"3"|"4"|"random"；op1: "m"|"d"|"a"|"s"|"random"；op2: "a"|"s"|"random"。
/// ranges：四个位置的取值区间，缺省用默认值（①②:1-9 / ③:10-50 / ④:1-100，三段难度）。
/// count：题量（5-50），缺省 30；同一 seed 下 count=30 与原来的输出逐题一致。
/// 当前符号组合无解时返回 null，由前端引导换运算符。
#[tauri::command]
pub fn generate_math_sheet(
    seed: u32,
    pos: String,
    op1: String,
    op2: String,
    ranges: Option<MathRanges>,
    count: Option<usize>,
) -> AppResult<Option<math::MathSheet>> {
    let pos = BlankSel::parse(&pos)?;
    let op1 = Op1Sel::parse(&op1)?;
    let op2 = Op2Sel::parse(&op2)?;
    let ranges = ranges.unwrap_or_default();
    ranges.validate()?;
    let count = count_of(count)?;
    Ok(math::generate_count(seed, pos, op1, op2, &ranges, count))
}

/// 把当前条件出的一张卷落库，开始上机做题。
/// 题目在开卷时快照进库（见 `repo::math_sessions`），所以同一 seed 出的卷与打印的纸质卷逐题一致。
#[tauri::command]
pub fn start_math_session(
    state: State<'_, AppState>,
    seed: u32,
    pos: String,
    op1: String,
    op2: String,
    ranges: Option<MathRanges>,
    count: Option<usize>,
) -> AppResult<MathPracticeDto> {
    let pos_sel = BlankSel::parse(&pos)?;
    let op1_sel = Op1Sel::parse(&op1)?;
    let op2_sel = Op2Sel::parse(&op2)?;
    let ranges = ranges.unwrap_or_default();
    ranges.validate()?;
    let count = count_of(count)?;
    let sheet = math::generate_count(seed, pos_sel, op1_sel, op2_sel, &ranges, count).ok_or_else(
        || AppError::msg("这组条件出不出这么多题，请换运算符、放宽取值区间或减少题量"),
    )?;
    // 条件快照进 config_json：以后成绩单能说清「这张卷是什么条件出的」
    let cfg = serde_json::json!({
        "pos": pos,
        "op1": op1,
        "op2": op2,
        "count": count,
        "ranges": {
            "aMin": ranges.a_min, "aMax": ranges.a_max,
            "bMin": ranges.b_min, "bMax": ranges.b_max,
            "cMin": ranges.c_min, "cMax": ranges.c_max,
            "dMin": ranges.d_min, "dMax": ranges.d_max,
        },
    });
    let conn = state.conn()?;
    math_sessions::create(&conn, seed, &cfg.to_string(), &sheet)
}

#[tauri::command]
pub fn get_math_practice(state: State<'_, AppState>, id: i64) -> AppResult<MathPracticeDto> {
    let conn = state.conn()?;
    math_sessions::get(&conn, id)
}

/// 最近一张没做完的卷（「继续上次」）；没有返回 null
#[tauri::command]
pub fn latest_running_math(state: State<'_, AppState>) -> AppResult<Option<MathPracticeDto>> {
    let conn = state.conn()?;
    math_sessions::latest_running(&conn)
}

/// 交一题：只回进度，不回对错（全场结束后统一公布）
#[tauri::command]
pub fn submit_math_answer(
    state: State<'_, AppState>,
    id: i64,
    seq: i64,
    input: String,
    elapsed_ms: Option<i64>,
) -> AppResult<MathAnswerAck> {
    let conn = state.conn()?;
    math_sessions::answer(&conn, id, seq, &input, elapsed_ms)
}

#[tauri::command]
pub fn finish_math_session(state: State<'_, AppState>, id: i64) -> AppResult<MathResultDto> {
    let conn = state.conn()?;
    math_sessions::finish(&conn, id)
}

#[tauri::command]
pub fn abandon_math_session(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = state.conn()?;
    math_sessions::abandon(&conn, id)
}

/// 做题记录列表（不含已放弃的卷；未交卷的也在里面，可以接着做）
#[tauri::command]
pub fn list_math_sessions(state: State<'_, AppState>, limit: i64) -> AppResult<Vec<MathSummaryDto>> {
    let conn = state.conn()?;
    math_sessions::list(&conn, limit)
}

/// 回看某一次的成绩单（只读，不写库）
#[tauri::command]
pub fn get_math_result(state: State<'_, AppState>, id: i64) -> AppResult<MathResultDto> {
    let conn = state.conn()?;
    math_sessions::result_of(&conn, id)
}

/// 各题量的「全对最快」纪录
#[tauri::command]
pub fn math_best_times(state: State<'_, AppState>) -> AppResult<Vec<MathBestDto>> {
    let conn = state.conn()?;
    math_sessions::best_times(&conn)
}

#[tauri::command]
pub fn delete_math_session(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = state.conn()?;
    math_sessions::delete(&conn, id)
}
