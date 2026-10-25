import { useCallback, useEffect, useRef, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import type { Session, Settings } from "../types";

interface QuizProps {
  sessionId: number;
  onFinish: (sessionId: number) => void;
  onExit: () => void;
}

export default function Quiz({ sessionId, onFinish, onExit }: QuizProps) {
  const [session, setSession] = useState<Session | null>(null);
  const [index, setIndex] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [askExit, setAskExit] = useState(false);
  // 防止连点「下一个」导致跳过词条（state 更新是异步的，用 ref 做同步锁）
  const advancing = useRef(false);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        const [detail, cfg] = await Promise.all([api.getSession(sessionId), api.getSettings()]);
        if (detail.status === "reviewing" || detail.status === "finished") {
          onFinish(sessionId);
          return;
        }
        setSession(detail);
        setSettings(cfg);
        setIndex(Math.min(detail.currentSeq, Math.max(detail.total - 1, 0)));
      } catch (e) {
        notify("error", errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  const items = session?.items ?? [];
  const current = items[index];

  // 翻到新词先自动显示，家长可以直接读；设置了时长就到点自动遮住
  useEffect(() => {
    if (session) setRevealed(true);
  }, [index, session]);

  // 每个词显示后按设置的秒数自动遮罩；手动点「显示」也会重新计时
  useEffect(() => {
    if (!revealed || !settings || settings.maskSeconds <= 0) return;
    const timer = setTimeout(() => setRevealed(false), settings.maskSeconds * 1000);
    return () => clearTimeout(timer);
  }, [revealed, settings, index]);

  const finish = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    try {
      await api.finishSession(sessionId);
      onFinish(sessionId);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  }, [busy, sessionId, onFinish, notify]);

  const advance = useCallback(
    async (skip: boolean) => {
      if (!session || !current || busy || advancing.current) return;
      advancing.current = true;
      const next = index + 1;
      try {
        await api.updateSessionProgress(session.id, next, skip ? current.id : null);
        setRevealed(false);
        if (next >= items.length) {
          await finish();
        } else {
          setIndex(next);
        }
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        advancing.current = false;
      }
    },
    [session, current, index, items.length, busy, finish, notify],
  );

  const back = useCallback(() => {
    setRevealed(false);
    setIndex((i) => Math.max(0, i - 1));
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        if (revealed) void advance(false);
        else setRevealed(true);
      } else if (e.key === "Enter") {
        e.preventDefault();
        void advance(true);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        back();
      } else if (e.key === "Escape") {
        setAskExit(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [revealed, advance, back]);

  if (!session) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">准备中…</div>;
  }

  const progress = session.total > 0 ? ((index + 1) / session.total) * 100 : 0;
  const lastOne = index === items.length - 1;

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-stone-50">
      <header className="flex items-center justify-between px-8 py-4">
        <div className="flex items-baseline gap-3">
          <span className="text-sm font-medium text-stone-700">
            第 {index + 1} / {session.total} 题
          </span>
          <span className="text-xs text-stone-400">{session.groupLabel}</span>
        </div>
        <button
          onClick={() => setAskExit(true)}
          className="rounded-md px-3 py-1.5 text-xs text-stone-500 hover:bg-stone-200/60"
        >
          退出（保留进度）
        </button>
      </header>

      <div className="px-8">
        <div className="h-1 overflow-hidden rounded-full bg-stone-200">
          <div className="h-full rounded-full bg-amber-500 transition-all" style={{ width: progress + "%" }} />
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col items-center justify-center px-8">
        <button
          onClick={() => setRevealed((r) => !r)}
          className="flex h-64 w-full max-w-2xl select-none items-center justify-center rounded-2xl border border-stone-200 bg-white shadow-sm transition hover:border-stone-300"
        >
          {revealed ? (
            <span
              className="font-semibold tracking-wide text-stone-900"
              style={{ fontSize: (settings?.wordFontSize ?? 96) + "px", lineHeight: 1.2 }}
            >
              {current?.text}
            </span>
          ) : (
            <span className="flex flex-col items-center gap-2 text-stone-400">
              <span className="text-5xl font-light tabular-nums">{index + 1}</span>
              <span className="text-xs">点击显示词语 · 空格</span>
            </span>
          )}
        </button>
        <p className="mt-4 text-xs text-stone-400">
          {revealed && settings && settings.maskSeconds > 0
            ? settings.maskSeconds + " 秒后自动遮住 · 空格=下一个 · Enter=不会"
            : "已遮住 · 点击词语或空格重新显示 · Enter=不会 · ←=上一个"}
        </p>
      </div>

      <footer className="flex items-center justify-center gap-3 px-8 pb-8">
        <button
          onClick={back}
          disabled={index === 0}
          className="rounded-md border border-stone-200 bg-white px-4 py-2 text-sm text-stone-600 hover:bg-stone-50 disabled:opacity-40"
        >
          上一个
        </button>
        <button
          onClick={() => setRevealed((r) => !r)}
          className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm font-medium text-stone-700 hover:bg-stone-50"
        >
          {revealed ? "隐藏" : "显示"}
        </button>
        <button
          onClick={() => void advance(true)}
          className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-600 hover:bg-stone-50"
        >
          不会
        </button>
        <button
          onClick={() => void advance(false)}
          className="rounded-md bg-stone-800 px-7 py-2 text-sm font-medium text-white hover:bg-stone-900"
        >
          {lastOne ? "完成并批改" : "下一个"}
        </button>
      </footer>

      {askExit && (
        <ConfirmModal
          title="退出默写"
          body="进度会保留，下次可以继续这次默写。也可以直接放弃这次默写。"
          confirmText="退出并保留"
          cancelText="继续默写"
          extraText="放弃这次"
          onExtra={async () => {
            try {
              await api.abandonSession(session.id);
              onExit();
            } catch (e) {
              notify("error", errorMessage(e));
            }
          }}
          onCancel={() => setAskExit(false)}
          onConfirm={() => {
            setAskExit(false);
            onExit();
          }}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}
