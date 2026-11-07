import { useCallback, useEffect, useRef, useState } from "react";
import StrokeWord, { type StrokeWordHandle } from "../components/StrokeWord";
import MnemonicCard from "../components/MnemonicCard";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import type { AiCard, Mistake, Settings } from "../types";

interface StrokeReviewProps {
  groupId: number | null;
  /** 副标题：与错题本当前的筛选保持一致 */
  title: string;
  /** 从错题本点了具体某个词进来：直接从它开始（找不到时从头开始） */
  wordId?: number;
  onBack: () => void;
}

/** 卡片内可用的横向空间：max-w-3xl(768) 减去 px-6 两侧 */
const CARD_INNER = 720;
const GAP = 20;

/** 按字数选田字格边长：短词给足尺寸，长词按「两行放得下」反算，别让格子挤成一片。
 *  13 字以上（一整句）会折算到 96px 下限、自然折成多行，由外层滚动兜底。 */
function boxSize(count: number): number {
  if (count <= 2) return 200;
  if (count === 3) return 176;
  if (count === 4) return 152;
  const cols = Math.ceil(count / 2);
  // 再留 8px 给卡片的左右边框，否则刚好卡在边界上会掉成 3 个一行
  const fit = Math.floor((CARD_INNER - 8 - (cols - 1) * GAP) / cols / 4) * 4;
  return Math.max(96, Math.min(132, fit));
}

/** 「自动朗读」偏好存后端 settings（strokeAutoSpeak，默认关，本机偏好）：
 *  复习页内开关与设置页开关读写同一个值 */

/** 错题本的「笔画复习」：逐词展示笔顺动画，可先遮住让孩子自己写再对照 */
export default function StrokeReview({ groupId, title, wordId, onBack }: StrokeReviewProps) {
  const [words, setWords] = useState<Mistake[]>([]);
  const [index, setIndex] = useState(0);
  const [hideMode, setHideMode] = useState(false);
  const [revealed, setRevealed] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  // 翻到每个词自动用系统语音读一遍（「先自己写」遮笔画不遮读音，孩子照读音写）。
  // 默认关；设置加载完成后以 strokeAutoSpeak 为准
  const [autoSpeak, setAutoSpeak] = useState(false);
  // AI 讲解卡：aiReady 决定「生成」按钮；card 可能来自同步快照（未配置 AI 也显示）
  const [aiReady, setAiReady] = useState(false);
  const [card, setCard] = useState<AiCard | null>(null);
  const [generating, setGenerating] = useState(false);
  // 最近一次「已保存」的完整设置：页内开关切换时要整体写回，只改 strokeAutoSpeak 一个字段
  const settingsRef = useRef<Settings | null>(null);
  // 挡住在途保存期间连点：两次并发写会以后到的为准，按钮状态却先变了
  const savingPreference = useRef(false);
  const wordRef = useRef<StrokeWordHandle>(null);
  // 挡住连点：一次翻两词会让孩子莫名其妙漏掉一个
  const navAt = useRef(0);
  // 串词竞态守卫：翻词 / 生成都可能异步返回，只有 id 与当前词一致才允许 setCard
  const currentWordId = useRef<number | null>(null);
  const { notice, notify } = useNotice();

  useEffect(() => {
    (async () => {
      try {
        const [list, cfg, aiCfg] = await Promise.all([
          api.listMistakes(groupId),
          api.getSettings(),
          api.getAiConfig(),
        ]);
        setWords(list);
        settingsRef.current = cfg;
        setAutoSpeak(cfg.strokeAutoSpeak);
        setAiReady(aiCfg.configured);
        // 错题本里点了某个词进来：直接落到那个词。找不到（比如刚被移出错题本）就从头开始
        const at = wordId == null ? 0 : list.findIndex((w) => w.id === wordId);
        setIndex(at > 0 ? at : 0);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [groupId, wordId]);

  const total = words.length;
  const word = words[index];
  const masked = hideMode && !revealed;
  const charCount = word ? [...word.text].length : 0;

  const speakText = useCallback(
    async (text: string) => {
      try {
        await api.speakText(text);
      } catch (e) {
        // 朗读失败不该打断复习，但也不能无声无息让家长误以为孩子听过音了
        notify("error", "朗读失败：" + errorMessage(e));
      }
    },
    [notify],
  );

  const speak = useCallback(() => {
    if (word) void speakText(word.text);
  }, [word, speakText]);

  // 翻词时取当前词的缓存卡（本地查询，快）；失败静默——没有卡不该报错
  useEffect(() => {
    currentWordId.current = word?.id ?? null;
    setCard(null);
    const id = word?.id;
    if (id == null) return;
    void api
      .getWordCard(id)
      .then((c) => {
        if (currentWordId.current !== id) return;
        setCard(c);
      })
      .catch(() => {});
  }, [word?.id]);

  // 生成讲解卡：busy 防连点；失败 toast，不掩盖已有的缓存卡
  const generate = useCallback(async () => {
    if (!word || generating) return;
    const id = word.id;
    setGenerating(true);
    try {
      const c = await api.generateWordCard(id);
      if (currentWordId.current !== id) return;
      setCard(c);
    } catch (e) {
      notify("error", "讲解生成失败：" + errorMessage(e));
    } finally {
      setGenerating(false);
    }
  }, [word, generating, notify]);

  // 换词 / 开关切换时朗读或停上一句。StrictMode 双跑无害：后端 speak 先停旧句再读，等于只读一次
  useEffect(() => {
    if (!word) return;
    if (autoSpeak) void speak();
    else void api.stopSpeak().catch(() => {});
  }, [word, autoSpeak, speak]);

  /** 页内开关：立即生效（开启就读当前词），成功后写回后端设置，失败回滚 */
  const toggleAutoSpeak = useCallback(async () => {
    const base = settingsRef.current;
    if (!base || savingPreference.current) return;
    savingPreference.current = true;
    const next = !base.strokeAutoSpeak;
    setAutoSpeak(next);
    try {
      const saved = await api.saveSettings({ ...base, strokeAutoSpeak: next });
      settingsRef.current = saved;
      setAutoSpeak(saved.strokeAutoSpeak);
    } catch (e) {
      setAutoSpeak(!next);
      notify("error", "自动朗读偏好保存失败：" + errorMessage(e));
    } finally {
      savingPreference.current = false;
    }
  }, [notify]);

  // 退出复习页停掉可能在播的朗读
  useEffect(() => () => void api.stopSpeak().catch(() => {}), []);
  // 词表是按行/逗号切分的，一句话没有标点也会变成一个很长的「词」；
  // boxSize 会随字数缩格子，再长的靠下面的滚动兜底
  const size = boxSize(charCount);

  const go = useCallback(
    (delta: number) => {
      const now = Date.now();
      if (now - navAt.current < 180) return;
      navAt.current = now;
      setIndex((i) => Math.min(Math.max(i + delta, 0), Math.max(total - 1, 0)));
      setRevealed(!hideMode);
    },
    [hideMode, total],
  );

  /** 往后翻；到最后一个词就是「完成」——键盘和页脚共用同一条路径，
   *  否则空格走到末词会变成死键（进度 100% 却毫无反应） */
  const forward = useCallback(() => {
    if (index + 1 >= total) {
      onBack();
      return;
    }
    go(1);
  }, [index, total, go, onBack]);

  /** 重播；遮住状态下先掀开（换了 masked 会重建 writer，自然从头播） */
  const replay = useCallback(
    (only?: number) => {
      if (masked) {
        setRevealed(true);
        return;
      }
      wordRef.current?.play(only);
    },
    [masked],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Space" || e.key === "ArrowRight") {
        e.preventDefault();
        forward();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        go(-1);
      } else if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        replay();
      } else if (e.key === "Enter" && masked) {
        e.preventDefault();
        setRevealed(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [forward, go, replay, masked]);

  const toggleHide = () => {
    const next = !hideMode;
    setHideMode(next);
    setRevealed(!next);
  };

  if (loading) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  if (error) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
        <p className="text-sm text-rose-600">{error}</p>
        <button
          onClick={onBack}
          className="rounded-md border border-stone-200 px-4 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
        >
          返回错题本
        </button>
      </div>
    );
  }

  if (total === 0 || !word) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
        <p className="text-sm text-stone-500">这里还没有可以复习的词</p>
        <p className="text-xs text-stone-400">默写批改出错的词会自动收进错题本</p>
        <button
          onClick={onBack}
          className="rounded-md border border-stone-200 px-4 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
        >
          返回错题本
        </button>
      </div>
    );
  }

  const last = index + 1 >= total;
  const progress = ((index + 1) / total) * 100;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-start justify-between gap-4 border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">笔画复习</h1>
          <p className="mt-1 text-sm text-stone-500">
            {title} · 共 {total} 个词
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => void toggleAutoSpeak()}
            title="翻到每个词时用系统语音自动读一遍（需系统装有中文语音）；与设置页共用同一开关"
            className={
              "rounded-md border px-3 py-1.5 text-sm transition " +
              (autoSpeak
                ? "border-amber-300 bg-amber-50 font-medium text-amber-700"
                : "border-stone-200 text-stone-600 hover:bg-stone-50")
            }
          >
            自动朗读{autoSpeak ? " ✓" : ""}
          </button>
          <button
            onClick={toggleHide}
            title="每个词先遮住笔画，孩子在纸上写完再点「显示笔画」对照"
            className={
              "rounded-md border px-3 py-1.5 text-sm transition " +
              (hideMode
                ? "border-amber-300 bg-amber-50 font-medium text-amber-700"
                : "border-stone-200 text-stone-600 hover:bg-stone-50")
            }
          >
            {hideMode ? "先自己写 ✓" : "先自己写"}
          </button>
          <button
            onClick={onBack}
            className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
          >
            退出
          </button>
        </div>
      </header>

      {/* 超长词（没有标点的一整句）会撑高内容：这里必须能滚，且用 m-auto 而不是
          justify-center——后者在内容高于容器时会把顶部顶出可视区，滚也滚不到 */}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8">
        <div className="m-auto flex w-full max-w-3xl flex-col items-center gap-5 py-4">
          <div className="w-full">
            <div className="mb-2 flex items-baseline justify-between text-xs text-stone-400">
              <span>
                第 {index + 1} / {total} 个
              </span>
              <span>{charCount} 个字</span>
            </div>
            <div className="h-1 overflow-hidden rounded-full bg-stone-200">
              <div className="h-full rounded-full bg-amber-500 transition-all" style={{ width: progress + "%" }} />
            </div>
          </div>

          <div className="flex min-h-72 w-full flex-col items-center justify-center rounded-2xl border border-stone-200 bg-white px-6 py-10 shadow-sm">
            <StrokeWord
              key={word.id}
              ref={wordRef}
              text={word.text}
              size={size}
              masked={masked}
              onPick={(i) => replay(i)}
            />
          </div>

          <MnemonicCard
            key={word.id}
            word={word}
            card={card}
            aiReady={aiReady}
            masked={masked}
            generating={generating}
            autoSpeak={autoSpeak}
            onGenerate={() => void generate()}
            onSpeak={(t) => void speakText(t)}
          />

          <p className="text-xs text-stone-400">
            {masked
              ? "先在纸上写一遍，写完点「显示笔画」对照"
              : "空格 / → 下一个 · ← 上一个 · R 重播 · 点田字格只看这个字"}
          </p>
          <p className="text-xs text-stone-400">
            <span className="text-stone-300">{word.groupName}</span>
            {" · 错过 " + word.wrongCount + " 次 · 连对 " + word.correctStreak + " 次"}
          </p>
        </div>
      </div>

      <footer className="flex items-center justify-center gap-3 px-8 pb-8">
        <button
          onClick={() => go(-1)}
          disabled={index === 0}
          className="rounded-md border border-stone-200 bg-white px-4 py-2 text-sm text-stone-600 hover:bg-stone-50 disabled:opacity-40"
        >
          上一个
        </button>
        <button
          onClick={() => replay()}
          className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
        >
          重播笔画
        </button>
        <button
          onClick={() => void speak()}
          title="用系统语音再读一遍这个词"
          className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
        >
          🔊 读一遍
        </button>
        {masked && (
          <button
            onClick={() => setRevealed(true)}
            className="rounded-md bg-amber-600 px-5 py-2 text-sm font-medium text-white hover:bg-amber-700"
          >
            显示笔画
          </button>
        )}
        <button
          onClick={forward}
          className="rounded-md bg-stone-800 px-7 py-2 text-sm font-medium text-white hover:bg-stone-900"
        >
          {last ? "完成" : "下一个"}
        </button>
      </footer>

      <Toast notice={notice} />
    </div>
  );
}
