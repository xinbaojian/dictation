import { useEffect, useState } from "react";
import Modal from "./Modal";
import { api } from "../lib/ipc";
import type { AddMistakeItem, Group, SplitResult } from "../types";

interface AddMistakeModalProps {
  groups: Group[];
  defaultGroupId: number | null;
  /** 单个模式提交；成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (groupId: number, text: string, chars: number[]) => Promise<void>;
  /** 批量模式提交（items 已切分去重，可带每词错字下标） */
  onSubmitBatch: (groupId: number, items: AddMistakeItem[]) => Promise<void>;
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
  /** 批量模式就地点字：词文本 → 标错的字下标集合（提交时按现存词惰性取值） */
  const [marked, setMarked] = useState<Map<string, Set<number>>>(new Map());
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

  const toggleMark = (word: string, index: number) => {
    setMarked((prev) => {
      const next = new Map(prev);
      const set = new Set(next.get(word) ?? []);
      if (set.has(index)) set.delete(index);
      else set.add(index);
      next.set(word, set);
      return next;
    });
  };

  const submitBatch = async () => {
    const words = preview?.words ?? [];
    if (words.length === 0 || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmitBatch(
        groupId,
        words.map((w) => ({
          text: w,
          wrongChars: [...(marked.get(w) ?? [])].sort((a, b) => a - b),
        })),
      );
    } finally {
      setBusy(false);
    }
  };

  const batchCount = preview?.words.length ?? 0;

  return (
    <Modal
      title="加入错题本"
      subtitle={
        mode === "batch"
          ? "批量模式粘贴整段词语，按空格、逗号等分隔"
          : "词语会标成错题；可以点字记下「不会写」的字"
      }
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
              aria-pressed={mode === m.key}
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
            <div>
              <span className="mb-1.5 block text-xs font-medium text-stone-500">
                不会写的字（可不点）
              </span>
              {glyphs.length > 0 ? (
                <div className="flex min-h-[1.875rem] flex-wrap gap-1.5">
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
              ) : (
                // 占位与一行 chips 等高：输入第一个字时区块不跳高度
                <p className="min-h-[1.875rem] py-1 text-xs leading-5 text-stone-300">
                  输入词语后，在这里点选不会写的字
                </p>
              )}
            </div>
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
            <div>
              <span className="mb-1.5 block text-xs font-medium text-stone-500">
                {preview
                  ? `共 ${batchCount} 个词` +
                    (preview.duplicates > 0 ? `，输入里重复 ${preview.duplicates} 个已去重` : "") +
                    (preview.dropped > 0 ? `，无效 ${preview.dropped} 条已过滤` : "")
                  : "切分预览"}
              </span>
              {/* 容器常驻 + 固定最小高度：预览出现/消失时弹窗高度不跳 */}
              <div className="max-h-32 min-h-[3rem] overflow-y-auto rounded-md border border-stone-100 bg-stone-50/60 px-3 py-2">
                {preview && preview.words.length > 0 ? (
                  <div className="flex flex-wrap items-center gap-2">
                    {preview.words.map((w) => {
                      const marks = marked.get(w) ?? new Set<number>();
                      return (
                        <span
                          key={w}
                          className="flex items-center gap-0.5 rounded-md border border-stone-200 bg-white py-0.5 pl-1.5 pr-1"
                        >
                          {[...w].map((ch, i) => (
                            <button
                              key={i}
                              onClick={() => toggleMark(w, i)}
                              title="标记不会写的字"
                              className={
                                "rounded px-1 py-0.5 text-sm transition " +
                                (marks.has(i)
                                  ? "bg-rose-100 font-medium text-rose-800"
                                  : "text-stone-700 hover:bg-stone-100")
                              }
                            >
                              {ch}
                            </button>
                          ))}
                        </span>
                      );
                    })}
                  </div>
                ) : (
                  <p className="py-1.5 text-xs leading-5 text-stone-300">
                    {preview ? "没有切分出可加入的词" : "粘贴或输入后，这里显示切分出的词语"}
                  </p>
                )}
              </div>
              <p className="mt-1.5 text-xs text-stone-400">
                点词里的字可标记「不会写的字」；已在错题本里的词加入时会自动跳过，标的字仍会记录
              </p>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}
