import { useCallback, useEffect, useMemo, useState, type DragEvent } from "react";
import ConfirmModal from "../components/ConfirmModal";
import GroupModal from "../components/GroupModal";
import PasteImportModal from "../components/PasteImportModal";
import { api, errorMessage, practiceLabel } from "../lib/ipc";
import type { Group, Word, WordStatus } from "../types";

type Notice = { kind: "ok" | "error"; text: string };

/** 拖动落点：落在行的上沿 = 排到前面，下沿 = 排到后面，中间 = 变成子分组 */
type DropPos = "before" | "after" | "inside";

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
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [dragId, setDragId] = useState<number | null>(null);
  const [dropHint, setDropHint] = useState<{ id: number; pos: DropPos } | null>(null);
  const [rootHint, setRootHint] = useState(false);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<Notice | null>(null);

  const [groupModal, setGroupModal] = useState<
    { mode: "create"; parent: Group | null } | { mode: "edit"; group: Group } | null
  >(null);
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
    setWords(await api.listWords(groupId));
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

  const current = useMemo(() => groups.find((g) => g.id === selectedId) ?? null, [groups, selectedId]);

  // 折叠的父分组下面的节点不显示
  const visibleGroups = useMemo(() => {
    const byId = new Map(groups.map((g) => [g.id, g]));
    return groups.filter((g) => {
      let parent = g.parentId;
      while (parent != null) {
        if (collapsed.has(parent)) return false;
        parent = byId.get(parent)?.parentId ?? null;
      }
      return true;
    });
  }, [groups, collapsed]);

  const childrenOf = (id: number) => groups.filter((g) => g.parentId === id);

  const descendants = (id: number) => {
    let groupCount = 0;
    let wordCount = 0;
    const walk = (pid: number) => {
      for (const child of childrenOf(pid)) {
        groupCount += 1;
        wordCount += child.wordCount;
        walk(child.id);
      }
    };
    walk(id);
    return { groupCount, wordCount };
  };

  const stats = useMemo(() => {
    const learning = words.filter((w) => w.status === "learning").length;
    const newCount = words.filter((w) => w.status === "new").length;
    return { total: words.length, learning, newCount };
  }, [words]);

  const toggleCollapse = (id: number) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // ---------- 拖动调整分组结构 ----------

  /** node 是否落在 ancestor 这棵子树里（含自己），用来挡住「拖进自己的子孙」 */
  const inSubtree = (node: number, ancestor: number) => {
    let cursor: number | null = node;
    for (let guard = 0; cursor != null && guard < 64; guard += 1) {
      if (cursor === ancestor) return true;
      cursor = groups.find((g) => g.id === cursor)?.parentId ?? null;
    }
    return false;
  };

  /** 算出某个落点对应的「新父级 + 排在谁前面」，非法落点返回 null */
  const targetFor = (g: Group, pos: DropPos): { parentId: number | null; beforeId: number | null } | null => {
    if (dragId == null || g.id === dragId) return null;
    if (pos === "inside") {
      return inSubtree(g.id, dragId) ? null : { parentId: g.id, beforeId: null };
    }
    const parentId = g.parentId;
    if (parentId != null && inSubtree(parentId, dragId)) return null;
    if (pos === "before") return { parentId, beforeId: g.id };
    const siblings = groups.filter((x) => x.parentId === parentId);
    const next = siblings[siblings.findIndex((x) => x.id === g.id) + 1];
    return { parentId, beforeId: next ? next.id : null };
  };

  const zoneOf = (e: DragEvent<HTMLDivElement>): DropPos => {
    const rect = e.currentTarget.getBoundingClientRect();
    const ratio = rect.height > 0 ? (e.clientY - rect.top) / rect.height : 0.5;
    if (ratio < 0.3) return "before";
    if (ratio > 0.7) return "after";
    return "inside";
  };

  const endDrag = () => {
    setDragId(null);
    setDropHint(null);
    setRootHint(false);
  };

  const onRowDragOver = (e: DragEvent<HTMLDivElement>, g: Group) => {
    if (dragId == null) return;
    const pos = zoneOf(e);
    if (!targetFor(g, pos)) {
      if (dropHint) setDropHint(null);
      return;
    }
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setRootHint(false);
    if (!dropHint || dropHint.id !== g.id || dropHint.pos !== pos) setDropHint({ id: g.id, pos });
  };

  const onRowDrop = (e: DragEvent<HTMLDivElement>, g: Group) => {
    const id = dragId;
    if (id == null) return;
    const target = targetFor(g, zoneOf(e));
    if (!target) return;
    e.preventDefault();
    void applyMove(id, target.parentId, target.beforeId);
  };

  const applyMove = async (id: number, parentId: number | null, beforeId: number | null) => {
    endDrag();
    try {
      const list = await api.moveGroup(id, parentId, beforeId);
      setGroups(list);
      if (parentId != null) {
        // 拖进折叠着的分组时自动展开，否则看起来像「词条消失了」
        setCollapsed((prev) => {
          if (!prev.has(parentId)) return prev;
          const next = new Set(prev);
          next.delete(parentId);
          return next;
        });
      }
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const saveGroup = async (code: string, name: string) => {
    try {
      if (groupModal?.mode === "edit") {
        await api.updateGroup(groupModal.group.id, code, name);
        notify("ok", "分组已更新");
        await refreshGroups(groupModal.group.id);
      } else {
        const parentId = groupModal?.mode === "create" ? groupModal.parent?.id ?? null : null;
        const created = await api.createGroup(parentId, code, name);
        if (parentId != null) {
          setCollapsed((prev) => {
            const next = new Set(prev);
            next.delete(parentId);
            return next;
          });
        }
        notify("ok", "已创建分组「" + created.name + "」");
        await refreshGroups(created.id);
      }
      setGroupModal(null);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const removeGroup = (group: Group) => {
    const { groupCount, wordCount } = descendants(group.id);
    setConfirm({
      title: "删除分组",
      body:
        "将删除「" + group.path + "」" +
        (groupCount > 0 ? "及其 " + groupCount + " 个子分组" : "") +
        "，共 " + (wordCount + group.wordCount) + " 个词条" +
        (group.totalLearningCount > 0 ? "（其中 " + group.totalLearningCount + " 个在错题本中）" : "") +
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
      if (selectedId != null) {
        await reloadWords(selectedId);
        await refreshGroups(selectedId);
      }
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

  const childCount = current ? childrenOf(current.id).length : 0;

  return (
    <div className="flex min-h-0 flex-1">
      {/* 分组树 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-stone-200 bg-white">
        <div className="flex items-baseline justify-between px-4 pt-4 pb-2">
          <h1 className="text-sm font-semibold text-stone-800">词语表</h1>
          <span className="text-xs text-stone-400">{groups.length} 组</span>
        </div>
        <div
          className="min-h-0 flex-1 overflow-y-auto px-2 pb-2"
          onDragOver={(e) => {
            // 拖到空白处（不是某一行）时清掉落点提示
            if (e.target !== e.currentTarget) return;
            if (dropHint) setDropHint(null);
            if (rootHint) setRootHint(false);
          }}
        >
          {groups.length === 0 && (
            <p className="px-2 py-6 text-center text-xs leading-relaxed text-stone-400">
              还没有分组
              <br />
              先建一个「二年级上册」
            </p>
          )}
          {groups.length > 0 && (
            <p className="px-2 pb-1.5 text-[11px] text-stone-400">拖动分组可调整层级与顺序</p>
          )}
          {visibleGroups.map((g) => {
            const active = g.id === selectedId;
            const children = groups.filter((x) => x.parentId === g.id).length;
            const drop = dropHint && dropHint.id === g.id ? dropHint.pos : null;
            const dragging = dragId === g.id;
            return (
              <div key={g.id} style={{ paddingLeft: g.depth * 14 }} className="relative">
                {drop === "before" && (
                  <span className="pointer-events-none absolute inset-x-2 top-0 z-10 h-0.5 rounded-full bg-amber-500" />
                )}
                <div
                  draggable
                  title="拖动可调整层级与顺序"
                  onDragStart={(e) => {
                    e.dataTransfer.effectAllowed = "move";
                    e.dataTransfer.setData("text/plain", String(g.id));
                    setDragId(g.id);
                  }}
                  onDragEnd={endDrag}
                  onDragOver={(e) => onRowDragOver(e, g)}
                  onDrop={(e) => onRowDrop(e, g)}
                  onClick={() => setSelectedId(g.id)}
                  className={
                    "mb-0.5 flex cursor-grab items-start gap-1 rounded-md py-1.5 pr-2 active:cursor-grabbing " +
                    (drop === "inside"
                      ? "bg-amber-100 ring-2 ring-amber-400"
                      : active
                        ? "bg-amber-50 ring-1 ring-amber-200"
                        : "hover:bg-stone-50") +
                    (dragging ? " opacity-40" : "")
                  }
                >
                  {children > 0 ? (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleCollapse(g.id);
                      }}
                      className="mt-1 w-4 shrink-0 text-center text-[10px] text-stone-400 hover:text-stone-700"
                    >
                      {collapsed.has(g.id) ? "▸" : "▾"}
                    </button>
                  ) : (
                    <span className="w-4 shrink-0" />
                  )}
                  <span
                    className={
                      "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded text-[10px] font-medium " +
                      (active ? "bg-amber-600 text-white" : "bg-stone-100 text-stone-500")
                    }
                  >
                    {g.code}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm text-stone-700">{g.name}</span>
                    <span className="block text-[11px] text-stone-400">
                      {g.totalWordCount} 词
                      {g.totalLearningCount > 0 && (
                        <span className="text-rose-500"> · {g.totalLearningCount} 错题</span>
                      )}
                      {children > 0 && <span className="text-stone-400"> · {children} 子组</span>}
                    </span>
                    <span className="block text-[11px] text-stone-400">
                      {practiceLabel(g.lastPracticedAt)}
                    </span>
                  </span>
                </div>
                {drop === "after" && (
                  <span className="pointer-events-none absolute inset-x-2 bottom-0 z-10 h-0.5 rounded-full bg-amber-500" />
                )}
              </div>
            );
          })}
        </div>
        <div className="space-y-1 border-t border-stone-100 p-2">
          {dragId != null && (
            <div
              onDragOver={(e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (dropHint) setDropHint(null);
                setRootHint(true);
              }}
              onDragLeave={(e) => {
                // 移到子节点上也会触发 dragleave，只有真正离开时才清掉
                if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setRootHint(false);
              }}
              onDrop={(e) => {
                e.preventDefault();
                void applyMove(dragId, null, null);
              }}
              className={
                "w-full rounded-md border border-dashed px-2 py-1.5 text-center text-[11px] " +
                (rootHint
                  ? "border-amber-400 bg-amber-50 text-amber-700"
                  : "border-stone-200 text-stone-400")
              }
            >
              拖到这里 → 移到最外层
            </div>
          )}
          <button
            onClick={() => setGroupModal({ mode: "create", parent: null })}
            className="w-full rounded-md border border-dashed border-stone-300 py-2 text-sm text-stone-500 hover:border-stone-400 hover:text-stone-700"
          >
            + 新建分组
          </button>
          {current && (
            <button
              onClick={() => setGroupModal({ mode: "create", parent: current })}
              className="w-full rounded-md py-1.5 text-xs text-stone-500 hover:bg-stone-50 hover:text-stone-700"
            >
              + 在「{current.name}」下新建子分组
            </button>
          )}
        </div>
      </aside>

      {/* 词条区 */}
      <section className="flex min-w-0 flex-1 flex-col">
        {current == null ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
            <p className="text-sm text-stone-500">先创建一个分组，再把课本词表粘进来</p>
            <button
              onClick={() => setGroupModal({ mode: "create", parent: null })}
              className="rounded-md bg-stone-800 px-4 py-2 text-sm font-medium text-white hover:bg-stone-900"
            >
              新建分组
            </button>
          </div>
        ) : (
          <>
            <header className="flex items-start justify-between gap-4 border-b border-stone-200 px-6 py-4">
              <div className="min-w-0">
                {current.depth > 0 && (
                  <p className="mb-0.5 truncate text-xs text-stone-400">
                    {current.pathSegments.slice(0, -1).join(" / ")}
                  </p>
                )}
                <h2 className="truncate text-lg font-semibold text-stone-800">
                  <span className="mr-2 text-stone-400">{current.code}</span>
                  {current.name}
                </h2>
                <p className="mt-1 text-xs text-stone-500">
                  本组 {stats.total} 词 · 未练 {stats.newCount} · 错题 {stats.learning} · 已会{" "}
                  {stats.total - stats.newCount - stats.learning}
                  {childCount > 0 && (
                    <span className="text-stone-400">
                      　|　含 {childCount} 个子分组共 {current.totalWordCount} 词
                    </span>
                  )}
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
                  onClick={() => onPrint(current.id, current.path)}
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
                placeholder="输入词语，回车添加到本组"
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
                  <p className="text-sm text-stone-500">
                    {childCount > 0 ? "这个分组本身没有词条" : "这个分组还没有词条"}
                  </p>
                  <p className="text-xs text-stone-400">
                    {childCount > 0
                      ? "词条都在下面的子分组里，抽题和打印会自动包含"
                      : "可以整段粘贴课本词表，也可以逐条输入"}
                  </p>
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
                              <span className={"rounded px-1.5 py-0.5 text-xs " + style.cls}>
                                {style.label}
                              </span>
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
                            <span className={"rounded px-1.5 py-0.5 text-xs " + style.cls}>
                              {style.label}
                            </span>
                          </td>
                          <td className="py-2.5 text-xs text-stone-500">
                            {word.wrongCount > 0 ? word.wrongCount : "—"}
                          </td>
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
          parent={groupModal.mode === "create" ? groupModal.parent : null}
          nextCode={String(
            (groupModal.mode === "create" && groupModal.parent
              ? groups.filter((g) => g.parentId === groupModal.parent!.id).length
              : groups.filter((g) => g.parentId === null).length) + 1,
          )}
          onSubmit={saveGroup}
          onClose={() => setGroupModal(null)}
        />
      )}

      {pasteOpen && current && (
        <PasteImportModal
          groupId={current.id}
          groupName={current.path}
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
