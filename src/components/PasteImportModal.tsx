import { useEffect, useState } from "react";
import { api, errorMessage } from "../lib/ipc";
import type { SplitResult } from "../types";
import Modal from "./Modal";

interface PasteImportModalProps {
  groupId: number;
  groupName: string;
  onClose: () => void;
  onImported: (inserted: number, skipped: number) => void;
}

const EMPTY: SplitResult = { words: [], duplicates: 0, dropped: 0 };

export default function PasteImportModal({
  groupId,
  groupName,
  onClose,
  onImported,
}: PasteImportModalProps) {
  const [raw, setRaw] = useState("");
  const [preview, setPreview] = useState<SplitResult>(EMPTY);
  const [words, setWords] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 解析始终走后端，保证和导入时的切分规则完全一致
  useEffect(() => {
    if (!raw.trim()) {
      setPreview(EMPTY);
      setWords([]);
      return;
    }
    let alive = true;
    const timer = setTimeout(() => {
      api
        .previewWords(raw)
        .then((r) => {
          if (!alive) return;
          setPreview(r);
          setWords(r.words);
          setError(null);
        })
        .catch((e) => {
          if (alive) setError(errorMessage(e));
        });
    }, 180);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [raw]);

  const importNow = async () => {
    if (busy || words.length === 0) return;
    setBusy(true);
    try {
      const r = await api.importWords(groupId, words);
      onImported(r.inserted, r.skipped);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal
      title="批量粘贴导入"
      subtitle={"导入到「" + groupName + "」，支持换行、逗号、顿号、空格分隔"}
      width="max-w-2xl"
      onClose={onClose}
      footer={
        <>
          <span className="mr-auto text-xs text-stone-500">
            解析 {preview.words.length} 个词
            {preview.duplicates > 0 && "，已去重 " + preview.duplicates + " 个"}
            {preview.dropped > 0 && "，忽略 " + preview.dropped + " 个无效项"}
          </span>
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            取消
          </button>
          <button
            onClick={importNow}
            disabled={words.length === 0 || busy}
            className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            导入 {words.length} 个词
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <textarea
          value={raw}
          onChange={(e) => setRaw(e.target.value)}
          autoFocus
          rows={8}
          placeholder={
            "把课本词表整段粘进来，例如：\n春天 夏天 秋天 冬天\n柳树，桃花、燕子"
          }
          className="w-full resize-none rounded-md border border-stone-200 px-3 py-2 text-sm leading-relaxed outline-none focus:border-stone-400"
        />
        {error && <p className="text-xs text-rose-600">{error}</p>}
        {words.length > 0 && (
          <div className="rounded-md border border-stone-100 bg-stone-50 p-2">
            <div className="flex flex-wrap gap-1.5">
              {words.map((word, index) => (
                <span
                  key={word + "-" + index}
                  className="group inline-flex items-center gap-1 rounded bg-white px-2 py-1 text-sm text-stone-700 ring-1 ring-stone-200"
                >
                  {word}
                  <button
                    onClick={() => setWords((prev) => prev.filter((_, i) => i !== index))}
                    title="移除这个词"
                    className="text-stone-300 hover:text-rose-500"
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
