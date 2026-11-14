/** 字典数据（部首/结构/释义）加载：public/dict/ 是 `scripts/sync-dict-data.mjs`
 *  打出的 64 个分片（码点 % 64），随安装包发、运行时不联网——与 lib/strokes.ts
 *  同一套约定：分片 Promise 常驻内存、失败不缓存。查不到的字返回 null（页面显示「—」）。 */

export interface DictEntry {
  /** 部首（仅常见字有；生僻字为 null） */
  radical: string | null;
  /** 教学结构标签（左右结构/上下结构/…；罕见复合结构为 null） */
  structure: string | null;
  /** 释义（新华字典风格原文，可能较长、含换行） */
  explanation: string | null;
}

type ShardEntry = { r?: string; s?: string; y?: string };
type Shard = Record<string, ShardEntry>;

const SHARDS = 64;

const shards = new Map<number, Promise<Shard>>();

function dataRoot(): string {
  // 与 lib/strokes.ts 同款：必须相对「站点根」，理由见那边注释（pitfalls 第 24 条）
  return import.meta.env.BASE_URL.endsWith("/")
    ? import.meta.env.BASE_URL
    : import.meta.env.BASE_URL + "/";
}

function loadShard(index: number): Promise<Shard> {
  const hit = shards.get(index);
  if (hit) return hit;

  const url = dataRoot() + "dict/s" + index.toString(16).padStart(2, "0") + ".json";
  const pending = (async () => {
    const res = await fetch(url);
    if (!res.ok) throw new Error("取不到字典数据（" + url + "）");
    const data = (await res.json()) as Shard;
    if (typeof data !== "object" || data === null) throw new Error("字典数据格式不对（" + url + "）");
    return data;
  })();

  shards.set(index, pending);
  pending.catch(() => shards.delete(index));
  return pending;
}

/** 查一个字的部首/结构/释义；查不到（或分片读不出）返回 null，不抛错 */
export async function loadDictEntry(char: string): Promise<DictEntry | null> {
  const codePoint = char.codePointAt(0) ?? 0;
  try {
    const shard = await loadShard(codePoint % SHARDS);
    const e = shard[codePoint.toString(16)];
    if (!e) return null;
    return { radical: e.r ?? null, structure: e.s ?? null, explanation: e.y ?? null };
  } catch {
    // 理论上不会发生（数据随包发）；真发生了当作查无此字，别让整页报错
    return null;
  }
}
