import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import ConfirmModal from "../../components/ConfirmModal";
import { Toast, useNotice } from "../../components/Toast";
import { api, errorMessage, formatDuration } from "../../lib/ipc";
import type { MathAnswerAck, MathItem, MathPractice, MathResult } from "../../types";

const OP_SYM: Record<string, string> = { m: "×", d: "÷", a: "+", s: "−" };

/** 四位数的顺序：a op1 b op2 c = d，留空的那位由孩子填 */
const values = (it: MathItem) => [it.a, it.b, it.c, it.d];
const answerOf = (it: MathItem) => values(it)[it.blankPos - 1];
/** 空位最多几位：①② 上限 20、③ 100、④ 200 */
const maxLenOf = (it: MathItem) => (it.blankPos <= 2 ? 2 : 3);
/** 存下来的原始输入（null = 没做，'' = 跳过） */
const savedValue = (it: MathItem | undefined) => it?.input ?? "";

interface MathPracticeProps {
  sessionId: number;
  /** 返回（做题中 = 保留进度回出题页；看记录 = 回记录页） */
  onExit: () => void;
  /** 再来一张：回出题页换新卷 */
  onAgain: () => void;
}

export default function MathPractice({ sessionId, onExit, onAgain }: MathPracticeProps) {
  const [practice, setPractice] = useState<MathPractice | null>(null);
  const [result, setResult] = useState<MathResult | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [index, setIndex] = useState(0);
  const [value, setValue] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [askExit, setAskExit] = useState(false);
  const [askSubmit, setAskSubmit] = useState(false);
  const [now, setNow] = useState(Date.now());
  const inputRef = useRef<HTMLInputElement>(null);
  /** 本题开始时间：只用于记录每题用时，不影响对错 */
  const qStart = useRef(Date.now());
  /** 已经花掉的时间（换题时结算进来）。只统计本次打开后的时间：
   *  续做一张三天前的卷不该显示「用时 1234:56」，真实总用时看成绩单（按每题记录求和） */
  const activeMs = useRef(0);
  /** 弹窗打开时键盘/输入都要停：Esc 连按不能把「交卷吗」变成「放弃这次」 */
  const modalOpen = askExit || askSubmit;
  const modalOpenRef = useRef(false);
  modalOpenRef.current = modalOpen;
  const { notice, notify } = useNotice();

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const p = await api.getMathPractice(sessionId);
        if (!alive) return;
        setPractice(p);
        // 续做：从第一道没做的题开始；全做完了就停在最后一题等交卷
        const firstUnanswered = p.items.findIndex((it) => it.input === null);
        const start = firstUnanswered === -1 ? Math.max(p.items.length - 1, 0) : firstUnanswered;
        setIndex(start);
        setValue(savedValue(p.items[start]));
        qStart.current = Date.now();
        if (p.status === "abandoned") {
          onExit();
          return;
        }
        if (p.status === "finished") {
          // 已交卷的卷（多从「记录」点进来）：只读取成绩单，不重复写库
          const r = await api.getMathResult(sessionId);
          if (alive) setResult(r);
        }
      } catch (e) {
        if (alive) setLoadError(errorMessage(e));
      }
    })();
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // 整卷计时（成绩单上的用时以每题记录之和为准，这里只给孩子看）
  useEffect(() => {
    if (result || !practice) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [result, practice?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = practice?.items ?? [];
  const current = items[index];

  /** 结算本题已花掉的时间（换题/交卷时调用），保证「用时」只增不减 */
  const takeTime = useCallback(() => {
    activeMs.current += Math.max(0, Date.now() - qStart.current);
    qStart.current = Date.now();
  }, []);

  const jump = useCallback(
    (target: number, from?: MathPractice) => {
      const list = (from ?? practice)?.items ?? [];
      if (target < 0 || target >= list.length) return;
      takeTime();
      // 必须同步换 value：只 setIndex 会让渲染先画出上一题的数字（48px，肉眼可见），
      // 这一帧里按回车还会把答案写到下一题上
      setValue(savedValue(list[target]));
      setIndex(target);
    },
    [practice, takeTime],
  );

  /** 把当前输入写库。allowSkip=true 时空输入也算一次作答（= 跳过，记错）；
   *  返回 null 表示没有提交（未改动 / 正在提交）；写库失败会抛出（调用方决定要不要继续跳题）。 */
  const commit = useCallback(
    async (allowSkip: boolean): Promise<MathAnswerAck | null> => {
      if (!practice || !current || submitting) return null;
      const raw = value.trim();
      const stored = savedValue(current).trim();
      if (!allowSkip && raw === "") return null; // 翻页时没输入就只是翻页，别记成跳过
      if (current.input !== null && raw === stored) return null; // 没改动，不必再发一次
      setSubmitting(true);
      const elapsed = Math.max(0, Date.now() - qStart.current);
      try {
        const ack = await api.submitMathAnswer(practice.id, current.seq, raw, elapsed);
        setPractice((p) =>
          p
            ? {
                ...p,
                items: p.items.map((it) =>
                  it.seq === current.seq
                    ? { ...it, input: raw, elapsedMs: Math.max(it.elapsedMs ?? 0, elapsed) }
                    : it,
                ),
              }
            : p,
        );
        return ack;
      } catch (e) {
        notify("error", errorMessage(e));
        throw e;
      } finally {
        setSubmitting(false);
      }
    },
    [practice, current, value, submitting, notify],
  );

  /** 跳题：先把当前输入存下来（有输入才存），再换题 */
  const goTo = useCallback(
    async (target: number) => {
      if (!practice || target < 0 || target >= items.length || modalOpen) return;
      try {
        await commit(false);
      } catch {
        return; // 保存失败就留在本题，别把输入带走
      }
      jump(target);
      inputRef.current?.focus();
    },
    [practice, items.length, modalOpen, commit, jump],
  );

  /** 显式「确定」：空输入 = 这题跳过；做完最后一题问一句要不要交卷 */
  const submitAndAdvance = useCallback(async () => {
    if (!practice || !current || submitting) return;
    const firstTime = current.input === null;
    let ack: MathAnswerAck | null = null;
    try {
      ack = await commit(true);
    } catch {
      return; // 写库失败就停在本题，别把答案丢了
    }
    if (!ack) return;
    const answered = new Set(
      practice.items.filter((it) => it.input !== null).map((it) => it.seq),
    );
    answered.add(current.seq);
    if (ack.answered >= ack.total) {
      // 统一公布模式下不逐题判对错：第一次做满全部题才问交卷；
      // 回看时改已答的题不弹，避免每改一题就打断检查
      if (firstTime) {
        setAskSubmit(true);
        return;
      }
      jump(Math.min(items.length - 1, index + 1));
      return;
    }
    const after = practice.items.findIndex((it, i) => i > index && !answered.has(it.seq));
    const first = practice.items.findIndex((it) => !answered.has(it.seq));
    jump(after !== -1 ? after : first);
  }, [practice, current, submitting, commit, jump, items.length, index]);

  const finish = useCallback(async () => {
    if (!practice || finishing) return;
    setFinishing(true);
    try {
      setAskSubmit(false);
      setResult(await api.finishMathSession(practice.id));
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setFinishing(false);
    }
  }, [practice, finishing, notify]);

  // Esc 退出、← 上一题（输入框为空时，避免和移动光标冲突）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 弹窗自己处理 Esc；这里再响应会和 Modal 的 onClose 打架
      if (modalOpenRef.current) return;
      if (e.key === "Escape") {
        e.preventDefault();
        setAskExit(true);
      } else if (e.key === "ArrowLeft" && (inputRef.current?.value ?? "") === "") {
        e.preventDefault();
        jump(index - 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [index, jump]);

  const elapsed = activeMs.current + Math.max(0, now - qStart.current);

  if (loadError) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-stone-500">
        <p>{loadError}</p>
        <button onClick={onExit} className="rounded-md border border-stone-200 px-4 py-2 text-xs">
          返回
        </button>
      </div>
    );
  }

  if (!practice) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-stone-400">正在打开卷子…</div>
    );
  }

  if (result) {
    return (
      <ResultView
        result={result}
        onAgain={onAgain}
        onExit={onExit}
        bottom={<Toast notice={notice} />}
      />
    );
  }

  // 已交卷 / 已放弃的卷（从「记录」点进来）：等成绩单，别闪一下做题界面
  if (practice.status !== "running") {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-stone-500">
        <p>{practice.status === "abandoned" ? "这次口算已经放弃了，没有成绩。" : "正在打开成绩单…"}</p>
        <button onClick={onExit} className="rounded-md border border-stone-200 px-4 py-2 text-xs">
          返回
        </button>
      </div>
    );
  }

  const answeredCount = items.filter((it) => it.input !== null).length;
  const filledCount = items.filter((it) => it.input !== null && it.input !== "").length;
  const progress = items.length > 0 ? (answeredCount / items.length) * 100 : 0;
  if (!current) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 text-sm text-stone-500">
        <p>这张卷里没有题目，请重新出一张。</p>
        <button onClick={onExit} className="rounded-md border border-stone-200 px-4 py-2 text-xs">
          返回
        </button>
      </div>
    );
  }
  const maxLen = maxLenOf(current);

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-stone-50">
      <header className="flex items-center justify-between px-8 py-4">
        <div className="flex items-baseline gap-3">
          <span className="text-sm font-medium text-stone-700">
            第 {index + 1} / {items.length} 题
          </span>
          <span className="text-xs text-stone-400">已答 {answeredCount} 题 · 答案交卷后统一公布</span>
        </div>
        <div className="flex items-center gap-4">
          <span className="text-xs tabular-nums text-stone-400">本次用时 {formatDuration(elapsed)}</span>
          <button
            onClick={() => setAskExit(true)}
            className="rounded-md px-3 py-1.5 text-xs text-stone-500 hover:bg-stone-200/60"
          >
            退出
          </button>
        </div>
      </header>

      <div className="px-8">
        <div className="h-1 overflow-hidden rounded-full bg-stone-200">
          <div className="h-full rounded-full bg-amber-500 transition-all" style={{ width: progress + "%" }} />
        </div>
      </div>

      {/* justify-start + 内层 my-auto：内容超高时能滚到顶部，矮屏/放大字号不会把算式裁掉 */}
      <div className="flex min-h-0 flex-1 flex-col items-center justify-start overflow-y-auto px-8 py-4">
        <div className="my-auto flex w-full flex-col items-center">
          <div className="flex w-full max-w-3xl items-center justify-center rounded-2xl border border-stone-200 bg-white px-10 py-16 shadow-sm">
            <div className="flex items-center gap-4 text-stone-900">
              <span className="mb-1 text-sm tabular-nums text-stone-300">
                {String(index + 1).padStart(2, "0")}
              </span>
              {values(current).map((v, k) => {
                const isBlank = k + 1 === current.blankPos;
                return (
                  <span key={k} className="flex items-center gap-4">
                    {k > 0 && (
                      <span className="text-3xl text-stone-400">
                        {k === 1 ? OP_SYM[current.op1] : k === 2 ? OP_SYM[current.op2] : "="}
                      </span>
                    )}
                    {isBlank ? (
                      <input
                        ref={inputRef}
                        value={value}
                        inputMode="numeric"
                        autoComplete="off"
                        disabled={modalOpen}
                        maxLength={maxLen}
                        onChange={(e) => setValue(e.target.value.replace(/[^\d]/g, ""))}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            void submitAndAdvance();
                          }
                        }}
                        className="rounded-lg border-2 border-amber-400 bg-amber-50/40 text-center font-semibold tabular-nums text-stone-900 outline-none focus:border-amber-500 focus:bg-amber-50 disabled:opacity-60"
                        style={{
                          width: maxLen >= 3 ? "2.6em" : "1.9em",
                          fontSize: "48px",
                          lineHeight: 1.35,
                        }}
                      />
                    ) : (
                      <span className="font-semibold tabular-nums" style={{ fontSize: "48px" }}>
                        {v}
                      </span>
                    )}
                  </span>
                );
              })}
            </div>
          </div>

          <NumPad
            disabled={modalOpen}
            onDigit={(d) => {
              setValue((v) => (v.length >= maxLen ? v : v + d));
              inputRef.current?.focus();
            }}
            onBackspace={() => {
              setValue((v) => v.slice(0, -1));
              inputRef.current?.focus();
            }}
            onSubmit={() => void submitAndAdvance()}
          />

          <p className="mt-3 text-xs text-stone-400">
            {value === ""
              ? "空着按回车 = 这题跳过（算错）· ← 上一题 · Esc 退出"
              : "回车 = 下一题 · ← 上一题 · 答完统一公布答案"}
          </p>

          {/* 题号条：统一公布模式下允许回头检查、改答案 */}
          <div className="mt-4 flex w-full max-w-3xl flex-wrap justify-center gap-1.5">
            {items.map((it, i) => (
              <button
                key={it.seq}
                onClick={() => void goTo(i)}
                title={
                  it.input === null
                    ? `第 ${i + 1} 题（没做）`
                    : it.input === ""
                      ? `第 ${i + 1} 题（空着交的）`
                      : `第 ${i + 1} 题`
                }
                className={
                  "h-7 w-7 rounded text-[11px] tabular-nums transition " +
                  (i === index
                    ? "bg-stone-800 font-medium text-white"
                    : it.input === ""
                      ? "bg-rose-50 text-rose-500 ring-1 ring-rose-200 hover:bg-rose-100"
                      : it.input !== null
                        ? "bg-amber-100 text-amber-800 hover:bg-amber-200"
                        : "bg-white text-stone-400 ring-1 ring-stone-200 hover:bg-stone-100")
                }
              >
                {i + 1}
              </button>
            ))}
          </div>
        </div>
      </div>

      <footer className="flex items-center justify-center gap-3 px-8 pb-6">
        <button
          onClick={() => void goTo(index - 1)}
          disabled={index === 0}
          className="rounded-md border border-stone-200 bg-white px-4 py-2 text-sm text-stone-600 hover:bg-stone-50 disabled:opacity-40"
        >
          上一题
        </button>
        <button
          onClick={() => void goTo(index + 1)}
          disabled={index === items.length - 1}
          className="rounded-md border border-stone-200 bg-white px-4 py-2 text-sm text-stone-600 hover:bg-stone-50 disabled:opacity-40"
        >
          下一题
        </button>
        <button
          onClick={() => void submitAndAdvance()}
          disabled={submitting}
          className="rounded-md bg-stone-800 px-7 py-2 text-sm font-medium text-white hover:bg-stone-900 disabled:opacity-60"
        >
          {submitting ? "提交中…" : "确定（回车）"}
        </button>
        <button
          onClick={() => setAskSubmit(true)}
          disabled={submitting}
          className="rounded-md border border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-800 hover:bg-amber-100 disabled:opacity-60"
        >
          交卷
        </button>
      </footer>

      {askExit && !askSubmit && (
        <ConfirmModal
          title="退出做题"
          body="已经做过的题都保存着，下次点「开始做题」可以从这道题接着做。"
          confirmText="退出并保留"
          cancelText="继续做题"
          extraText="放弃这次"
          onExtra={async () => {
            try {
              await api.abandonMathSession(practice.id);
              onExit();
            } catch (e) {
              notify("error", errorMessage(e));
            }
          }}
          onCancel={() => {
            setAskExit(false);
            inputRef.current?.focus();
          }}
          onConfirm={() => {
            void (async () => {
              try {
                await commit(false); // 别把刚敲的数字丢了
              } catch {
                // 存不上也让人走，输入还在库里上一次的值
              }
              setAskExit(false);
              onExit();
            })();
          }}
        />
      )}

      {askSubmit && (
        <ConfirmModal
          title="交卷吗？"
          body={
            answeredCount >= items.length
              ? filledCount === items.length
                ? `${items.length} 题都做完了。交卷后统一公布答案和错题，就不能再改了。`
                : `有 ${items.length - filledCount} 题是空着的（会算错）。交卷后统一公布答案。`
              : `还有 ${items.length - answeredCount} 题没做，空着的会算错。交卷后统一公布答案。`
          }
          confirmText={finishing ? "正在交卷…" : "交卷看成绩"}
          cancelText="再检查一下"
          onCancel={() => {
            setAskSubmit(false);
            inputRef.current?.focus();
          }}
          onConfirm={() => void finish()}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}

/** 屏下数字键盘：没有键盘 / 触屏时也能做题 */
function NumPad({
  disabled,
  onDigit,
  onBackspace,
  onSubmit,
}: {
  disabled: boolean;
  onDigit: (d: string) => void;
  onBackspace: () => void;
  onSubmit: () => void;
}) {
  const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
  return (
    <div className="mt-6 grid w-full max-w-xs grid-cols-3 gap-2">
      {keys.map((k) => (
        <button
          key={k}
          onClick={() => onDigit(k)}
          disabled={disabled}
          className="h-11 rounded-lg border border-stone-200 bg-white text-lg tabular-nums text-stone-700 hover:bg-stone-100 active:bg-stone-200 disabled:opacity-50"
        >
          {k}
        </button>
      ))}
      <button
        onClick={onBackspace}
        disabled={disabled}
        className="h-11 rounded-lg border border-stone-200 bg-white text-sm text-stone-500 hover:bg-stone-100 active:bg-stone-200 disabled:opacity-50"
      >
        退格
      </button>
      <button
        onClick={() => onDigit("0")}
        disabled={disabled}
        className="h-11 rounded-lg border border-stone-200 bg-white text-lg tabular-nums text-stone-700 hover:bg-stone-100 active:bg-stone-200 disabled:opacity-50"
      >
        0
      </button>
      <button
        onClick={onSubmit}
        disabled={disabled}
        className="h-11 rounded-lg bg-stone-800 text-sm font-medium text-white hover:bg-stone-900 active:bg-black disabled:opacity-50"
      >
        ✓
      </button>
    </div>
  );
}

/** 成绩页：全场统一公布——对几题、用时、错题逐条回看 */
function ResultView({
  result,
  onAgain,
  onExit,
  bottom,
}: {
  result: MathResult;
  onAgain: () => void;
  onExit: () => void;
  bottom: ReactNode;
}) {
  const wrong = result.items.filter((it) => it.correct !== true);
  const avg = result.total > 0 ? result.durationMs / result.total / 1000 : 0;
  const rate = result.total > 0 ? result.correctTotal / result.total : 0;
  const praise =
    wrong.length === 0
      ? "全部做对，太棒了！"
      : rate >= 0.9
        ? `非常接近满分，错的这 ${wrong.length} 道再看一眼。`
        : rate >= 0.8
          ? "不错，把错题再做一遍就稳了。"
          : "错的有点多，先把错题弄懂再来一张。";
  const unanswered = wrong.filter((it) => it.input === null).length;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-stone-50">
      <div className="mx-auto w-full max-w-3xl px-8 py-10">
        <div className="rounded-2xl border border-stone-200 bg-white px-8 py-7 shadow-sm">
          <div className="flex items-center justify-between">
            <p className="text-xs text-stone-400">口算练习 · 已交卷 · {result.total} 道</p>
            {result.newBest && (
              <span className="rounded-full bg-amber-100 px-2.5 py-1 text-[11px] font-medium text-amber-800">
                新的全对最快纪录
              </span>
            )}
          </div>
          <div className="mt-2 flex items-end gap-6">
            <p className="text-stone-900">
              <span className="text-5xl font-semibold tabular-nums text-amber-600">
                {result.correctTotal}
              </span>
              <span className="ml-1 text-lg text-stone-400">/ {result.total} 题</span>
            </p>
            <div className="mb-2 flex gap-6 text-xs text-stone-500">
              <span>
                用时 <b className="tabular-nums text-stone-700">{formatDuration(result.durationMs)}</b>
              </span>
              <span>
                平均每题 <b className="tabular-nums text-stone-700">{avg.toFixed(1)}</b> 秒
              </span>
            </div>
          </div>
          <p className="mt-3 text-sm text-stone-600">{praise}</p>
          <p className="mt-1.5 text-xs text-stone-400">
            {result.bestMs == null
              ? `${result.total} 道全对最快纪录：还没有（全对一次就会出现）`
              : `${result.total} 道全对最快：${formatDuration(result.bestMs)}`}
            {result.newBest && " · 就是这一次"}
          </p>
        </div>

        {wrong.length > 0 ? (
          <div className="mt-5 rounded-2xl border border-stone-200 bg-white px-6 py-5 shadow-sm">
            <div className="flex items-baseline justify-between">
              <h2 className="text-sm font-medium text-stone-700">
                错题 {wrong.length} 道
                {unanswered > 0 && (
                  <span className="ml-1 text-xs font-normal text-stone-400">
                    （含 {unanswered} 道没做）
                  </span>
                )}
              </h2>
              <span className="text-xs text-stone-400">你的答案 → 正确答案</span>
            </div>
            <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
              {wrong.map((it) => (
                <WrongRow key={it.seq} item={it} />
              ))}
            </div>
          </div>
        ) : (
          <div className="mt-5 rounded-2xl border border-stone-200 bg-white px-6 py-8 text-center text-sm text-stone-500 shadow-sm">
            没有错题，可以再来一张更难的。
          </div>
        )}

        <div className="mt-6 flex items-center justify-end gap-3">
          <button
            onClick={onExit}
            className="rounded-md border border-stone-200 bg-white px-4 py-2 text-sm text-stone-600 hover:bg-stone-50"
          >
            返回
          </button>
          <button
            onClick={onAgain}
            className="rounded-md bg-stone-800 px-5 py-2 text-sm font-medium text-white hover:bg-stone-900"
          >
            再来一张
          </button>
        </div>
      </div>
      {bottom}
    </div>
  );
}

/** 一条错题：完整算式（空位填正确答案）+ 孩子写的是什么 */
function WrongRow({ item }: { item: MathItem }) {
  const nums = values(item);
  const mine = item.input === null ? "没做" : item.input === "" ? "空着" : item.input;
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-stone-100 bg-stone-50/60 px-3 py-2">
      <span className="flex items-baseline gap-1.5 text-sm tabular-nums text-stone-700">
        <span className="text-[11px] text-stone-400">{String(item.seq + 1).padStart(2, "0")}</span>
        {nums.map((v, k) => (
          <span key={k} className="flex items-baseline gap-1.5">
            {k > 0 && (
              <span className="text-stone-400">
                {k === 1 ? OP_SYM[item.op1] : k === 2 ? OP_SYM[item.op2] : "="}
              </span>
            )}
            {k + 1 === item.blankPos ? (
              <b className="rounded bg-emerald-50 px-1 font-semibold text-emerald-700">{v}</b>
            ) : (
              <span>{v}</span>
            )}
          </span>
        ))}
      </span>
      <span className="shrink-0 text-xs tabular-nums text-stone-400">
        <span className={item.correct === false ? "text-rose-500" : ""}>{mine}</span>
        <span className="mx-1">→</span>
        <b className="text-stone-600">{answerOf(item)}</b>
      </span>
    </div>
  );
}
