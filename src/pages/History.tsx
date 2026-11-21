import { useEffect, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import Modal from "../components/Modal";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage, formatTime } from "../lib/ipc";
import type { Session, SessionItem, SessionSummary } from "../types";

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
  const [pendingDelete, setPendingDelete] = useState<SessionSummary | null>(null);
  const [busy, setBusy] = useState(false);
  /** 正在保存修改的条目 id：一次只发一条，避免连点把对/错来回翻 */
  const [revisingId, setRevisingId] = useState<number | null>(null);
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

  /** 修改一条批改结果（家长手误补救）：对↔错互换、错词换错字，进度由后端差量补偿 */
  const revise = async (item: SessionItem, toWrong: boolean, wrongChars: number[]) => {
    if (!detail || revisingId !== null) return;
    setRevisingId(item.id);
    try {
      const r = await api.reviseReviewItem(item.id, toWrong, wrongChars);
      setDetail(r.session);
      setSessions(await api.listSessions(50)); // 列表里的「错词 N 个」要跟着变
      notify(
        "ok",
        !toWrong
          ? r.graduated
            ? "已改回对，连对达标、移出了错题本"
            : "已改回对"
          : item.result === "wrong"
            ? "已更新不会写的字" // 原本就是错词，只换了标错的字，不重复计错
            : "已记为错，进错题本",
      );
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setRevisingId(null);
    }
  };

  const remove = async (target: SessionSummary) => {
    try {
      await api.deleteSession(target.id);
      if (detail?.id === target.id) setDetail(null);
      setSessions(await api.listSessions(50));
      notify("ok", "已删除这条记录");
    } catch (e) {
      notify("error", errorMessage(e));
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
                <th className="w-36 py-2 pr-8 text-right font-normal">操作</th>
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
                  <td className="whitespace-nowrap py-2.5 pr-8 text-right">
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
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setPendingDelete(s);
                      }}
                      className="ml-1 rounded px-2 py-1 text-xs text-stone-400 hover:bg-rose-50 hover:text-rose-600"
                    >
                      删除
                    </button>
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
          subtitle={
            formatTime(detail.startedAt) +
            " · 共 " +
            detail.total +
            " 词" +
            (detail.status === "finished" ? " · 错 " + detail.wrongTotal : "")
          }
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
          {detail.status === "finished" && (
            <p className="mb-2 text-xs text-stone-400">
              批改点错了？点右侧「对 / 错」修改这一条，错题本和练习进度会一起更正。
            </p>
          )}
          <div className="space-y-1.5">
            {detail.items.map((item) => {
              const wrong = item.result === "wrong";
              const editable = detail.status === "finished" && revisingId === null;
              const marks = new Set(item.wrongChars);
              const toggleChar = (i: number) => {
                const next = new Set(marks);
                if (next.has(i)) next.delete(i);
                else next.add(i);
                return [...next].sort((a, b) => a - b);
              };
              return (
                <div
                  key={item.id}
                  className={
                    "rounded-md border px-3 py-2 " +
                    (wrong ? "border-rose-200 bg-rose-50" : "border-stone-100 bg-white")
                  }
                >
                  <div className="flex items-center gap-3">
                    <span className="w-6 text-xs tabular-nums text-stone-400">{item.seq}</span>
                    <span className="flex-1 text-sm text-stone-800">
                      {[...item.text].map((ch, i) => (
                        <span
                          key={i}
                          className={
                            item.wrongChars.includes(i)
                              ? "rounded bg-rose-200/70 px-0.5 text-rose-800"
                              : ""
                          }
                        >
                          {ch}
                        </span>
                      ))}
                    </span>
                    {detail.status === "finished" ? (
                      <button
                        onClick={() => void revise(item, !wrong, wrong ? item.wrongChars : [])}
                        disabled={!editable}
                        title={wrong ? "其实写对了？点这里改回「对」" : "其实写错了？点这里改记为「错」"}
                        className={
                          "shrink-0 rounded border px-2 py-0.5 text-xs font-medium transition " +
                          (wrong
                            ? "border-rose-300 bg-rose-100 text-rose-700 hover:bg-rose-200"
                            : "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-100") +
                          " disabled:opacity-50"
                        }
                      >
                        {wrong ? "错" : "对"}
                      </button>
                    ) : (
                      <span
                        className={
                          "shrink-0 text-xs " + (wrong ? "font-medium text-rose-500" : "text-emerald-600")
                        }
                      >
                        {item.result === "pending" ? "未批改" : wrong ? "错" : "对"}
                      </span>
                    )}
                  </div>
                  {detail.status === "finished" && wrong && (
                    <div className="mt-1.5 flex flex-wrap items-center gap-1 pl-9">
                      {[...item.text].map((ch, i) => (
                        <button
                          key={i}
                          onClick={() => void revise(item, true, toggleChar(i))}
                          disabled={!editable}
                          title="标为不会写的字"
                          className={
                            "h-6 w-6 rounded border text-xs transition " +
                            (marks.has(i)
                              ? "border-rose-400 bg-rose-200 text-rose-800"
                              : "border-stone-200 bg-white text-stone-400 hover:border-stone-300") +
                            " disabled:opacity-50"
                          }
                        >
                          {ch}
                        </button>
                      ))}
                      <span className="ml-1 text-[10px] leading-tight text-stone-400">
                        点字标错字，
                        <br />
                        不点=整词错
                      </span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </Modal>
      )}

      {pendingDelete && (
        <ConfirmModal
          title="删除默写记录"
          body={
            "将删除 " +
            formatTime(pendingDelete.startedAt) +
            " 的「" +
            pendingDelete.groupLabel +
            "」记录（" +
            pendingDelete.total +
            " 词）" +
            (pendingDelete.status === "reviewing" ? "，这次默写还没批改，删了就不能再批" : "") +
            "。删除后无法恢复，词条的错题状态和练习进度不受影响。"
          }
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => {
            const target = pendingDelete;
            setPendingDelete(null);
            void remove(target);
          }}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}
