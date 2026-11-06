/** 笔画数据加载：数据由 `scripts/sync-hanzi-data.mjs` 打成 256 个分片放进
 *  public/hanzi/，随安装包一起发出去，运行时不联网（与「唯一联网行为是自动升级」
 *  的约定一致）。分片规则是「码点 % 256」，前后端各算一次，不需要清单文件。 */

import type { CharacterJson } from "hanzi-writer";

/** 汉字（含扩展 A 与扩展 B 之后的补充平面）——只有这些字才有笔顺数据 */
const HANZI = /[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]|[\u{20000}-\u{2FA1F}]/u;

const SHARDS = 256;

export function isHanzi(char: string): boolean {
  return HANZI.test(char);
}

type Shard = Record<string, CharacterJson>;

/** 一个分片约 37 个字，同一个词里的字多半落在相邻的几片，加载后常驻内存 */
const shards = new Map<number, Promise<Shard>>();

function dataRoot(): string {
  // 必须相对「站点根」而不是 document.baseURI：后者会把路径挂到当前页面所在
  // 目录下（index.html 在根目录时两者等价，换个入口就 404，且 dev server 的
  // SPA 兜底还会返回 HTML + 200，看起来像「数据坏了」）。
  return import.meta.env.BASE_URL.endsWith("/")
    ? import.meta.env.BASE_URL
    : import.meta.env.BASE_URL + "/";
}

function loadShard(index: number): Promise<Shard> {
  const hit = shards.get(index);
  if (hit) return hit;

  const url = dataRoot() + "hanzi/s" + index.toString(16).padStart(2, "0") + ".json";
  const pending = (async () => {
    const res = await fetch(url);
    if (!res.ok) throw new Error("取不到笔画数据（" + url + "）");
    const data = (await res.json()) as Shard;
    // 路径写错时 dev server 的 SPA 兜底会返回 index.html（状态码还是 200），
    // 这里挡一下，免得 hanzi-writer 抛一个看不懂的解析错误
    if (typeof data !== "object" || data === null) throw new Error("笔画数据格式不对（" + url + "）");
    return data;
  })();

  shards.set(index, pending);
  // 失败不留在缓存里：下次翻回这个词还能重试
  pending.catch(() => shards.delete(index));
  return pending;
}

export async function loadStrokeData(char: string): Promise<CharacterJson> {
  const codePoint = char.codePointAt(0) ?? 0;
  const shard = await loadShard(codePoint % SHARDS);
  const data = shard[codePoint.toString(16)];
  if (!data || !Array.isArray(data.strokes) || data.strokes.length === 0) {
    throw new Error("「" + char + "」没有笔画数据");
  }
  return data;
}

/** 只要轮廓路径（SVG path d 列表）：打印单拿它把字画成实心字形，
 *  这样「楷体示范 / 描红」不依赖系统装没装楷体 */
export async function loadStrokePaths(char: string): Promise<string[]> {
  const data = await loadStrokeData(char);
  return data.strokes;
}
