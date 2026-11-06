// 拼音：pinyin-pro 纯离线转换（内置词典，多音字按词消歧）。
// 这是 ADR-1 的修订范围：只在笔画复习页做「显示」用途，拼音永不进数据库、
// 导入解析也仍然不处理拼音（见 wiki/decisions.md ADR-1 / ADR-19）。
import { pinyin } from "pinyin-pro";

export interface PinyinChunk {
  /** 原文片段（isZh 时恰为一个汉字） */
  origin: string;
  /** 带调拼音；非汉字为 null */
  pinyin: string | null;
  isZh: boolean;
}

/**
 * 把词语按「汉字逐个、非汉字连续段整块」切开并标注拼音。
 * 必须整词一次性转换——逐字转换会把多音字按单字常用音读错（「长大」的 cháng/zhǎng）。
 */
export function pinyinChunks(text: string): PinyinChunk[] {
  const results = pinyin(text, { type: "all", toneType: "symbol", nonZh: "consecutive" });
  // 文档未承诺单字输入一定返回数组，防御一下（实测 v3 是数组）
  const arr = Array.isArray(results) ? results : [results];
  return arr.map((r) => ({
    origin: r.origin,
    pinyin: r.isZh ? r.pinyin : null,
    isZh: r.isZh,
  }));
}

/**
 * 与 `[...text]` 逐字符对齐的拼音数组：每个汉字一项，非汉字（标点/字母）为空串。
 * 遮住模式下拼音仍显示——读音提示不泄露字形，正好配合「先自己写」。
 */
export function charPinyins(text: string): string[] {
  const out: string[] = [];
  for (const chunk of pinyinChunks(text)) {
    if (chunk.isZh) {
      out.push(chunk.pinyin ?? "");
    } else {
      // 非汉字连续段：按字符摊开，每项占位空串，保持与田字格一一对应
      const chars = [...chunk.origin];
      out.push(...chars.map(() => ""));
    }
  }
  return out;
}
