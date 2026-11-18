# 2026-11-18 错题本：词语编辑 + 批量加入（update_mistake / add_mistakes）

## 背景与目标

错题本此前只能「整词移出（已掌握）」和「单个手工加词」。两个缺口：

1. **编辑**：手滑打错字、孩子实际只是某个字不会写而词录错了，都没法改；
  改词文本后老的手工错字下标会失效（`words::update` 直接清空），需要一个顺手的地方重标。
2. **批量**：孩子一次听写错五六个词，家长只能一个个弹窗加，而词表导入早就支持
  「粘贴整段 → 按分隔符切分」。

目标：错题本行内可编辑（改文本 + 重标错字）；「加入错题本」弹窗支持批量粘贴
（空格/逗号/顿号/分号/斜杠/换行分隔，复用 `domain::text::split_words`）。

## 用户确认的三个决策

1. 编辑范围 = **改文本 + 重标错字**（不含改分组；挪组走词表页既有能力）。
2. 弹窗交互 = **双模式开关**（单个：现状不动；批量：textarea + 切分预览）。
3. 批量遇已在错题本（`status='learning'`）的词 = **跳过、不重复计错误次数**；
  错误次数只由默写批改（`session_items`）与首次手工加入累计，重复粘贴不虚增、幂等。

## 方案选型

- **A（采纳）：两个新命令，repo 层单事务**。
  `add_mistakes`：事务内逐词处理，任一失败整批回滚；`update_mistake`：事务内
  「改文本（复用 `words::update` 语义）+ 覆盖写手工错字下标」。
- B（否决）：前端循环调现有 `add_mistake`——无事务，加一半失败留部分数据；
  「跳过已存在」语义前端表达不了（要先查再判，竞态）；N 次 IPC。
- C（否决）：编辑拆成 `update_word` + 新的 `set_wrong_chars` 两条 IPC——两次往返
  中间态可见（文本已改、错字还没标），也没必要，一条组合命令更原子。

## 关键实现点

### 后端（commands/quiz.rs，命令 14 → 16；总计 58 → 60）

**`add_mistakes(state, group_id, texts: Vec<String>) -> BatchMistakeResult`**

- `repo/words.rs::add_mistakes`：`unchecked_transaction` 内对每个 `text`：
  - `normalize_word` 后为空 → 报错（前端预览已过滤，兜底）；
  - 查同分组同名词条：存在且 `status='learning'` → `skipped_existing += 1`（不写库）；
  - 否则复用单条 `add_mistake` 的内部逻辑（存在则复用词条 + `word_progress::mark_wrong`；
    不存在则建词条再 `mark_wrong`），`added += 1`；
  - 输入内的重复词由前端 `preview_words` 切分时去重，repo 不再处理（与 `import` 的
    分工一致：切分归 domain、写入归 repo）。
- `BatchMistakeResult { added: i64, skipped_existing: i64 }`，serde camelCase。

**`update_mistake(state, word_id, text: String, wrong_chars: Vec<i64>) -> WordDto`**

- `repo/words.rs::update_mistake`：事务内：
  1. `words::update`（改文本；文本变化时其内部已清 `wrong_chars`、删作废 AI 卡；
     同组重名走既有「该分组中已有「X」」报错）；
  2. `format_char_indices` 越界过滤后覆盖写 `words.wrong_chars`（只写这一列，
     不动 status / wrong_count / correct_streak —— 硬约束 1 的进度字段一个不碰）；
  3. 返回 `get(conn, word_id)`。
- 编辑不调 `mark_wrong`：改词不是「又错了一次」。

### 前端

**`components/AddMistakeModal.tsx`：顶部双模式开关（单个 / 批量）**

- 单个模式：现有 UI 与行为完全不动。
- 批量模式：textarea（`preview_words` 防抖预览：词 chips + 「共 N 个；重复丢弃 X、无效 Y」）、
  分组选择共用；提交调 `api.addMistakes`，Toast 汇报
  「已加入 N 个词」+（skipped>0 时）「M 个已在错题本，跳过」，成功后关弹窗刷新列表。
- 切分预览复用现有 `previewWords` IPC，不加新命令。

**`components/EditMistakeModal.tsx`（新）**

- 预填词语文本 + 手工标错字（`wrong_chars` 高亮）；无分组选择（改分组不在本次范围）；
  改文本时清空字标（下标已失效，UI 提示重新点）；提交调 `api.updateMistake`。
- `pages/Mistakes.tsx` 操作列加「编辑」按钮（「已掌握」左侧），打开该弹窗。

**`lib/ipc.ts` / `types.ts`**：`addMistakes`、`updateMistake` 两方法 + `BatchMistakeResult` 类型。

## 错误处理

- 批量事务失败：整批回滚，Toast 显示 `AppError` 中文信息（如「词语不能为空」）。
- 编辑重名：`words::update` 既有报错直出；弹窗不关闭，家长改完再提交。
- 批量输入全被过滤/为空：提交按钮禁用。

## 测试（repo 层，`words.rs` tests）

- `add_mistakes`：全新词批量建并标错；词条已存在（new）复用并标错；已在错题本跳过
  （`wrong_count` 不变）；混合批次统计数正确；同组重名不冲突（复用）。
- `update_mistake`：只改字不动文本 → wrong_count/status 不变；改文本 → 旧下标清空、
  新下标写入、越界下标丢弃；改文本撞同组重名 → 报错且事务回滚（wrong_chars 不残留半改状态）；
  改文本后 AI 讲解卡被删（复用 update 语义，加断言）。

## 文档同步

- `wiki/frontend.md`：错题本行补「行内编辑（改文本+重标错字）」与「批量加入（双模式弹窗）」。
- `wiki/data-model.md`：`wrong_chars` 行补「可经 update_mistake 覆盖编辑」。
- `AGENTS.md`：命令计数 quiz 14→16、总计 58→60。
- 不动：`word_progress` 写入口、错题本=learning 等价性、抽题/批改/会话、同步快照结构
  （全部走既有 words 表，快照自动覆盖）。

## 已知取舍

- 批量模式不逐词点字标错：需要标错的词加入后用「编辑」补，避免逐词走查的重交互
  （用户已确认）。
- 「已在错题本跳过」只看 `status='learning'`，不比对历史批改记录；被家长误标已掌握的
  词再粘贴会重新标错（复用 `add_mistake` 语义），符合「手工加入=认定这次错了」。
- 单个模式维持现状（重复加入仍 `wrong_count+1`）：改它会变更既有行为，与本次目标无关。
- 编辑弹窗预填的错字是显示值（批改历史 + 手工合并），保存覆盖写会把批改标过的字一并
  固化进 `words.wrong_chars`。显示（`load_char_marks` 合并两者）与 AI 讲解卡上下文
  （`marks()` 同源）均无行为差异，仅存储归属模糊化，接受。
