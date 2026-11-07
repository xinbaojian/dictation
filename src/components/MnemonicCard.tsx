import { useEffect, useState } from "react";
import type { AiCard, Mistake } from "../types";

interface MnemonicCardProps {
  word: Mistake;
  card: AiCard | null;
  /** AI 已配置（无缓存卡时才显示「生成」按钮）；没配置也可能有同步来的缓存卡 */
  aiReady: boolean;
  /** 「先自己写」遮住笔画中：口诀与部件等于答案提示，整卡隐藏 */
  masked: boolean;
  generating: boolean;
  autoSpeak: boolean;
  onGenerate: () => void;
  onSpeak: (text: string) => void;
}

/** 部件积木块的循环配色（沿用应用暖色系，不引新色板） */
const BLOCK_BG = ["bg-amber-100", "bg-emerald-100", "bg-sky-100", "bg-rose-100"];

/** 复习页「✨ 巧记」卡：孩子先看（部件积木 → 大字口诀 → 例句），家长补充（易混字辨析） */
export default function MnemonicCard({
  word,
  card,
  aiReady,
  masked,
  generating,
  autoSpeak,
  onGenerate,
  onSpeak,
}: MnemonicCardProps) {
  const [open, setOpen] = useState(false);
  const c = card?.content ?? null;

  // 展开时自动读口诀：二年级识字量有限，听觉是主通道；与「自动朗读」共用一个偏好
  useEffect(() => {
    if (open && autoSpeak && c) onSpeak(c.mnemonic);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (masked) return null;
  if (!c && !aiReady) return null;

  // 没有缓存卡：一个按钮直接触发生成
  if (!c) {
    return (
      <button
        onClick={onGenerate}
        disabled={generating}
        className="rounded-full border border-amber-300 bg-amber-50 px-5 py-2 text-sm text-amber-700 hover:bg-amber-100 disabled:cursor-wait disabled:opacity-60"
      >
        {generating ? "讲解生成中，稍等…" : "✨ 生成巧记讲解"}
      </button>
    );
  }

  const wordChars = new Set([...word.text]);
  return (
    <div className="w-full rounded-2xl border border-amber-200 bg-amber-50/60 px-6 py-4">
      <div className="flex items-center justify-between">
        <button
          onClick={() => setOpen(!open)}
          className="text-sm font-medium text-amber-700 hover:text-amber-800"
        >
          ✨ 巧记 {open ? "▴" : "▾"}
        </button>
        <div className="flex items-center gap-2">
          <button
            onClick={() => onSpeak(c.mnemonic)}
            title="用系统语音读一遍口诀"
            className="rounded-md border border-amber-200 bg-white px-2.5 py-1 text-xs text-stone-600 hover:bg-amber-50"
          >
            🔊 读口诀
          </button>
          <button
            onClick={onGenerate}
            disabled={generating}
            title="重新生成（会覆盖当前内容）"
            className="rounded-md px-2 py-1 text-xs text-stone-400 hover:text-amber-700 disabled:opacity-50"
          >
            {generating ? "生成中…" : "重新生成"}
          </button>
        </div>
      </div>
      {open && (
        <div className="mt-3 flex flex-col gap-3">
          {c.decomposition.length >= 2 && (
            <div className="flex items-end justify-center gap-2">
              {c.decomposition.map((part, i) => (
                <span
                  key={i}
                  className={
                    "flex h-14 w-14 items-center justify-center rounded-xl text-3xl text-stone-700 " +
                    BLOCK_BG[i % BLOCK_BG.length]
                  }
                >
                  {part}
                </span>
              ))}
              <span className="pb-1 text-xs text-stone-400">合起来就是它</span>
            </div>
          )}
          <p className="text-center text-2xl leading-relaxed text-stone-800">
            {[...c.mnemonic].map((ch, i) => (
              <span
                key={i}
                className={wordChars.has(ch) ? "rounded bg-amber-200/80 px-0.5" : ""}
              >
                {ch}
              </span>
            ))}
          </p>
          {c.sentences.map((s, i) => (
            <p key={i} className="text-center text-base text-stone-600">
              {s.split(word.text).map((part, j, arr) => (
                <span key={j}>
                  {part}
                  {j < arr.length - 1 && (
                    <u className="decoration-amber-500 decoration-2 underline-offset-4">
                      {word.text}
                    </u>
                  )}
                </span>
              ))}
            </p>
          ))}
          {c.confusables.length > 0 && (
            <div className="rounded-lg bg-white/70 px-3 py-2">
              <p className="text-[11px] text-stone-400">👤 给家长：孩子容易写混的字</p>
              <ul className="mt-1 flex flex-col gap-0.5">
                {c.confusables.map((f, i) => (
                  <li key={i} className="text-xs text-stone-600">
                    {f.char}：{f.hint}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
