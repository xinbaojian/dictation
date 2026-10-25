import { useEffect, useState } from "react";
import Modal from "../components/Modal";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage, formatTime } from "../lib/ipc";
import type { Session, SessionSummary } from "../types";

interface HistoryProps {
  onStart: (sessionId: number) => void;
  onReview: (sessionId: number) => void;
}

const STATUS_TEXT: Record<string, string> = {
  running: "未完成",
  reviewing: "待批改",
  finished: "已批改",
  abandoned: "已放弃",
};

export default function History({ onStart, onReview }: HistoryProps) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [detail, setDetail] = useState<Session | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        setSessions(await api.listSessions(50));
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        setLoading(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const open = async (id: number) => {
    try {
      setDetail(await api.getSession(id));
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const retry = async () => {
    if (!detail || busy) return;
    setBusy(true);
    try {
      const session = await api.retrySession(detail.id);
      setDetail(null);
      onStart(session.id);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="border-b border-stone-200 px-8 py-5">
        <h1 className="text-lg font-semibold text-stone-800">默写记录</h1>
        <p className="mt-1 text-sm text-stone-500">最近 50 次，点开可以看抽取顺序和错词</p>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-stone-400">
            还没有默写记录
          </div>
        ) : (
          <table className="w-full border-collapse text-sm">
            <thead className="sticky top-0 z-10 bg-stone-50/95 text-xs text-stone-500 backdrop-blur">
              <tr>
                <th className="w-32 py-2 pl-8 text-left font-normal">时间</th>
                <th className="py-2 text-left font-normal">范围</th>
                <th className="w-24 py-2 text-left font-normal">题量</th>
                <th className="w-24 py-2 text-left font-normal">错词</th>
                <th className="w-24 py-2 text-left font-normal">状态</th>
                <th className="w-32 py-2 pr-8 text-right font-normal">操作</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((s) => (
                <tr
                  key={s.id}
                  onClick={() => void open(s.id)}
                  className="cursor-pointer border-t border-stone-100 hover:bg-stone-50/70"
                >
                  <td className="py-2.5 pl-8 text-xs text-stone-500">{formatTime(s.startedAt)}</td>
                  <td className="py-2.5">
                    <span className="text-sm text-stone-700">{s.groupLabel}</span>
                    {s.mode === "mistake" && (
                      <span className="ml-2 rounded bg-rose-50 px-1.5 py-0.5 text-[11px] text-rose-600">
                        错题
                      </span>
                    )}
                  </td>
                  <td className="py-2.5 text-xs text-stone-500">{s.total} 词</td>
                  <td className="py-2.5 text-xs">
                    {s.status === "finished" ? (
                      <span className={s.wrongTotal > 0 ? "text-rose-600" : "text-emerald-600"}>
                        {s.wrongTotal > 0 ? s.wrongTotal + " 个" : "全对"}
                      </span>
                    ) : (
                      <span className="text-stone-400">—</span>
                    )}
                  </td>
                  <td className="py-2.5 text-xs text-stone-500">{STATUS_TEXT[s.status] ?? s.status}</td>
                  <td className="py-2.5 pr-8 text-right">
                    {s.status === "reviewing" && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onReview(s.id);
                        }}
                        className="rounded px-2 py-1 text-xs text-amber-700 hover:bg-amber-50"
                      >
                        去批改
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {detail && (
        <Modal
          title={"默写详情 · " + detail.groupLabel}
          subtitle={formatTime(detail.startedAt) + " · 共 " + detail.total + " 词"}
          width="max-w-2xl"
          onClose={() => setDetail(null)}
          footer={
            <>
              <button
                onClick={() => setDetail(null)}
                className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
              >
                关闭
              </button>
              {detail.items.some((i) => i.result === "wrong") && (
                <button
                  onClick={retry}
                  disabled={busy}
                  className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
                >
                  只练本次错词
                </button>
              )}
            </>
          }
        >
          <div className="space-y-1.5">
            {detail.items.map((item) => {
              const wrong = item.result === "wrong";
              return (
                <div
                  key={item.id}
                  className={
                    "flex items-center gap-3 rounded-md border px-3 py-2 " +
                    (wrong ? "border-rose-200 bg-rose-50" : "border-stone-100 bg-white")
                  }
                >
                  <span className="w-6 text-xs tabular-nums text-stone-400">{item.seq}</span>
                  <span className="flex-1 text-sm text-stone-800">{item.text}</span>
                  <span
                    className={
                      "text-xs " + (wrong ? "font-medium text-rose-500" : "text-emerald-600")
                    }
                  >
                    {item.result === "pending" ? "未批改" : wrong ? "错" : "对"}
                  </span>
                </div>
              );
            })}
          </div>
        </Modal>
      )}

      <Toast notice={notice} />
    </div>
  );
}
