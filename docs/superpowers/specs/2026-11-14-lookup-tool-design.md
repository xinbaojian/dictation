# 查字工具设计（第三顶栏工具「查字」）

> 日期：2026-11-14
> 状态：已与用户逐节确认通过
> 场景：孩子问「爸爸，xx 字怎么写」。家长输入词或字 → 逐字笔顺动画 + 每字字典信息（部首 / 笔画 / 结构 / 释义），AI 可按字生成详细讲解。

## 1. 定位与命名

- 本应用（学习小工具集合）**第三个顶栏工具**，id `lookup`，顶栏 label「查字」；侧栏单页（同 math 工具结构），页内路由 `{ name: "lookup", page: "lookup" }`。
- 查字工具**独立于词表**：输入任意字词，不要求先录入词表，不读写学习进度（ADR-20 红线：LLM 永不写进度，这里更进一步——本工具完全不碰 `word_progress`）。
- 注册路径照 registry.ts 注释：`ToolId` 加变体 + `Route` 加变体 + `TOOLS` / `NAV_BY_TOOL` 各加一项 + `App.tsx` 挂页面。

## 2. 字典数据管线（离线随包，ADR-18 同思路）

### 2.1 数据源

| 信息 | 来源 | 说明 |
|---|---|---|
| 结构 | `chinese-characters-decomposition`（npm，MIT；数据出自 Wikimedia Commons CCD 表，21169 字） | compositionType 符号（一/吕/回/吅/咒…）+ **外框部件规则表**（辶→左下包围、疒→左上包围、门/冂→上三包围、囗→全包围、凵→下三包围、匚→左三包围、勹/气→右上包围；CCD 的几何拆分与教学口径不一致：病=吕、远=吅，必须看部件再定）转教学标签：左右结构、上下结构、左中右、上中下、全包围、左上包围、左下包围、上三包围、下三包围、左三包围、右上包围、半包围、独体字；叕/弼/* 等罕见复合结构标「—」 |
| 部首 + 释义 | chinese-xinhua `data/word.json`（16142 字） | 不在 npm 上：一次性精简后 vendor 进仓库 `scripts/dict-src/xinhua.json`（只留部首/释义，去 oldword/more/strokes 等字段，约 8MB）；授权状态：社区抓取、无明确授权，README 注明「个人学习用途」 |
| 部首兜底 | CCD 康熙部首 | xinhua 缺字时用；**部首以 xinhua 为准**（简体教学口径，如「绿→纟」） |
| 笔画数 | 本地笔顺数据 `strokes.length` | 精确值，不打包第二份 |
| 拼音 | `pinyin-pro` 前端整词转换（StrokeWord 已内建） | 不打包字典拼音，避免两套数据打架；多音字按词消歧 |

### 2.2 脚本与产物

- 新脚本 `scripts/sync-dict-data.mjs`，挂进 `predev` / `prebuild`（在 sync-hanzi-data 之后），`.stamp` 版本跳过机制同款（含「产物在不在」校验）。
- 产物 `public/dict/s00…s3f.json`：**64 分片**，码点 % 64，片内以码点 hex 为键（纯 ASCII，同 hanzi 约定），每字 `{ 部首, 结构, 释义 }`。生成物不进库。
- 前端 `src/lib/dict.ts`：`loadDictEntry(char)`，分片 Promise 缓存 + 失败重试，URL 用 `import.meta.env.BASE_URL` 拼站点根（pitfalls 第 24 条）。

## 3. 页面交互（`src/pages/Lookup.tsx`）

- **输入区**（顶部，默认聚焦）：文本框 + 「查一查」（Enter 提交）；≤12 字，提交后过滤出汉字序列；全非汉字时提示「请输入汉字」。
- **展示区**：复用 `StrokeWord`（`masked=false`，自适应格子大小，含拼音行、串行动画、点单字重播、缺笔顺数据静态字形回退）+「重播笔画」+「🔊 读一遍」（`api.speakText` 整词；离开页面 `stopSpeak`，同 StrokeReview 模式）。
- **信息卡**（每字一张，横排自动换行）：
  - 字头：大字 + 拼音（整词消歧后的该字读音）；
  - 信息行：`部首 纟 · 共 11 画 · 左右结构`（首版不显示部首画数——xinhua 只有部首字，CCD 是康熙口径，混用会错，明确不显示）；
  - 释义：本地字典文本，超长折叠可展开。
- **降级**：字典缺→信息行「—」、笔画数仍显示；笔顺缺→静态字形（现有行为）；两者都缺→「暂无数据」。

## 4. AI 按字详解（后端）

- **`src-tauri/src/domain/char_card.rs`**（纯函数 + 单测，仿 ai_card.rs）：
  - `CharContext { char }`；人设同「给二年级孩子讲语文的老师」；
  - 输出 JSON：`{ meaning: 字义/字源小故事（≤60字）, mnemonic: 巧记口诀（≤40字）, words: 组词 2~4 个（含该字）, sentences: 例句 1~2 句（≤40字，含该字） }`；
  - `validate`：JSON 解析 + 长度上限 + 无换行 + 术语黑名单（口诀/例句）+ 例句必须含该字；不合格回喂病因重试一次（复用 client.rs 编排）。
- **`migrations/0007_char_cards.sql`**（0006 已被 ai_cards 占用）：`char_cards (char TEXT PRIMARY KEY, content_json, model, created_at, updated_at)`；`db/mod.rs::MIGRATIONS` 追加。
  - 键是字文本本身，独立于词表：词表增删改、快照同步互不影响；随整库快照自动同步（ADR-17 零改动）。
  - `repo/char_cards.rs`：`upsert`（同字覆盖）/ `get`。
  - `repo/sync.rs::TABLES` 加入 `char_cards`：只生成字卡也要改变内容哈希，否则推送被跳过（与 ai_cards 同一坑）。备份 JSON 不收录（同 ai_cards 先例：讲解缓存不入备份）。
- **命令**（commands/ai.rs 加两个；锁编排照 `generate_word_card`：拿锁→放锁等 HTTP→拿锁落库）：
  - `get_char_card(char) -> Option<CharCardDto>`
  - `generate_char_card(char) -> CharCardDto`
  - lib.rs 注册 + `ipc.ts` / `types.ts`。
- **配置复用** `.ai-config.json`（ADR-20）：未配置 AI 时信息卡不显示生成按钮，显示浅色提示「在设置中配置 AI 后可生成详细讲解」；有缓存直接展示（含重新生成）。

## 5. 决策记录（ADR-22 + 修订 ADR-19）

- **ADR-22 查字工具**：字典数据随包分片（结构=CCD / 部首释义=xinhua vendor 源）、笔画数取笔顺数据、拼音前端转换；AI 详解按字缓存进 `char_cards`（LLM 永不写学习进度，红线不变）；xinhua 数据授权状态如实登记。
- **修订 ADR-19**：拼音显示范围从「仅笔画复习页」延伸为「笔画复习页与查字工具」（均为对照学习场景，非默写场景）。
- wiki 各页（overview / frontend / data-model / decisions）登记，AGENTS.md 目录表补一行。

## 6. 测试与验证

- Rust 单测：`domain/char_card`（prompt 构造、validate 各拒绝分支）、`repo/char_cards`（内存库 roundtrip / upsert 覆盖 / 迁移后表存在）。
- 脚本验收：产出 64 分片 + stamp 幂等；抽查「碧」「绿」「的」等字部首/结构/释义正确。
- `cargo test` + `cargo clippy --all-targets` + `npm run typecheck` 全绿；按 AGENTS.md 流程起干净子 agent 审查，结论落 `.workbuddy/memory/`。

## 7. 边界情况

- 生成中切换字/词：竞态守卫（同 StrokeReview 的 `currentWordId` 模式）；生成按钮 busy 防连点。
- 多音字：整词转换优先（「长大」→ zhǎng dà）；单字查询给默认音，信息卡不重复标注拼音。
- 超长释义：折叠 +「展开」。
- 输入全非汉字：提示「请输入汉字」。
