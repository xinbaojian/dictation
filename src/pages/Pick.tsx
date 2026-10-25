import { useEffect, useMemo, useState } from "react";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage, formatTime } from "../lib/ipc";
import type { Group, Scope, Session } from "../types";

interface PickProps {
  onStart: (sessionId: number) => void;
  onReview: (sessionId: number) => void;
  onGoWordbook: () => void;
}

const SCOPES: { value: Scope; title: string; desc: string }[] = [
  { value: "unlearned", title: "仅未掌握", desc: "新词 + 错题本里的词" },
  { value: "all", title: "全部词语", desc: "含已掌握，适合期末复习" },
  { value: "mistake", title: "错题本", desc: "只抽标记为错的词" },
];

const LIMITS = [10, 20, 30, 0];

export default function Pick({ onStart, onReview, onGoWordbook }: PickProps) {
  const [groups, setGroups] = useState<Group[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [scope, setScope] = useState<Scope>("unlearned");
  const [limit, setLimit] = useState(20);
  const [count, setCount] = useState(0);
  const [active, setActive] = useState<Session | null>(null);
  const [chainByChar, setChainByChar] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        const [list, session, settings] = await Promise.all([
          api.listGroups(),
          api.activeSession(),
          // 读设置失败不该挡住开始默写，拿不到就按默认（不连字）显示
          api.getSettings().catch(() => null),
        ]);
        setGroups(list);
        setSelected(list.map((g) => g.id));
        setActive(session);
        setChainByChar(settings?.chainByChar ?? false);
      } catch (e) {
        notify("error", errorMessage(e));
      } finally {
        setLoading(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let alive = true;
    const timer = setTimeout(() => {
      api
        .countCandidates(selected, scope)
        .then((n) => {
          if (alive) setCount(n);
        })
        .catch((e) => {
          if (alive) notify("error", errorMessage(e));
        });
    }, 120);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, scope]);

  const effectiveLimit = limit === 0 ? count : Math.min(limit, count);
  // 根分组的总词数之和，避免父子重复计算
  const totalWords = useMemo(
    () => groups.filter((g) => g.parentId === null).reduce((sum, g) => sum + g.totalWordCount, 0),
    [groups],
  );

  const subtreeIds = (id: number): number[] => {
    const out = [id];
    for (let i = 0; i < out.length; i += 1) {
      for (const g of groups) {
        if (g.parentId === out[i] && !out.includes(g.id)) out.push(g.id);
      }
    }
    return out;
  };

  const toggleGroup = (id: number) => {
    const ids = subtreeIds(id);
    const allSelected = ids.every((x) => selected.includes(x));
    setSelected((prev) =>
      allSelected
        ? prev.filter((x) => !ids.includes(x))
        : Array.from(new Set([...prev, ...ids])),
    );
  };

  const start = async () => {
    if (busy || count === 0) return;
    setBusy(true);
    try {
      const session = await api.createSession(selected, scope, limit, true);
      onStart(session.id);
    } catch (e) {
      notify("error", errorMessage(e));
      setBusy(false);
    }
  };

  const abandon = async () => {
    if (!active) return;
    try {
      await api.abandonSession(active.id);
      setActive(null);
      notify("ok", "已放弃上次默写");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  if (groups.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
        <p className="text-sm text-stone-500">还没有词表，先去录入课本词语</p>
        <button
          onClick={onGoWordbook}
          className="rounded-md bg-stone-800 px-4 py-2 text-sm font-medium text-white hover:bg-stone-900"
        >
          去录入词表
        </button>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-8 py-8">
        <h1 className="text-xl font-semibold text-stone-800">开始默写</h1>
        <p className="mt-1 text-sm text-stone-500">选好分组和范围，随机抽词，每个词只出现一次</p>

        {active && (
          <div className="mt-5 flex items-center justify-between gap-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-amber-900">
                {active.status === "running" ? "上次默写还没做完" : "上次默写还没批改"}
              </p>
              <p className="mt-0.5 truncate text-xs text-amber-700">
                {active.groupLabel} · {active.total} 词 · {formatTime(active.startedAt)}
              </p>
            </div>
            <div className="flex shrink-0 gap-2">
              <button
                onClick={abandon}
                className="rounded-md px-2.5 py-1.5 text-xs text-amber-800 hover:bg-amber-100"
              >
                放弃
              </button>
              <button
                onClick={() => (active.status === "running" ? onStart(active.id) : onReview(active.id))}
                className="rounded-md bg-amber-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-amber-700"
              >
                {active.status === "running" ? "继续默写" : "去批改"}
              </button>
            </div>
          </div>
        )}

        <section className="mt-7">
          <div className="mb-2 flex items-baseline justify-between">
            <h2 className="text-sm font-medium text-stone-700">选择分组</h2>
            <button
              onClick={() => setSelected(selected.length === groups.length ? [] : groups.map((g) => g.id))}
              className="text-xs text-stone-500 hover:text-stone-800"
            >
              {selected.length === groups.length ? "全部取消" : "全选"}
            </button>
          </div>
          <div className="overflow-hidden rounded-lg border border-stone-200 bg-white">
            {groups.map((g) => {
              const on = selected.includes(g.id);
              const partial = !on && subtreeIds(g.id).some((x) => selected.includes(x));
              return (
                <button
                  key={g.id}
                  onClick={() => toggleGroup(g.id)}
                  style={{ paddingLeft: 12 + g.depth * 18 }}
                  className={
                    "flex w-full items-center gap-2.5 border-b border-stone-100 px-3 py-2 text-left last:border-b-0 " +
                    (on ? "bg-amber-50/70" : "hover:bg-stone-50")
                  }
                >
                  <span
                    className={
                      "flex h-4 w-4 shrink-0 items-center justify-center rounded border text-[9px] " +
                      (on
                        ? "border-amber-600 bg-amber-600 text-white"
                        : partial
                          ? "border-amber-400 bg-amber-100 text-amber-600"
                          : "border-stone-300 text-transparent")
                    }
                  >
                    {on ? "✓" : partial ? "–" : "✓"}
                  </span>
                  <span className="w-5 shrink-0 text-center text-[11px] text-stone-400">{g.code}</span>
                  <span className="min-w-0 flex-1 truncate text-sm text-stone-700">
                    {g.depth === 0 ? g.name : "· " + g.name}
                  </span>
                  <span className="shrink-0 text-xs text-stone-400">
                    {g.totalWordCount} 词
                    {g.totalLearningCount > 0 && (
                      <span className="text-rose-500"> · {g.totalLearningCount} 错题</span>
                    )}
                  </span>
                </button>
              );
            })}
          </div>
          <p className="mt-2 text-xs text-stone-400">
            共 {totalWords} 个词条；选父分组会连带选中下面的子分组，抽题时也会包含
          </p>
        </section>

        <section className="mt-7">
          <h2 className="mb-2 text-sm font-medium text-stone-700">抽题范围</h2>
          <div className="grid grid-cols-3 gap-2">
            {SCOPES.map((s) => (
              <button
                key={s.value}
                onClick={() => setScope(s.value)}
                className={
                  "rounded-lg border px-3 py-2.5 text-left transition " +
                  (scope === s.value
                    ? "border-stone-800 bg-white ring-1 ring-stone-800"
                    : "border-stone-200 bg-white hover:border-stone-300")
                }
              >
                <span className="block text-sm font-medium text-stone-800">{s.title}</span>
                <span className="mt-0.5 block text-xs text-stone-500">{s.desc}</span>
              </button>
            ))}
          </div>
        </section>

        <section className="mt-7">
          <h2 className="mb-2 text-sm font-medium text-stone-700">题量</h2>
          <div className="flex gap-2">
            {LIMITS.map((value) => (
              <button
                key={value}
                onClick={() => setLimit(value)}
                className={
                  "rounded-md border px-4 py-1.5 text-sm transition " +
                  (limit === value
                    ? "border-stone-800 bg-stone-800 text-white"
                    : "border-stone-200 bg-white text-stone-600 hover:border-stone-300")
                }
              >
                {value === 0 ? "全部" : value + " 词"}
              </button>
            ))}
          </div>
        </section>

        <div className="mt-8 flex items-center justify-between rounded-xl border border-stone-200 bg-white px-5 py-4">
          <div>
            <p className="text-sm text-stone-600">
              可抽 <span className="text-lg font-semibold text-stone-900">{count}</span> 词
              {count > 0 && limit > 0 && count > limit ? "，本次抽 " + effectiveLimit + " 个" : ""}
            </p>
            <p className="mt-0.5 text-xs text-stone-400">
              {chainByChar ? "顺序随机，下一个词优先和上一个词有相同的字" : "顺序随机"}，抽完自动进入批改
            </p>
          </div>
          <button
            onClick={start}
            disabled={count === 0 || busy}
            className="rounded-lg bg-amber-600 px-6 py-2.5 text-sm font-medium text-white hover:bg-amber-700 disabled:cursor-not-allowed disabled:bg-stone-300"
          >
            开始默写
          </button>
        </div>
      </div>
      <Toast notice={notice} />
    </div>
  );
}
