import HanziWriter from "hanzi-writer";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { charPinyins } from "../lib/pinyin";
import { isHanzi, loadStrokeData } from "../lib/strokes";
import GridLines from "./GridLines";

export interface StrokeWordHandle {
  /** 从第一个字开始重播；传 index 就只重播这一个字 */
  play: (only?: number) => void;
}

/** 拼音字号随格子缩放：长词格子小，拼音也要跟着小，否则比字还抢眼 */
function pinyinFontSize(boxSize: number): number {
  if (boxSize >= 150) return 16;
  if (boxSize >= 110) return 14;
  return 12;
}

interface StrokeWordProps {
  text: string;
  /** 每个田字格的边长（px） */
  size: number;
  /** 遮住笔画：只留空田字格，让孩子先自己写 */
  masked: boolean;
  /** 点了某一个田字格（由页面决定是「先显示」还是「只重播这个字」） */
  onPick: (index: number) => void;
}

/**
 * 一个词的逐字笔顺展示：每个字一个田字格，笔画动画串行播放。
 * 数据从本地 public/hanzi/ 取，取不到就退回静态字形（不会留白）。
 */
const StrokeWord = forwardRef<StrokeWordHandle, StrokeWordProps>(function StrokeWord(
  { text, size, masked, onPick },
  ref,
) {
  const chars = useMemo(() => [...text], [text]);
  // 整词转换（多音字按词消歧），再与字符一一对齐；非汉字占位空串
  const pinyins = useMemo(() => charPinyins(text), [text]);
  const hosts = useRef<Array<HTMLDivElement | null>>([]);
  const writers = useRef<Array<HanziWriter | null>>([]);
  // 动画串行用的代际号：每次重播 / 换词都 +1，旧的循环看到号变了就收手
  const runId = useRef(0);
  // 组件这一轮挂载是否还有效：数据加载是异步的，回来时可能已经换词或卸载。
  // 必须和 runId 分开——play() 也会 +1，共用会让「加载失败」被误判成「过期回调」，
  // 于是缺数据的字既不回退静态字形、也没有提示，只剩一个空田字格。
  const alive = useRef(0);
  const [missing, setMissing] = useState<number[]>([]);

  const play = useCallback((only?: number) => {
    const id = ++runId.current;
    const list = only == null ? writers.current : [writers.current[only] ?? null];
    void (async () => {
      for (const writer of list) {
        if (!writer || runId.current !== id) return;
        try {
          // 数据还没到时会自动等加载完成；失败时这里抛错，交给下面的 catch
          await writer.animateCharacter();
        } catch {
          // 没有笔画数据的字：静态字形已经渲染出来了，动画跳过即可
        }
      }
    })();
  }, []);

  useEffect(() => {
    const gen = ++alive.current;
    const list: Array<HanziWriter | null> = [];
    const failed: number[] = [];

    chars.forEach((char, i) => {
      const host = hosts.current[i];
      if (!host) return;
      host.innerHTML = "";
      if (!isHanzi(char)) {
        failed.push(i);
        return;
      }
      try {
        list[i] = HanziWriter.create(host, char, {
          width: size,
          height: size,
          padding: Math.round(size * 0.06),
          charDataLoader: (c) => loadStrokeData(c),
          // 初始一律不画笔画：否则还没轮到的字会先整字显示出来，轮到时再被擦掉重描，
          // 孩子看到的是「先给答案再重写一遍」。淡灰轮廓留着当描红底。
          // 遮住模式下连轮廓都不画，否则等于给了答案。
          showCharacter: false,
          showOutline: !masked,
          strokeColor: "#1c1917",
          outlineColor: "#e7e5e4",
          strokeWidth: 2,
          strokeAnimationSpeed: 1.15,
          delayBetweenStrokes: 200,
          strokeFadeDuration: 200,
          onLoadCharDataError: () => {
            // 数据缺失 / 分片读不出来：回退成静态字形，别留一个空框
            if (alive.current !== gen) return;
            list[i] = null;
            setMissing((prev) => (prev.includes(i) ? prev : [...prev, i]));
          },
        });
      } catch {
        failed.push(i);
      }
    });

    writers.current = list;
    setMissing(failed);
    if (!masked) play();

    return () => {
      alive.current += 1;
      runId.current += 1;
    };
  }, [chars, size, masked, play]);

  useImperativeHandle(ref, () => ({ play }), [play]);

  return (
    <div className="flex flex-wrap items-end justify-center gap-x-5 gap-y-3">
      {chars.map((char, i) => (
        <div key={i} className="flex shrink-0 flex-col items-center gap-1">
          {/* 拼音行：田字格上方，像拼音田字格本；非汉字与无拼音占位保持对齐 */}
          <span
            className="select-none text-center text-stone-400"
            style={{ width: size, fontSize: pinyinFontSize(size), lineHeight: 1.2 }}
          >
            {pinyins[i] ?? ""}
          </span>
          <button
            type="button"
            onClick={() => onPick(i)}
            title={
              missing.includes(i)
                ? "「" + char + "」暂无笔画数据"
                : masked
                  ? "点一下显示笔画"
                  : "点一下只看这个字"
            }
            className="ge shrink-0 cursor-pointer rounded-sm bg-white transition hover:border-amber-400"
            style={{ width: size, height: size }}
          >
            <GridLines kind="tian" />
            <div
              ref={(el) => {
                hosts.current[i] = el;
              }}
              className="absolute inset-0"
            />
            {/* 没有笔画数据的字退回静态字形；遮住模式下不画，否则等于给了答案 */}
            {missing.includes(i) && !masked && (
              <span
                className="pointer-events-none absolute inset-0 flex items-center justify-center text-stone-800"
                style={{ fontSize: size * 0.62, lineHeight: 1 }}
              >
                {char}
              </span>
            )}
          </button>
        </div>
      ))}
    </div>
  );
});

export default StrokeWord;
