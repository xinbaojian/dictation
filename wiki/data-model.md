---
title: 数据模型
updated: 2026-10-25
tags: [database, sqlite]
related: [quiz-flow.md, architecture.md]
---

# 数据模型

库文件：`%APPDATA%\com.dictation.app\dictation.db`（WAL）。结构定义在 `src-tauri/migrations/`。

## 表

### groups（分组 = 课本单元/课）

| 字段 | 说明 |
|---|---|
| `code` | 编号，可自动生成（排序号 / 10），如 `1`、`第3单元` |
| `name` | 显示名 |
| `sort_order` | 决定分组顺序，也决定跨组去重时保留哪一条 |
| `is_archived` | 预留，V1 未使用，查询一律过滤 `= 0` |

### words（词条）

| 字段 | 说明 |
|---|---|
| `group_id` | 外键，`ON DELETE CASCADE` |
| `text` | 归一化后的词语（去控制字符与首尾空白） |
| `note` | 可选备注 |
| `status` | `new` / `learning` / `mastered`，见状态机 |
| `wrong_count` | 累计答错次数，只增不减 |
| `correct_streak` | 连续答对次数，答错归零 |
| `last_practiced_at` | 最近一次被批改的时间 |

唯一约束 `UNIQUE(group_id, text)`：同组不许重词。跨组允许重复，但抽题时会去重。

### sessions（一次默写）

| 字段 | 说明 |
|---|---|
| `mode` | `group`（按分组抽）/ `mistake`（错题专项、重试） |
| `status` | `running` 进行中 / `reviewing` 待批改 / `finished` 已批改 / `abandoned` 已放弃 |
| `config_json` | 抽题参数快照（分组 id、分组名、范围、题量），历史回看靠它解释 |
| `current_seq` | 已推进到的位置，用于中断恢复（**不是**待答词序号） |
| `wrong_total` | 批改后写入的错词数 |

### session_items（会话明细）

| 字段 | 说明 |
|---|---|
| `seq` | 1-based 抽取顺序，展示与复盘用 |
| `skip_marked` | 默写时家长点了「不会」，批改页默认标错 |
| `result` | `pending` / `correct` / `wrong` |

双唯一索引：`UNIQUE(session_id, seq)`、`UNIQUE(session_id, word_id)`。

### settings

键值表，V1 只有一行：`key='app'`，值是一段 JSON（`maskSeconds / streakToMaster / wordFontSize`）。读时解析失败一律回落默认值。

## 不变量

1. 错题本 ≡ `words.status = 'learning'`，没有独立表
2. 一场会话里同一个 `word_id` 最多出现一次（数据库约束）
3. `sessions.total = COUNT(session_items)`，建会话时一次性写入
4. 只有 `word_progress.rs` 能改 `status / wrong_count / correct_streak`
5. `current_seq` ∈ [0, total]，表示「已完成多少个」

## 迁移策略

- `PRAGMA user_version` 记录版本，`src-tauri/src/db/mod.rs` 的 `MIGRATIONS` 数组按版本号执行
- **发布过的脚本永不修改**，只能追加 `000N_xxx.sql` 并在数组末尾登记
- 0002 是 `ALTER TABLE sessions ADD COLUMN current_seq`，已在既有库上实测升级成功

## 备份格式

`backups/backup-YYYYMMDD-HHMMSS.json`，结构：`{ version, exportedAt, groups:[{code,name,words:[{text,note,status,wrongCount,correctStreak}]}] }`。
导入是**合并非覆盖**：同 `code + name` 视为同一分组，词条 `INSERT OR IGNORE`，不修改已有进度。
