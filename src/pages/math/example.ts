/** 口算示范条：中文数字、乘法口诀与「先算能算的一步，再倒着找缺的数」的示范。
 *  移植自口算练习 HTML 出题器；纯前端展示逻辑。 */

export type MathOp = "m" | "d" | "a" | "s";

const SYM: Record<MathOp, string> = { m: "×", d: "÷", a: "+", s: "−" };

const CN = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

function cnNum(n: number): string {
  if (n < 10) return CN[n];
  if (n === 10) return "一十"; // 「二五一十」，不是「二五十」
  if (n < 20) return "十" + (n % 10 ? CN[n % 10] : "");
  return CN[Math.floor(n / 10)] + "十" + (n % 10 ? CN[n % 10] : "");
}

/** 乘法口诀：小的数在前，积 <10 用「得」 */
function verse(x: number, y: number): string {
  const lo = Math.min(x, y);
  const hi = Math.max(x, y);
  const p = lo * hi;
  return cnNum(lo) + cnNum(hi) + (p < 10 ? "得" : "") + cnNum(p);
}

function evalQ(a: number, b: number, c: number, o1: MathOp, o2: MathOp): { t: number; d: number } | null {
  let t: number;
  if (o1 === "m") t = a * b;
  else if (o1 === "d") {
    if (b === 0 || a % b !== 0) return null;
    t = a / b;
  } else if (o1 === "a") t = a + b;
  else t = a - b;
  if (t < 1) return null;
  const d = o2 === "s" ? t - c : t + c;
  if (d < 1 || d > 100) return null;
  return { t, d };
}

export interface MathExample {
  expr: string;
  /** 示范步骤（结尾「填 X」由调用方渲染强调） */
  step: string;
  /** 最终要填的数 */
  answer: string;
}

export function buildExample(pos: number, o1: MathOp, o2: MathOp): MathExample {
  let A = 8;
  let B = 4;
  let C = 10;
  let e = evalQ(A, B, C, o1, o2);
  if (!e) {
    A = 6;
    B = 3;
    C = 20;
    e = evalQ(A, B, C, o1, o2);
  }
  if (!e) {
    return { expr: "", step: "先看清楚空在哪一位，能算的那一步先算出来，再倒着想缺的数。", answer: "" };
  }
  const { t, d } = e;
  const nums = [A, B, C, d];
  const ans = nums[pos - 1];
  const parts = nums.map((v, k) => (k + 1 === pos ? "___" : String(v)));
  const expr = `${parts[0]} ${SYM[o1]} ${parts[1]} ${SYM[o2]} ${parts[2]} = ${parts[3]}`;
  let step: string;
  if (pos === 4) {
    step = `${A} ${SYM[o1]} ${B} = ${t}，${t} ${SYM[o2]} ${C} = ${d}`;
  } else if (pos === 3) {
    const s1 = `${A} ${SYM[o1]} ${B} = ${t}`;
    const s2 = o2 === "a" ? `${d} − ${t} = ${C}` : `${t} − ${d} = ${C}`;
    step = `${s1}，${s2}`;
  } else {
    const backT = o2 === "a" ? `${d} − ${C} = ${t}` : `${d} + ${C} = ${t}`;
    let back: string;
    if (o1 === "m") back = `想「${pos === 1 ? `几 × ${B}` : `${A} × 几`} = ${t}」→ ${verse(A, B)}`;
    else if (o1 === "d") back = `想 ${pos === 1 ? `${B} × ${t}` : `${t} × ${B}`} = ${A} → ${verse(B, t)}`;
    else if (o1 === "a") back = pos === 1 ? `${t} − ${B} = ${A}` : `${t} − ${A} = ${B}`;
    else back = pos === 1 ? `${t} + ${B} = ${A}` : `${A} − ${t} = ${B}`;
    step = `${backT}，${back}`;
  }
  return { expr, step, answer: String(ans) };
}
