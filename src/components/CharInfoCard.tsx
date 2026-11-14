import { useState } from "react";
import type { DictEntry } from "../lib/dict";
import type { CharCard } from "../types";

/** 一个字在页面里的全部信息（Lookup 加载，卡片只展示） */
export interface CharInfo {
  pinyin: string;
  strokes: number | null;
  dict: DictEntry | null;
  card: CharCard | null;
}

interface CharInfoCardProps {
  char: string;
  /** undefined = 本地数据还在加载 */
  info: CharInfo | undefined;
  aiReady: boolean;
  generating: boolean;
  onGenerate: () => void;
}

/** 查字页的字信息卡：部首/笔画/结构/释义（本地）+ AI 单字讲解（可选） */
export default function CharInfoCard({ char, info, aiReady, generating, onGenerate }: CharInfoCardProps) {
  const [expanded, setExpanded] = useState(false);
  const c = info?.card?.content ?? null;

  const radical = info?.dict?.radical ?? null;
  const structure = info?.dict?.structure ?? null;
  const strokes = info?.strokes ?? null;

  return (
    <div className="w-56 shrink-0 rounded-2xl border border-stone-200 bg-white px-4 py-3 shadow-sm">
      <div className="flex items-baseline gap-2">
        <span className="text-3xl font-medium text-stone-800">{char}</span>
        <span className="text-sm text-stone-400">{info?.pinyin || ""}</span>
      </div>
      <p className="mt-1 text-xs text-stone-500">
        {info === undefined ? (
          <span className="text-stone-300">查询中…</span>
        ) : (
          <>
            部首 {radical ?? "—"} · 共 {strokes ?? "—"} 画 · {structure ?? "—"}
            {!structure && !radical && strokes != null ? "（暂无字典信息）" : ""}
          </>
        )}
      </p>
      {info?.dict?.explanation ? (
        <div className="mt-2 border-t border-stone-100 pt-2">
          <p
            className={"whitespace-pre-line text-xs leading-relaxed text-stone-600 " + (expanded ? "" : "line-clamp-4")}
          >
            {info.dict.explanation}
          </p>
          {[...info.dict.explanation].length > 80 && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="mt-1 text-[11px] text-amber-700 hover:text-amber-800"
            >
              {expanded ? "收起" : "展开全文"}
            </button>
          )}
        </div>
      ) : (
        info !== undefined && (
          <p className="mt-2 border-t border-stone-100 pt-2 text-xs text-stone-300">暂无释义</p>
        )
      )}

      {/* AI 单字讲解 */}
      {info !== undefined && (
        <div className="mt-2 border-t border-stone-100 pt-2">
          {c ? (
            <div className="flex flex-col gap-1.5">
              <p className="text-xs leading-relaxed text-stone-600">{c.meaning}</p>
              <p className="text-center text-sm text-stone-800">“{c.mnemonic}”</p>
              <div className="flex flex-wrap gap-1">
                {c.words.map((w, i) => (
                  <span key={i} className="rounded bg-amber-100 px-1.5 py-0.5 text-xs text-stone-700">
                    {w}
                  </span>
                ))}
              </div>
              {c.sentences.map((s, i) => (
                <p key={i} className="text-xs text-stone-500">
                  {s}
                </p>
              ))}
              <button
                onClick={onGenerate}
                disabled={generating}
                title="重新生成（会覆盖当前内容）"
                className="self-end text-[11px] text-stone-400 hover:text-amber-700 disabled:opacity-50"
              >
                {generating ? "生成中…" : "重新生成"}
              </button>
            </div>
          ) : aiReady ? (
            <button
              onClick={onGenerate}
              disabled={generating}
              className="w-full rounded-full border border-amber-300 bg-amber-50 px-3 py-1.5 text-xs text-amber-700 hover:bg-amber-100 disabled:cursor-wait disabled:opacity-60"
            >
              {generating ? "讲解生成中，稍等…" : "✨ 讲讲这个字"}
            </button>
          ) : (
            <p className="text-[11px] text-stone-300">在设置中配置 AI 后，可生成详细讲解</p>
          )}
        </div>
      )}
    </div>
  );
}
