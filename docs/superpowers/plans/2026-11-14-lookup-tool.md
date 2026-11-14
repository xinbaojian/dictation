# 查字工具实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 第三个顶栏工具「查字」：家长输入字词 → 逐字笔顺动画 + 每字部首/笔画/结构/释义（本地离线字典），AI 可按字生成讲解并缓存。

**Architecture:** 字典数据走与笔顺数据同款的「构建期脚本分片 + 前端懒加载」管线（新脚本 `sync-dict-data.mjs`，源为 vendor 的新华字典精简数据 + CCD npm 包）；AI 单字讲解沿用 ADR-20 的 OpenAI 兼容客户端，新 domain `char_card` + 新表 `char_cards`（按字文本缓存，随整库快照同步）；前端新页面复用 `StrokeWord` 组件。

**Tech Stack:** Rust + Tauri 2 + rusqlite；React 18 + TypeScript + Vite + Tailwind CSS 4；数据源 `chinese-characters-decomposition@0.1.0`（npm，MIT）+ chinese-xinhua word.json（vendor 精简）。

**设计文档:** `docs/superpowers/specs/2026-11-14-lookup-tool-design.md`

## Global Constraints

- 全部面向用户的文案用中文；错误信息走 `AppError`（序列化成中文提示）。
- 迁移只增不改：新迁移编号 **0007**（0006 已被 ai_cards 占用）；`db/mod.rs::MIGRATIONS` 追加，历史文件绝不动。
- 学习进度只能经 `repo/word_progress.rs` 修改——本工具**完全不读写进度**。
- 前端不拼 SQL、不绕过 `src/lib/ipc.ts`；新后端能力 = 命令 + `lib.rs` 注册 + `ipc.ts` 方法 + `types.ts` 类型。
- 静态数据 URL 一律用 `import.meta.env.BASE_URL` 拼站点根（pitfalls 第 24 条），不用 `document.baseURI`。
- LLM 输出必须过 `domain` 层 `validate` 才入库；AI 凭据只存数据目录 `.ai-config.json`，不进 `settings` 表。
- 网络命令 `async fn` + `spawn_blocking`，`MutexGuard` 不跨 await（锁编排照抄 `generate_word_card`）。
- 时间戳一律 SQLite `strftime('%Y-%m-%dT%H:%M:%fZ','now')`；前端 camelCase ↔ Rust snake_case 由 Tauri 自动转换。
- 生成物 `public/dict/` 不进库（gitignore）；源数据 `scripts/dict-src/` 进库。
- Rust 禁用 `char` 作参数名（关键字），命令参数用 `character`。

---

### Task 1: vendor 字典源数据（xinhua 精简源）

**Files:**
- Create: `scripts/dict-src/xinhua.json`（vendor 产物，~6MB）
- Create: `scripts/dict-src/README.md`
- 已完成（本仓库 HEAD 已含）: `package.json` devDependency `chinese-characters-decomposition@^0.1.0`

**Interfaces:**
- Produces: `scripts/dict-src/xinhua.json` 形状 `Record<string, { r?: string; y?: string }>`——键是单字，`r`=部首（仅当原始 radicals 恰好一个字符），`y`=释义原文（可能含换行）。Task 2 的脚本消费它。

- [ ] **Step 1: 确认 CCD 依赖在位**

```bash
node -e "console.log(require('chinese-characters-decomposition/package.json').version)"
```

Expected: `0.1.0`（若空则 `npm install --save-dev chinese-characters-decomposition`）

- [ ] **Step 2: 下载原始 word.json 并精简**

```bash
curl -sL -o /tmp/xinhua-word.json https://raw.githubusercontent.com/pwxcoo/chinese-xinhua/master/data/word.json
node -e '
const fs = require("fs");
const src = JSON.parse(fs.readFileSync("/tmp/xinhua-word.json", "utf8"));
const out = {};
let kept = 0, withRadical = 0;
for (const e of src) {
  const w = (e.word ?? "").trim();
  if ([...w].length !== 1) continue;                 // 只要单字条目
  const radical = [...(e.radicals ?? "")];
  const r = radical.length === 1 ? radical[0] : undefined;   // 部首仅当恰好一个字符
  const y = (e.explanation ?? "").trim() || undefined;
  if (!r && !y) continue;                            // 部首释义全无的条目不要
  const prev = out[w];
  if (!prev) {                                       // 首次见到这个字：建条目
    const entry = {};
    if (r) { entry.r = r; withRadical++; }
    if (y) entry.y = y;
    out[w] = entry;
    kept++;
    continue;
  }
  // 上游有 1332 个单字重复条目：填空式合并——只补缺失字段，
  // 首个非空 r / y 胜出，绝不用后来的空值或重复条目覆盖已有值
  // （末条覆盖会让 83 个字丢掉部首，见 Task 1 报告）。
  if (r && !prev.r) { prev.r = r; withRadical++; }
  if (y && !prev.y) { prev.y = y; }
}
fs.mkdirSync("scripts/dict-src", { recursive: true });
fs.writeFileSync("scripts/dict-src/xinhua.json", JSON.stringify(out));
console.log("kept:", kept, "withRadical:", withRadical);
'
```

Expected: `kept: ~14800`（唯一字数）、`withRadical: ~7093`（键级带部首数）；上游快照若有小幅变动允许 ±1 漂移。

- [ ] **Step 3: 验证产物**

```bash
node -e '
const d = require("./scripts/dict-src/xinhua.json");
console.log("碧.r =", d["碧"].r, "| 释义含青绿:", d["碧"].y.includes("青绿"));
console.log("绿.r =", d["绿"].r, "| 的.r =", d["的"].r, "| 回.r =", d["回"].r);
console.log("薭.r =", d["薭"].r, "| 重复条目合并后释义非空:", d["薭"].y.length > 0);
' && ls -la scripts/dict-src/xinhua.json
```

Expected: `碧.r = 石 | 释义含青绿: true`、`绿.r = 纟`、`的.r = 白`、`回.r = 囗`，文件 < 10MB；`薭.r = 艹`（该字上游有两条重复条目、末条无部首，是「填空式合并」的回归点）。

- [ ] **Step 4: 写 `scripts/dict-src/README.md`**

```markdown
# 字典源数据（vendor）

`xinhua.json`：新华字典字级数据精简版，键是单字，值 `{ r?: 部首, y?: 释义 }`。

- 上游：https://github.com/pwxcoo/chinese-xinhua 的 `data/word.json`（16142 条）
- 精简规则：只留单字条目；部首仅当原始 `radicals` 恰好一个字符；释义去首尾空白保留原文。
- 抓取时间：2026-11-14。上游数据为社区抓取整理、无明确授权，**仅作个人学习用途**；
  若日后收到侵权通知应删除并改换数据源。
- 重新生成：见仓库根 `docs/superpowers/plans/2026-11-14-lookup-tool.md` Task 1 的命令。
- 消费方：`scripts/sync-dict-data.mjs`（打成 public/dict/ 分片）。
- 上游 word.json sha256：8ae3453eacc5b0f3fdfba47eac8bb686cd73914d278e8b851ae6ef81082f80e7
```

- [ ] **Step 5: Commit**

```bash
git add scripts/dict-src/
git commit -m "feat(lookup): vendor 新华字典精简源数据（部首/释义）"
```

---

### Task 2: 字典分片脚本 `sync-dict-data.mjs`

**Files:**
- Create: `scripts/sync-dict-data.mjs`
- Modify: `package.json`（predev / prebuild）
- Modify: `.gitignore`（`public/dict/`）

**Interfaces:**
- Consumes: `scripts/dict-src/xinhua.json`（Task 1）；`node_modules/chinese-characters-decomposition/ccd.json`（`{ headers, rows }`，rows 每行 `[component, strokes, compositionType, leftComponent, leftStrokes, rightComponent, rightStrokes, signature, notes, section]`）
- Produces: `public/dict/s00…s3f.json` 共 64 片（码点 % 64，片内键 = 码点 hex），条目 `{ r?, s?, y? }`；Task 3 的 `loadDictEntry` 消费。

- [ ] **Step 1: 写脚本**

```js
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
for (const row of ccd.rows) {
  const cp = row[0].codePointAt(0);
  if (cp === undefined || entries.has(cp)) {
    // xinhua 没有的字也收（只要结构）；有的字不动它已有的 r/y
  }
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
```

注意上面 `for (const row of ccd.rows)` 里有一段写了但没用的空 if（`if (cp === undefined || entries.has(cp))`）——写最终版时**删掉这个空分支**，直接 `const cp = ...; const label = ...`。（此处特意标注，防抄错。）

- [ ] **Step 2: 挂进 pre 钩子与 gitignore**

`package.json`：

```json
"predev": "node scripts/sync-hanzi-data.mjs && node scripts/sync-dict-data.mjs",
"prebuild": "node scripts/sync-hanzi-data.mjs && node scripts/sync-dict-data.mjs",
```

`.gitignore` 把 `public/hanzi/` 那行的注释与条目改为：

```
# 笔画数据是从 node_modules/hanzi-writer-data 生成出来的（scripts/sync-hanzi-data.mjs）
public/hanzi/
# 字典数据（部首/结构/释义）生成物（scripts/sync-dict-data.mjs）
public/dict/
```

- [ ] **Step 3: 跑脚本并抽查**

```bash
node scripts/sync-dict-data.mjs
node -e '
const fs = require("fs");
const shard = (ch) => JSON.parse(fs.readFileSync(`public/dict/s${(ch.codePointAt(0) % 64).toString(16).padStart(2, "0")}.json`, "utf8"))[ch.codePointAt(0).toString(16)];
const expect = { "碧": { s: "上下结构", r: "石" }, "绿": { s: "左右结构", r: "纟" }, "远": { s: "左下包围" }, "问": { s: "上三包围" }, "病": { s: "左上包围" }, "回": { s: "全包围", r: "囗" }, "中": { s: "独体字" }, "亮": { s: "上下结构" }, "凶": { s: "下三包围" }, "区": { s: "左三包围" } };
let bad = 0;
for (const [ch, want] of Object.entries(expect)) {
  const got = shard(ch) ?? {};
  const sOk = got.s === want.s, rOk = want.r === undefined || got.r === want.r;
  if (!sOk || !rOk) { bad++; console.log("FAIL", ch, "got", JSON.stringify(got)); }
}
if (bad) process.exit(1);
console.log("抽查 10 字全部通过");
'
```

Expected: `[dict] 已生成 …` + `抽查 10 字全部通过`

- [ ] **Step 4: 验证 stamp 幂等**

```bash
node scripts/sync-dict-data.mjs
```

Expected: `[dict] 字典数据已同步（…），跳过`

- [ ] **Step 5: Commit**

```bash
git add scripts/sync-dict-data.mjs package.json .gitignore
git commit -m "feat(lookup): 字典数据分片脚本（部首/结构/释义，64 分片随包发）"
```

---

### Task 3: 前端加载器 `src/lib/dict.ts`

**Files:**
- Create: `src/lib/dict.ts`

**Interfaces:**
- Consumes: `public/dict/sXX.json`（Task 2）
- Produces: `loadDictEntry(char: string): Promise<DictEntry | null>`；`DictEntry { radical: string | null; structure: string | null; explanation: string | null }`（Task 7 页面消费）

- [ ] **Step 1: 写 `src/lib/dict.ts`**

```ts
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
```

- [ ] **Step 2: 类型检查**

```bash
npm run typecheck
```

Expected: 无错误。

- [ ] **Step 3: Commit**

```bash
git add src/lib/dict.ts
git commit -m "feat(lookup): 字典数据前端加载器（64 分片懒加载）"
```

---

### Task 4: 后端领域层 `domain/char_card.rs`（TDD）

**Files:**
- Create: `src-tauri/src/domain/char_card.rs`
- Modify: `src-tauri/src/domain/mod.rs`（加 `pub mod char_card;`）

**Interfaces:**
- Consumes: `domain::ai_card::{ChatMessage, strip_code_fence}`（已存在，pub）
- Produces: `CharContext { char: String }`；`CharCardContent { meaning, mnemonic, words: Vec<String>, sentences: Vec<String> }`（serde camelCase）；`build_messages(ctx) -> Vec<ChatMessage>`；`validate(raw: &str, ch: &str) -> AppResult<CharCardContent>`——Task 5/6 消费。

- [ ] **Step 1: 写失败测试（文件底部 `#[cfg(test)] mod tests`，与实现同文件，风格照 `domain/ai_card.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_good_payload() {
        let raw = r#"{"meaning":"碧就是青绿色的玉石","mnemonic":"王爷家的白玉石，青绿青绿真好看","words":["碧绿","碧玉"],"sentences":["湖水碧绿得像一块玉石。"]}"#;
        let c = validate(raw, "碧").unwrap();
        assert_eq!(c.meaning, "碧就是青绿色的玉石");
        assert_eq!(c.words.len(), 2);
        assert_eq!(c.sentences.len(), 1);
    }

    #[test]
    fn strips_code_fence_and_parses() {
        let raw = "```json\n{\"meaning\":\"字义\",\"mnemonic\":\"口诀\",\"words\":[\"碧绿\"],\"sentences\":[\"湖水碧绿。\"]}\n```";
        assert!(validate(raw, "碧").is_ok());
    }

    #[test]
    fn rejects_broken_json() {
        assert!(validate("我不是 JSON", "碧").unwrap_err().to_string().contains("JSON"));
    }

    #[test]
    fn rejects_empty_or_long_meaning() {
        let base = |m: &str| {
            format!(r#"{{"meaning":"{m}","mnemonic":"口诀","words":["碧绿"],"sentences":["湖水碧绿。"]}}"#)
        };
        assert!(validate(&base(""), "碧").is_err());
        let long = "长".repeat(61);
        assert!(validate(&base(&long), "碧").is_err());
    }

    #[test]
    fn rejects_terms_in_mnemonic_and_sentence() {
        let raw = r#"{"meaning":"字义","mnemonic":"碧的部首是石","words":["碧绿"],"sentences":["湖水碧绿。"]}"#;
        assert!(validate(raw, "碧").unwrap_err().to_string().contains("术语"));
        let raw2 = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿"],"sentences":["碧绿的笔顺很长。"]}"#;
        assert!(validate(raw2, "碧").unwrap_err().to_string().contains("术语"));
    }

    #[test]
    fn rejects_sentence_missing_char() {
        let raw = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿"],"sentences":["今天天气很好。"]}"#;
        assert!(validate(raw, "碧").unwrap_err().to_string().contains("碧"));
    }

    #[test]
    fn rejects_bad_words() {
        let base = |w: &str| {
            format!(r#"{{"meaning":"字义","mnemonic":"口诀","words":{w},"sentences":["湖水碧绿。"]}}"#)
        };
        assert!(validate(&base(r#"["绿"]"#), "碧").is_err(), "组词必须含该字");
        assert!(validate(&base(r#"["碧绿"]"#), "碧").is_err(), "至少 2 个组词");
        assert!(
            validate(&base(r#"["碧绿","碧玉","碧空","碧海","碧波"]"#), "碧").is_err(),
            "最多 4 个组词"
        );
    }

    #[test]
    fn rejects_newlines_and_bad_counts() {
        let raw = "{\"meaning\":\"字义\",\"mnemonic\":\"口\\n诀\",\"words\":[\"碧绿\",\"碧玉\"],\"sentences\":[\"湖水碧绿。\"]}";
        assert!(validate(raw, "碧").is_err());
        let raw2 = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿","碧玉"],"sentences":["湖水碧绿。","碧空如洗。","太多了。"]}"#;
        assert!(validate(raw2, "碧").is_err());
    }

    #[test]
    fn build_messages_carries_char() {
        let msgs = build_messages(&CharContext { char: "碧".into() });
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("二年级"));
        assert!(msgs[1].content.contains("碧"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test char_card
```

Expected: 编译失败（`char_card` 模块不存在）。先在 `domain/mod.rs` 加 `pub mod char_card;` 并建空文件再跑，预期 0 个测试通过/编译错误。

- [ ] **Step 3: 写实现**

```rust
//! 查字工具的 AI 字卡领域逻辑：prompt 构造 + LLM 输出校验（纯函数，不依赖 Tauri / DB）。
//! 与 ai_card.rs 同一套路，但对象是「任意单个汉字」（不要求在词表里），缓存键是字本身。
use serde::{Deserialize, Serialize};

use crate::domain::ai_card::{strip_code_fence, ChatMessage};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharCardContent {
    /// 字义 / 来历小故事（讲给孩子听）
    pub meaning: String,
    /// 巧记口诀
    pub mnemonic: String,
    /// 含该字的常用词语 2~4 个
    pub words: Vec<String>,
    /// 例句 1~2 句，每句包含该字
    pub sentences: Vec<String>,
}

/// 一次字卡生成的上下文（查字工具没有错题信息，只有字本身）
pub struct CharContext {
    pub char: String,
}

const SYSTEM_PROMPT: &str = "你是给中国小学二年级孩子（7~8 岁）讲语文的老师，家长刚问你这个字怎么写、怎么记。请严格按以下要求输出：
1. meaning：用一两句话给孩子讲这个字的意思或来历，不超过 60 个字，比喻只用孩子熟悉的事物（动物、学校、家里、玩具、食物）。
2. mnemonic：一句能念出声的巧记口诀，不超过 40 个字，朗朗上口。
3. words：2~4 个含有这个字的常用词语，每个 2~4 个字。
4. sentences：1~2 个例句，每句不超过 40 个字，必须完整包含这个字，写二年级孩子的生活场景，用词不超纲。
5. 口诀和例句里不许出现这些词：形近字、音近字、部首、音序、笔顺、笔画、偏旁。所有内容都不要换行。
6. 只输出一个 JSON 对象，不要输出 JSON 以外的任何文字，格式示例：
{\"meaning\":\"碧就是青绿色的玉石，像天空和大海的颜色\",\"mnemonic\":\"王爷家的白玉石，青绿青绿真好看\",\"words\":[\"碧绿\",\"碧玉\",\"碧空\"],\"sentences\":[\"湖水碧绿得像一块玉石。\"]}";

/// 课堂术语黑名单：口诀和例句是孩子直接念的（与 ai_card 同一约束）
const TERMS: [&str; 7] = ["形近字", "音近字", "部首", "音序", "笔顺", "笔画", "偏旁"];

pub fn build_messages(ctx: &CharContext) -> Vec<ChatMessage> {
    vec![
        ChatMessage { role: "system".into(), content: SYSTEM_PROMPT.into() },
        ChatMessage {
            role: "user".into(),
            content: format!("字：{}", ctx.char),
        },
    ]
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn find_term(text: &str) -> Option<&'static str> {
    TERMS.iter().find(|t| text.contains(*t)).copied()
}

/// LLM 输出的唯一入库关卡：解析 → 规整 → 校验。文案要能回喂 LLM 也能直接给家长看。
pub fn validate(raw: &str, ch: &str) -> AppResult<CharCardContent> {
    let clean = strip_code_fence(raw);
    let mut c: CharCardContent = serde_json::from_str(clean)
        .map_err(|_| AppError::msg("AI 输出不是合法的 JSON（缺字段或格式不对）"))?;
    c.meaning = c.meaning.trim().to_string();
    c.mnemonic = c.mnemonic.trim().to_string();
    c.words = c.words.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    c.sentences = c.sentences.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    // 字义（讲来历可能自然带部件说法，不查术语，只查长度与换行）
    if c.meaning.is_empty() {
        return Err(AppError::msg("字义不能为空"));
    }
    if c.meaning.contains('\n') {
        return Err(AppError::msg("字义里不能换行"));
    }
    if char_len(&c.meaning) > 60 {
        return Err(AppError::msg("字义太长了，请压到 60 个字以内"));
    }
    // 口诀
    if c.mnemonic.is_empty() {
        return Err(AppError::msg("口诀不能为空"));
    }
    if c.mnemonic.contains('\n') {
        return Err(AppError::msg("口诀里不能换行"));
    }
    if char_len(&c.mnemonic) > 60 {
        return Err(AppError::msg("口诀太长了，请压到 40 个字以内"));
    }
    if let Some(t) = find_term(&c.mnemonic) {
        return Err(AppError::msg(format!("口诀里用了课堂术语「{t}」，请换成孩子听得懂的说法")));
    }
    // 组词
    if c.words.len() < 2 || c.words.len() > 4 {
        return Err(AppError::msg("组词要 2~4 个"));
    }
    for w in &c.words {
        if w.contains('\n') || char_len(w) > 6 {
            return Err(AppError::msg("每个组词是 2~4 个字的常用词"));
        }
        if !w.contains(ch) {
            return Err(AppError::msg(format!("每个组词里都要有「{ch}」")));
        }
    }
    // 例句
    if c.sentences.is_empty() {
        return Err(AppError::msg("至少要有一句例句"));
    }
    if c.sentences.len() > 2 {
        return Err(AppError::msg("例句最多两句"));
    }
    for s in &c.sentences {
        if s.contains('\n') {
            return Err(AppError::msg("例句里不能换行"));
        }
        if char_len(s) > 40 {
            return Err(AppError::msg("例句太长了，请压到 40 个字以内"));
        }
        if !s.contains(ch) {
            return Err(AppError::msg(format!("每句例句里都要包含「{ch}」")));
        }
        if let Some(t) = find_term(s) {
            return Err(AppError::msg(format!("例句里用了课堂术语「{t}」，请换成孩子听得懂的说法")));
        }
    }
    Ok(c)
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test char_card
```

Expected: 9 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain/char_card.rs src-tauri/src/domain/mod.rs
git commit -m "feat(lookup): AI 单字讲解卡领域层（prompt + 校验）"
```

---

### Task 5: 迁移 0007 + `repo/char_cards.rs` + 同步哈希登记（TDD）

**Files:**
- Create: `src-tauri/migrations/0007_char_cards.sql`
- Create: `src-tauri/src/repo/char_cards.rs`
- Modify: `src-tauri/src/db/mod.rs`（MIGRATIONS 数组，第 7-14 行附近）
- Modify: `src-tauri/src/repo/mod.rs`（`pub mod char_cards;`）
- Modify: `src-tauri/src/repo/sync.rs`（`TABLES` 常量，第 32 行附近 + 测试）

**Interfaces:**
- Consumes: `domain::char_card::CharCardContent`（Task 4）
- Produces: `char_cards::CharCardDto { char, content, model, created_at, updated_at }`（serde camelCase）；`char_cards::upsert(conn, ch, content, model) -> AppResult<CharCardDto>`；`char_cards::get(conn, ch) -> AppResult<Option<CharCardDto>>`——Task 6 消费。

- [ ] **Step 1: 写迁移 `src-tauri/migrations/0007_char_cards.sql`**

```sql
-- 查字工具的 AI 字卡缓存：按字文本唯一（与 ai_cards 按 word_id 不同，
-- 查字工具的字不要求在词表里）。内容随整库快照同步（ADR-17 / ADR-22）；
-- 词表增删改不影响本表，所以没有外键、也没有清卡联动。
CREATE TABLE char_cards (
  char         TEXT PRIMARY KEY,
  content_json TEXT NOT NULL,
  model        TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
```

- [ ] **Step 2: `db/mod.rs` MIGRATIONS 追加一行（0006 之后）**

```rust
    (6, include_str!("../../migrations/0006_ai_cards.sql")),
    (7, include_str!("../../migrations/0007_char_cards.sql")),
];
```

- [ ] **Step 3: 写失败测试 `src-tauri/src/repo/char_cards.rs`（tests mod）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn sample(meaning: &str) -> CharCardContent {
        CharCardContent {
            meaning: meaning.into(),
            mnemonic: "王爷家的白玉石".into(),
            words: vec!["碧绿".into(), "碧玉".into()],
            sentences: vec!["湖水碧绿。".into()],
        }
    }

    #[test]
    fn user_version_is_7_after_migrations() {
        let conn = db::open_memory();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 7, "0007 迁移后 user_version 必须是 7");
    }

    #[test]
    fn upsert_roundtrip_and_overwrite() {
        let conn = db::open_memory();
        let first = upsert(&conn, "碧", &sample("青绿色的玉石"), "m1").unwrap();
        assert_eq!(first.char, "碧");
        assert_eq!(first.content.meaning, "青绿色的玉石");
        assert_eq!(first.model, "m1");

        let second = upsert(&conn, "碧", &sample("换一句"), "m2").unwrap();
        assert_eq!(second.content.meaning, "换一句");
        assert_eq!(second.model, "m2");
        assert_eq!(second.created_at, first.created_at, "覆盖不换创建时间");
    }

    #[test]
    fn get_missing_returns_none() {
        let conn = db::open_memory();
        assert!(get(&conn, "没有").unwrap().is_none());
    }
}
```

- [ ] **Step 4: 跑测试确认失败**

```bash
cd src-tauri && cargo test char_cards
```

Expected: 编译失败（`char_cards` 模块/函数不存在）。

- [ ] **Step 5: 写实现**

```rust
use crate::domain::char_card::CharCardContent;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharCardDto {
    pub char: String,
    pub content: CharCardContent,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 生成或重新生成一张字卡：同字覆盖（UPSERT，连点两次生成后写者胜）
pub fn upsert(
    conn: &Connection,
    ch: &str,
    content: &CharCardContent,
    model: &str,
) -> AppResult<CharCardDto> {
    let raw = serde_json::to_string(content)?;
    conn.execute(
        "INSERT INTO char_cards (char, content_json, model,
                                 created_at, updated_at)
         VALUES (?1, ?2, ?3,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(char) DO UPDATE SET
           content_json = excluded.content_json,
           model = excluded.model,
           updated_at = excluded.updated_at",
        params![ch, raw, model],
    )?;
    get(conn, ch)?.ok_or_else(|| AppError::msg("字卡保存失败，请重试"))
}

pub fn get(conn: &Connection, ch: &str) -> AppResult<Option<CharCardDto>> {
    let row = conn
        .query_row(
            "SELECT char, content_json, model, created_at, updated_at
             FROM char_cards WHERE char = ?1",
            params![ch],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((ch, raw, model, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let content: CharCardContent = serde_json::from_str(&raw)?;
    Ok(Some(CharCardDto { char: ch, content, model, created_at, updated_at }))
}
```

`repo/mod.rs` 加 `pub mod char_cards;`。

- [ ] **Step 6: 跑测试确认通过**

```bash
cd src-tauri && cargo test char_cards
```

Expected: 3 个测试 PASS。

- [ ] **Step 7: 同步哈希登记（`repo/sync.rs`）**

`TABLES` 常量（第 32 行附近）改为（注释同步扩写）：

```rust
/// `ai_cards`（迁移 0006 的讲解卡缓存）与 `char_cards`（0007 的查字字卡缓存）必须在这里：
/// 只生成/重新生成了卡片、词没变时，内容哈希也要变，否则推送会被判成「内容没变」而跳过。
const TABLES: [&str; 6] = ["groups", "words", "sessions", "session_items", "ai_cards", "char_cards"];
```

测试模块加一个测试（放在 ai_cards 那条哈希测试旁边，注意 use 已有 `content_hash` 等）：

```rust
    #[test]
    fn char_cards_participate_in_content_hash() {
        let conn = db::open_memory();
        let before = content_hash(&conn).unwrap();
        char_cards::upsert(
            &conn,
            "碧",
            &CharCardContent {
                meaning: "青绿色的玉石".into(),
                mnemonic: "白玉石".into(),
                words: vec!["碧绿".into()],
                sentences: vec!["湖水碧绿。".into()],
            },
            "test-model",
        )
        .unwrap();
        let after = content_hash(&conn).unwrap();
        assert_ne!(
            before, after,
            "只生成字卡也必须改变内容哈希，否则推送被跳过（与 ai_cards 同一坑）"
        );
    }
```

（测试模块顶部如缺 `use crate::domain::char_card::CharCardContent;` 与 `use crate::repo::char_cards;` 则补上——sync.rs 测试模块用 `use super::*;`，模块内直接写全路径也行。）

- [ ] **Step 8: 全量后端测试 + clippy**

```bash
cd src-tauri && cargo test && cargo clippy --all-targets
```

Expected: 全部 PASS、clippy 无新告警。

- [ ] **Step 9: Commit**

```bash
git add src-tauri/migrations/0007_char_cards.sql src-tauri/src/db/mod.rs src-tauri/src/repo/
git commit -m "feat(lookup): char_cards 表（0007）+ repo + 同步内容哈希登记"
```

---

### Task 6: AI 客户端编排 + 命令 + IPC

**Files:**
- Modify: `src-tauri/src/ai/client.rs`（加 `generate_char_content`）
- Modify: `src-tauri/src/commands/ai.rs`（加两命令）
- Modify: `src-tauri/src/lib.rs`（注册，`generate_word_card` 之后）
- Modify: `src/types.ts`（`CharCardContent` / `CharCard`）
- Modify: `src/lib/ipc.ts`（`getCharCard` / `generateCharCard`）

**Interfaces:**
- Consumes: Task 4 `domain::char_card`、Task 5 `repo::char_cards`、既有 `ai::client::{post_chat 之上的 build_body、generate_content 编排}`、`ai::config::load_required`
- Produces: Tauri 命令 `get_char_card(character: String)` / `generate_char_card(character: String)`；前端 `api.getCharCard(ch)` / `api.generateCharCard(ch)`——Task 7 消费。

- [ ] **Step 1: `ai/client.rs` 末尾（tests 之前）加编排函数**

```rust
use crate::domain::char_card::{self, CharCardContent, CharContext};

/// 生成单字讲解卡内容（查字工具）：与 generate_content 同一套编排（校验失败回喂重试一次）。
pub fn generate_char_content(cfg: &AiConfig, ctx: &CharContext) -> AppResult<(CharCardContent, String)> {
    let mut messages = char_card::build_messages(ctx);
    let raw = post_chat(cfg, &build_body(&cfg.model, &messages, cfg.max_output_tokens))?;
    match char_card::validate(&raw, &ctx.char) {
        Ok(content) => Ok((content, cfg.model.clone())),
        Err(first) => {
            messages.push(ChatMessage { role: "assistant".into(), content: raw });
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "你上次的输出不合格：{first}。请严格按要求的 JSON 格式重新输出，只输出 JSON 本身。"
                ),
            });
            let raw2 = post_chat(cfg, &build_body(&cfg.model, &messages, cfg.max_output_tokens))?;
            let content = char_card::validate(&raw2, &ctx.char).map_err(|e| {
                AppError::msg(format!("生成的内容不合格式（{e}），请重试或换个模型"))
            })?;
            Ok((content, cfg.model.clone()))
        }
    }
}
```

（`use` 行放到文件顶部与现有 use 合并；顶部已有 `use crate::domain::ai_card::{self, CardContent, ChatMessage, WordContext};`，新增的 char_card use 不冲突。）

- [ ] **Step 2: `commands/ai.rs` 加两命令（文件末尾）**

```rust
use crate::domain::char_card::CharContext;
use crate::repo::char_cards;

#[tauri::command]
pub fn get_char_card(
    state: State<'_, AppState>,
    character: String,
) -> AppResult<Option<char_cards::CharCardDto>> {
    let conn = state.conn()?;
    char_cards::get(&conn, &character)
}

/// 生成/重新生成单字讲解卡（查字工具）。参数校验（必须正好一个字）属参数转换，
/// 放命令层；锁编排与 generate_word_card 相同：拿锁落库 → 放锁等 HTTP → 拿锁落库。
#[tauri::command]
pub async fn generate_char_card(
    state: State<'_, AppState>,
    character: String,
) -> AppResult<char_cards::CharCardDto> {
    if character.chars().count() != 1 {
        return Err(AppError::msg("一次只讲一个字"));
    }
    let ctx = CharContext { char: character.clone() };
    let dir = state.data_dir.clone();
    let cfg = join(
        tauri::async_runtime::spawn_blocking(move || config::load_required(&dir)).await,
    )?;
    let (content, model) = join(
        tauri::async_runtime::spawn_blocking(move || client::generate_char_content(&cfg, &ctx)).await,
    )?;
    let conn = state.conn()?;
    char_cards::upsert(&conn, &character, &content, &model)
}
```

- [ ] **Step 3: `lib.rs` 注册（`commands::ai::generate_word_card` 之后）**

```rust
            commands::ai::get_word_card,
            commands::ai::generate_word_card,
            commands::ai::get_char_card,
            commands::ai::generate_char_card,
```

- [ ] **Step 4: `src/types.ts` 加类型（AiCard 相关类型之后）**

```ts
/** AI 单字讲解卡内容（对齐 Rust domain::char_card::CharCardContent） */
export interface CharCardContent {
  /** 字义 / 来历小故事（讲给孩子听） */
  meaning: string;
  /** 巧记口诀 */
  mnemonic: string;
  /** 含该字的常用词语 2~4 个 */
  words: string[];
  /** 例句 1~2 句，每句包含该字 */
  sentences: string[];
}

/** 查字工具的 AI 字卡（按字缓存，独立于词表） */
export interface CharCard {
  char: string;
  content: CharCardContent;
  model: string;
  createdAt: string;
  updatedAt: string;
}
```

- [ ] **Step 5: `src/lib/ipc.ts` 加方法（`generateWordCard` 之后；类型 import 列表加 `CharCard`）**

```ts
  getCharCard: (character: string) => invoke<CharCard | null>("get_char_card", { character }),
  generateCharCard: (character: string) => invoke<CharCard>("generate_char_card", { character }),
```

- [ ] **Step 6: 验证**

```bash
cd src-tauri && cargo test && cargo clippy --all-targets
npm run typecheck
```

Expected: 全绿。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/ai/client.rs src-tauri/src/commands/ai.rs src-tauri/src/lib.rs src/types.ts src/lib/ipc.ts
git commit -m "feat(lookup): get_char_card / generate_char_card 命令与前端 API"
```

---

### Task 7: 前端页面「查字」+ 组件 + 工具注册

**Files:**
- Create: `src/components/CharInfoCard.tsx`
- Create: `src/pages/Lookup.tsx`
- Modify: `src/tools/registry.ts`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `StrokeWord`（`text/size/masked/onPick` + ref `play(only?)`）、`lib/strokes::{isHanzi, loadStrokeData}`、`lib/dict.loadDictEntry`（Task 3）、`lib/pinyin.charPinyins`、`api.{getAiConfig,getCharCard,generateCharCard,speakText,stopSpeak}`（Task 6）、`Toast/useNotice`
- Produces: 默认导出 `Lookup` 页面组件（无 props），App.tsx 挂载。

- [ ] **Step 1: 写 `src/components/CharInfoCard.tsx`**

```tsx
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
```

- [ ] **Step 2: 写 `src/pages/Lookup.tsx`**

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import CharInfoCard, { type CharInfo } from "../components/CharInfoCard";
import StrokeWord, { type StrokeWordHandle } from "../components/StrokeWord";
import { Toast, useNotice } from "../components/Toast";
import { api, errorMessage } from "../lib/ipc";
import { loadDictEntry } from "../lib/dict";
import { charPinyins } from "../lib/pinyin";
import { isHanzi, loadStrokeData } from "../lib/strokes";

/** 输入长度上限：查字不是抄句子，超长没有意义 */
const MAX_CHARS = 12;

/** 卡片内可用横向空间与间距（与 StrokeReview 同款折算；那边未导出，查字场景照抄取值） */
const CARD_INNER = 720;
const GAP = 20;

function boxSize(count: number): number {
  if (count <= 2) return 200;
  if (count === 3) return 176;
  if (count === 4) return 152;
  const cols = Math.ceil(count / 2);
  const fit = Math.floor((CARD_INNER - 8 - (cols - 1) * GAP) / cols / 4) * 4;
  return Math.max(96, Math.min(132, fit));
}

/** 查字工具：家长输入字词 → 逐字笔顺动画 + 部首/笔画/结构/释义 + AI 单字讲解 */
export default function Lookup() {
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [aiReady, setAiReady] = useState(false);
  const [infos, setInfos] = useState<Record<string, CharInfo>>({});
  const [loadingInfo, setLoadingInfo] = useState(false);
  /** 正在生成的字（一次一个；null = 没在生成） */
  const [generating, setGenerating] = useState<string | null>(null);
  const wordRef = useRef<StrokeWordHandle>(null);
  // 数据加载竞态守卫：换词后旧请求回来不许再写 state
  const queryGen = useRef(0);
  const { notice, notify } = useNotice();

  useEffect(() => {
    api
      .getAiConfig()
      .then((c) => setAiReady(c.configured))
      .catch(() => {});
  }, []);

  // 退出页面停掉可能在播的朗读
  useEffect(() => () => void api.stopSpeak().catch(() => {}), []);

  const submit = useCallback(() => {
    const chars = [...text].filter(isHanzi);
    if (chars.length === 0) {
      notify("error", "请输入汉字");
      return;
    }
    if (chars.length > MAX_CHARS) chars.length = MAX_CHARS;
    setQuery(chars.join(""));
  }, [text, notify]);

  // 换词后逐字取：笔画数（笔顺数据）+ 字典条目 + AI 缓存卡（全本地，失败静默降级「—」）
  useEffect(() => {
    if (!query) return;
    const gen = ++queryGen.current;
    const chars = [...query];
    const unique = [...new Set(chars)];
    const pinyins = charPinyins(query);
    setLoadingInfo(true);
    setInfos({});
    void (async () => {
      const next: Record<string, CharInfo> = {};
      await Promise.all(
        unique.map(async (ch) => {
          const firstAt = chars.indexOf(ch);
          const [strokes, dict, card] = await Promise.all([
            loadStrokeData(ch)
              .then((d) => d.strokes.length)
              .catch(() => null),
            loadDictEntry(ch),
            api.getCharCard(ch).catch(() => null),
          ]);
          next[ch] = { pinyin: pinyins[firstAt] ?? "", strokes, dict, card };
        }),
      );
      if (queryGen.current !== gen) return;
      setInfos(next);
      setLoadingInfo(false);
    })();
  }, [query]);

  const generate = useCallback(
    async (ch: string) => {
      if (generating) return;
      setGenerating(ch);
      try {
        const card = await api.generateCharCard(ch);
        // 词已换、新词里没有这个字：这张卡不属于当前页面，丢弃
        if (![...query].includes(ch)) return;
        setInfos((prev) => {
          const base = prev[ch] ?? { pinyin: "", strokes: null, dict: null, card: null };
          return { ...prev, [ch]: { ...base, card } };
        });
      } catch (e) {
        notify("error", "讲解生成失败：" + errorMessage(e));
      } finally {
        setGenerating(null);
      }
    },
    [generating, query, notify],
  );

  const speak = useCallback(() => {
    if (!query) return;
    void api.speakText(query).catch((e) => notify("error", "朗读失败：" + errorMessage(e)));
  }, [query, notify]);

  const replay = useCallback((only?: number) => wordRef.current?.play(only), []);

  const uniqueChars = useMemo(() => [...new Set([...query])], [query]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center justify-between gap-4 border-b border-stone-200 px-8 py-5">
        <div>
          <h1 className="text-lg font-semibold text-stone-800">查字</h1>
          <p className="mt-1 text-sm text-stone-500">输入字词，看笔顺动画和字的解释</p>
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
          className="flex items-center gap-2"
        >
          <input
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="输入字或词，如：碧绿"
            maxLength={MAX_CHARS * 2}
            className="w-64 rounded-md border border-stone-300 px-3 py-1.5 text-sm outline-none focus:border-amber-400"
          />
          <button
            type="submit"
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900"
          >
            查一查
          </button>
        </form>
      </header>

      {query ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8">
          <div className="m-auto flex w-full max-w-3xl flex-col items-center gap-5 py-4">
            <div className="flex min-h-72 w-full flex-col items-center justify-center rounded-2xl border border-stone-200 bg-white px-6 py-10 shadow-sm">
              <StrokeWord
                key={query}
                ref={wordRef}
                text={query}
                size={boxSize([...query].length)}
                masked={false}
                onPick={(i) => replay(i)}
              />
            </div>
            <div className="flex items-center gap-3">
              <button
                onClick={() => replay()}
                className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
              >
                重播笔画
              </button>
              <button
                onClick={speak}
                title="用系统语音读一遍这个词"
                className="rounded-md border border-stone-300 bg-white px-5 py-2 text-sm text-stone-700 hover:bg-stone-50"
              >
                🔊 读一遍
              </button>
            </div>
            <div className="flex w-full flex-wrap justify-center gap-3">
              {uniqueChars.map((ch) => (
                <CharInfoCard
                  key={ch}
                  char={ch}
                  info={loadingInfo ? undefined : infos[ch]}
                  aiReady={aiReady}
                  generating={generating === ch}
                  onGenerate={() => void generate(ch)}
                />
              ))}
            </div>
            <p className="text-xs text-stone-400">点田字格只重播这个字</p>
          </div>
        </div>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center">
          <p className="text-sm text-stone-500">孩子问「这个字怎么写」？在这里查给他看</p>
          <p className="text-xs text-stone-400">逐字笔顺动画 · 部首 · 笔画 · 结构 · 释义</p>
        </div>
      )}
      <Toast notice={notice} />
    </div>
  );
}
```

- [ ] **Step 3: 注册工具 `src/tools/registry.ts`**

```ts
export type ToolId = "dictation" | "math" | "lookup";
```

Route 联合类型加：

```ts
  // ── 口算练习 ──
  | { name: "math"; page: "sheet" }
  // ── 查字 ──
  | { name: "lookup"; page: "lookup" }
  // ── 全局 ──
```

TOOLS 加：

```ts
  { id: "math", label: "口算练习", home: { name: "math", page: "sheet" } },
  { id: "lookup", label: "查字", home: { name: "lookup", page: "lookup" } },
```

NAV_BY_TOOL 加（注意 label ≤2 汉字的侧栏约束）：

```ts
  math: [{ key: "sheet", label: "出题", route: { name: "math", page: "sheet" } }],
  lookup: [{ key: "lookup", label: "查字", route: { name: "lookup", page: "lookup" } }],
```

- [ ] **Step 4: `src/App.tsx` 挂载**

1. import：`import LookupPage from "./pages/Lookup";`
2. activeNav 判定链（`else if (route.name === "math") activeNav = "sheet";` 之后）加：

```tsx
  else if (route.name === "lookup") activeNav = "lookup";
```

3. 生词默写路由收窄改为按 name 判等（原来是「settings 或 math 时为 null」，现在非 dictation 的工具多了，反转条件更不容易漏）：

```tsx
  const dr = route.name === "dictation" ? route : null;
```

4. quietRoute 数组（孩子盯着看动画，升级/同步横幅不弹）：

```tsx
  const quietRoute = ["quiz", "review", "stroke", "print", "settings", "lookup"].includes(routeKey);
```

5. 挂载（`{route.name === "math" && <MathSheetPage />}` 旁）：

```tsx
          {route.name === "math" && <MathSheetPage />}
          {route.name === "lookup" && <LookupPage />}
```

- [ ] **Step 5: 类型检查**

```bash
npm run typecheck
```

Expected: 无错误。

- [ ] **Step 6: 手测（`npm run tauri dev`）**

按清单验证（wiki 验收路径同步登记，见 Task 8）：

1. 顶栏出现「查字」，点击进入，输入框自动聚焦；侧栏「查字」高亮。
2. 查「碧绿」：两个田字格串行动画、上方拼音 bì lǜ；下方两张信息卡：碧（部首 石 · 共 14 画 · 上下结构 + 释义）、绿（部首 纟 · 共 11 画 · 左右结构 + 释义）。
3. 点田字格只重播该字；「重播笔画」全词重播；「🔊 读一遍」能出声；切页面后朗读停。
4. 查「爸爸」：信息卡只出一张（爸）。
5. 输入「abc！！」提交 → toast「请输入汉字」。
6. 未配置 AI：信息卡显示「在设置中配置 AI 后，可生成详细讲解」；配置后点「✨ 讲讲这个字」→ 生成内容展示、再查同字秒出缓存；生成中按钮不可连点。
7. 长释义（如「碧」）默认 4 行折叠，可展开/收起。
8. 生僻字（如「𠀀」或笔画数据缺失字）：静态字形/「—」不白屏。

- [ ] **Step 7: Commit**

```bash
git add src/pages/Lookup.tsx src/components/CharInfoCard.tsx src/tools/registry.ts src/App.tsx
git commit -m "feat(lookup): 查字工具页面（笔顺动画 + 字典信息 + AI 讲解）与顶栏注册"
```

---

### Task 8: 文档（ADR-22、ADR-19 修订、wiki、AGENTS.md）

**Files:**
- Modify: `wiki/decisions.md`（ADR-22 + ADR-19 追加修订行）
- Modify: `wiki/overview.md`、`wiki/frontend.md`、`wiki/data-model.md`
- Modify: `AGENTS.md`（目录表 + 常见任务表各补一行）

**Interfaces:** 无代码接口；内容要求见下。

- [ ] **Step 1: `wiki/decisions.md` 追加 ADR-22，并给 ADR-19 补修订行**

ADR-19 末尾追加一行：

```markdown
- **修订（2026-11-14，ADR-22）**：拼音显示范围延伸为「笔画复习页与查字工具」；其余页面、打印单、数据模型仍无拼音
```

文末追加（风格对齐既有 ADR）：

```markdown
## ADR-22 查字工具：离线字典随包 + AI 单字讲解按字缓存

- **结论**：第三个顶栏工具「查字」：家长输入字词，逐字笔顺动画（复用 `StrokeWord`）+ 每字部首/笔画数/结构/释义。结构来自 CCD（`chinese-characters-decomposition`，MIT；符号 + 外框部件规则表转教学标签——CCD 是几何拆分，「病」标吕、「远」标吅，必须看部件再定包围结构）；部首与释义来自 vendor 的 chinese-xinhua 精简源（`scripts/dict-src/`，社区抓取、无明确授权，仅个人学习用途，README 已注明）；`scripts/sync-dict-data.mjs` 打成 64 分片进 `public/dict/`（生成物不进库），前端 `lib/dict.ts` 懒加载。笔画数取笔顺数据 `strokes.length`（精确），拼音取 pinyin-pro（修订 ADR-19 范围），都不重复打包。xinhua 缺部首的 9044 条全是生僻字，常见字覆盖实测 122/122，缺就显示「—」，不引第二数据源
- **理由**：「爸爸，xx 字怎么写」是默写场景之外的高频请求；部首/结构/释义必须离线秒开（默认纯离线是产品定义，ADR-18 同思路），AI 详解是可选增强（ADR-20 边界不变：显式配置、LLM 永不写学习进度）
- **AI 字卡**：新 domain `char_card`（字义/口诀/组词/例句，validate 与 ai_card 同一套路）+ 表 `char_cards`（0007 迁移，按字文本主键，独立于词表；随整库快照同步，`sync.rs::TABLES` 已登记——只生成字卡也要改变内容哈希；不进备份 JSON，同 ai_cards 先例）
- **代价**：仓库 +约 6MB（vendor 源）、安装包 +约 7MB（字典分片）；部首口径用 xinhua（简体教学口径）而非 CCD 康熙部首（「绿→纟」不是「糸」），两源部首冲突时以 xinhua 为准
```

- [ ] **Step 2: `wiki/data-model.md` 加 char_cards 表小节**（对照 ai_cards 小节的写法：字段、键语义、同步行为、不进备份、无清卡联动）

- [ ] **Step 3: `wiki/frontend.md` 加「查字工具」小节**（页面结构、`lib/dict.ts` 分片规则、验收路径清单——把 Task 7 Step 6 的 8 条手测清单抄进去）

- [ ] **Step 4: `wiki/overview.md` 工具列表加「查字」**（一句话定位）

- [ ] **Step 5: `AGENTS.md`**

- 「这是什么」段落：工具集合描述加「查字」（家长输字词看笔顺与字义，AI 可选详解）
- 目录表 `src/pages/` 行补 `Lookup /`；`scripts/` 附近补 `sync-dict-data.mjs` 一行说明
- 常见任务表「笔顺动画 / 笔画复习」行不变，另加一行：

```markdown
| 查字工具 / 字典数据 | `pages/Lookup.tsx` + `components/CharInfoCard.tsx` + `lib/dict.ts`（数据 `scripts/sync-dict-data.mjs` → `public/dict/`，源在 `scripts/dict-src/`）；AI 字卡 `domain/char_card.rs` + `repo/char_cards.rs` |
```

- [ ] **Step 6: Commit**

```bash
git add wiki/ AGENTS.md
git commit -m "docs: ADR-22 查字工具 + wiki 与 AGENTS.md 登记"
```

---

### Task 9: 全量验证 + 干净子 agent 审查（AGENTS.md 流程）

**Files:** 无新文件；可能按审查结论修复（每条真问题补一个能复现的测试）。

- [ ] **Step 1: 全量验证基线**

```bash
cd src-tauri && cargo test && cargo clippy --all-targets
npm run typecheck
node scripts/sync-dict-data.mjs   # 幂等跳过
```

Expected: 全绿。

- [ ] **Step 2: 起全新子 agent 审查（一次性，不复用，只读不改）**

prompt 要素（照 AGENTS.md）：`git status --short` + `git diff 875a0cd..HEAD --stat` 的文件清单；要求读全文不只看 diff；附硬约束清单（AGENTS.md「硬约束」节 + 本计划 Global Constraints）；已跑通的验证命令与结果；输出格式 `文件:行号 + 触发路径 + P1/P2 + 最小修法`；明确「只读，不改代码；没有问题就直说未发现问题」。

- [ ] **Step 3: 主 agent 逐条判定**

真问题 → 修 + 补复现测试；误报 → 记录理由。

- [ ] **Step 4: 修复后重跑 Step 1，再起一个新的干净子 agent 复审**

下一轮 prompt 显式列出「已判定为非问题：X，理由 Y」。

- [ ] **Step 5: 收敛后把每轮结论写进 `.workbuddy/memory/2026-11-14.md`**

（发现几条、哪几条真问题、怎么修的、哪几条误报及理由。）

---

## Self-Review 记录

- **Spec 覆盖**：设计文档 §1 定位→Task 7 注册；§2 数据管线→Task 1/2/3；§3 页面→Task 7；§4 AI→Task 4/5/6；§5 ADR→Task 8；§6 测试→各 task + Task 9；§7 边界→Task 7（竞态守卫/折叠/全非汉字提示）✓
- **占位符**：Task 8 wiki 各页只给了要点提纲——文档内容型任务，提纲即交付要求，非代码占位 ✓
- **类型一致性**：`DictEntry{radical,structure,explanation}`（Task 3 = Task 7 消费）；`CharCardContent{meaning,mnemonic,words,sentences}`（Task 4 Rust = Task 6 透传 = Task 7 types.ts）；`char_cards::{upsert,get}` 签名 Task 5 = Task 6 调用；命令参数 `character`（Rust/JS 两侧一致）✓
