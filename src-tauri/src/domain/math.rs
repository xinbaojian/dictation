//! 口算练习出题引擎：三数两步逆推（① op1 ② op2 ③ = ④，从左到右两步计算）。
//! 移植自口算练习 HTML 出题器；纯函数、无 IO。
//! 确定性约定：同 seed + 同选项 ⇒ 完全同一张卷（题目与留空位置）。

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 运算符（序列化与前端约定：m 乘 / d 除 / a 加 / s 减）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Op {
    #[serde(rename = "m")]
    Mul,
    #[serde(rename = "d")]
    Div,
    #[serde(rename = "a")]
    Add,
    #[serde(rename = "s")]
    Sub,
}

/// 留空位置选择
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlankSel {
    One,
    Two,
    Three,
    Four,
    Random,
}

/// 第一步运算符选择
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op1Sel {
    Fixed(Op),
    Random,
}

/// 第二步运算符选择（只有加减；「除/减 配 减」无解，由候选池过滤兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op2Sel {
    Add,
    Sub,
    Random,
}

/// 四个位置的取值区间（① 数 / ② 数 / ③ 数 / ④ 得数）。
/// 默认 = 原出题器行为（三段难度递进）；自定义区间改用单池平铺（不再分段），
/// 唯一解校验也按自定义值域枚举。min/max 上限见 RANGE_LIMITS。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MathRanges {
    pub a_min: i32,
    pub a_max: i32,
    pub b_min: i32,
    pub b_max: i32,
    pub c_min: i32,
    pub c_max: i32,
    pub d_min: i32,
    pub d_max: i32,
}

impl Default for MathRanges {
    fn default() -> Self {
        Self {
            a_min: 1,
            a_max: 9,
            b_min: 1,
            b_max: 9,
            c_min: 10,
            c_max: 50,
            d_min: 1,
            d_max: 100,
        }
    }
}

/// 各位置允许的绝对上下限：①② 保持「表内」上限 20，③ 100，④ 200
const RANGE_LIMITS: [(i32, i32); 4] = [(1, 20), (1, 20), (1, 100), (1, 200)];

impl MathRanges {
    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    /// 留空位置 p 对应的取值区间
    pub fn range_of(self, pos: u8) -> (i32, i32) {
        match pos {
            1 => (self.a_min, self.a_max),
            2 => (self.b_min, self.b_max),
            3 => (self.c_min, self.c_max),
            _ => (self.d_min, self.d_max),
        }
    }

    /// 区间合法性：≥ 下限、≤ 上限、min ≤ max；错误信息直接给家长看
    pub fn validate(self) -> AppResult<()> {
        let names = ["第 1 个数", "第 2 个数", "第 3 个数", "第 4 个数（得数）"];
        let pairs = [
            (self.a_min, self.a_max),
            (self.b_min, self.b_max),
            (self.c_min, self.c_max),
            (self.d_min, self.d_max),
        ];
        for (i, &(lo, hi)) in pairs.iter().enumerate() {
            let (lim_lo, lim_hi) = RANGE_LIMITS[i];
            if lo < lim_lo || lo > lim_hi || hi < lim_lo || hi > lim_hi {
                return Err(AppError::msg(format!(
                    "{}的取值需在 {}-{} 之间",
                    names[i], lim_lo, lim_hi
                )));
            }
            if lo > hi {
                return Err(AppError::msg(format!("{}的区间下限不能大于上限", names[i])));
            }
        }
        Ok(())
    }
}

/// 一道题：a op1 b = t，t op2 c = d（t 为中间结果）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct MathQuestion {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub op1: Op,
    pub op2: Op,
    pub t: i32,
    pub d: i32,
}

/// 一张卷：30 题与每题留空位置（1-4）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MathSheet {
    pub questions: Vec<MathQuestion>,
    pub blanks: Vec<u8>,
}

impl BlankSel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "1" => Ok(BlankSel::One),
            "2" => Ok(BlankSel::Two),
            "3" => Ok(BlankSel::Three),
            "4" => Ok(BlankSel::Four),
            "random" => Ok(BlankSel::Random),
            other => Err(AppError::msg(format!("未知的留空位置：{other}"))),
        }
    }
}

impl Op1Sel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "m" => Ok(Op1Sel::Fixed(Op::Mul)),
            "d" => Ok(Op1Sel::Fixed(Op::Div)),
            "a" => Ok(Op1Sel::Fixed(Op::Add)),
            "s" => Ok(Op1Sel::Fixed(Op::Sub)),
            "random" => Ok(Op1Sel::Random),
            other => Err(AppError::msg(format!("未知的运算符：{other}"))),
        }
    }
}

impl Op2Sel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "a" => Ok(Op2Sel::Add),
            "s" => Ok(Op2Sel::Sub),
            "random" => Ok(Op2Sel::Random),
            other => Err(AppError::msg(format!("未知的第二步运算符：{other}"))),
        }
    }
}

/// mulberry32：与原 JS 出题器逐位一致（wrapping 运算、无符号右移）
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Rng(seed)
    }

    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(1 | t);
        // 注意外层还要 ^ t（原 JS：t = (t + imul(...)) ^ t），漏掉会得到完全不同的序列
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}

/// Fisher-Yates 洗牌（与 JS shuffle 逐调用一致）
fn shuffle<T>(arr: &mut [T], rnd: &mut Rng) {
    for i in (1..arr.len()).rev() {
        let j = ((rnd.next_f64() * (i as f64 + 1.0)) as usize).min(i);
        arr.swap(i, j);
    }
}

fn calc(x: i32, op: Op, y: i32) -> Option<i32> {
    match op {
        Op::Add => Some(x + y),
        Op::Sub => Some(x - y),
        Op::Mul => Some(x * y),
        // 表内除法：只保留整除
        Op::Div => {
            if y != 0 && x % y == 0 {
                Some(x / y)
            } else {
                None
            }
        }
    }
}

/// 从左到右两步：t = a op1 b（≥1），d = t op2 c 且落在 dr 区间内
fn eval_q(a: i32, b: i32, c: i32, o1: Op, o2: Op, dr: (i32, i32)) -> Option<(i32, i32)> {
    let t = calc(a, o1, b)?;
    if t < 1 {
        return None;
    }
    let d = calc(t, o2, c)?;
    if !(dr.0..=dr.1).contains(&d) {
        return None;
    }
    Some((t, d))
}

/// 三段难度：(a 下界, a 上界, c 下界, c 上界)，b 恒在 1-9。
/// 仅默认区间使用；历史数值写死保证同 seed 出卷与原出题器逐题一致。
const STAGES: [(i32, i32, i32, i32); 3] = [(1, 4, 10, 30), (3, 6, 10, 50), (5, 9, 10, 50)];

fn pool(stage: usize, o1: Op, o2: Op, ranges: &MathRanges) -> Vec<MathQuestion> {
    let (a0, a1, c0, c1) = STAGES[stage];
    pool_over((a0, a1), (ranges.b_min, ranges.b_max), (c0, c1), (ranges.d_min, ranges.d_max), o1, o2)
}

/// 单池枚举：a、b、c 在各自区间内，d 落在 dr 区间才算候选
fn pool_over(
    ar: (i32, i32),
    br: (i32, i32),
    cr: (i32, i32),
    dr: (i32, i32),
    o1: Op,
    o2: Op,
) -> Vec<MathQuestion> {
    let mut v = Vec::new();
    for a in ar.0..=ar.1 {
        for b in br.0..=br.1 {
            for c in cr.0..=cr.1 {
                if let Some((t, d)) = eval_q(a, b, c, o1, o2, dr) {
                    v.push(MathQuestion { a, b, c, op1: o1, op2: o2, t, d });
                }
            }
        }
    }
    v
}

/// 每个符号组合的候选池：默认区间 = 三段；自定义区间 = 单池平铺
struct SheetPools {
    /// 分段池（默认 3 段 / 自定义 1 段），build 按比例把 30 题分到各段
    stages: Vec<Vec<MathQuestion>>,
    all: Vec<MathQuestion>,
}

fn pools_for(o1: Op, o2: Op, ranges: &MathRanges) -> SheetPools {
    if ranges.is_default() {
        let stages = [pool(0, o1, o2, ranges), pool(1, o1, o2, ranges), pool(2, o1, o2, ranges)];
        let all = [stages[0].clone(), stages[1].clone(), stages[2].clone()].concat();
        SheetPools { stages: stages.into(), all }
    } else {
        // 自定义区间：单池平铺，30 题都从同一区间抽（不分难度段）
        let p = pool_over(
            (ranges.a_min, ranges.a_max),
            (ranges.b_min, ranges.b_max),
            (ranges.c_min, ranges.c_max),
            (ranges.d_min, ranges.d_max),
            o1,
            o2,
        );
        SheetPools { stages: vec![p.clone()], all: p }
    }
}

/// 只保留各池都有足够候选的符号组合（例如「除后减」中间结果小，减不动 ≥10 的第三数）
fn combos_for(o1s: Op1Sel, o2s: Op2Sel, ranges: &MathRanges) -> Vec<(Op, Op)> {
    const O1: [Op; 4] = [Op::Mul, Op::Div, Op::Add, Op::Sub];
    const O2: [Op; 2] = [Op::Add, Op::Sub];
    let l1: &[Op] = match o1s {
        Op1Sel::Random => &O1,
        Op1Sel::Fixed(Op::Mul) => &[Op::Mul],
        Op1Sel::Fixed(Op::Div) => &[Op::Div],
        Op1Sel::Fixed(Op::Add) => &[Op::Add],
        Op1Sel::Fixed(Op::Sub) => &[Op::Sub],
    };
    let l2: &[Op] = match o2s {
        Op2Sel::Random => &O2,
        Op2Sel::Add => &[Op::Add],
        Op2Sel::Sub => &[Op::Sub],
    };
    let mut out = Vec::new();
    for &o1 in l1 {
        for &o2 in l2 {
            if pools_for(o1, o2, ranges).stages.iter().all(|p| p.len() >= 8) {
                out.push((o1, o2));
            }
        }
    }
    out
}

/// 留空位置 p 是否唯一解（暴力枚举该位置的取值域，域宽跟随自定义区间）
fn unique_at(q: &MathQuestion, p: u8, ranges: &MathRanges) -> bool {
    let (lo, hi) = ranges.range_of(p);
    let mut sols = 0;
    for v in lo..=hi {
        let (a, b, c, d) = (q.a, q.b, q.c, q.d);
        let (a, b, c, d) = match p {
            1 => (v, b, c, d),
            2 => (a, v, c, d),
            3 => (a, b, v, d),
            _ => (a, b, c, v),
        };
        if let Some((_, dd)) = eval_q(a, b, c, q.op1, q.op2, (ranges.d_min, ranges.d_max)) {
            if dd == d {
                sols += 1;
            }
        }
    }
    sols == 1
}

fn unique_all(q: &MathQuestion, ranges: &MathRanges) -> bool {
    (1..=4u8).all(|p| unique_at(q, p, ranges))
}

/// 组卷：符号按配额打乱、题目去重、相邻题 b/d 错开；失败返回 None
fn build(seed: u32, combos: &[(Op, Op)], ranges: &MathRanges) -> Option<Vec<MathQuestion>> {
    let mut plan: Vec<(Op, Op)> = (0..30).map(|i| combos[i % combos.len()]).collect();
    shuffle(&mut plan, &mut Rng::new(seed ^ 0x9E3779B9));

    // 预计算每个组合的候选池（默认 3 段 / 自定义 1 段），避免 1500 次重试里反复枚举
    let mut pools: HashMap<(Op, Op), SheetPools> = HashMap::new();
    for &(o1, o2) in combos {
        pools.insert((o1, o2), pools_for(o1, o2, ranges));
    }

    let mut rnd = Rng::new(seed);
    let mut qs: Vec<MathQuestion> = Vec::new();
    let mut seen: HashSet<(i32, i32, i32, Op, Op, i32)> = HashSet::new();
    for (i, &cb) in plan.iter().enumerate() {
        // 段索引按比例：3 段时 i/10（前 10 题第 1 段…），1 段时恒 0
        let sp = &pools[&cb];
        let s = i * sp.stages.len() / 30;
        let p = &sp.stages[s];
        let ap = &sp.all;
        let mut got: Option<MathQuestion> = None;
        for _ in 0..1500 {
            // 与 JS 随机数调用次序一致：先阈值（段池足够大才掷点，短路），再索引
            let src = if p.len() >= 30 && rnd.next_f64() < 0.8 { p } else { ap };
            let q = match src.get((rnd.next_f64() * src.len() as f64) as usize) {
                Some(q) => *q,
                None => continue,
            };
            let key = (q.a, q.b, q.c, q.op1, q.op2, q.d);
            if seen.contains(&key) {
                continue;
            }
            if let Some(last) = qs.last() {
                if q.b == last.b || q.d == last.d {
                    continue;
                }
            }
            got = Some(q);
            break;
        }
        let q = got?;
        if !unique_all(&q, ranges) {
            return None;
        }
        seen.insert((q.a, q.b, q.c, q.op1, q.op2, q.d));
        qs.push(q);
    }
    Some(qs)
}

/// 分布评分：c/d 覆盖越宽、b 与 d 取值越多越好
fn score(qs: &[MathQuestion]) -> f64 {
    let mut cmin = i32::MAX;
    let mut cmax = i32::MIN;
    let mut dmin = i32::MAX;
    let mut dmax = i32::MIN;
    let mut bs: HashSet<i32> = HashSet::new();
    let mut ds: HashSet<i32> = HashSet::new();
    for q in qs {
        cmin = cmin.min(q.c);
        cmax = cmax.max(q.c);
        dmin = dmin.min(q.d);
        dmax = dmax.max(q.d);
        bs.insert(q.b);
        ds.insert(q.d);
    }
    (cmax - cmin) as f64
        + (dmax - dmin) as f64 * 0.6
        + bs.len() as f64 * 3.0
        + ds.len() as f64 * 0.4
}

/// 24 个相邻 seed 各出一卷，取分布最好的；全部失败返回 None
fn build_best(seed: u32, combos: &[(Op, Op)], ranges: &MathRanges) -> Option<Vec<MathQuestion>> {
    let mut best: Option<(f64, Vec<MathQuestion>)> = None;
    for k in 0..24u32 {
        if let Some(qs) = build(seed.wrapping_add(k), combos, ranges) {
            let sc = score(&qs);
            if best.as_ref().is_none_or(|b| sc > b.0) {
                best = Some((sc, qs));
            }
        }
    }
    best.map(|(_, qs)| qs)
}

/// 出题。返回 None 表示当前符号组合无解（前端提示换运算符）。
/// ranges 为默认值时走三段难度；自定义区间用单池平铺（不再分段）。
pub fn generate(
    seed: u32,
    pos: BlankSel,
    op1: Op1Sel,
    op2: Op2Sel,
    ranges: &MathRanges,
) -> Option<MathSheet> {
    let combos = combos_for(op1, op2, ranges);
    // 与 JS 出题器一致：无可用符号组合时不出卷，而不是 panic
    if combos.is_empty() {
        return None;
    }
    let questions = build_best(seed, &combos, ranges)?;
    // 留空位置与题目无关（同 seed 换位置不换题）；随机模式四位置配额打乱
    let blanks = match pos {
        BlankSel::Random => {
            let mut arr: Vec<u8> = (0..30).map(|i| (i % 4 + 1) as u8).collect();
            shuffle(&mut arr, &mut Rng::new(seed ^ 0x5BF03635));
            arr
        }
        BlankSel::One => vec![1; 30],
        BlankSel::Two => vec![2; 30],
        BlankSel::Three => vec![3; 30],
        BlankSel::Four => vec![4; 30],
    };
    Some(MathSheet { questions, blanks })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u32 = 20261105;

    fn sheet(pos: BlankSel, op1: Op1Sel, op2: Op2Sel) -> MathSheet {
        generate(SEED, pos, op1, op2, &MathRanges::default()).expect("默认组合应能出满 30 题")
    }

    /// 测试内独立重算：留空位置 p 在指定值域内有几个解（不复用实现里的唯一解校验）
    fn solution_count(q: &MathQuestion, p: u8, ranges: MathRanges) -> i32 {
        let (lo, hi) = ranges.range_of(p);
        let mut sols = 0;
        for v in lo..=hi {
            let (a, b, c, d) = (q.a, q.b, q.c, q.d);
            let (a, b, c, d) = match p {
                1 => (v, b, c, d),
                2 => (a, v, c, d),
                3 => (a, b, v, d),
                _ => (a, b, c, v),
            };
            let t = match q.op1 {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => {
                    if b != 0 && a % b == 0 {
                        a / b
                    } else {
                        continue;
                    }
                }
            };
            if t < 1 {
                continue;
            }
            let d2 = match q.op2 {
                Op::Add => t + c,
                _ => t - c,
            };
            if d2 == d {
                sols += 1;
            }
        }
        sols
    }

    #[test]
    fn mulberry32_matches_reference_vectors() {
        // 向量值由原 JS 出题器的 rngFrom 独立实现（Python）算出，钉死「逐位一致」不被回归
        let mut rnd = Rng::new(20261105);
        assert_eq!(rnd.next_f64(), 0.10204124473966658);
        assert_eq!(rnd.next_f64(), 0.6270352604333311);
        assert_eq!(rnd.next_f64(), 0.22736229840666056);
    }

    #[test]
    fn same_seed_same_sheet() {
        let s1 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let s2 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert_eq!(s1, s2);
    }

    #[test]
    fn thirty_unique_questions() {
        let s = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert_eq!(s.questions.len(), 30);
        assert_eq!(s.blanks.len(), 30);
        let keys: HashSet<_> = s
            .questions
            .iter()
            .map(|q| (q.a, q.b, q.c, q.op1, q.op2, q.d))
            .collect();
        assert_eq!(keys.len(), 30, "同一张卷里不应有重复题");
    }

    #[test]
    fn three_difficulty_stages() {
        // 出题是 80% 取当前难度段、20% 取总池（与 JS 出题器一致），
        // 因此断言每 10 题的多数在段内，而非全部
        let s = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let ranges = [(1, 4), (3, 6), (5, 9)];
        for (block, range) in ranges.iter().enumerate() {
            let in_stage = s.questions[block * 10..block * 10 + 10]
                .iter()
                .filter(|q| (range.0..=range.1).contains(&q.a))
                .count();
            assert!(in_stage >= 6, "第 {} 段只有 {} 题在难度段内", block + 1, in_stage);
        }
    }

    #[test]
    fn unique_solution_at_every_blank() {
        let s = sheet(BlankSel::Random, Op1Sel::Random, Op2Sel::Random);
        for q in &s.questions {
            for p in 1..=4u8 {
                assert_eq!(
                    solution_count(q, p, MathRanges::default()),
                    1,
                    "{:?} 在位置 {} 不是唯一解",
                    q,
                    p
                );
            }
        }
    }

    #[test]
    fn value_ranges_hold() {
        let s = sheet(BlankSel::Two, Op1Sel::Random, Op2Sel::Random);
        for q in &s.questions {
            assert!((1..=9).contains(&q.a) && (1..=9).contains(&q.b));
            assert!((10..=50).contains(&q.c));
            assert!((1..=100).contains(&q.d));
            assert!(q.t >= 1);
            if q.op1 == Op::Div {
                assert!(q.b != 0 && q.a % q.b == 0, "除法必须表内整除");
            }
        }
    }

    #[test]
    fn div_then_sub_is_unsolvable() {
        assert!(
            generate(
                SEED,
                BlankSel::Two,
                Op1Sel::Fixed(Op::Div),
                Op2Sel::Sub,
                &MathRanges::default()
            )
            .is_none()
        );
        assert!(
            generate(
                SEED,
                BlankSel::Two,
                Op1Sel::Fixed(Op::Sub),
                Op2Sel::Sub,
                &MathRanges::default()
            )
            .is_none()
        );
    }

    #[test]
    fn random_mode_skips_unsolvable_combos() {
        // 第一步随机 + 第二步减：应跳过（除,减）与（减,减）仍出满 30 题
        let s = sheet(BlankSel::Two, Op1Sel::Random, Op2Sel::Sub);
        for q in &s.questions {
            assert!(q.op2 == Op::Sub);
            assert!(q.op1 == Op::Mul || q.op1 == Op::Add);
        }
    }

    #[test]
    fn random_blanks_cover_all_positions() {
        let s = sheet(BlankSel::Random, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let mut counts = [0usize; 5];
        for &b in &s.blanks {
            assert!((1..=4).contains(&b));
            counts[b as usize] += 1;
        }
        for (p, &n) in counts.iter().enumerate().skip(1) {
            assert!((7..=8).contains(&n), "位置 {} 配额应为 7-8 题", p);
        }
    }

    #[test]
    fn fixed_blank_position() {
        let s = sheet(BlankSel::Three, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert!(s.blanks.iter().all(|&b| b == 3));
    }

    #[test]
    fn different_seed_different_sheet() {
        // build_best 各扫 24 个相邻 seed 取最优，相邻 seed 的窗口高度重叠、
        // 常会选中同一张卷（设计内行为）；相隔 1000 窗口不相交，卷必然不同
        let s1 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let s2 = generate(
            SEED + 1000,
            BlankSel::Two,
            Op1Sel::Fixed(Op::Mul),
            Op2Sel::Random,
            &MathRanges::default(),
        )
        .expect("相隔的 seed 也应能出卷");
        assert_ne!(s1.questions, s2.questions);
    }

    use std::collections::HashSet;

    #[test]
    fn custom_ranges_widen_c_beyond_default() {
        // 第三数放宽到 100：平铺模式应出现默认三段上限（50）之外的题
        let ranges = MathRanges {
            c_min: 10,
            c_max: 100,
            ..Default::default()
        };
        let s = generate(SEED, BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random, &ranges)
            .expect("自定义区间应能出卷");
        assert_eq!(s.questions.len(), 30);
        for q in &s.questions {
            assert!((10..=100).contains(&q.c));
        }
        assert!(
            s.questions.iter().any(|q| q.c > 50),
            "默认三段上限 50，平铺模式应出现更大的第三数"
        );
    }

    #[test]
    fn custom_ranges_keep_unique_in_custom_domains() {
        // 得数限定 50-100：所有答案落在区间内，且按自定义值域重验四位置唯一解
        let ranges = MathRanges {
            d_min: 50,
            d_max: 100,
            ..Default::default()
        };
        let s = generate(SEED, BlankSel::Random, Op1Sel::Random, Op2Sel::Random, &ranges)
            .expect("自定义得数区间应能出卷");
        for q in &s.questions {
            assert!((50..=100).contains(&q.d));
            for p in 1..=4u8 {
                assert_eq!(solution_count(q, p, ranges), 1, "{:?} 在位置 {} 不是唯一解", q, p);
            }
        }
    }

    #[test]
    fn custom_ranges_reproducible() {
        let ranges = MathRanges {
            a_min: 2,
            a_max: 8,
            c_min: 20,
            c_max: 60,
            ..Default::default()
        };
        let s1 = generate(SEED, BlankSel::One, Op1Sel::Random, Op2Sel::Add, &ranges).unwrap();
        let s2 = generate(SEED, BlankSel::One, Op1Sel::Random, Op2Sel::Add, &ranges).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn ranges_validation_rejects_bad_input() {
        assert!(MathRanges { a_min: 5, a_max: 1, ..Default::default() }.validate().is_err());
        assert!(MathRanges { a_min: 0, a_max: 9, ..Default::default() }.validate().is_err());
        assert!(MathRanges { b_min: 1, b_max: 99, ..Default::default() }.validate().is_err());
        assert!(MathRanges { c_min: 1, c_max: 999, ..Default::default() }.validate().is_err());
        assert!(MathRanges::default().validate().is_ok());
    }

    #[test]
    fn parse_rejects_unknown_values() {
        assert!(BlankSel::parse("5").is_err());
        assert!(Op1Sel::parse("x").is_err());
        assert!(Op2Sel::parse("m").is_err());
    }
}
