import { useCallback, useEffect, useState } from "react";
import ConfirmModal from "../../components/ConfirmModal";
import { Toast, useNotice } from "../../components/Toast";
import { api, errorMessage, formatDuration, formatTime } from "../../lib/ipc";
import type { MathBest, MathSummary } from "../../types";

interface MathHistoryProps {
  /** 点开一条：已交卷的看成绩单，没交卷的接着做 */
  onOpen: (sessionId: number) => void;
  onStart: () => void;
}

export default function MathHistory({ onOpen, onStart }: MathHistoryProps) {
  const [rows, setRows] = useState<MathSummary[] | null>(null);
  const [bests, setBests] = useState<MathBest[]>([]);
  const [pendingDelete, setPendingDelete] = useState<MathSummary | null>(null);
  const { notice, notify } = useNotice();

  const load = useCallback(async () => {
    try {
      const [list, best] = await Promise.all([api.listMathSessions(50), api.mathBestTimes()]);
      setRows(list);
      setBests(best);
    } catch (e) {
      setRows([]);
      notify("error", errorMessage(e));
    }
  }, [notify]);

  useEffect(() => {
    void load();
  }, [load]);

  const remove = async (target: MathSummary) => {
    try {
      await api.deleteMathSession(target.id);
      setPendingDelete(null);
      await load();
      notify("ok", "已删除这条记录");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  if (!rows) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start justify-between border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">口算记录</h1>
          <p className="mt-1 text-sm text-stone-500">
            最近 50 次，点开可以看那次做错的题和正确答案
          </p>
        </div>
        <button
          onClick={onStart}
          className="rounded-md bg-stone-800 px-4 py-2 text-xs font-medium text-white hover:bg-stone-900"
        >
          新做一张
        </button>
      </header>

      {/* 全对最快按题量分组：10 道和 30 道的用时不可比，不能混在一起 */}
      <div className="flex flex-wrap items-center gap-2 border-b border-stone-100 bg-amber-50/40 px-8 py-3">
        <span className="text-xs font-medium text-stone-600">全对最快</span>
        {bests.length === 0 ? (
          <span className="text-xs text-stone-400">还没有全对过，全对一次就会记下来</span>
        ) : (
          bests.map((b) => (
            <span
              key={b.total}
              className="rounded-md border border-amber-200 bg-white px-2 py-1 text-xs text-amber-800"
            >
              {b.total} 道 <b className="tabular-nums">{formatDuration(b.bestMs)}</b>
              <span className="ml-1.5 text-[11px] text-amber-600">{formatTime(b.achievedAt)}</span>
            </span>
          ))
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {rows.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-sm text-stone-400">
            <p>还没有做过口算</p>
            <button
              onClick={onStart}
              className="rounded-md border border-stone-200 px-4 py-2 text-xs text-stone-600 hover:bg-stone-50"
            >
              去出一张
            </button>
          </div>
        ) : (
          <table className="w-full border-collapse text-sm">
            <thead className="sticky top-0 z-10 bg-stone-50/95 text-xs text-stone-500 backdrop-blur">
              <tr>
                <th className="w-32 py-2 pl-8 text-left font-normal">时间</th>
                <th className="w-20 py-2 text-left font-normal">题量</th>
                <th className="w-28 py-2 text-left font-normal">做对</th>
                <th className="w-24 py-2 text-left font-normal">用时</th>
                <th className="py-2 text-left font-normal">状态</th>
                <th className="w-28 py-2 pr-8 text-right font-normal">操作</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const finished = r.status === "finished";
                const perfect = finished && r.correctTotal === r.total;
                return (
                  <tr
                    key={r.id}
                    onClick={() => onOpen(r.id)}
                    className="cursor-pointer border-t border-stone-100 hover:bg-stone-50/70"
                  >
                    <td className="py-2.5 pl-8 text-xs text-stone-500">
                      {formatTime(r.finishedAt ?? r.startedAt)}
                    </td>
                    <td className="py-2.5 text-xs text-stone-500">{r.total} 道</td>
                    <td className="py-2.5 text-xs">
                      {finished ? (
                        <span className={perfect ? "text-emerald-600" : "text-rose-600"}>
                          {r.correctTotal} / {r.total}
                        </span>
                      ) : (
                        <span className="text-stone-400">—</span>
                      )}
                    </td>
                    <td className="py-2.5 text-xs tabular-nums text-stone-600">
                      {finished && r.durationMs != null ? formatDuration(r.durationMs) : "—"}
                    </td>
                    <td className="py-2.5 text-xs">
                      {finished ? (
                        <span className="flex items-center gap-1.5">
                          <span className={perfect ? "text-emerald-600" : "text-stone-500"}>
                            {perfect ? "全对" : "已交卷"}
                          </span>
                          {r.isBest && (
                            <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[11px] text-amber-800">
                              最快
                            </span>
                          )}
                        </span>
                      ) : (
                        <span className="text-amber-700">未交卷（已答 {r.answered} 道）</span>
                      )}
                    </td>
                    <td className="whitespace-nowrap py-2.5 pr-8 text-right">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onOpen(r.id);
                        }}
                        className="rounded px-2 py-1 text-xs text-stone-500 hover:bg-stone-100"
                      >
                        {finished ? "看错题" : "继续做"}
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setPendingDelete(r);
                        }}
                        className="ml-1 rounded px-2 py-1 text-xs text-stone-400 hover:bg-rose-50 hover:text-rose-600"
                      >
                        删除
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {pendingDelete && (
        <ConfirmModal
          title="删除这条记录"
          body={`删除后这次口算的题目、答案和用时都没了，无法恢复。${
            pendingDelete.isBest ? "它还是当前的最快纪录，删掉后纪录会换成次快的一次。" : ""
          }`}
          confirmText="删除"
          cancelText="保留"
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => void remove(pendingDelete)}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}
