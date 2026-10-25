import { useEffect, useState } from "react";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage, formatTime } from "../lib/ipc";
import type { Group, Mistake } from "../types";

interface MistakesProps {
  onStart: (sessionId: number) => void;
  onGoWordbook: () => void;
  onPrint: (groupId: number | null, title: string) => void;
}

/** 与批改页一致：标红的字 = 历次批改里标过「不会写」的字 */
function MistakeWord({ word }: { word: Mistake }) {
  const chars = [...word.text];
  const marked = word.wrongChars.filter((i) => i >= 0 && i < chars.length);
  const label =
    marked.length === 0
      ? word.wholeWordWrong > 0
        ? "整词错"
        : ""
      : marked.length === chars.length
        ? "整词错"
        : "错字：" + marked.map((i) => chars[i]).join("、");

  return (
    <td className="py-2.5 pl-8">
      <span className="text-[15px] text-stone-800">
        {chars.map((ch, i) => (
          <span
            key={i}
            className={marked.includes(i) ? "rounded bg-rose-200/70 px-0.5 text-rose-800" : ""}
          >
            {ch}
          </span>
        ))}
      </span>
      {label && <span className="mt-0.5 block text-[11px] text-rose-500">{label}</span>}
    </td>
  );
}

export default function Mistakes({ onStart, onGoWordbook, onPrint }: MistakesProps) {
  const [mistakes, setMistakes] = useState<Mistake[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [filter, setFilter] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [streak, setStreak] = useState(2);
  const { notice, notify } = useNotice();

  const reload = async (groupId: number | null) => {
    const list = await api.listMistakes(groupId);
    setMistakes(list);
  };

  useEffect(() => {
    (async () => {
      try {
        const [list, gs, cfg] = await Promise.all([
          api.listMistakes(null),
          api.listGroups(),
          api.getSettings(),
        ]);
        setMistakes(list);
        setGroups(gs.filter((g) => g.learningCount > 0));
        setStreak(cfg.streakToMaster);
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        setLoading(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const changeFilter = async (groupId: number | null) => {
    setFilter(groupId);
    try {
      await reload(groupId);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const practice = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const session = await api.createSession(filter == null ? [] : [filter], "mistake", 0, true);
      onStart(session.id);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  };

  const master = async (word: Mistake) => {
    try {
      await api.setWordMastered(word.id, true);
      await reload(filter);
      notify("ok", "「" + word.text + "」已移出错题本");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start justify-between gap-4 border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">错题本</h1>
          <p className="mt-1 text-sm text-stone-500">
            共 {mistakes.length} 个词 · 连续答对 {streak} 次自动移出
          </p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={filter ?? ""}
            onChange={(e) => void changeFilter(e.target.value === "" ? null : Number(e.target.value))}
            className="rounded-md border border-stone-200 bg-white px-2.5 py-1.5 text-sm text-stone-600"
          >
            <option value="">全部分组</option>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.path}
              </option>
            ))}
          </select>
          <button
            onClick={practice}
            disabled={mistakes.length === 0 || busy}
            className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700 disabled:cursor-not-allowed disabled:bg-stone-300"
          >
            错题专项默写
          </button>
          <button
            onClick={() =>
              onPrint(
                filter,
                filter == null
                  ? "错词复习单"
                  : (groups.find((g) => g.id === filter)?.path ?? "") + " · 错词复习单",
              )
            }
            disabled={mistakes.length === 0}
            className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-40"
          >
            打印复习单
          </button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {mistakes.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
            <p className="text-sm text-stone-500">错题本是空的</p>
            <p className="text-xs text-stone-400">默写后批改出错的词会自动收进来</p>
            <button
              onClick={onGoWordbook}
              className="rounded-md border border-stone-200 px-4 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
            >
              去词语表看看
            </button>
          </div>
        ) : (
          <table className="w-full border-collapse text-sm">
            <thead className="sticky top-0 z-10 bg-stone-50/95 text-xs text-stone-500 backdrop-blur">
              <tr>
                <th className="py-2 pl-8 text-left font-normal">词语</th>
                <th className="w-40 py-2 text-left font-normal">所属分组</th>
                <th className="w-24 py-2 text-left font-normal">错误次数</th>
                <th className="w-40 py-2 text-left font-normal">连对进度</th>
                <th className="w-36 py-2 text-left font-normal">最近出错</th>
                <th className="w-28 py-2 pr-8 text-right font-normal">操作</th>
              </tr>
            </thead>
            <tbody>
              {mistakes.map((word) => (
                <tr key={word.id} className="group border-t border-stone-100 hover:bg-stone-50/70">
                  <MistakeWord word={word} />
                  <td className="py-2.5 text-xs text-stone-500">
                    {word.groupName}
                  </td>
                  <td className="py-2.5 text-xs text-rose-600">{word.wrongCount} 次</td>
                  <td className="py-2.5">
                    <span className="flex items-center gap-1.5">
                      {Array.from({ length: streak }).map((_, i) => (
                        <span
                          key={i}
                          className={
                            "h-1.5 w-6 rounded-full " +
                            (i < word.correctStreak ? "bg-emerald-500" : "bg-stone-200")
                          }
                        />
                      ))}
                      <span className="ml-1 text-xs text-stone-400">
                        {word.correctStreak}/{streak}
                      </span>
                    </span>
                  </td>
                  <td className="py-2.5 text-xs text-stone-400">{formatTime(word.lastPracticedAt)}</td>
                  <td className="py-2.5 pr-8 text-right">
                    <button
                      onClick={() => master(word)}
                      className="rounded px-2 py-1 text-xs text-stone-400 hover:bg-emerald-50 hover:text-emerald-600"
                    >
                      已掌握
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <Toast notice={notice} />
    </div>
  );
}
