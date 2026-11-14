import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import CharInfoCard, { type CharInfo } from "../components/CharInfoCard";
import StrokeWord, { type StrokeWordHandle } from "../components/StrokeWord";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import { loadDictEntry } from "../lib/dict";
import { charPinyins } from "../lib/pinyin";
import { isHanzi, loadStrokeData } from "../lib/strokes";
import type { CharCard } from "../types";

/** 输入长度上限：查字不是抄句子，超长没有意义 */
const MAX_CHARS = 12;

/** 卡片内可用横向空间与间距（与 StrokeReview 同款折算；那边未导出，查字场景照抄取值） */
const CARD_INNER = 720;
const GAP = 20;

function boxSize(count: number): number {
  if (count <= 2) return 200;
  if (count === 3) return 176;
  if (count === 4) return 152;
  const cols = Math.ceil(count / 2);
  const fit = Math.floor((CARD_INNER - 8 - (cols - 1) * GAP) / cols / 4) * 4;
  return Math.max(96, Math.min(132, fit));
}

/** 查字工具：家长输入字词 → 逐字笔顺动画 + 部首/笔画/结构/释义 + AI 单字讲解 */
export default function Lookup() {
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [aiReady, setAiReady] = useState(false);
  const [infos, setInfos] = useState<Record<string, CharInfo>>({});
  const [loadingInfo, setLoadingInfo] = useState(false);
  /** 正在生成的字（一次一个；null = 没在生成） */
  const [generating, setGenerating] = useState<string | null>(null);
  const wordRef = useRef<StrokeWordHandle>(null);
  // 数据加载竞态守卫：换词后旧请求回来不许再写 state
  // 最近一次成功生成的字卡：generate 完成时词已换，加载 effect 的 setInfos(next)
  // 会把它覆盖掉（卡已入后端库但页面不显示），这里记住以便下次加载时并回
  const lastCardRef = useRef<{ ch: string; card: CharCard } | null>(null);
  const queryGen = useRef(0);
  const { notice, notify } = useNotice();

  useEffect(() => {
    api
      .getAiConfig()
      .then((c) => setAiReady(c.configured))
      .catch(() => {});
  }, []);

  // 退出页面停掉可能在播的朗读
  useEffect(() => () => void api.stopSpeak().catch(() => {}), []);

  const submit = useCallback(() => {
    const chars = [...text].filter(isHanzi);
    if (chars.length === 0) {
      notify("error", "请输入汉字");
      return;
    }
    if (chars.length > MAX_CHARS) {
      chars.length = MAX_CHARS;
      // notify 只支持 ok/error，截断是中性提示，用 ok 的非阻断样式
      notify("ok", `最多查 ${MAX_CHARS} 个字，已取前 ${MAX_CHARS} 个`);
    }
    setQuery(chars.join(""));
  }, [text, notify]);

  // 换词后逐字取：笔画数（笔顺数据）+ 字典条目 + AI 缓存卡（全本地，失败静默降级「—」）
  useEffect(() => {
    if (!query) return;
    const gen = ++queryGen.current;
    const chars = [...query];
    const unique = [...new Set(chars)];
    const pinyins = charPinyins(query);
    setLoadingInfo(true);
    setInfos({});
    void (async () => {
      const next: Record<string, CharInfo> = {};
      await Promise.all(
        unique.map(async (ch) => {
          const firstAt = chars.indexOf(ch);
          const [strokes, dict, card] = await Promise.all([
            loadStrokeData(ch)
              .then((d) => d.strokes.length)
              .catch(() => null),
            loadDictEntry(ch),
            api.getCharCard(ch).catch(() => null),
          ]);
          next[ch] = { pinyin: pinyins[firstAt] ?? "", strokes, dict, card };
        }),
      );
      if (queryGen.current !== gen) return;
      // generate 完成时词已换的那张卡，若本次查询包含该字，并回结果（本地缓存可能还没它）
      const last = lastCardRef.current;
      if (last && [...query].includes(last.ch)) {
        const base = next[last.ch] ?? { pinyin: "", strokes: null, dict: null, card: null };
        next[last.ch] = { ...base, card: last.card };
      }
      setInfos(next);
      setLoadingInfo(false);
    })();
  }, [query]);

  const generate = useCallback(
    async (ch: string) => {
      if (generating) return;
      setGenerating(ch);
      try {
        const card = await api.generateCharCard(ch);
        // 记住这张卡：即使词已换、本次不显示，下次加载该字时也能并回来
        lastCardRef.current = { ch, card };
        // 词已换、新词里没有这个字：这张卡不属于当前页面，丢弃
        if (![...query].includes(ch)) return;
        setInfos((prev) => {
          const base = prev[ch] ?? { pinyin: "", strokes: null, dict: null, card: null };
          return { ...prev, [ch]: { ...base, card } };
        });
      } catch (e) {
        notify("error", "讲解生成失败：" + errorMessage(e));
      } finally {
        setGenerating(null);
      }
    },
    [generating, query, notify],
  );

  const speak = useCallback(() => {
    if (!query) return;
    void api.speakText(query).catch((e) => notify("error", "朗读失败：" + errorMessage(e)));
  }, [query, notify]);

  const replay = useCallback((only?: number) => wordRef.current?.play(only), []);

  const uniqueChars = useMemo(() => [...new Set([...query])], [query]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center justify-between gap-4 border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">查字</h1>
          <p className="mt-1 text-sm text-stone-500">输入字词，看笔顺动画和字的解释</p>
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
          className="flex items-center gap-2"
        >
          <input
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="输入字或词，如：碧绿"
            maxLength={MAX_CHARS * 2}
            className="w-64 rounded-md border border-stone-300 px-3 py-1.5 text-sm outline-none focus:border-amber-400"
          />
          <button
            type="submit"
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900"
          >
            查一查
          </button>
        </form>
      </header>

      {query ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8">
          <div className="m-auto flex w-full max-w-3xl flex-col items-center gap-5 py-4">
            <div className="flex min-h-72 w-full flex-col items-center justify-center rounded-2xl border border-stone-200 bg-white px-6 py-10 shadow-sm">
              <StrokeWord
                key={query}
                ref={wordRef}
                text={query}
                size={boxSize([...query].length)}
                masked={false}
                onPick={(i) => replay(i)}
              />
            </div>
            <div className="flex items-center gap-3">
              <button
                onClick={() => replay()}
                className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
              >
                重播笔画
              </button>
              <button
                onClick={speak}
                title="用系统语音读一遍这个词"
                className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
              >
                🔊 读一遍
              </button>
            </div>
            <div className="flex w-full flex-col gap-3">
              {uniqueChars.map((ch) => (
                <CharInfoCard
                  key={ch}
                  char={ch}
                  info={loadingInfo ? undefined : infos[ch]}
                  aiReady={aiReady}
                  generating={generating === ch}
                  onGenerate={() => void generate(ch)}
                />
              ))}
            </div>
            <p className="text-xs text-stone-400">点田字格只重播这个字</p>
          </div>
        </div>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center">
          <p className="text-sm text-stone-500">孩子问「这个字怎么写」？在这里查给他看</p>
          <p className="text-xs text-stone-400">逐字笔顺动画 · 部首 · 笔画 · 结构 · 释义</p>
        </div>
      )}
      <Toast notice={notice} />
    </div>
  );
}
