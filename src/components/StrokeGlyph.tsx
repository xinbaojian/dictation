import HanziWriter from "hanzi-writer";

/** 字形的虚拟画布（viewBox 边长）；实际毫米尺寸由外层 .ge 决定，SVG 撑满即可 */
const VIEW = 100;
/** 留白：字帖里的字占格子八成左右，贴边会显得挤，也不方便描红 */
const PADDING = 10;

// 笔顺数据的坐标系是 1024 见方、y 轴朝上；用 hanzi-writer 自己的换算保证
// 和复习页的动画字形完全一致（含 y 翻转），不要手写这个矩阵。
const TRANSFORM = HanziWriter.getScalingTransform(VIEW, VIEW, PADDING).transform;

interface StrokeGlyphProps {
  /** `loadStrokePaths()` 的结果；null/undefined 表示这个字没有笔画数据 */
  paths: string[] | null | undefined;
  color: string;
  /** 没有笔画数据时退回普通文本（字母、数字、生僻字） */
  char: string;
  /** 退回文本时的字号，按格子大小传，避免撑破格子 */
  fallbackSize: string;
}

/** 把单个汉字渲染成实心字形：打印单里的「示范字」与「描红」都用它 */
export default function StrokeGlyph({ paths, color, char, fallbackSize }: StrokeGlyphProps) {
  if (!paths || paths.length === 0) {
    return (
      <span
        className="pointer-events-none absolute inset-0 flex items-center justify-center leading-none"
        style={{ fontSize: fallbackSize, color }}
      >
        {char}
      </span>
    );
  }
  return (
    <svg
      viewBox={`0 0 ${VIEW} ${VIEW}`}
      width="100%"
      height="100%"
      className="pointer-events-none absolute inset-0"
      aria-hidden
    >
      <g transform={TRANSFORM}>
        {paths.map((d, i) => (
          <path key={i} d={d} fill={color} />
        ))}
      </g>
    </svg>
  );
}
