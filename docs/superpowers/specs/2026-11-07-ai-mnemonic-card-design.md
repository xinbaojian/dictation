# AI 错字讲解卡（Phase 1）与在线朗读（Phase 2）· 设计文档

- 日期：2026-11-07
- 状态：已与产品负责人逐节确认通过
- 关联：ADR-17（opt-in 联网先例）、ADR-18（默认纯离线）、ADR-19（系统离线 TTS，本设计 Phase 2 修订其「不引联网引擎」结论）

## 1. 背景与目标

错题复习链路目前止步于「看笔顺动画」（`pages/StrokeReview.tsx`），缺「怎么记住」这一环：批改数据已精确到字（`session_items.wrong_chars` / `words.wrong_chars`，ADR-14/16），但没有任何内容帮孩子把反复错的字真正记住。家长最讲不出的恰恰是记忆口诀、形近字辨析这类内容——这正是 LLM 的强项。

**目标**：

1. 错题复习页为每个错词提供一张「巧记讲解卡」：部件拆解、记忆口诀、例句、形近字辨析
2. 内容按需生成、入库存缓存，**生成一次、离线复用**；缓存随整库快照同步（ADR-17 红利）
3. 默认安装零新联网行为：AI 能力只在用户显式配置后启用（对齐 MySQL 同步的 opt-in 模式）
4. 内容为孩子视角设计——StrokeReview 是全应用唯一孩子直视的页面，呈现必须适合二年级孩子

**非目标**：

- 不做批改辅助、抽题调度、进度写入等任何判定/调度型 AI（见仓库分析结论，红线）
- 不做孩子对话助手、云 TTS 强推、游戏化
- Phase 1 不做讲解卡打印（留作后续演进，管线相同）

## 2. 总体架构

```
StrokeReview / Settings ──invoke──> commands/ai.rs ──> ai/client.rs（OpenAI 兼容 chat completions，
                                          │            reqwest + rustls，新增依赖）
                                          ├──> domain/ai_card.rs（prompt 构造 + 输出校验，纯函数）
                                          └──> repo/ai_cards.rs（缓存表读写，唯一写 SQL 处）
```

生成流程（`generate_word_card`，async 命令）：

1. 拿锁读词（`text / wrong_chars / wrong_count`）→ 立即放锁
2. 纯函数构造 prompt → 调 API（无锁等待，30s 超时，温度 0.7，max_tokens 500）
3. 纯函数校验输出；不合格 → 将病因回喂重试 1 次
4. 拿锁 UPSERT 入 `ai_cards` → 放锁 → 返回

`MutexGuard` 不跨 await（与同步命令同规矩）；前端生成期间按钮 loading 防连点，同一词并发生成后写者胜（UPSERT 幂等，无碍）。

## 3. 数据模型（`migrations/0006_ai_cards.sql`，只增不改）

```sql
CREATE TABLE ai_cards (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  word_id      INTEGER NOT NULL REFERENCES words(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL DEFAULT 'mnemonic',  -- 预留：将来语境句等 AI 内容共用此表
  content_json TEXT NOT NULL,                     -- 结构化讲解内容
  model        TEXT NOT NULL,                     -- 生成时所用模型，溯源
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_ai_cards_word_kind ON ai_cards(word_id, kind);
```

`kind='mnemonic'` 的 `content_json`：

```json
{
  "decomposition": ["王", "白", "石"],
  "mnemonic": "碧 = 王 + 白 + 石：老王爷坐在白石头上钓鱼",
  "confusables": [{ "char": "壁", "hint": "土字底，和墙土有关" }],
  "sentences": ["碧绿的湖水像一块玉石。"]
}
```

- 表在 SQLite 内 → 随整库快照自动同步，一台生成两台可用
- 词删除 CASCADE 删卡；**词文本被编辑 → 删该词全部卡片**（对齐 ADR-16 改词清手工错字的做法，讲解针对旧字形）
- 不放 `words` 表、不占 `note` 字段：`words` 只放词条本身属性；AI 内容结构化、可重生成，必须独立
- 挂 `word_id` 不挂孩子 → 与未来「多孩子」拆表天然兼容
- 时间戳用 `strftime('%Y-%m-%dT%H:%M:%fZ','now')`，与全库一致

## 4. AI 配置与凭据（照搬 sync 模式）

`.ai-config.json` 存数据目录（权限 0600，同 `.sync-credentials.json`）：`baseUrl / apiKey / model`。

- **不进 settings 表、apiKey 不回传前端**；`get_ai_config` 只返回 baseUrl、model、key 掩码（`sk-***abcd`）
- **已配置 = 启用，清除 = 禁用**，不引入额外 enabled 开关
- 凭据文件不随快照同步（本机配置，与 sync 凭据一致）
- `save_ai_config(baseUrl, apiKey?, model)`：apiKey 传空 = 保留旧值（前端只显示掩码）
- Phase 2 增补：TTS 默认跟随 AI 配置（baseUrl + `/audio/speech`）；可单独覆盖一组（见 §8）

## 5. Prompt 与校验（`domain/ai_card.rs`，纯函数、全单测）

**系统提示词约束（孩子视角）**：

- 人设：语文老师给 8 岁孩子讲故事；比喻只用孩子熟悉的事物（动物、学校、家里、玩具）
- 口诀：能念出声的顺口短句，有节奏感，≤40 字，不用书面语
- 例句：只用二年级以内词汇，写孩子的生活场景，每句必须包含目标词全文
- 辨析：禁用「形近字 / 音近字 / 部首」等课堂术语，改说「长得像它的字」「怎么分清」
- `decomposition`：部件拆解（可选），每项单个汉字，2~4 项
- 输出纯 JSON（给 schema 与示例），不依赖各家专有 json_mode

**用户内容**：目标词、错字下标（让错过的字成为讲解焦点）、错误次数（反复错与首错措辞不同）。

**校验器**（LLM 输出的唯一入库关卡，全分支单测，LLM 不进测试路径）：

- 剥 ```json 围栏 → 解析 JSON
- `mnemonic` 非空、≤60 字；`sentences` 1~2 条、每条含目标词全文、≤40 字
- `confusables` ≤3 个，每个为单个汉字 + 非空提示
- `decomposition` 每项单个汉字、2~4 项
- **口诀与例句不得出现教学术语黑名单**（形近字、音近字、部首、音序、笔顺名等）；辨析 hint 放宽（该区标给家长）
- 全部文本禁换行、长度上限兜底；任何一条不过 → 带病因中文错误，回喂重试 1 次，仍败则报「生成内容不合格式，请重试或换模型」

## 6. 前端交互

**StrokeReview**（`pages/StrokeReview.tsx`）——「孩子先看、家长补充」分层：

| 层 | 内容 | 规格 |
|---|---|---|
| ① 部件积木块 | `decomposition` 渲染成 2~4 个大号彩色方块字 + 小字「合起来就是它」 | 孩子第一眼的视觉记忆 |
| ② 口诀 | text-2xl 大字、行高宽松、目标字 amber 高亮 | 孩子念的顺口溜 |
| ③ 例句 | 中号字，目标词下划线标出 | 孩子读得懂的句子 |
| ④ 辨析 | 小字 + 「👤 给家长」小标签 | 术语内容家长消化后讲给孩子 |

状态逻辑：

- 有卡：默认收起，点开显示；角落「重新生成」小按钮；「🔊 读口诀」放大突出（复用 `speak_text`）
- 无卡 + 已配 AI：显示「生成巧记讲解」按钮（生成中 loading）
- 无卡 + 未配 AI：什么都不显示，页面与现在一致
- 「自动朗读」开着 → 展开卡片时自动读口诀（翻词时照旧读词，不叠加）
- 「先自己写」遮笔画模式 → 讲解卡**强制收起且不可展开**（口诀/积木块等于答案提示），点「显示笔画」后可展开
- 配色沿用 amber/stone 暖色系，不加动画、不做游戏化（overview 红线）

**Settings**（`pages/Settings.tsx`）：新「AI 讲解」区块——说明文案（默认关闭；生成时会把词语发送到所配服务，仅此而已；内容缓存在本地、离线可看）+ baseUrl / model / apiKey 三输入框（key 显示掩码，重填才更新）+ 保存 / 测试连接 / 清除。

**ipc.ts / types.ts**：`AiConfigDto`、`AiCardDto`、`CardContent` 等类型与命令一一对应；前端不拼 SQL、只走 ipc。

## 7. 命令层（`commands/ai.rs`，第 5 组命令）

| 命令 | 说明 |
|---|---|
| `get_ai_config` | 掩码视图；前端据此决定「生成」按钮是否出现 |
| `save_ai_config(baseUrl, apiKey?, model)` | apiKey 空 = 保留旧值 |
| `clear_ai_config` | 删配置文件 |
| `test_ai_config` | 一次最小请求验证连通（设置页「测试连接」） |
| `get_word_card(wordId)` | 读缓存（本地查询，翻词随取） |
| `generate_word_card(wordId)` | 生成/重新生成并入库 |

全部 async；注册进 `lib.rs` 的 `invoke_handler`。

## 8. Phase 2：在线朗读（在线 TTS）

**事实基础**（2026-11 查证）：

- DeepSeek API 无 TTS（文档仅 chat/视觉/搜索）
- 智谱开放平台有 GLM-TTS：`POST {baseUrl}/audio/speech`（baseUrl = `https://open.bigmodel.cn/api/paas/v4`），Bearer 用同一把 API Key，请求形状与 OpenAI `audio/speech` 基本一致（`model/input/voice/response_format`）；音色 tongtong/chuichui/xiaochen/jam/kazi/douji/luodo；音频格式 wav（24kHz）/pcm；Coding Plan 是编程模型订阅，TTS 预计走按量计费（价格以控制台为准，给孩子读词的量级花费可忽略）
- GLM-TTS 默认加 AI 水印（个人中心「去水印管理」可关），家用无碍

**设计**：

1. 设置页「朗读引擎」：系统离线（默认）/ 在线（更自然）；在线配置默认**跟随 AI 配置**，可单独覆盖（TTS 专属 baseUrl/key/voice）
2. 请求按 `POST {ttsBaseUrl}/audio/speech`，`response_format=wav`，voice 默认 `tongtong`（智谱）；所配服务不支持该端点 → 中文提示并**自动回落系统语音**，不阻塞复习
3. **缓存离线复用**：音频按内容哈希存数据目录 `ai-tts/`；词、口诀、例句听过即缓存，之后零流量零延迟，断网可重听；设置页提供「清空朗读缓存」
4. 播放：新增 `rodio`（纯 Rust，含 wav 解码）统一三平台；`speak_text` 语义不变（先停上一句再读、幂等，StrictMode 双跑无害）；`Speaker` trait 保留，在线实现为并列分支，`stop_speak` 同时停 rodio sink 与系统语音
5. 受益面：读词、读口诀、将来例句全部换自然嗓音
6. **ADR-21**：修订 ADR-19「不引联网引擎」为「默认系统离线；在线引擎为显式配置的可选项」

## 9. 错误处理与离线行为

- 未配置点生成 →「请先在设置里配置 AI 服务」；断网/超时（30s）→ 中文提示可重试
- 校验两轮失败 →「生成内容不合格式，请重试或换模型」
- 已缓存内容永远离线可看；未配 AI 时零新联网行为，产品定义不变
- 统一 `AppError` → 前端 `errorMessage`；破坏性操作无（重新生成覆盖旧卡为用户主动行为）

## 10. 测试策略

- `domain/ai_card.rs`：prompt 构造（含错字/错误次数上下文断言）、校验器全分支（合法/缺字段/超长/例句不含目标词/术语黑名单/围栏剥离）
- `repo/ai_cards.rs`：in-memory 库 CRUD + 词删除级联 + 词改文删卡 + UPSERT 幂等
- 凭据文件：0600 权限、损坏可重存修复（对齐 sync 现有测试形态）
- 收尾三件套：`cargo test`、`cargo clippy --all-targets`、`npm run typecheck`，随后按 AGENTS.md 审查流程起干净子 agent 审查

## 11. 红线自查（对照 AGENTS.md 硬约束）

- 不碰 `repo/word_progress.rs`（讲解是内容非进度）✅
- 不建 mistakes 表 ✅；不碰会话 / 拼音 / 分组树 / identifier / updater / 同步逻辑 / single-instance / 笔画数据 ✅
- 前端只走 `ipc.ts` ✅；迁移 0006 只增不改 ✅
- 凭据不进 settings 表、不回传前端 ✅；默认零联网（opt-in）✅
- 新增依赖：`reqwest`（rustls-tls，避免系统 OpenSSL）；Phase 2 加 `rodio`
- ADR-20（AI 内容生成 opt-in 边界）+ ADR-21（在线 TTS 修订 ADR-19）待随实现落 `wiki/decisions.md`；overview「默认唯一联网行为」表述扩成清单

## 12. 分期

- **Phase 1（本期）**：讲解卡全链路（表 / 命令 / 校验 / StrokeReview 卡片 / Settings 配置），朗读口诀暂用系统语音
- **Phase 2（紧随）**：在线 TTS（rodio / 音频缓存 / 回落 / ADR-21），管线与 Phase 1 的「生成-校验-缓存」同构
- **后续演进（不在本 spec）**：讲解卡 A4 打印、批改后自动生成（默认关设置项）、语境句默写、口算应用题
