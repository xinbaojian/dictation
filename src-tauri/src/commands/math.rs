use crate::domain::math::{self, BlankSel, Op1Sel, Op2Sel};
use crate::error::AppResult;

/// 出一张口算卷（30 题 + 每题留空位置）。
/// pos: "1"|"2"|"3"|"4"|"random"；op1: "m"|"d"|"a"|"s"|"random"；op2: "a"|"s"|"random"。
/// 当前符号组合无解时返回 null，由前端引导换运算符。
#[tauri::command]
pub fn generate_math_sheet(
    seed: u32,
    pos: String,
    op1: String,
    op2: String,
) -> AppResult<Option<math::MathSheet>> {
    let pos = BlankSel::parse(&pos)?;
    let op1 = Op1Sel::parse(&op1)?;
    let op2 = Op2Sel::parse(&op2)?;
    Ok(math::generate(seed, pos, op1, op2))
}
