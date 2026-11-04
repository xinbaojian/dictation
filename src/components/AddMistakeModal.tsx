import { useState } from "react";
import Modal from "./Modal";
import type { Group } from "../types";

interface AddMistakeModalProps {
  groups: Group[];
  defaultGroupId: number | null;
  /** 成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (groupId: number, text: string, chars: number[]) => Promise<void>;
  onClose: () => void;
}

export default function AddMistakeModal({ groups, defaultGroupId, onSubmit, onClose }: AddMistakeModalProps) {
  const [groupId, setGroupId] = useState<number>(defaultGroupId ?? groups[0]?.id ?? 0);
  const [text, setText] = useState("");
  const [chars, setChars] = useState<Set<number>>(new Set());
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
    if (!trimmed || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmit(groupId, trimmed, [...chars].sort((a, b) => a - b));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title="加入错题本"
      subtitle="词语会标成错题；可以点字记下「不会写」的字"
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
            disabled={!trimmed || !groupId || busy}
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            加入
          </button>
        </>
      }
    >
      <div className="space-y-4">
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
            placeholder="如：春天"
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