import { useEffect, useState } from "react";

export type Notice = { kind: "ok" | "error"; text: string };

export function useNotice() {
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  return {
    notice,
    notify: (kind: Notice["kind"], text: string) => setNotice({ kind, text }),
    clear: () => setNotice(null),
  };
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
