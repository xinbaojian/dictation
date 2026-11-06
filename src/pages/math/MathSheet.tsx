import { useEffect, useState } from "react";
import { Toast, useNotice } from "../../components/Toast";
import { api, errorMessage } from "../../lib/ipc";
import type { MathSheet } from "../../types";
import { buildExample, type MathOp } from "./example";
import "./math-sheet.css";

/** 选项与原 HTML 出题器的 URL 参数一一对应；桌面版存 localStorage（seed 不存，每次进来换新卷） */
interface MathOptions {
  pos: string; // "1" | "2" | "3" | "4" | "random"
  op1: string; // "m" | "d" | "a" | "s" | "random"
  op2: string; // "a" | "s" | "random"
  showAns: boolean;
}

const DEFAULT_OPTIONS: MathOptions = { pos: "2", op1: "m", op2: "random", showAns: false };
const STORAGE_KEY = "mathSheet.options";

function loadOptions(): MathOptions {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...DEFAULT_OPTIONS, ...(JSON.parse(raw) as Partial<MathOptions>) };
  } catch {
    // 坏数据一律回默认
  }
  return DEFAULT_OPTIONS;
}

const POS_OPTIONS = [
  { value: "1", label: "① 第 1 个" },
  { value: "2", label: "② 第 2 个" },
  { value: "3", label: "③ 第 3 个" },
  { value: "4", label: "④ 第 4 个" },
  { value: "random", label: "随机（每题不同）" },
];
const OP1_OPTIONS = [
  { value: "m", label: "× 乘" },
  { value: "d", label: "÷ 除" },
  { value: "a", label: "+ 加" },
  { value: "s", label: "− 减" },
  { value: "random", label: "随机" },
];
const OP2_OPTIONS = [
  { value: "a", label: "+ 加" },
  { value: "s", label: "− 减" },
  { value: "random", label: "随机" },
];

const OP_SYM: Record<string, string> = { m: "×", d: "÷", a: "+", s: "−" };
const OP1_NAME: Record<string, string> = { m: "乘", d: "除", a: "加", s: "减" };

const posLabel = (p: string) => (p === "random" ? "每题随机留空" : `填第 ${p} 个数`);
const op1Label = (s: string) => (s === "random" ? "随机" : OP1_NAME[s]);
const op2Label = (s: string) => (s === "random" ? "加减" : s === "a" ? "加" : "减");

/** 每次访问换一套新题；>>> 0 保证落在 u32 范围内与后端 seed 对齐 */
const newSeed = () => (Date.now() ^ Math.floor(Math.random() * 2147483647)) >>> 0;

export default function MathSheetPage() {
  const [options, setOptions] = useState<MathOptions>(loadOptions);
  const [seed, setSeed] = useState(newSeed);
  const [sheet, setSheet] = useState<MathSheet | null>(null);
  const [loading, setLoading] = useState(true);
  const { notice, notify } = useNotice();

  // 第一步为 ÷/− 时中间结果 ≤9，减不动 ≥10 的第三数：第二步强制用加（与原 normalize 一致）
  const eff =
    (options.op1 === "d" || options.op1 === "s") && options.op2 === "s"
      ? { pos: options.pos, op1: options.op1, op2: "a" }
      : { pos: options.pos, op1: options.op1, op2: options.op2 };
  const autoNote = (options.op1 === "d" || options.op1 === "s") && options.op2 === "s";
  // 随机（或除/减）配 减/随机 时，无解的符号组合会被引擎略过——明说，别让家长以为随机失效
  const skipNote =
    (eff.op1 === "random" || eff.op1 === "d" || eff.op1 === "s") &&
    (eff.op2 === "s" || eff.op2 === "random");

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(options));
  }, [options]);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    (async () => {
      try {
        const s = await api.generateMathSheet(seed, eff.pos, eff.op1, eff.op2);
        if (alive) setSheet(s);
      } catch (e) {
        if (alive) {
          setSheet(null);
          notify("error", errorMessage(e));
        }
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
    // eff 由下方三个原始值派生，只依赖原始值即可
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seed, options.pos, options.op1, options.op2]);

  const first = sheet?.questions[0];
  const example = first
    ? buildExample(
        options.pos === "random" ? 3 : Number(options.pos),
        first.op1 as MathOp,
        first.op2 as MathOp,
      )
    : null;

  return (
    <div className="print-root min-h-0 flex-1 overflow-y-auto bg-stone-200/60">
      {/* 本页满幅打印；@page 无法按 class 隔离，由各打印页自己渲染（默写单是 12mm） */}
      <style>{"@page { size: A4 portrait; margin: 0 }"}</style>

      <div className="no-print">
        <div className="ms-toolbar">
          <div className="ms-tb-line">
            <span className="ms-tb-title">二年级口算 · A4 竖版 1 页 30 题</span>
            <span className="ms-hint">
              从左到右两步算：① 数 <em>×/÷/+/−</em> ② 数 <em>±</em> ③ 数 = ④ 得数
            </span>
            <span className="ms-hint">打印：边距「无」、缩放 100%</span>
          </div>
          <div className="ms-tb-line">
            {(
              [
                { id: "pos", label: "让第几个数留空", items: POS_OPTIONS },
                { id: "op1", label: "第一步", items: OP1_OPTIONS },
                { id: "op2", label: "第二步", items: OP2_OPTIONS },
              ] as const
            ).map((g) => (
              <div className="ms-seg" key={g.id}>
                <span className="ms-seg-label">{g.label}</span>
                {g.items.map((it) => (
                  <label key={it.value}>
                    <input
                      type="radio"
                      name={g.id}
                      value={it.value}
                      checked={options[g.id] === it.value}
                      onChange={() => setOptions((o) => ({ ...o, [g.id]: it.value }))}
                    />
                    {it.label}
                  </label>
                ))}
              </div>
            ))}
          </div>
          <div className="ms-tb-line">
            <button
              className="ms-btn ms-btn-ghost"
              type="button"
              aria-pressed={!options.showAns}
              onClick={() => setOptions((o) => ({ ...o, showAns: !o.showAns }))}
            >
              {options.showAns ? "隐藏答案 · 给孩子" : "显示答案 · 家长版"}
            </button>
            <button className="ms-btn ms-btn-alt" type="button" onClick={() => setSeed(newSeed())}>
              重新生成算式
            </button>
            <button className="ms-btn" type="button" onClick={() => window.print()}>
              打印 / 存为 PDF
            </button>
          </div>
        </div>
      </div>

      <div className="ms-stack">
        <p className="ms-cap no-print">
          二年级口算 · {posLabel(options.pos)} · {eff.op1 === "random" ? "混合" : OP_SYM[eff.op1]} →{" "}
          {eff.op2 === "random" ? "±" : OP_SYM[eff.op2]} · 题目 #{seed} ·{" "}
          {options.showAns ? "答案条可沿虚线剪下" : "已隐藏答案，打印出来直接给孩子做"}
          {autoNote ? " · 第一步是除或减时第二步只能用加，已自动切换" : ""}
          {skipNote ? " · 随机已跳过无解组合（除 / 减 配 减）" : ""}
        </p>

        {loading ? (
          <div className="flex w-full items-center justify-center py-20 text-sm text-stone-400">
            正在出题…
          </div>
        ) : !sheet ? (
          <div className="flex w-[210mm] max-w-full flex-col items-center gap-2 rounded bg-white px-6 py-20 shadow-md">
            <p className="text-sm text-stone-500">这组运算符暂时出不出 30 道题</p>
            <p className="text-xs text-stone-400">请换一个第二步运算符，或把第一步改成「随机」</p>
          </div>
        ) : (
          <div className={"ms-sheet" + (options.showAns ? "" : " ms-hide-ans")}>
            <div className="ms-hd">
              <div className="ms-hd-top">
                <div>
                  <div className="ms-eyebrow">
                    <i /> 每日口算
                  </div>
                  <h1 className="ms-h1">想一想，横线上填几？</h1>
                </div>
                <div className="ms-tag">
                  {posLabel(eff.pos)} · 先{op1Label(eff.op1)}，再{op2Label(eff.op2)}
                </div>
              </div>
              <div className="ms-meta">
                <span>
                  姓名<span className="ms-fill" />
                </span>
                <span>
                  日期<span className="ms-fill" />
                </span>
                <span>
                  用时<span className="ms-fill w-s" />
                  分<span className="ms-fill w-s" />
                  秒
                </span>
                <span>
                  做对<span className="ms-fill w-s" />
                  / 30 题
                </span>
              </div>
            </div>

            <div className="ms-tip">
              <div>
                <span className="lead">
                  {options.pos === "random"
                    ? "先看清空在哪一位、两步各是什么运算："
                    : eff.op2 === "random"
                      ? "先看第二步是 + 还是 −，再倒着把缺的数找出来："
                      : "先算能算的那一步，再倒着把缺的数找出来："}
                </span>
              </div>
              {example && (
                <div className="ex">
                  {options.pos === "random" ? "举例：" : "例："}
                  {example.expr} → {example.step}
                  {example.answer !== "" && (
                    <>
                      ，填 <em>{example.answer}</em>
                    </>
                  )}
                  。
                </div>
              )}
            </div>

            <div className="ms-grid">
              {sheet.questions.map((q, i) => {
                const pos = sheet.blanks[i];
                const nums = [q.a, q.b, q.c, q.d];
                const cells = nums.map((v, k) =>
                  k + 1 === pos ? <span className="ms-blank" key={k} /> : <span key={k}>{v}</span>,
                );
                return (
                  <div className="ms-q" key={i}>
                    <span className="ms-no">{String(i + 1).padStart(2, "0")}</span>
                    <span className="ms-expr">
                      {cells[0]}
                      <span className="ms-op">{OP_SYM[q.op1]}</span>
                      {cells[1]}
                      <span className="ms-op">{OP_SYM[q.op2]}</span>
                      {cells[2]}
                      <span className="ms-op">=</span>
                      {cells[3]}
                    </span>
                  </div>
                );
              })}
            </div>

            <div className="ms-ansbar">
              <div className="ms-ans-h">
                参考答案<em>沿虚线剪下，做完再核对</em>
              </div>
              <div className="ms-ans-grid">
                {sheet.questions.map((q, i) => {
                  const nums = [q.a, q.b, q.c, q.d];
                  return (
                    <span key={i}>
                      {i + 1}.&nbsp;<b>{nums[sheet.blanks[i] - 1]}</b>
                    </span>
                  );
                })}
              </div>
            </div>
          </div>
        )}
      </div>

      <Toast notice={notice} />
    </div>
  );
}
