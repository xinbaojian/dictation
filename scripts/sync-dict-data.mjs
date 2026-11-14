// 把「部首 / 结构 / 释义」字典数据打包成 64 个分片，供前端 lib/dict.ts 按字懒加载。
// 源两份：
//   scripts/dict-src/xinhua.json   —— 新华字典精简源（部首 + 释义，见该目录 README）
//   chinese-characters-decomposition（npm，MIT；Wikimedia Commons CCD 表）—— 结构类型 + 部件
// 笔画数与拼音不进本数据：笔画数取笔顺数据（strokes.length，精确），拼音用 pinyin-pro 前端转换。
//
// 结构标签的来历：CCD 的 compositionType 是几何拆分（吅=横二分、吕=竖二分、回=包围），
// 与教学口径不一致——「病」是吕型（疒+丙 上下叠）但教学叫左上包围，「远」是吅型但教学叫
// 左下包围。所以除了符号映射，还要看外框部件（辶/疒/门/囗/凵/匚/勹…）再定标签。
//
// public/dict/ 是生成物，不进版本库；predev / prebuild 自动跑（缺源会直接失败）。
// 手动重跑：node scripts/sync-dict-data.mjs

import { existsSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const XINHUA = join(root, "scripts", "dict-src", "xinhua.json");
const CCD = join(root, "node_modules", "chinese-characters-decomposition", "ccd.json");
const DEST = join(root, "public", "dict");
const STAMP = join(DEST, ".stamp");
const SHARDS = 64;
// 分片口径变了就改这个后缀，脚本自动重刷
const FORMAT = "-s64a";

if (!existsSync(XINHUA)) {
  console.error("[dict] 缺少 scripts/dict-src/xinhua.json（vendor 源，见该目录 README）");
  process.exit(1);
}
if (!existsSync(CCD)) {
  console.error("[dict] 缺少依赖 chinese-characters-decomposition，请先执行 npm install");
  process.exit(1);
}

const ccdVersion = JSON.parse(
  readFileSync(join(root, "node_modules", "chinese-characters-decomposition", "package.json"), "utf8"),
).version;
const stamp = `xinhua-20261114+ccd-${ccdVersion}${FORMAT}`;

// 跳过条件同时看 stamp 与产物在不在（上一次跑挂了不能当成功放行）
const existing = existsSync(DEST)
  ? readdirSync(DEST).filter((n) => /^s[0-9a-f]{2}\.json$/.test(n)).length
  : 0;
if (existsSync(STAMP) && readFileSync(STAMP, "utf8").trim() === stamp && existing === SHARDS) {
  console.log(`[dict] 字典数据已同步（${stamp}），跳过`);
  process.exit(0);
}

// ── 结构标签：符号 + 外框部件规则表 ──────────────────────────────
// top/left/bottom/right 均为 CCD 的部件串（吕型: top/bottom；吅型: left/right；回型: outer/inner）
const LEFT_TOP_ENCLOSURE = ["疒", "户", "虍", "广"]; // 左上包围（外框在左上）
const BOTTOM_LEFT_ENCLOSURE = ["辶", "廴"]; // 左下包围（走之底托右）
const WRAP_FULL = ["囗"]; // 全包围
const WRAP_TOP = ["门", "冂"]; // 上三包围
const WRAP_BOTTOM = ["凵"]; // 下三包围
const WRAP_LEFT = ["匚"]; // 左三包围
const WRAP_RIGHT = ["勹", "气"]; // 右上包围
const startsWithAny = (s, list) => list.some((c) => (s ?? "").startsWith(c));

function structureLabel(type, a, b) {
  switch (type) {
    case "一":
    case "+": // 交叉重叠（中、串、丸）：教学按独体处理
      return "独体字";
    case "冖": // 上盖下（亮、亭、党）
      return "上下结构";
    case "吕":
      if (startsWithAny(a, LEFT_TOP_ENCLOSURE)) return "左上包围";
      if (b === "凵") return "下三包围"; // 画
      return "上下结构";
    case "吅":
      if (startsWithAny(a, BOTTOM_LEFT_ENCLOSURE)) return "左下包围"; // 远、这、建
      return "左右结构";
    case "回": {
      if (startsWithAny(a, WRAP_FULL)) return "全包围";
      if (startsWithAny(a, WRAP_TOP)) return "上三包围";
      if (startsWithAny(a, WRAP_BOTTOM)) return "下三包围";
      if (startsWithAny(a, WRAP_LEFT)) return "左三包围";
      if (startsWithAny(a, WRAP_RIGHT)) return "右上包围";
      return "半包围";
    }
    case "咒":
      return "上中下";
    case "品":
      return "左中右";
    default:
      return ""; // 叕 / 弼 / * 等罕见复合结构：不显示
  }
}

// ── 合并两个源 ──────────────────────────────────────────────
const xinhua = JSON.parse(readFileSync(XINHUA, "utf8"));
const ccd = JSON.parse(readFileSync(CCD, "utf8"));
// ccd.json 是 { headers, rows }；列序：0 component 1 strokes 2 compositionType
//               3 leftComponent 5 rightComponent（见包 README）
const entries = new Map(); // codepoint -> { r?, s?, y? }
for (const [ch, e] of Object.entries(xinhua)) {
  const cp = ch.codePointAt(0);
  if (cp === undefined) continue;
  entries.set(cp, { ...e });
}
let ccdUsed = 0;
// xinhua 没有的字也收（只要结构）；有的字不动它已有的 r/y
for (const row of ccd.rows) {
  const cp = row[0].codePointAt(0);
  const label = structureLabel(row[2], row[3], row[5]);
  if (!label) continue;
  const prev = entries.get(cp) ?? {};
  entries.set(cp, { ...prev, s: label });
  ccdUsed += 1;
}

// 先全部算完再动 DEST：中途失败不删上一版可用产物
const shards = Array.from({ length: SHARDS }, () => ({}));
let bytes = 0;
for (const [cp, e] of entries) {
  if (Object.keys(e).length === 0) continue;
  shards[cp % SHARDS][cp.toString(16)] = e;
}

mkdirSync(DEST, { recursive: true });
for (const name of readdirSync(DEST)) {
  if (/^s[0-9a-f]{2}\.json$/.test(name)) unlinkSync(join(DEST, name));
}
shards.forEach((bucket, i) => {
  const body = JSON.stringify(bucket);
  bytes += body.length;
  writeFileSync(join(DEST, "s" + i.toString(16).padStart(2, "0") + ".json"), body);
});

writeFileSync(
  join(DEST, "SOURCE.md"),
  `字典数据来源：\n- 部首/释义：chinese-xinhua（社区抓取整理，无明确授权，仅个人学习用途）\n- 结构：chinese-characters-decomposition（MIT），原始数据出自 Wikimedia Commons CCD 表\n生成脚本：scripts/sync-dict-data.mjs\n`,
);
writeFileSync(STAMP, stamp + "\n");
console.log(
  `[dict] 已生成 ${entries.size} 个字（结构 ${ccdUsed} 条）/ ${SHARDS} 个分片（${(bytes / 1024 / 1024).toFixed(1)}MB）-> public/dict/`,
);
