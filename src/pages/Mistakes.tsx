import { useEffect, useState } from "react";
import AddMistakeModal from "../components/AddMistakeModal";
import ConfirmModal from "../components/ConfirmModal";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage, formatTime } from "../lib/ipc";
import type { Group, Mistake } from "../types";

interface MistakesProps {
  onStart: (sessionId: number) => void;
  onGoWordbook: () => void;
  onPrint: (groupId: number | null, title: string) => void;
  /** 进笔画复习页（按当前分组筛选）；带 wordId = 从错题本点了具体某个词，直接从它开始 */
  onStroke: (groupId: number | null, title: string, wordId?: number) => void;
}

/** 与批改页一致：标红的字 = 历次批改里标过「不会写」的字。
 *  点词语本身 = 直接复习这个词的笔画（不用从列表头开始翻）。 */
function MistakeWord({ word, onStroke }: { word: Mistake; onStroke: (wordId: number) => void }) {
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
      <button
        type="button"
        onClick={() => onStroke(word.id)}
        title="看这个词的笔顺动画"
        className="-mx-1 rounded px-1 text-left hover:bg-amber-50 focus-visible:bg-amber-50 focus-visible:outline-none"
      >
        <span className="text-[15px] text-stone-800 underline decoration-stone-300 underline-offset-4 hover:decoration-amber-400">
          {chars.map((ch, i) => (
            <span
              key={i}
              className={marked.includes(i) ? "rounded bg-rose-200/70 px-0.5 text-rose-800" : ""}
            >
              {ch}
            </span>
          ))}
        </span>
      </button>
      {label && <span className="mt-0.5 block text-[11px] text-rose-500">{label}</span>}
    </td>
  );
}

export default function Mistakes({ onStart, onGoWordbook, onPrint, onStroke }: MistakesProps) {
  const [mistakes, setMistakes] = useState<Mistake[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [filter, setFilter] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  // 开始默写前先确认：默写会整屏切走，误点一下孩子就被打断，还平白多出一场记录
  const [askPractice, setAskPractice] = useState(false);
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
        setGroups(gs);
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
      const session = await api.createSession(filter == null ? [] : [filter], "mistake", 0, true, {
        chain: false,
        repeatWrong: true,
        weakFirst: true,
        newFirst: true,
        lastBack: true,
        diverse: true,
      });
      onStart(session.id);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  };

  const addMistake = async (groupId: number, text: string, chars: number[]) => {
    try {
      await api.addMistake(groupId, text, chars);
      setAddOpen(false);
      await reload(filter);
      setGroups(await api.listGroups());
      notify("ok", "「" + text + "」已加入错题本");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const master = async (word: Mistake) => {
    try {
      await api.setWordMastered(word.id, true);
      await reload(filter);
      setGroups(await api.listGroups());
      notify("ok", "「" + word.text + "」已移出错题本");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  // 笔画复习页的副标题：跟当前筛选保持一致，免得孩子以为复习的是别的分组
  const scopeLabel =
    filter == null ? "全部错词" : (groups.find((g) => g.id === filter)?.path ?? "错题");

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start justify-between gap-4 border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">错题本</h1>
          <p className="mt-1 text-sm text-stone-500">
            共 {mistakes.length} 个词 · 点词语可直接复习笔画 · 连续答对 {streak} 次自动移出
          </p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={filter ?? ""}
            onChange={(e) => void changeFilter(e.target.value === "" ? null : Number(e.target.value))}
            className="rounded-md border border-stone-200 bg-white px-2.5 py-1.5 text-sm text-stone-600"
          >
            <option value="">全部分组</option>
            {groups.filter((g) => g.learningCount > 0).map((g) => (
              <option key={g.id} value={g.id}>
                {g.path}
              </option>
            ))}
          </select>
          <button
            onClick={() => setAddOpen(true)}
            disabled={groups.length === 0}
            title={groups.length === 0 ? "先去词语表建一个分组" : "手工把一个词加入错题本"}
            className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-40"
          >
            添加词语
          </button>
          <button
            onClick={() => onStroke(filter, scopeLabel)}
            disabled={mistakes.length === 0}
            title="逐词看笔顺动画，可以遮住让孩子先自己写"
            className="rounded-md border border-amber-200 bg-amber-50 px-3 py-1.5 text-sm text-amber-700 hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-40"
          >
            复习笔画
          </button>
          <button
            onClick={() => setAskPractice(true)}
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
            <p className="text-xs text-stone-400">默写后批改出错的词会自动收进来，也可以手动添加</p>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setAddOpen(true)}
                disabled={groups.length === 0}
                className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700 disabled:cursor-not-allowed disabled:bg-stone-300"
              >
                添加词语
              </button>
              <button
                onClick={onGoWordbook}
                className="rounded-md border border-stone-200 px-4 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
              >
                去词语表看看
              </button>
            </div>
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
                  <MistakeWord word={word} onStroke={(id) => onStroke(filter, scopeLabel, id)} />
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

      {askPractice && (
        <ConfirmModal
          title="开始错题专项默写？"
          body={
            "范围：" +
            scopeLabel +
            "，共 " +
            mistakes.length +
            " 个词。\n会把整屏切到默写页：家长读词、孩子在纸上写，写完进入批改。\n中途退出会保留进度，下次可以接着写。"
          }
          confirmText="开始默写"
          cancelText="再想想"
          onCancel={() => setAskPractice(false)}
          onConfirm={() => {
            setAskPractice(false);
            void practice();
          }}
        />
      )}

      {addOpen && (
        <AddMistakeModal
          groups={groups}
          defaultGroupId={filter}
          onSubmit={addMistake}
          onClose={() => setAddOpen(false)}
        />
      )}

      <Toast notice={notice} />
    </div>
  );
}
