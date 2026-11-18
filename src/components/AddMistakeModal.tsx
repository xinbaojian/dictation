import { useEffect, useState } from "react";
import Modal from "./Modal";
import { api } from "../lib/ipc";
import type { Group, SplitResult } from "../types";

interface AddMistakeModalProps {
  groups: Group[];
  defaultGroupId: number | null;
  /** 单个模式提交；成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (groupId: number, text: string, chars: number[]) => Promise<void>;
  /** 批量模式提交（texts 已切分去重） */
  onSubmitBatch: (groupId: number, texts: string[]) => Promise<void>;
  onClose: () => void;
}

type Mode = "single" | "batch";

export default function AddMistakeModal({
  groups,
  defaultGroupId,
  onSubmit,
  onSubmitBatch,
  onClose,
}: AddMistakeModalProps) {
  const [mode, setMode] = useState<Mode>("single");
  const [groupId, setGroupId] = useState<number>(defaultGroupId ?? groups[0]?.id ?? 0);
  const [text, setText] = useState("");
  const [chars, setChars] = useState<Set<number>>(new Set());
  const [batchText, setBatchText] = useState("");
  const [preview, setPreview] = useState<SplitResult | null>(null);
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

  // 批量输入防抖预览：与抽题页 count 查询同节奏
  useEffect(() => {
    if (mode !== "batch") return;
    if (!batchText.trim()) {
      setPreview(null);
      return;
    }
    let alive = true;
    const timer = setTimeout(() => {
      api
        .previewWords(batchText)
        .then((r) => {
          if (alive) setPreview(r);
        })
        .catch(() => {
          if (alive) setPreview(null);
        });
    }, 120);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [mode, batchText]);

  const submitSingle = async () => {
    if (!trimmed || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmit(groupId, trimmed, [...chars].sort((a, b) => a - b));
    } finally {
      setBusy(false);
    }
  };

  const submitBatch = async () => {
    const words = preview?.words ?? [];
    if (words.length === 0 || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmitBatch(groupId, words);
    } finally {
      setBusy(false);
    }
  };

  const batchCount = preview?.words.length ?? 0;

  return (
    <Modal
      title="加入错题本"
      subtitle="批量模式粘贴整段词语，按空格、逗号等分隔"
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
          {mode === "single" ? (
            <button
              onClick={submitSingle}
              disabled={!trimmed || !groupId || busy}
              className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
            >
              加入
            </button>
          ) : (
            <button
              onClick={submitBatch}
              disabled={batchCount === 0 || !groupId || busy}
              className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
            >
              加入 {batchCount > 0 ? batchCount + " 个词" : ""}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        <div className="flex gap-1 rounded-lg bg-stone-100 p-1">
          {(
            [
              { key: "single", label: "单个" },
              { key: "batch", label: "批量" },
            ] as { key: Mode; label: string }[]
          ).map((m) => (
            <button
              key={m.key}
              onClick={() => setMode(m.key)}
              className={
                "flex-1 rounded-md px-3 py-1.5 text-sm transition " +
                (mode === m.key
                  ? "bg-white font-medium text-stone-800 shadow-sm"
                  : "text-stone-500 hover:text-stone-700")
              }
            >
              {m.label}
            </button>
          ))}
        </div>
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">分组</span>
          <select
            value={groupId}
            onChange={(e) => setGroupId(Number(e.target.value))}
            className="w-full rounded-md border border-stone-200 bg-white px-3 py-2 text-sm text-stone-700 outline-none focus:border-stone-400"
          >
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.path}
              </option>
            ))}
          </select>
        </label>
        {mode === "single" ? (
          <>
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
                  if (e.key === "Enter") void submitSingle();
                }}
                placeholder="如：春天"
                className="w-full rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
              />
            </label>
            {glyphs.length > 0 && (
              <div>
                <span className="mb-1.5 block text-xs font-medium text-stone-500">
                  不会写的字（可不点）
                </span>
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
          </>
        ) : (
          <>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-stone-500">
                词语（空格、逗号、顿号、换行都行）
              </span>
              <textarea
                value={batchText}
                autoFocus
                onChange={(e) => setBatchText(e.target.value)}
                placeholder={"如：春天 秋天，\n月亮、太阳"}
                rows={4}
                className="w-full resize-none rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
              />
            </label>
            {preview && (
              <div>
                <span className="mb-1.5 block text-xs font-medium text-stone-500">
                  共 {batchCount} 个词
                  {preview.duplicates > 0 ? `，输入里重复 ${preview.duplicates} 个已去重` : ""}
                  {preview.dropped > 0 ? `，无效 ${preview.dropped} 条已过滤` : ""}
                </span>
                <div className="max-h-32 overflow-y-auto rounded-md border border-stone-100 bg-stone-50/60 px-3 py-2">
                  <div className="flex flex-wrap gap-1.5">
                    {preview.words.map((w, i) => (
                      <span
                        key={i}
                        className="rounded-md border border-stone-200 bg-white px-2 py-0.5 text-sm text-stone-700"
                      >
                        {w}
                      </span>
                    ))}
                  </div>
                </div>
                <p className="mt-1.5 text-xs text-stone-400">
                  已在错题本里的词加入时会自动跳过；需要标「不会写的字」的词，加入后点「编辑」补
                </p>
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
