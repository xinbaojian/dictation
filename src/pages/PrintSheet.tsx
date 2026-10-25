import { useEffect, useState } from "react";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";

interface PrintSheetProps {
  mode: "mistakes" | "group";
  groupId: number | null;
  title: string;
  onBack: () => void;
}

interface Row {
  id: number;
  text: string;
}

export default function PrintSheet({ mode, groupId, title, onBack }: PrintSheetProps) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [showWord, setShowWord] = useState(false);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        if (mode === "mistakes") {
          const list = await api.listMistakes(groupId);
          setRows(list.map((m) => ({ id: m.id, text: m.text })));
        } else {
          if (groupId == null) {
            setRows([]);
            return;
          }
          const list = await api.listWords(groupId, true);
          setRows(list.map((w) => ({ id: w.id, text: w.text })));
        }
      } catch (e) {
        notify("error", errorMessage(e));
        setRows([]);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, groupId]);

  const today = new Date().toLocaleDateString("zh-CN");
  // 田字格数量按字数给，最少 2 个，最多 4 个
  const cellCount = (text: string) => Math.min(Math.max(text.length, 2), 4);

  if (!rows) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="print-root min-h-0 flex-1 overflow-y-auto bg-stone-200/60 pb-10">
      <div className="no-print mx-auto flex w-[210mm] max-w-full items-center justify-between gap-3 px-1 py-4">
        <div className="flex items-center gap-2">
          <button
            onClick={onBack}
            className="rounded-md bg-white px-3 py-1.5 text-sm text-stone-600 shadow-sm hover:bg-stone-50"
          >
            返回
          </button>
          <span className="text-sm text-stone-500">共 {rows.length} 个词</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowWord((v) => !v)}
            className={
              "rounded-md px-3 py-1.5 text-sm shadow-sm " +
              (showWord ? "bg-stone-800 text-white" : "bg-white text-stone-600 hover:bg-stone-50")
            }
          >
            {showWord ? "抄写单（含词语）" : "默写单（不含词语）"}
          </button>
          <button
            onClick={() => window.print()}
            className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700"
          >
            打印
          </button>
        </div>
      </div>

      {rows.length === 0 ? (
        <div className="mx-auto flex w-[210mm] max-w-full flex-col items-center gap-2 rounded bg-white px-6 py-20 shadow-md">
          <p className="text-sm text-stone-500">这里还没有可打印的词</p>
          <p className="text-xs text-stone-400">先去默写一次并批改，错词会自动进错题本</p>
        </div>
      ) : (
        <div className="print-page mx-auto w-[210mm] bg-white px-[14mm] py-[12mm] shadow-md">
          <header className="mb-6 flex items-end justify-between border-b border-stone-300 pb-3">
            <div>
              <h1 className="text-[22px] font-semibold text-stone-900">{title}</h1>
              <p className="mt-1 text-[11px] text-stone-500">
                共 {rows.length} 个词 · {today}
              </p>
            </div>
            <p className="text-[11px] text-stone-500">姓名 __________　日期 __________</p>
          </header>

          <div className="space-y-2.5">
            {rows.map((row, index) => (
              <div key={row.id} className="print-row flex items-center gap-2">
                <span className="w-6 shrink-0 text-right text-[11px] tabular-nums text-stone-400">
                  {index + 1}
                </span>
                <span
                  className="shrink-0 text-[18px] leading-none text-stone-800"
                  style={{ width: "26mm" }}
                >
                  {showWord ? row.text : ""}
                </span>
                {Array.from({ length: cellCount(row.text) }).map((_, i) => (
                  <span key={i} className="tian" style={{ width: "14mm", height: "14mm" }} />
                ))}
              </div>
            ))}
          </div>

          <p className="mt-8 text-center text-[10px] text-stone-400">
            生词默写 · {showWord ? "先看后写，写完自己对照" : "家长读词，孩子在田字格中默写"}
          </p>
        </div>
      )}

      <Toast notice={notice} />
    </div>
  );
}
