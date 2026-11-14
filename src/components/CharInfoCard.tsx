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

/** 查字页的字信息卡：横向布局——左边字头列（大字/拼音/部首/笔画/结构），
 *  右边释义 + AI 讲解放进限高滚动区。释义原文很长（新华字典风格全文），
 *  竖排会把卡片拉出一屏高，横排 + 内部滚动让每张卡高度可控、多字时两列对齐。 */
export default function CharInfoCard({ char, info, aiReady, generating, onGenerate }: CharInfoCardProps) {
  const c = info?.card?.content ?? null;

  const radical = info?.dict?.radical ?? null;
  const structure = info?.dict?.structure ?? null;
  const strokes = info?.strokes ?? null;

  return (
    <div className="flex w-full min-w-80 gap-4 rounded-2xl border border-stone-200 bg-white px-4 py-3 shadow-sm min-[652px]:w-[calc(50%-0.375rem)]">
      {/* 字头列：大字 + 拼音 + 基本信息（竖排小字，居中） */}
      <div className="flex w-20 shrink-0 flex-col items-center gap-0.5">
        <span className="text-4xl font-medium leading-tight text-stone-800">{char}</span>
        <span className="text-sm text-stone-400">{info?.pinyin || ""}</span>
        <div className="mt-1 flex flex-col items-center text-[11px] leading-5 text-stone-400">
          {info === undefined ? (
            <span className="text-stone-300">查询中…</span>
          ) : (
            <>
              <span>部首 {radical ?? "—"}</span>
              <span>共 {strokes ?? "—"} 画</span>
              <span>{structure ?? "—"}</span>
              {!structure && !radical && strokes != null && <span>（暂无字典信息）</span>}
            </>
          )}
        </div>
      </div>

      {/* 右列：释义限高滚动 + AI 讲解常驻（AI 移出滚动区，长释义时生成入口不被埋没） */}
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="max-h-44 overflow-y-auto pr-1">
          {info?.dict?.explanation ? (
            <p className="whitespace-pre-line text-xs leading-relaxed text-stone-600">
              {info.dict.explanation}
            </p>
          ) : (
            info !== undefined && <p className="text-xs text-stone-300">暂无释义</p>
          )}
        </div>

        {/* AI 单字讲解 */}
        {info !== undefined && (
          <div className="border-t border-stone-100 pt-2">
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
      </div>
  );
}
