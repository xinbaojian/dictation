import { useCallback, useEffect, useMemo, useState } from "react";
import ConfirmModal from "../components/ConfirmModal";
import GroupModal from "../components/GroupModal";
import PasteImportModal from "../components/PasteImportModal";
import { api, errorMessage, practiceLabel } from "../lib/ipc";
import type { Group, Word, WordStatus } from "../types";

type Notice = { kind: "ok" | "error"; text: string };

const STATUS_STYLE: Record<WordStatus, { label: string; cls: string }> = {
  new: { label: "未练", cls: "bg-stone-100 text-stone-500" },
  learning: { label: "错题", cls: "bg-rose-50 text-rose-600" },
  mastered: { label: "已会", cls: "bg-emerald-50 text-emerald-600" },
};

interface WordbookProps {
  onPrint: (groupId: number, title: string) => void;
}

export default function Wordbook({ onPrint }: WordbookProps) {
  const [groups, setGroups] = useState<Group[]>([]);
  const [words, setWords] = useState<Word[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<Notice | null>(null);

  const [groupModal, setGroupModal] = useState<{ mode: "create" } | { mode: "edit"; group: Group } | null>(null);
  const [pasteOpen, setPasteOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [edit, setEdit] = useState<{ id: number; text: string; note: string } | null>(null);
  const [confirm, setConfirm] = useState<{ title: string; body: string; run: () => Promise<void> } | null>(null);

  const notify = (kind: Notice["kind"], text: string) => setNotice({ kind, text });

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  const refreshGroups = useCallback(async (preferId?: number | null) => {
    const list = await api.listGroups();
    setGroups(list);
    setSelectedId((prev) => {
      const want = preferId === undefined ? prev : preferId;
      if (want != null && list.some((g) => g.id === want)) return want;
      return list.length > 0 ? list[0].id : null;
    });
    return list;
  }, []);

  const reloadWords = useCallback(async (groupId: number) => {
    const list = await api.listWords(groupId);
    setWords(list);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        await refreshGroups();
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [refreshGroups]);

  useEffect(() => {
    if (selectedId == null) {
      setWords([]);
      return;
    }
    let alive = true;
    reloadWords(selectedId).catch((e) => {
      if (alive) notify("error", errorMessage(e));
    });
    return () => {
      alive = false;
    };
  }, [selectedId, reloadWords]);

  const current = useMemo(
    () => groups.find((g) => g.id === selectedId) ?? null,
    [groups, selectedId],
  );

  const stats = useMemo(() => {
    const learning = words.filter((w) => w.status === "learning").length;
    const newCount = words.filter((w) => w.status === "new").length;
    return { total: words.length, learning, newCount };
  }, [words]);

  const saveGroup = async (code: string, name: string) => {
    try {
      if (groupModal?.mode === "edit") {
        await api.updateGroup(groupModal.group.id, code, name);
        notify("ok", "分组已更新");
        await refreshGroups(groupModal.group.id);
      } else {
        const created = await api.createGroup(code, name);
        notify("ok", "已创建分组「" + created.name + "」");
        await refreshGroups(created.id);
      }
      setGroupModal(null);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const removeGroup = (group: Group) => {
    setConfirm({
      title: "删除分组",
      body:
        "将删除「" + group.code + " " + group.name + "」及其 " + group.wordCount +
        " 个词条" + (group.learningCount > 0 ? "（其中 " + group.learningCount + " 个在错题本中）" : "") +
        "，该操作不可撤销。",
      run: async () => {
        try {
          await api.deleteGroup(group.id);
          notify("ok", "已删除分组");
          await refreshGroups(null);
        } catch (e) {
          notify("error", errorMessage(e));
        }
      },
    });
  };

  const addWord = async () => {
    const text = draft.trim();
    if (!text || selectedId == null) return;
    try {
      await api.addWord(selectedId, text, null);
      setDraft("");
      await reloadWords(selectedId);
      await refreshGroups(selectedId);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const saveEdit = async () => {
    if (!edit) return;
    try {
      await api.updateWord(edit.id, edit.text, edit.note || null);
      setEdit(null);
      notify("ok", "已保存");
      if (selectedId != null) await reloadWords(selectedId);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const toggleStatus = async (word: Word) => {
    const toLearning = word.status !== "learning";
    try {
      await api.setWordMastered(word.id, !toLearning);
      if (selectedId != null) {
        await reloadWords(selectedId);
        await refreshGroups(selectedId);
      }
      notify("ok", toLearning ? "「" + word.text + "」已加入错题本" : "「" + word.text + "」已移出错题本");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const removeWord = (word: Word) => {
    setConfirm({
      title: "删除词条",
      body: "确定删除「" + word.text + "」吗？",
      run: async () => {
        try {
          await api.deleteWord(word.id);
          if (selectedId != null) {
            await reloadWords(selectedId);
            await refreshGroups(selectedId);
          }
        } catch (e) {
          notify("error", errorMessage(e));
        }
      },
    });
  };

  const handleImported = async (inserted: number, skipped: number) => {
    setPasteOpen(false);
    notify("ok", "导入完成：" + inserted + " 个新词" + (skipped > 0 ? "，" + skipped + " 个已存在被跳过" : ""));
    if (selectedId != null) {
      await reloadWords(selectedId);
      await refreshGroups(selectedId);
    }
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="flex min-h-0 flex-1">
      {/* 分组列表 */}
      <aside className="flex w-64 shrink-0 flex-col border-r border-stone-200 bg-white">
        <div className="flex items-baseline justify-between px-4 pt-4 pb-2">
          <h1 className="text-sm font-semibold text-stone-800">词语表</h1>
          <span className="text-xs text-stone-400">{groups.length} 组</span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {groups.length === 0 && (
            <p className="px-2 py-6 text-center text-xs leading-relaxed text-stone-400">
              还没有分组
              <br />
              先建一个「第 1 单元」
            </p>
          )}
          {groups.map((g) => {
            const active = g.id === selectedId;
            return (
              <button
                key={g.id}
                onClick={() => setSelectedId(g.id)}
                className={
                  "mb-0.5 flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left transition " +
                  (active ? "bg-amber-50 ring-1 ring-amber-200" : "hover:bg-stone-50")
                }
              >
                <span
                  className={
                    "flex h-6 w-6 shrink-0 items-center justify-center rounded text-xs font-medium " +
                    (active ? "bg-amber-600 text-white" : "bg-stone-100 text-stone-500")
                  }
                >
                  {g.code}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm text-stone-700">{g.name}</span>
                  <span className="block text-xs text-stone-400">
                    {g.wordCount} 词
                    {g.learningCount > 0 && <span className="text-rose-500"> · {g.learningCount} 错题</span>}
                  </span>
                  <span className="block text-[11px] text-stone-400">{practiceLabel(g.lastPracticedAt)}</span>
                </span>
              </button>
            );
          })}
        </div>
        <div className="border-t border-stone-100 p-2">
          <button
            onClick={() => setGroupModal({ mode: "create" })}
            className="w-full rounded-md border border-dashed border-stone-300 py-2 text-sm text-stone-500 hover:border-stone-400 hover:text-stone-700"
          >
            + 新建分组
          </button>
        </div>
      </aside>

      {/* 词条区 */}
      <section className="flex min-w-0 flex-1 flex-col">
        {current == null ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
            <p className="text-sm text-stone-500">先创建一个分组，再把课本词表粘进来</p>
            <button
              onClick={() => setGroupModal({ mode: "create" })}
              className="rounded-md bg-stone-800 px-4 py-2 text-sm font-medium text-white hover:bg-stone-900"
            >
              新建分组
            </button>
          </div>
        ) : (
          <>
            <header className="flex items-start justify-between gap-4 border-b border-stone-200 px-6 py-4">
              <div className="min-w-0">
                <h2 className="truncate text-lg font-semibold text-stone-800">
                  <span className="mr-2 text-stone-400">{current.code}</span>
                  {current.name}
                </h2>
                <p className="mt-1 text-xs text-stone-500">
                  共 {stats.total} 词 · 未练 {stats.newCount} · 错题 {stats.learning} · 已会{" "}
                  {stats.total - stats.newCount - stats.learning}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <button
                  onClick={() => setPasteOpen(true)}
                  className="rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-700"
                >
                  批量粘贴导入
                </button>
                <button
                  onClick={() => onPrint(current.id, current.code + " " + current.name)}
                  className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
                >
                  打印
                </button>
                <button
                  onClick={() => setGroupModal({ mode: "edit", group: current })}
                  className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-100"
                >
                  编辑
                </button>
                <button
                  onClick={() => removeGroup(current)}
                  className="rounded-md px-3 py-1.5 text-sm text-stone-500 hover:bg-rose-50 hover:text-rose-600"
                >
                  删除
                </button>
              </div>
            </header>

            <div className="flex items-center gap-2 border-b border-stone-100 px-6 py-3">
              <input
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void addWord();
                }}
                placeholder="输入词语，回车添加"
                className="w-64 rounded-md border border-stone-200 px-3 py-1.5 text-sm outline-none focus:border-stone-400"
              />
              <button
                onClick={addWord}
                disabled={!draft.trim()}
                className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-40"
              >
                添加
              </button>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto">
              {words.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                  <p className="text-sm text-stone-500">这个分组还没有词条</p>
                  <p className="text-xs text-stone-400">可以整段粘贴课本词表，也可以逐条输入</p>
                </div>
              ) : (
                <table className="w-full border-collapse text-sm">
                  <thead className="sticky top-0 z-10 bg-stone-50/95 text-xs text-stone-500 backdrop-blur">
                    <tr>
                      <th className="w-14 py-2 pl-6 text-left font-normal">#</th>
                      <th className="py-2 text-left font-normal">词语</th>
                      <th className="w-48 py-2 text-left font-normal">备注</th>
                      <th className="w-24 py-2 text-left font-normal">状态</th>
                      <th className="w-20 py-2 text-left font-normal">错次</th>
                      <th className="w-44 py-2 pr-6 text-right font-normal">操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {words.map((word, index) => {
                      const style = STATUS_STYLE[word.status];
                      if (edit && edit.id === word.id) {
                        return (
                          <tr key={word.id} className="border-t border-stone-100 bg-amber-50/40">
                            <td className="py-2 pl-6 text-xs text-stone-400">{index + 1}</td>
                            <td className="py-1.5 pr-2">
                              <input
                                value={edit.text}
                                onChange={(e) => setEdit({ ...edit, text: e.target.value })}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") void saveEdit();
                                  if (e.key === "Escape") setEdit(null);
                                }}
                                autoFocus
                                className="w-40 rounded border border-stone-300 px-2 py-1 text-sm outline-none"
                              />
                            </td>
                            <td className="py-1.5 pr-2">
                              <input
                                value={edit.note}
                                onChange={(e) => setEdit({ ...edit, note: e.target.value })}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") void saveEdit();
                                  if (e.key === "Escape") setEdit(null);
                                }}
                                placeholder="可选"
                                className="w-full rounded border border-stone-300 px-2 py-1 text-sm outline-none"
                              />
                            </td>
                            <td className="py-2" colSpan={2}>
                              <span className={"rounded px-1.5 py-0.5 text-xs " + style.cls}>{style.label}</span>
                            </td>
                            <td className="py-1.5 pr-6 text-right">
                              <button
                                onClick={saveEdit}
                                className="rounded bg-stone-800 px-2 py-1 text-xs text-white hover:bg-stone-900"
                              >
                                保存
                              </button>
                              <button
                                onClick={() => setEdit(null)}
                                className="ml-1 rounded px-2 py-1 text-xs text-stone-500 hover:bg-stone-200"
                              >
                                取消
                              </button>
                            </td>
                          </tr>
                        );
                      }
                      return (
                        <tr key={word.id} className="group border-t border-stone-100 hover:bg-stone-50/70">
                          <td className="py-2.5 pl-6 text-xs text-stone-400">{index + 1}</td>
                          <td className="py-2.5 pr-2 text-[15px] text-stone-800">{word.text}</td>
                          <td className="py-2.5 pr-2 text-xs text-stone-400">
                            <span className="line-clamp-1">{word.note ?? ""}</span>
                          </td>
                          <td className="py-2.5">
                            <span className={"rounded px-1.5 py-0.5 text-xs " + style.cls}>{style.label}</span>
                          </td>
                          <td className="py-2.5 text-xs text-stone-500">{word.wrongCount > 0 ? word.wrongCount : "—"}</td>
                          <td className="py-2.5 pr-6 text-right">
                            <span className="opacity-0 transition group-hover:opacity-100">
                              <button
                                onClick={() => void toggleStatus(word)}
                                title={word.status === "learning" ? "移出错题本" : "加入错题本"}
                                className="rounded px-2 py-1 text-xs text-stone-500 hover:bg-stone-200"
                              >
                                {word.status === "learning" ? "已会" : "标错"}
                              </button>
                              <button
                                onClick={() => setEdit({ id: word.id, text: word.text, note: word.note ?? "" })}
                                className="rounded px-2 py-1 text-xs text-stone-500 hover:bg-stone-200"
                              >
                                编辑
                              </button>
                              <button
                                onClick={() => removeWord(word)}
                                className="rounded px-2 py-1 text-xs text-stone-500 hover:bg-rose-50 hover:text-rose-600"
                              >
                                删除
                              </button>
                            </span>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </>
        )}
      </section>

      {groupModal && (
        <GroupModal
          group={groupModal.mode === "edit" ? groupModal.group : null}
          nextCode={String(groups.length + 1)}
          onSubmit={saveGroup}
          onClose={() => setGroupModal(null)}
        />
      )}

      {pasteOpen && current && (
        <PasteImportModal
          groupId={current.id}
          groupName={current.name}
          onClose={() => setPasteOpen(false)}
          onImported={handleImported}
        />
      )}

      {confirm && (
        <ConfirmModal
          title={confirm.title}
          body={confirm.body}
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            const run = confirm.run;
            setConfirm(null);
            void run();
          }}
        />
      )}

      {notice && (
        <div
          className={
            "fixed bottom-5 right-5 z-50 max-w-sm rounded-lg px-4 py-2.5 text-sm shadow-lg " +
            (notice.kind === "error" ? "bg-rose-600 text-white" : "bg-stone-800 text-white")
          }
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}
