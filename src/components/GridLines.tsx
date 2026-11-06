/** 方格辅助线：十字（田字格）/ 十字 + 对角线（米字格）。
 *
 *  为什么不画在 CSS 的 background-image 里：打印时浏览器的「背景图形」默认是关的，
 *  渐变画的线不会出现在纸上——屏幕和打印预览里都看得见，唯独出纸后没了，
 *  而这两个选项（田 / 米 / 方）的意义全在这几条线上。SVG 描边属于前景内容，
 *  一定跟着打印；顺带和描红字形走同一条 DOM → outerHTML → 打印的路径。 */

export type GridKind = "tian" | "mi" | "fang";

const TIAN = "M50 0V100M0 50H100";
const MI = "M50 0V100M0 50H100M0 0L100 100M100 0L0 100";

interface GridLinesProps {
  kind: GridKind;
}

export default function GridLines({ kind }: GridLinesProps) {
  if (kind === "fang") return null;
  return (
    <svg
      className="pointer-events-none absolute inset-0"
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      aria-hidden
    >
      <path
        d={kind === "tian" ? TIAN : MI}
        fill="none"
        stroke="#e7e5e4"
        strokeWidth={1}
        // 格子尺寸是 mm 换算出来的，线宽要保持 1px，不能被 viewBox 拉伸
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
