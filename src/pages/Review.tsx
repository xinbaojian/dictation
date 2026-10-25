import { useEffect, useState } from "react";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import type { Session } from "../types";

interface ReviewProps {
  sessionId: number;
  onDone: () => void;
  onBack: () => void;
}

export default function Review({ sessionId, onDone, onBack }: ReviewProps) {
  const [session, setSession] = useState<Session | null>(null);
  const [wrong, setWrong] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        const detail = await api.getSession(sessionId);
        setSession(detail);
        // 默写中点了「不会」的词，默认标为错
        setWrong(new Set(detail.items.filter((i) => i.skipMarked).map((i) => i.id)));
      } catch (e) {
        notify("error", errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  const toggle = (id: number) => {
    setWrong((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const submit = async () => {
    if (!session || busy) return;
    setBusy(true);
    try {
      const r = await api.submitReview(session.id, [...wrong]);
      if (r.wrong === 0) {
        notify("ok", "全对！" + (r.graduated > 0 ? r.graduated + " 个词已移出错题本" : ""));
      } else {
        notify(
          "ok",
          "已记录 " + r.wrong + " 个错词" + (r.graduated > 0 ? "，" + r.graduated + " 个词已移出错题本" : ""),
        );
      }
      setResult("本次默写完成");
      setTimeout(onDone, 900);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  };

  if (!session) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  const done = session.status === "finished";
  const total = session.items.length;
  const correct = total - wrong.size;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="border-b border-stone-200 px-8 py-5">
        <h1 className="text-lg font-semibold text-stone-800">
          {done ? "本次批改结果" : "批改：点出错词"}
        </h1>
        <p className="mt-1 text-sm text-stone-500">
          {session.groupLabel} · 共 {total} 词
          {!done && " · 默认全部正确，点击词语标为「错」"}
        </p>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
          {session.items.map((item) => {
            const isWrong = done ? item.result === "wrong" : wrong.has(item.id);
            return (
              <button
                key={item.id}
                disabled={done}
                onClick={() => toggle(item.id)}
                className={
                  "flex items-center gap-2 rounded-lg border px-3 py-3 text-left transition " +
                  (isWrong
                    ? "border-rose-300 bg-rose-50"
                    : "border-stone-200 bg-white hover:border-stone-300")
                }
              >
                <span className="w-6 shrink-0 text-xs tabular-nums text-stone-400">{item.seq}</span>
                <span
                  className={
                    "min-w-0 flex-1 truncate text-[15px] " + (isWrong ? "text-rose-700" : "text-stone-800")
                  }
                >
                  {item.text}
                </span>
                {isWrong && <span className="shrink-0 text-xs font-medium text-rose-500">错</span>}
              </button>
            );
          })}
        </div>
      </div>

      <footer className="flex items-center justify-between border-t border-stone-200 px-8 py-4">
        <p className="text-sm text-stone-600">
          错 <span className="font-semibold text-rose-600">{wrong.size}</span> 个 · 对{" "}
          <span className="font-semibold text-emerald-600">{correct}</span> 个
        </p>
        <div className="flex gap-2">
          {done ? (
            <button
              onClick={onBack}
              className="rounded-md bg-stone-800 px-5 py-2 text-sm font-medium text-white hover:bg-stone-900"
            >
              返回
            </button>
          ) : (
            <>
              <button
                onClick={onBack}
                className="rounded-md px-4 py-2 text-sm text-stone-500 hover:bg-stone-100"
              >
                稍后再批
              </button>
              <button
                onClick={submit}
                disabled={busy}
                className="rounded-md bg-stone-800 px-6 py-2 text-sm font-medium text-white hover:bg-stone-900 disabled:opacity-50"
              >
                {wrong.size === 0 ? "全对，完成" : "完成批改"}
              </button>
            </>
          )}
        </div>
      </footer>

      {result && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-stone-900/20">
          <div className="rounded-xl bg-white px-8 py-6 text-center shadow-xl">
            <p className="text-base font-medium text-stone-800">{result}</p>
            <p className="mt-1 text-xs text-stone-500">错词已进入错题本，连续答对够了自动移出</p>
          </div>
        </div>
      )}

      <Toast notice={notice} />
    </div>
  );
}
