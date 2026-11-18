import { useState } from "react";
import Modal from "./Modal";
import type { Mistake } from "../types";

interface EditMistakeModalProps {
  word: Mistake;
  /** 成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (text: string, chars: number[]) => Promise<void>;
  onClose: () => void;
}

export default function EditMistakeModal({ word, onSubmit, onClose }: EditMistakeModalProps) {
  const [text, setText] = useState(word.text);
  const [chars, setChars] = useState<Set<number>>(new Set(word.wrongChars));
  const [busy, setBusy] = useState(false);

  const trimmed = text.trim();
  const glyphs = [...trimmed];

  const toggle = (index: number) => {
    setChars((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const submit = async () => {
    if (!trimmed || busy) return;
    setBusy(true);
    try {
      await onSubmit(trimmed, [...chars].sort((a, b) => a - b));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title="编辑错题"
      subtitle="改词语文字后，之前标的字会失效，需要重新点"
      width="max-w-md"
      onClose={onClose}
      footer={
        <>
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            取消
          </button>
          <button
            onClick={submit}
            disabled={!trimmed || busy}
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            保存
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">词语</span>
          <input
            value={text}
            autoFocus
            onChange={(e) => {
              setText(e.target.value);
              // 字变了，之前标的下标就失效了
              setChars(new Set());
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
            }}
            className="w-full rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
          />
        </label>
        {glyphs.length > 0 && (
          <div>
            <span className="mb-1.5 block text-xs font-medium text-stone-500">不会写的字（可不点）</span>
            <div className="flex flex-wrap gap-1.5">
              {glyphs.map((ch, i) => (
                <button
                  key={i}
                  onClick={() => toggle(i)}
                  className={
                    "rounded-md border px-2.5 py-1 text-sm " +
                    (chars.has(i)
                      ? "border-rose-300 bg-rose-100 text-rose-800"
                      : "border-stone-200 bg-white text-stone-700 hover:border-stone-300")
                  }
                >
                  {ch}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
