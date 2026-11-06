import { useEffect, useState } from "react";
import GridLines, { type GridKind } from "../components/GridLines";
import StrokeGlyph from "../components/StrokeGlyph";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import { printWithToast } from "../lib/print";
import { isHanzi, loadStrokePaths } from "../lib/strokes";

interface PrintSheetProps {
  mode: "mistakes" | "group";
  groupId: number | null;
  title: string;
  onBack: () => void;
}

interface Row {
  id: number;
  text: string;
}

/** 字帖选项；与口算卷一样存 localStorage，家长调过一次就不用每次重调 */
interface SheetOptions {
  /** 抄写单：每行第一遍印黑色示范字；关掉就是整行空白的默写单 */
  showWord: boolean;
  grid: GridKind;
  /** 方格边长（mm） */
  cell: number;
  /** 描红遍数（浅灰字），仅抄写单有意义 */
  trace: number;
}

const DEFAULT_OPTIONS: SheetOptions = { showWord: true, grid: "tian", cell: 12, trace: 1 };
const STORAGE_KEY = "printSheet.options";
const CELL_CHOICES = [10, 12, 14];
const TRACE_CHOICES = [0, 1, 2];
const GRID_CHOICES: GridKind[] = ["tian", "mi", "fang"];
// 只作 sanity 上限：单字词在小格子下能排十几遍，别把整页排成复读机
const MAX_COPIES = 12;

// 版面尺寸（mm）。页面预览的左右留白故意与 @page 的 12mm 对齐，
// 这样屏幕上算出来的「一行能排几遍」和打印出来完全一致。
const PAGE_WIDTH_MM = 210;
const PAGE_PAD_MM = 12;
const NUM_COL_MM = 8;
const COPY_GAP_MM = 3;
/** 保守留白：字宽按整格估、再少算 2mm，宁可少排一遍也不让行溢出 */
const ROW_SAFETY_MM = 2;

const MODEL_COLOR = "#1c1917";
const TRACE_COLOR = "#c9c5be";

function loadOptions(): SheetOptions {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const saved = JSON.parse(raw) as Partial<SheetOptions>;
      // 每一项都过白名单/类型：localStorage 是能被人手改、也留着旧版本的
      return {
        showWord: typeof saved.showWord === "boolean" ? saved.showWord : DEFAULT_OPTIONS.showWord,
        grid: GRID_CHOICES.includes(saved.grid as GridKind)
          ? (saved.grid as GridKind)
          : DEFAULT_OPTIONS.grid,
        cell: CELL_CHOICES.includes(saved.cell as number) ? (saved.cell as number) : DEFAULT_OPTIONS.cell,
        trace: TRACE_CHOICES.includes(saved.trace as number)
          ? (saved.trace as number)
          : DEFAULT_OPTIONS.trace,
      };
    }
  } catch {
    // 坏数据一律回默认
  }
  return DEFAULT_OPTIONS;
}

/** 一行能排几遍这个词：整行铺满，词之间留 COPY_GAP_MM */
function copiesPerRow(charCount: number, cellMm: number): number {
  const usable = PAGE_WIDTH_MM - PAGE_PAD_MM * 2 - NUM_COL_MM - ROW_SAFETY_MM;
  // 连排的字共用一条边（-1px 重叠），宽度近似按整格算
  const wordWidth = charCount * cellMm;
  const fit = Math.floor((usable + COPY_GAP_MM) / (wordWidth + COPY_GAP_MM));
  return Math.max(1, Math.min(MAX_COPIES, fit));
}

/** 这一遍该印什么颜色：第一遍示范字、接下来几遍描红、其余留空自己写 */
function colorOf(copy: number, showWord: boolean, trace: number): string | null {
  const start = showWord ? 1 : 0;
  if (showWord && copy === 0) return MODEL_COLOR;
  if (copy >= start && copy < start + trace) return TRACE_COLOR;
  return null;
}

interface SegmentedProps<T> {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}

function Segmented<T>({ label, value, options, onChange }: SegmentedProps<T>) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-[11px] text-stone-500">{label}</span>
      <div className="flex overflow-hidden rounded-md border border-stone-300 bg-white">
        {options.map((option) => (
          <button
            key={String(option.value)}
            onClick={() => onChange(option.value)}
            className={
              "px-2 py-1 text-xs transition " +
              (option.value === value
                ? "bg-stone-800 font-medium text-white"
                : "text-stone-600 hover:bg-stone-50")
            }
          >
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export default function PrintSheet({ mode, groupId, title, onBack }: PrintSheetProps) {
  const [rows, setRows] = useState<Row[] | null>(null);
  // 字 → 轮廓路径（没有数据的字存 null，渲染时退回普通文本）
  const [glyphs, setGlyphs] = useState<Map<string, string[] | null> | null>(null);
  const [options, setOptions] = useState<SheetOptions>(loadOptions);
  const { notice, notify } = useNotice();

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(options));
  }, [options]);

  useEffect(() => {
    let alive = true;
    setRows(null);
    setGlyphs(null);
    (async () => {
      try {
        let list: Row[] = [];
        if (mode === "mistakes") {
          list = (await api.listMistakes(groupId)).map((m) => ({ id: m.id, text: m.text }));
        } else if (groupId != null) {
          list = (await api.listWords(groupId, true)).map((w) => ({ id: w.id, text: w.text }));
        }
        if (!alive) return;

        // 一次性把整页用到的字取齐：打印是同步的，不能在点「打印」时还有字没加载。
        // 非汉字（标点、字母、数字）本来就没有笔顺数据，别为它们去拉分片。
        const chars = [...new Set(list.flatMap((row) => [...row.text]))];
        const entries = await Promise.all(
          chars.map(
            async (char): Promise<[string, string[] | null]> => [
              char,
              isHanzi(char) ? await loadStrokePaths(char).catch(() => null) : null,
            ],
          ),
        );
        if (!alive) return;
        setRows(list);
        setGlyphs(new Map(entries));
      } catch (e) {
        if (!alive) return;
        notify("error", errorMessage(e));
        setRows([]);
        setGlyphs(new Map());
      }
    })();
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, groupId]);

  const today = new Date().toLocaleDateString("zh-CN");
  // 默写单不描红：整行空白才是「默写」的意思，混着灰字会自相矛盾
  const trace = options.showWord ? options.trace : 0;

  if (!rows || !glyphs) {
    return <div className="flex flex-1 items-center justify-center text-sm text-stone-400">加载中…</div>;
  }

  return (
    <div className="print-root min-h-0 flex-1 overflow-y-auto bg-stone-200/60 pb-10">
      {/* 本页打印边距自声明：@page 无法按 class 作用域隔离，由各打印页自己渲染（口算卷是 margin:0 满幅） */}
      <style>{"@page { size: A4; margin: 12mm }"}</style>

      <div className="no-print mx-auto flex w-[210mm] max-w-full flex-wrap items-center gap-x-3 gap-y-2 px-1 py-3">
        <button
          onClick={onBack}
          className="rounded-md bg-white px-3 py-1.5 text-sm text-stone-600 shadow-sm hover:bg-stone-50"
        >
          返回
        </button>
        <span className="text-sm text-stone-500">共 {rows.length} 个词</span>

        <div className="ml-auto flex flex-wrap items-center gap-2">
          <button
            onClick={() => setOptions((o) => ({ ...o, showWord: !o.showWord }))}
            className={
              "rounded-md px-3 py-1.5 text-sm shadow-sm " +
              (options.showWord ? "bg-stone-800 text-white" : "bg-white text-stone-600 hover:bg-stone-50")
            }
          >
            {options.showWord ? "抄写单" : "默写单"}
          </button>
          <Segmented<GridKind>
            label="方格"
            value={options.grid}
            options={[
              { value: "tian", label: "田" },
              { value: "mi", label: "米" },
              { value: "fang", label: "方" },
            ]}
            onChange={(grid) => setOptions((o) => ({ ...o, grid }))}
          />
          <Segmented<number>
            label="大小"
            value={options.cell}
            options={CELL_CHOICES.map((mm) => ({ value: mm, label: mm + "" }))}
            onChange={(cell) => setOptions((o) => ({ ...o, cell }))}
          />
          {options.showWord && (
            <Segmented<number>
              label="描红"
              value={options.trace}
              options={[
                { value: 0, label: "不描" },
                { value: 1, label: "1 遍" },
                { value: 2, label: "2 遍" },
              ]}
              onChange={(t) => setOptions((o) => ({ ...o, trace: t }))}
            />
          )}
          <button
            onClick={() => printWithToast(".print-page", "字帖-" + title, notify)}
            disabled={rows.length === 0}
            className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-amber-700 disabled:cursor-not-allowed disabled:bg-stone-300"
          >
            打印
          </button>
        </div>
      </div>

      {rows.length === 0 ? (
        <div className="mx-auto flex w-[210mm] max-w-full flex-col items-center gap-2 rounded bg-white px-6 py-20 shadow-md">
          <p className="text-sm text-stone-500">这里还没有可打印的词</p>
          <p className="text-xs text-stone-400">先去默写一次并批改，错词会自动进错题本</p>
        </div>
      ) : (
        <div
          className="print-page mx-auto w-[210mm] bg-white shadow-md"
          style={{ padding: `12mm ${PAGE_PAD_MM}mm` }}
        >
          <header className="mb-5 flex items-end justify-between border-b border-stone-300 pb-3">
            <div>
              <h1 className="text-[22px] font-semibold text-stone-900">{title}</h1>
              <p className="mt-1 text-[11px] text-stone-500">
                共 {rows.length} 个词 · {today}
              </p>
            </div>
            <p className="text-[11px] text-stone-500">姓名 __________　日期 __________</p>
          </header>

          {rows.map((row, index) => {
            const chars = [...row.text];
            // 抄写单至少留一编空格：长词一排只放得下一遍时，那一遍是示范字，
            // 否则整行一个空位都没有，「照着写」就成了空话（挤不下就折行）
            const copies = Math.max(
              options.showWord ? trace + 2 : 1,
              copiesPerRow(chars.length, options.cell),
            );
            return (
              <div key={row.id} className="print-row flex items-center" style={{ marginBottom: "3mm" }}>
                <span
                  className="shrink-0 pr-1 text-right text-[11px] tabular-nums text-stone-400"
                  style={{ width: NUM_COL_MM + "mm" }}
                >
                  {index + 1}
                </span>
                <div className="flex flex-wrap items-center" style={{ gap: COPY_GAP_MM + "mm" }}>
                  {Array.from({ length: copies }).map((_, copy) => {
                    const color = colorOf(copy, options.showWord, trace);
                    return (
                      <div key={copy} className="flex flex-wrap">
                        {chars.map((char, i) => (
                          <span
                            key={i}
                            className="ge"
                            style={{
                              width: options.cell + "mm",
                              height: options.cell + "mm",
                              // 连排的字共用一条边，别让虚线叠成两像素
                              marginLeft: i > 0 ? "-1px" : undefined,
                            }}
                          >
                            <GridLines kind={options.grid} />
                            {color && (
                              <StrokeGlyph
                                paths={glyphs.get(char)}
                                color={color}
                                char={char}
                                fallbackSize={options.cell * 0.72 + "mm"}
                              />
                            )}
                          </span>
                        ))}
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}

          <p className="mt-6 text-center text-[10px] text-stone-400">
            生词默写 ·{" "}
            {options.showWord
              ? "照着示范字写：先描红，剩下的空格自己写"
              : "家长读词，孩子在空格里默写，写完对照错题本"}
          </p>
        </div>
      )}

      <Toast notice={notice} />
    </div>
  );
}
