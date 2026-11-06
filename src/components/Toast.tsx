import { useCallback, useEffect, useState } from "react";

export type Notice = { kind: "ok" | "error"; text: string };

export function useNotice() {
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  // notify 必须是稳定引用：调用方常把它放进 useCallback/effect 依赖，
  // 内联函数会让「notify → setNotice → 渲染 → 新 notify」形成重跑循环
  // （曾让朗读失败提示无限弹 toast，见 2026-11-07 审查 P1-1）
  const notify = useCallback((kind: Notice["kind"], text: string) => {
    setNotice({ kind, text });
  }, []);
  const clear = useCallback(() => setNotice(null), []);

  return { notice, notify, clear };
}

export function Toast({ notice }: { notice: Notice | null }) {
  if (!notice) return null;
  return (
    <div
      className={
        "fixed bottom-5 right-5 z-50 max-w-sm rounded-lg px-4 py-2.5 text-sm shadow-lg " +
        (notice.kind === "error" ? "bg-rose-600 text-white" : "bg-stone-800 text-white")
      }
    >
      {notice.text}
    </div>
  );
}
