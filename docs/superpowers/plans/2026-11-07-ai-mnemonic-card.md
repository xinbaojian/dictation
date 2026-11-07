# AI 错字讲解卡（Phase 1）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 错题复习页为每个错词提供按需生成、本地缓存的「巧记讲解卡」（部件拆解、口诀、例句、易混字辨析），AI 走 OpenAI 兼容协议、显式配置才启用。

**Architecture:** 迁移 0006 新增 `ai_cards` 缓存表 → `domain/ai_card.rs` 纯函数（prompt 构造 + 输出校验）→ `ai/config.rs`（.ai-config.json 凭据）+ `ai/client.rs`（reqwest blocking 客户端）→ `commands/ai.rs` 六命令（生成时「拿锁读词→放锁调 API→拿锁写库」）→ 前端 `MnemonicCard`（复习页分层卡片）+ `AiSettings`（设置页配置区块）。

**Tech Stack:** Rust + Tauri 2 + rusqlite（现有）+ reqwest 0.12（新增，rustls-tls，blocking）；React 18 + TS（现有）。

**设计文档：** `docs/superpowers/specs/2026-11-07-ai-mnemonic-card-design.md`（已确认）

## Global Constraints

- 注释与错误文案一律**中文**；`AppError` 序列化后前端可直接展示
- DTO 一律 `#[serde(rename_all = "camelCase")]`；前端不拼 SQL，所有后端调用只走 `src/lib/ipc.ts`
- 时间戳只用 SQLite `strftime('%Y-%m-%dT%H:%M:%fZ','now')`
- **学习进度只准走 `repo/word_progress.rs`——本计划完全不碰它**
- **凭据红线**：API Key 只存数据目录 `.ai-config.json`（0600 权限），不进 `settings` 表、原值不回传前端（只回掩码）
- 迁移只增不改：新文件 `0006`，注册进 `db/mod.rs::MIGRATIONS`
- **默认零联网**：未配置 AI 时不得新增任何请求路径；网络/文件 I/O 一律 async 命令 + `spawn_blocking`，`MutexGuard` 绝不跨 await
- 新依赖仅 `reqwest`，`default-features = false` + `rustls-tls`（禁拉 openssl-sys）
- 验证基线：`cd src-tauri && cargo test`（当前 136 个通过）、`cargo clippy --all-targets`、`npm run typecheck`（仓库根）；`cargo test` 与 `tauri dev` 争 target 锁，串行跑
- ⚠️ **执行前置**：工作区现有 8 个未提交文件（`src/pages/StrokeReview.tsx`、`src/pages/Settings.tsx`、`src/types.ts`、`src-tauri/src/repo/settings.rs`、`src-tauri/src/repo/sync.rs`、`wiki/data-model.md`、`wiki/decisions.md`、`wiki/frontend.md`），与本计划的文件重叠。**开工前先请用户处理**（提交或确认一并提交），否则各任务的 `git add` 会把无关改动卷进提交。

---

### Task 1: 迁移 0006 与 `repo/ai_cards.rs`（缓存表 CRUD）

**Files:**
- Create: `src-tauri/migrations/0006_ai_cards.sql`
- Modify: `src-tauri/src/db/mod.rs`（MIGRATIONS 数组，约第 7-13 行）
- Create: `src-tauri/src/repo/ai_cards.rs`
- Modify: `src-tauri/src/repo/mod.rs`（加 `pub mod ai_cards;`）

**Interfaces:**
- Consumes: `crate::domain::ai_card::CardContent`（Task 3 定义；本任务先建结构骨架——见步骤 3 的临时定义，Task 3 会替换为正式版。**为避免返工，推荐先执行 Task 3 再回头做本任务的测试步骤**；两个任务的提交仍各自独立）
- Produces:
  - `pub struct AiCardDto { id: i64, word_id: i64, kind: String, content: CardContent, model: String, created_at: String, updated_at: String }`（serde camelCase）
  - `pub fn upsert(conn: &Connection, word_id: i64, kind: &str, content: &CardContent, model: &str) -> AppResult<AiCardDto>`
  - `pub fn get(conn: &Connection, word_id: i64, kind: &str) -> AppResult<Option<AiCardDto>>`
  - `pub fn delete_for_word(conn: &Connection, word_id: i64) -> AppResult<()>`

- [ ] **Step 1: 写迁移 SQL**

创建 `src-tauri/migrations/0006_ai_cards.sql`：

```sql
-- AI 讲解卡缓存：按 (word_id, kind) 唯一。API Key 不入库（在数据目录 .ai-config.json）。
-- 本表内容随整库快照同步（ADR-17）：一台机器生成、两台可用。
-- 词删除靠外键级联；词文本被编辑时由 repo/words.rs::update 主动清卡（讲解针对旧字形）。
CREATE TABLE ai_cards (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  word_id      INTEGER NOT NULL REFERENCES words(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL DEFAULT 'mnemonic',
  content_json TEXT NOT NULL,
  model        TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_ai_cards_word_kind ON ai_cards(word_id, kind);
```

- [ ] **Step 2: 注册迁移**

`src-tauri/src/db/mod.rs` 的 MIGRATIONS 数组末尾追加一行：

```rust
    (6, include_str!("../../migrations/0006_ai_cards.sql")),
```

- [ ] **Step 3: 写 `repo/ai_cards.rs`（含测试）**

```rust
use crate::domain::ai_card::CardContent;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCardDto {
    pub id: i64,
    pub word_id: i64,
    pub kind: String,
    pub content: CardContent,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 生成或重新生成一张卡：同 (word_id, kind) 覆盖（UPSERT，连点两次生成后写者胜）
pub fn upsert(
    conn: &Connection,
    word_id: i64,
    kind: &str,
    content: &CardContent,
    model: &str,
) -> AppResult<AiCardDto> {
    let raw = serde_json::to_string(content)?;
    conn.execute(
        "INSERT INTO ai_cards (word_id, kind, content_json, model,
                               created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(word_id, kind) DO UPDATE SET
           content_json = excluded.content_json,
           model = excluded.model,
           updated_at = excluded.updated_at",
        params![word_id, kind, raw, model],
    )?;
    get(conn, word_id, kind)?.ok_or_else(|| AppError::msg("讲解卡保存失败，请重试"))
}

pub fn get(conn: &Connection, word_id: i64, kind: &str) -> AppResult<Option<AiCardDto>> {
    let row = conn
        .query_row(
            "SELECT id, word_id, kind, content_json, model, created_at, updated_at
             FROM ai_cards WHERE word_id = ?1 AND kind = ?2",
            params![word_id, kind],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((id, word_id, kind, raw, model, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let content: CardContent = serde_json::from_str(&raw)?;
    Ok(Some(AiCardDto { id, word_id, kind, content, model, created_at, updated_at }))
}

/// 词文本被编辑时清卡（由 words::update 调用）；词整删由外键级联兜底
pub fn delete_for_word(conn: &Connection, word_id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM ai_cards WHERE word_id = ?1", params![word_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        let g = crate::repo::groups::create(&conn, None, "1", "第一单元").unwrap();
        (conn, g.id)
    }

    fn sample(mnemonic: &str) -> CardContent {
        CardContent {
            decomposition: vec!["王".into(), "白".into(), "石".into()],
            mnemonic: mnemonic.into(),
            confusables: vec![crate::domain::ai_card::ConfusableHint {
                char: "壁".into(),
                hint: "下面是土，和墙有关".into(),
            }],
            sentences: vec!["湖水碧绿得像一块玉石。".into()],
        }
    }

    #[test]
    fn upsert_roundtrip_and_overwrite() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();

        let first = upsert(&conn, w.id, "mnemonic", &sample("老王爷坐在白石头上"), "test-model")
            .unwrap();
        assert_eq!(first.content.mnemonic, "老王爷坐在白石头上");
        assert_eq!(first.word_id, w.id);

        let second =
            upsert(&conn, w.id, "mnemonic", &sample("换一句口诀"), "test-model-2").unwrap();
        assert_eq!(second.id, first.id, "UPSERT 不换行");
        assert_eq!(second.content.mnemonic, "换一句口诀");
        assert_eq!(second.model, "test-model-2");
    }

    #[test]
    fn get_missing_returns_none() {
        let (conn, _gid) = setup();
        assert!(get(&conn, 999, "mnemonic").unwrap().is_none());
    }

    #[test]
    fn word_delete_cascades() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();
        upsert(&conn, w.id, "mnemonic", &sample("口诀"), "m").unwrap();
        crate::repo::words::delete(&conn, w.id).unwrap();
        assert!(get(&conn, w.id, "mnemonic").unwrap().is_none());
    }

    #[test]
    fn delete_for_word_removes_all_kinds() {
        let (conn, gid) = setup();
        let w = crate::repo::words::add(&conn, gid, "碧绿", None).unwrap();
        upsert(&conn, w.id, "mnemonic", &sample("口诀"), "m").unwrap();
        delete_for_word(&conn, w.id).unwrap();
        assert!(get(&conn, w.id, "mnemonic").unwrap().is_none());
    }
}
```

`src-tauri/src/repo/mod.rs` 加一行：`pub mod ai_cards;`

**依赖说明**：本文件引用的 `CardContent` / `ConfusableHint` 在 Task 3 正式定义。为了让本任务独立可编译，先在 `src-tauri/src/domain/mod.rs` 加 `pub mod ai_card;` 并创建最小骨架（Task 3 会补全——两个任务谁先做都要保证 `cargo test` 绿）：

```rust
//! domain/ai_card.rs（Task 1 时的临时骨架，Task 3 补全校验与 prompt）
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfusableHint {
    pub char: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardContent {
    #[serde(default)]
    pub decomposition: Vec<String>,
    pub mnemonic: String,
    #[serde(default)]
    pub confusables: Vec<ConfusableHint>,
    pub sentences: Vec<String>,
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test ai_cards`
Expected: PASS（4 个测试）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/migrations/0006_ai_cards.sql src-tauri/src/db/mod.rs src-tauri/src/repo/ai_cards.rs src-tauri/src/repo/mod.rs src-tauri/src/domain/mod.rs src-tauri/src/domain/ai_card.rs
git commit -m "feat: ai_cards 讲解卡缓存表与读写（迁移 0006）"
```

---

### Task 2: `words.rs`——单词错字聚合 `marks` + 改文清卡

**Files:**
- Modify: `src-tauri/src/repo/words.rs`（`update` 函数约 173-198 行；文件末尾 tests 模块）

**Interfaces:**
- Consumes: `crate::repo::ai_cards::delete_for_word`（Task 1）
- Produces:
  - `pub struct WordMarks { pub wrong_chars: Vec<i64>, pub whole_word_wrong: i64, pub wrong_count: i64 }`
  - `pub fn marks(conn: &Connection, word_id: i64) -> AppResult<WordMarks>`（AI 生成上下文用：批改标错 + 手工标错合并，字下标升序去重）

- [ ] **Step 1: 写失败测试（追加到 words.rs 的 tests 模块）**

```rust
    #[test]
    fn marks_merge_session_and_manual() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let wid = mark_wrong(&conn, gid, "春天", vec![1]);
        // 手工标记（错题本「添加词语」路径写的 words.wrong_chars）也要并进来
        conn.execute(
            "UPDATE words SET wrong_chars = '0' WHERE id = ?1",
            params![wid],
        )
        .unwrap();
        let m = marks(&conn, wid).unwrap();
        assert_eq!(m.wrong_chars, vec![0, 1], "批改与手工标错合并、升序去重");
        assert_eq!(m.whole_word_wrong, 0);
        assert_eq!(m.wrong_count, 1);
    }

    #[test]
    fn marks_counts_whole_word_wrong() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let wid = mark_wrong(&conn, gid, "春天", vec![]);
        let m = marks(&conn, wid).unwrap();
        assert!(m.wrong_chars.is_empty());
        assert_eq!(m.whole_word_wrong, 1);
    }

    #[test]
    fn update_changed_text_deletes_ai_cards() {
        let (conn, gid) = setup();
        let w = add(&conn, gid, "碧绿", None).unwrap();
        crate::repo::ai_cards::upsert(
            &conn,
            w.id,
            "mnemonic",
            &crate::domain::ai_card::CardContent {
                decomposition: vec![],
                mnemonic: "口诀".into(),
                confusables: vec![],
                sentences: vec!["碧绿的湖水。".into()],
            },
            "m",
        )
        .unwrap();
        // 没改文本：卡保留
        update(&conn, w.id, "碧绿", None).unwrap();
        assert!(crate::repo::ai_cards::get(&conn, w.id, "mnemonic")
            .unwrap()
            .is_some());
        // 改了文本：讲解针对旧字形，必须清掉
        update(&conn, w.id, "碧色", None).unwrap();
        assert!(crate::repo::ai_cards::get(&conn, w.id, "mnemonic")
            .unwrap()
            .is_none());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test marks_ && cargo test update_changed_text`
Expected: FAIL（`marks` 未定义 / 卡未清）

- [ ] **Step 3: 实现**

在 `words.rs` 的 `update` 中，成功 UPDATE 之后、`get(conn, id)` 之前加：

```rust
    // 讲解卡内容针对旧字形，改词后作废（与上面清 wrong_chars 同一理由，ADR-16/20）
    if text_changed {
        crate::repo::ai_cards::delete_for_word(conn, id)?;
    }
```

在 `load_char_marks` 附近新增（放在 `list_mistakes` 之前）：

```rust
/// 单个词的「标错过的字」聚合，给 AI 讲解卡当上下文。
/// 逻辑与 load_char_marks 相同，只是按 word_id 过滤：批改记录 + 手工标记合并，升序去重。
pub struct WordMarks {
    pub wrong_chars: Vec<i64>,
    pub whole_word_wrong: i64,
    pub wrong_count: i64,
}

pub fn marks(conn: &Connection, word_id: i64) -> AppResult<WordMarks> {
    let mut counts: BTreeSet<i64> = BTreeSet::new();
    let mut whole = 0i64;
    {
        let mut stmt = conn.prepare(
            "SELECT wrong_chars FROM session_items
             WHERE word_id = ?1 AND result = 'wrong'",
        )?;
        let rows = stmt.query_map(params![word_id], |r| r.get::<_, String>(0))?;
        for row in rows {
            let chars = parse_char_indices(&row?);
            if chars.is_empty() {
                whole += 1;
            } else {
                counts.extend(chars);
            }
        }
    }
    {
        let raw: Option<String> = conn
            .query_row(
                "SELECT wrong_chars FROM words WHERE id = ?1 AND wrong_chars <> ''",
                params![word_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            counts.extend(parse_char_indices(&raw));
        }
    }
    let wrong_count: i64 = conn.query_row(
        "SELECT wrong_count FROM words WHERE id = ?1",
        params![word_id],
        |r| r.get(0),
    )?;
    Ok(WordMarks {
        wrong_chars: counts.into_iter().collect(),
        whole_word_wrong: whole,
        wrong_count,
    })
}
```

（`BTreeSet` / `OptionalExtension` 文件里已有引入则不重复。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test marks_ && cargo test update_changed_text && cargo test words::`
Expected: PASS（新旧全绿）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/repo/words.rs
git commit -m "feat: 词条改文清讲解卡 + 单词错字聚合 marks"
```

---

### Task 3: `domain/ai_card.rs`——prompt 构造与输出校验（纯函数）

**Files:**
- Modify: `src-tauri/src/domain/ai_card.rs`（替换 Task 1 的骨架，补全校验与 prompt）

**Interfaces:**
- Consumes: 无（纯逻辑，不依赖 Tauri / DB）
- Produces:
  - `pub struct WordContext { pub text: String, pub wrong_chars: Vec<i64>, pub wrong_count: i64 }`
  - `pub struct ChatMessage { pub role: String, pub content: String }`
  - `pub fn build_messages(ctx: &WordContext) -> Vec<ChatMessage>`
  - `pub fn strip_code_fence(raw: &str) -> &str`
  - `pub fn validate(raw: &str, word: &str) -> AppResult<CardContent>`
  - `CardContent` / `ConfusableHint`（Task 1 已定义，本任务不动字段）

- [ ] **Step 1: 写失败测试（追加到 ai_card.rs 的 `#[cfg(test)] mod tests`）**

```rust
    #[cfg(test)]
    mod tests {
        use super::*;

        fn ctx() -> WordContext {
            WordContext { text: "碧绿".into(), wrong_chars: vec![0], wrong_count: 3 }
        }

        const GOOD: &str = r#"{"decomposition":["王","白","石"],"mnemonic":"老王爷坐在白石头上钓鱼","confusables":[{"char":"壁","hint":"下面是土，和墙有关"}],"sentences":["湖水碧绿得像一块玉石。"]}"#;

        #[test]
        fn validates_good_payload() {
            let c = validate(GOOD, "碧绿").unwrap();
            assert_eq!(c.mnemonic, "老王爷坐在白石头上钓鱼");
            assert_eq!(c.decomposition.len(), 3);
            assert_eq!(c.sentences.len(), 1);
        }

        #[test]
        fn strips_code_fence() {
            assert_eq!(
                strip_code_fence("```json\n{ \"a\": 1 }\n```"),
                "{ \"a\": 1 }"
            );
            assert_eq!(strip_code_fence("```\n{ \"a\": 1 }\n```"), "{ \"a\": 1 }");
            assert_eq!(strip_code_fence("  { \"a\": 1 } "), "{ \"a\": 1 }");
        }

        #[test]
        fn rejects_broken_json() {
            let err = validate("我不是 JSON", "碧绿").unwrap_err();
            assert!(err.to_string().contains("JSON"));
        }

        #[test]
        fn rejects_sentence_missing_word() {
            let raw = r#"{"mnemonic":"口诀","sentences":["今天天气很好。"]}"#;
            let err = validate(raw, "碧绿").unwrap_err();
            assert!(err.to_string().contains("碧绿"));
        }

        #[test]
        fn rejects_terms_in_mnemonic_and_sentence() {
            let raw = r#"{"mnemonic":"碧是王白石的形近字口诀","sentences":["碧绿的湖水。"]}"#;
            assert!(validate(raw, "碧绿").unwrap_err().to_string().contains("术语"));
            let raw2 = r#"{"mnemonic":"好口诀","sentences":["碧绿的部首很有趣。"]}"#;
            assert!(validate(raw2, "碧绿").unwrap_err().to_string().contains("术语"));
        }

        #[test]
        fn allows_terms_in_confusable_hint() {
            // 辨析区标「给家长」，术语放宽
            let raw = r#"{"mnemonic":"好口诀","confusables":[{"char":"壁","hint":"部首是土"}],"sentences":["湖水碧绿。"]}"#;
            assert!(validate(raw, "碧绿").is_ok());
        }

        #[test]
        fn rejects_too_long_mnemonic_and_sentences() {
            let long: String = std::iter::repeat('好').take(61).collect();
            let raw = format!(r#"{{"mnemonic":"{long}","sentences":["湖水碧绿。"]}}"#);
            assert!(validate(&raw, "碧绿").unwrap_err().to_string().contains("口诀"));
            let long_sentence: String = std::iter::repeat('美').take(41).collect();
            let raw2 = format!(
                r#"{{"mnemonic":"口诀","sentences":["碧绿的{long_sentence}。"]}}"#
            );
            assert!(validate(&raw2, "碧绿").unwrap_err().to_string().contains("例句"));
        }

        #[test]
        fn rejects_newlines_and_bad_shapes() {
            let raw = "{\"mnemonic\":\"口\\n诀\",\"sentences\":[\"湖水碧绿。\"]}";
            assert!(validate(raw, "碧绿").is_err());
            let raw2 = r#"{"mnemonic":"口诀","sentences":[]}"#;
            assert!(validate(raw2, "碧绿").is_err());
            let raw3 = r#"{"mnemonic":"口诀","sentences":["湖水碧绿。","碧绿的山。","太多例句了。"]}"#;
            assert!(validate(raw3, "碧绿").is_err());
            let raw4 = r#"{"mnemonic":"口诀","confusables":[{"char":"壁墙","hint":"x"}],"sentences":["湖水碧绿。"]}"#;
            assert!(validate(raw4, "碧绿").is_err());
            let raw5 = r#"{"mnemonic":"口诀","decomposition":["王白","石"],"sentences":["湖水碧绿。"]}"#;
            assert!(validate(raw5, "碧绿").is_err());
        }

        #[test]
        fn build_messages_carries_context() {
            let msgs = build_messages(&ctx());
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[0].role, "system");
            assert!(msgs[0].content.contains("二年级"));
            assert!(msgs[1].content.contains("碧绿"));
            assert!(msgs[1].content.contains("「碧」"), "错字下标要翻译成字");
            assert!(msgs[1].content.contains("反复错"), "错 3 次要提示编得特别顺口");
        }

        #[test]
        fn build_messages_without_marks_says_whole_word() {
            let c = WordContext { text: "春天".into(), wrong_chars: vec![], wrong_count: 1 };
            let msgs = build_messages(&c);
            assert!(msgs[1].content.contains("整词写错"));
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test ai_card`
Expected: FAIL（`build_messages` / `validate` / `strip_code_fence` 未定义）

- [ ] **Step 3: 实现（补全 ai_card.rs）**

在骨架基础上追加：

```rust
use crate::error::{AppError, AppResult};

/// 一次讲解卡生成的上下文（repo 组装，domain 只消费）
pub struct WordContext {
    pub text: String,
    pub wrong_chars: Vec<i64>,
    pub wrong_count: i64,
}

/// OpenAI 兼容消息（ai/client.rs 消费）
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

const SYSTEM_PROMPT: &str = "你是给中国小学二年级孩子（7~8 岁）讲语文的老师，孩子刚在默写里写错了词语，你要帮他记住怎么写。请严格按以下要求输出：
1. mnemonic：一句能念出声的记忆口诀，不超过 40 个字，朗朗上口，比喻只用孩子熟悉的事物（动物、学校、家里、玩具、食物）。
2. decomposition：把词语里难写的字拆成部件，每个部件一个汉字，给 2~4 个；确实没法拆就给空数组。
3. confusables：0~3 个和它长得像、孩子容易写混的字，每个给一句「怎么分清它们」的小提示；不要用课堂术语。
4. sentences：1~2 个例句，每句不超过 40 个字，必须完整包含这个词语本身，写二年级孩子的生活场景，用词不超纲。
5. 口诀和例句里不许出现这些词：形近字、音近字、部首、音序、笔顺、笔画、偏旁。所有内容都不要换行。
6. 只输出一个 JSON 对象，不要输出 JSON 以外的任何文字，格式示例：
{\"decomposition\":[\"王\",\"白\",\"石\"],\"mnemonic\":\"老王爷坐在白石头上钓鱼\",\"confusables\":[{\"char\":\"壁\",\"hint\":\"下面是土，和墙有关\"}],\"sentences\":[\"湖水碧绿得像一块玉石。\"]}";

/// 课堂术语黑名单：口诀和例句是孩子直接念的，不许出现；辨析 hint（给家长）放宽
const TERMS: [&str; 7] = ["形近字", "音近字", "部首", "音序", "笔顺", "笔画", "偏旁"];

pub fn build_messages(ctx: &WordContext) -> Vec<ChatMessage> {
    let chars: Vec<char> = ctx.text.chars().collect();
    let wrong = if ctx.wrong_chars.is_empty() {
        "整词写错，没有标具体字".to_string()
    } else {
        let names = ctx
            .wrong_chars
            .iter()
            .filter_map(|i| chars.get(*i as usize).map(|c| c.to_string()))
            .collect::<Vec<_>>()
            .join("、");
        format!("「{names}」")
    };
    let repeat = if ctx.wrong_count >= 3 {
        "\n这个词已经反复错了，请把口诀编得特别顺口好记。"
    } else {
        ""
    };
    vec![
        ChatMessage { role: "system".into(), content: SYSTEM_PROMPT.into() },
        ChatMessage {
            role: "user".into(),
            content: format!(
                "词语：{}\n孩子写错的字：{}\n这个词语已经错了 {} 次。{}",
                ctx.text, wrong, ctx.wrong_count, repeat
            ),
        },
    ]
}

/// LLM 常爱把 JSON 包进 ``` 围栏，先剥掉
pub fn strip_code_fence(raw: &str) -> &str {
    let t = raw.trim();
    let Some(after) = t.strip_prefix("```") else { return t };
    let body = match after.find('\n') {
        Some(i) => &after[i + 1..],
        None => "",
    };
    let body = body.trim();
    match body.strip_suffix("```") {
        Some(b) => b.trim(),
        None => body,
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn is_single_char(s: &str) -> bool {
    char_len(s) == 1
}

fn find_term(text: &str) -> Option<&'static str> {
    TERMS.iter().find(|t| text.contains(*t)).copied()
}

/// LLM 输出的唯一入库关卡：解析 → 规整 → 校验，返回规整后的内容。
/// 错误文案要能同时喂回给 LLM 重试、直接展示给家长。
pub fn validate(raw: &str, word: &str) -> AppResult<CardContent> {
    let clean = strip_code_fence(raw);
    let mut c: CardContent = serde_json::from_str(clean)
        .map_err(|_| AppError::msg("AI 输出不是合法的 JSON（缺字段或格式不对）"))?;
    // 规整：去首尾空白，过滤空项
    c.mnemonic = c.mnemonic.trim().to_string();
    c.decomposition = c
        .decomposition
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    c.confusables = c
        .confusables
        .into_iter()
        .map(|f| ConfusableHint { char: f.char.trim().to_string(), hint: f.hint.trim().to_string() })
        .filter(|f| !f.char.is_empty() && !f.hint.is_empty())
        .collect();
    c.sentences = c
        .sentences
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

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
        return Err(AppError::msg(format!(
            "口诀里用了课堂术语「{t}」，请换成孩子听得懂的说法"
        )));
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
        if !s.contains(word) {
            return Err(AppError::msg(format!("每句例句里都要包含词语「{word}」")));
        }
        if let Some(t) = find_term(s) {
            return Err(AppError::msg(format!(
                "例句里用了课堂术语「{t}」，请换成孩子听得懂的说法"
            )));
        }
    }
    // 部件拆解
    if c.decomposition.len() > 4 {
        return Err(AppError::msg("部件最多拆 4 个"));
    }
    if c.decomposition.iter().any(|s| !is_single_char(s)) {
        return Err(AppError::msg("每个部件必须正好是一个汉字"));
    }
    // 易混字（提示给家长，术语放宽，只查形状与长度）
    if c.confusables.len() > 3 {
        return Err(AppError::msg("易混字最多 3 个"));
    }
    for f in &c.confusables {
        if !is_single_char(&f.char) {
            return Err(AppError::msg("易混字必须正好是一个汉字"));
        }
        if char_len(&f.hint) > 40 {
            return Err(AppError::msg("易混字的提示太长了"));
        }
    }
    Ok(c)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test ai_card`
Expected: PASS（10 个测试全绿）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain/ai_card.rs
git commit -m "feat: 讲解卡 prompt 构造与输出校验（术语黑名单、例句含词）"
```

---

### Task 4: `ai/config.rs`——配置文件读写（掩码、0600）

**Files:**
- Modify: `src-tauri/src/repo/sync.rs`（`write_private` 从私有改 `pub(crate)`，约第 100 行）
- Create: `src-tauri/src/ai/mod.rs`、`src-tauri/src/ai/config.rs`
- Modify: `src-tauri/src/lib.rs`（顶部加 `mod ai;`）

**Interfaces:**
- Consumes: `crate::repo::sync::write_private`（原子落盘 + 0600）
- Produces:
  - `pub struct AiConfig { pub base_url: String, pub api_key: String, pub model: String }`（serde camelCase，磁盘格式）
  - `pub struct AiConfigView { pub configured: bool, pub base_url: String, pub model: String, pub key_mask: String }`（serde camelCase，回前端）
  - `pub fn load(dir: &Path) -> AppResult<Option<AiConfig>>`
  - `pub fn load_required(dir: &Path) -> AppResult<AiConfig>`（未配置时报「请先在设置里配置 AI 服务」）
  - `pub fn save(dir: &Path, base_url: &str, model: &str, api_key: Option<String>) -> AppResult<()>`（`None`/空 = 沿用旧 key）
  - `pub fn clear(dir: &Path) -> AppResult<()>`
  - `pub fn view(dir: &Path) -> AiConfigView`

- [ ] **Step 1: 写失败测试（ai/config.rs 底部）**

`src-tauri/src/ai/mod.rs`：

```rust
//! AI 讲解卡（OpenAI 兼容）：config 管凭据文件，client 管对话调用。
//! 默认不启用；只有数据目录里存在 .ai-config.json 才会有联网行为（ADR-20）。
pub mod client;
pub mod config;
```

（先临时把 `pub mod client;` 注释掉，Task 5 再放开，保证本任务可编译。）

`src-tauri/src/ai/config.rs`：

```rust
use crate::error::{AppError, AppResult};
use crate::repo::sync::write_private;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = ".ai-config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// 回传前端的视图：key 只给掩码，原值绝不离开后端
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigView {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub key_mask: String,
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

pub fn load(dir: &Path) -> AppResult<Option<AiConfig>> {
    let path = config_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn load_required(dir: &Path) -> AppResult<AiConfig> {
    load(dir)?.ok_or_else(|| AppError::msg("请先在设置里配置 AI 服务"))
}

/// `api_key = None` 或空白表示沿用已保存的 key（前端密钥框留空即「不修改」）
pub fn save(dir: &Path, base_url: &str, model: &str, api_key: Option<String>) -> AppResult<()> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let model = model.trim().to_string();
    if base_url.is_empty() || model.is_empty() {
        return Err(AppError::msg("请填写服务地址和模型名"));
    }
    let existing = load(dir).unwrap_or(None);
    let api_key = match api_key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => existing.map(|c| c.api_key).unwrap_or_default(),
    };
    if api_key.is_empty() {
        return Err(AppError::msg("请填写 API Key（只存在本机数据目录，不会上传）"));
    }
    let cfg = AiConfig { base_url, model, api_key };
    write_private(&config_path(dir), serde_json::to_string_pretty(&cfg)?.as_bytes())
}

pub fn clear(dir: &Path) -> AppResult<()> {
    let path = config_path(dir);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// 读失败（文件损坏等）一律视为未配置：设置页仍可重新保存覆盖修复
pub fn view(dir: &Path) -> AiConfigView {
    match load(dir) {
        Ok(Some(c)) => AiConfigView {
            configured: true,
            base_url: c.base_url,
            model: c.model,
            key_mask: mask_key(&c.api_key),
        },
        _ => AiConfigView {
            configured: false,
            base_url: String::new(),
            model: String::new(),
            key_mask: String::new(),
        },
    }
}

/// 掩码：保留头 3 尾 4，中间打码；短 key 全打码
fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}***{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ai-cfg-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_view_masks_key() {
        let dir = tmp_dir();
        save(&dir, "https://api.deepseek.com/", "deepseek-chat", Some("sk-1234567890abcd".into()))
            .unwrap();
        let v = view(&dir);
        assert!(v.configured);
        assert_eq!(v.base_url, "https://api.deepseek.com", "尾部斜杠要归一");
        assert_eq!(v.model, "deepseek-chat");
        assert_eq!(v.key_mask, "sk-***abcd");
        assert!(!serde_json::to_string(&v).unwrap().contains("1234567890"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_key_keeps_old_one() {
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-oldkey12345".into())).unwrap();
        save(&dir, "https://x.example", "m2", None).unwrap();
        let cfg = load(&dir).unwrap().unwrap();
        assert_eq!(cfg.api_key, "sk-oldkey12345");
        assert_eq!(cfg.model, "m2");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_without_any_key_errors() {
        let dir = tmp_dir();
        let err = save(&dir, "https://x.example", "m", None).unwrap_err();
        assert!(err.to_string().contains("API Key"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_then_unconfigured() {
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-abcdef123456".into())).unwrap();
        clear(&dir).unwrap();
        assert!(!view(&dir).configured);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_reports_unconfigured_and_can_be_repaired() {
        let dir = tmp_dir();
        std::fs::write(config_path(&dir), b"{broken").unwrap();
        assert!(!view(&dir).configured);
        save(&dir, "https://x.example", "m", Some("sk-goodkey12345".into())).unwrap();
        assert!(view(&dir).configured);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-secret12345".into())).unwrap();
        let mode = std::fs::metadata(config_path(&dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "凭据文件必须 0600");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

同时：
- `src-tauri/src/repo/sync.rs` 的 `fn write_private(` 改为 `pub(crate) fn write_private(`
- `src-tauri/src/lib.rs` 模块声明区（`mod commands;` 之前）加 `mod ai;`

- [ ] **Step 2: 跑测试确认通过**

Run: `cd src-tauri && cargo test ai::config`
Expected: PASS（6 个测试；macOS 上含 0600 权限检查）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/ai/mod.rs src-tauri/src/ai/config.rs src-tauri/src/repo/sync.rs src-tauri/src/lib.rs
git commit -m "feat: AI 配置文件读写（key 掩码、0600、损坏可修复）"
```

---

### Task 5: `ai/client.rs`——OpenAI 兼容客户端 + 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/ai/client.rs`
- Modify: `src-tauri/src/ai/mod.rs`（放开 `pub mod client;`）

**Interfaces:**
- Consumes: `AiConfig`（Task 4）、`CardContent` / `ChatMessage` / `WordContext` / `build_messages` / `validate`（Task 3）
- Produces:
  - `pub fn build_body(model: &str, messages: &[ChatMessage]) -> serde_json::Value`（纯函数）
  - `pub fn test_connection(cfg: &AiConfig) -> AppResult<String>`
  - `pub fn generate_content(cfg: &AiConfig, ctx: &WordContext) -> AppResult<(CardContent, String)>`（返回 (内容, 模型名)；调用 → 校验 → 不合格把病因回喂重试一次）

- [ ] **Step 1: 加依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]`（`mysql` 那行附近）追加：

```toml
# AI 讲解卡：OpenAI 兼容对话（默认不启用；只有配置了 .ai-config.json 才联网）。
# rustls 避免跨平台拉系统 OpenSSL
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
```

- [ ] **Step 2: 写 `ai/client.rs`（含纯函数测试与一个默认忽略的真服务集成测试）**

```rust
use crate::ai::config::AiConfig;
use crate::domain::ai_card::{self, CardContent, ChatMessage, WordContext};
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// OpenAI 兼容 chat/completions 请求体（纯函数，便于单测）
pub fn build_body(model: &str, messages: &[ChatMessage]) -> Value {
    json!({
        "model": model,
        "messages": messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>(),
        "temperature": 0.7,
        "max_tokens": 500,
        "stream": false,
    })
}

/// 发请求并抽出 choices[0].message.content。30 秒超时；只被 spawn_blocking 调用。
fn post_chat(cfg: &AiConfig, body: &Value) -> AppResult<String> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::msg(format!("初始化 AI 客户端失败：{e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(body)
        .send()
        .map_err(|e| AppError::msg(format!("连接 AI 服务失败：{e}（请检查网络与地址）")))?;
    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| AppError::msg(format!("读取 AI 响应失败：{e}")))?;
    if !status.is_success() {
        let brief: String = text.chars().take(200).collect();
        return Err(AppError::msg(format!("AI 服务返回错误（HTTP {status}）：{brief}")));
    }
    let v: Value = serde_json::from_str(&text)?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::msg("AI 返回内容为空，请重试或换个模型"))
}

/// 设置页「测试连接」：最小请求验证连通与鉴权
pub fn test_connection(cfg: &AiConfig) -> AppResult<String> {
    let body = json!({
        "model": cfg.model,
        "messages": [{ "role": "user", "content": "请只回复四个字：连接正常" }],
        "max_tokens": 16,
        "temperature": 0,
        "stream": false,
    });
    let reply = post_chat(cfg, &body)?;
    let brief: String = reply.chars().take(50).collect();
    Ok(brief)
}

/// 生成讲解卡内容：调用 → domain 校验 → 不合格把病因回喂重试一次。
/// 只做 I/O 编排；校验规则全在 domain::ai_card（有单测）。
pub fn generate_content(cfg: &AiConfig, ctx: &WordContext) -> AppResult<(CardContent, String)> {
    let mut messages = ai_card::build_messages(ctx);
    let raw = post_chat(cfg, &build_body(&cfg.model, &messages))?;
    match ai_card::validate(&raw, &ctx.text) {
        Ok(content) => Ok((content, cfg.model.clone())),
        Err(first) => {
            messages.push(ChatMessage { role: "assistant".into(), content: raw });
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "你上次的输出不合格：{first}。请严格按要求的 JSON 格式重新输出，只输出 JSON 本身。"
                ),
            });
            let raw2 = post_chat(cfg, &build_body(&cfg.model, &messages))?;
            let content = ai_card::validate(&raw2, &ctx.text).map_err(|e| {
                AppError::msg(format!("生成的内容不合格式（{e}），请重试或换个模型"))
            })?;
            Ok((content, cfg.model.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_carries_model_and_messages() {
        let msgs = vec![
            ChatMessage { role: "system".into(), content: "s".into() },
            ChatMessage { role: "user".into(), content: "u".into() },
        ];
        let b = build_body("deepseek-chat", &msgs);
        assert_eq!(b["model"], "deepseek-chat");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "u");
        assert_eq!(b["max_tokens"], 500);
        assert_eq!(b["stream"], false);
    }

    /// 需要真实服务：设 AI_BASE_URL / AI_KEY / AI_MODEL 后
    /// `cargo test real_provider -- --ignored` 手动跑（与 sync 的真实服务器测试同规格）
    #[test]
    #[ignore = "需要真实 API Key"]
    fn real_provider_generates_valid_card() {
        let cfg = AiConfig {
            base_url: std::env::var("AI_BASE_URL").unwrap(),
            api_key: std::env::var("AI_KEY").unwrap(),
            model: std::env::var("AI_MODEL").unwrap(),
        };
        let ctx = WordContext { text: "碧绿".into(), wrong_chars: vec![0], wrong_count: 3 };
        let (content, _) = generate_content(&cfg, &ctx).unwrap();
        assert!(!content.mnemonic.is_empty());
        assert!(content.sentences.iter().all(|s| s.contains("碧绿")));
    }
}
```

`src-tauri/src/ai/mod.rs` 放开 `pub mod client;`。

- [ ] **Step 3: 跑测试确认通过**

Run: `cd src-tauri && cargo test ai::`
Expected: PASS（config 6 个 + client 1 个；ignored 1 个不计）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/ai/client.rs src-tauri/src/ai/mod.rs src-tauri/Cargo.lock
git commit -m "feat: OpenAI 兼容对话客户端（reqwest rustls、校验失败回喂重试）"
```

---

### Task 6: `commands/ai.rs`——六命令与注册（生成锁编排）

**Files:**
- Create: `src-tauri/src/commands/ai.rs`
- Modify: `src-tauri/src/commands/mod.rs`（加 `pub mod ai;`）
- Modify: `src-tauri/src/lib.rs`（invoke_handler 注册 6 条）

**Interfaces:**
- Consumes: Task 1/2/3/4/5 的全部 Produces
- Produces（前端 Task 7 对齐）:
  - `get_ai_config() -> AiConfigView`
  - `save_ai_config(baseUrl: String, model: String, apiKey: Option<String>) -> AppResult<AiConfigView>`
  - `clear_ai_config() -> AiConfigView`
  - `test_ai_config() async -> AppResult<String>`
  - `get_word_card(wordId: i64) -> AppResult<Option<AiCardDto>>`
  - `generate_word_card(wordId: i64) async -> AppResult<AiCardDto>`

- [ ] **Step 1: 写 `commands/ai.rs`**

```rust
use crate::ai::{client, config};
use crate::ai::config::AiConfigView;
use crate::domain::ai_card::WordContext;
use crate::error::{AppError, AppResult};
use crate::repo::{ai_cards, words};
use crate::AppState;
use tauri::State;

/// 网络/文件 I/O 挪去阻塞线程池的统一包装（与 commands/sync.rs 同款）
fn join<T, E>(r: Result<AppResult<T>, E>) -> AppResult<T> {
    match r {
        Ok(v) => v,
        Err(_) => Err(AppError::msg("AI 任务异常，请重试")),
    }
}

#[tauri::command]
pub fn get_ai_config(state: State<'_, AppState>) -> AiConfigView {
    config::view(&state.data_dir)
}

/// `apiKey = None` 表示沿用已保存的 key（前端密钥框留空即不修改）
#[tauri::command]
pub fn save_ai_config(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> AppResult<AiConfigView> {
    config::save(&state.data_dir, &base_url, &model, api_key)?;
    Ok(config::view(&state.data_dir))
}

#[tauri::command]
pub fn clear_ai_config(state: State<'_, AppState>) -> AiConfigView {
    // 清除不存在也算成功：目的是「回到未配置状态」
    let _ = config::clear(&state.data_dir);
    config::view(&state.data_dir)
}

#[tauri::command]
pub async fn test_ai_config(state: State<'_, AppState>) -> AppResult<String> {
    let dir = state.data_dir.clone();
    join(tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load_required(&dir)?;
        client::test_connection(&cfg)
    })
    .await)
}

#[tauri::command]
pub fn get_word_card(
    state: State<'_, AppState>,
    word_id: i64,
) -> AppResult<Option<ai_cards::AiCardDto>> {
    let conn = state.conn()?;
    ai_cards::get(&conn, word_id, "mnemonic")
}

/// 生成/重新生成讲解卡。锁编排（MutexGuard 绝不跨 await）：
/// 拿锁读词与错字上下文 → 放锁 → 无锁等 HTTP（30s 超时 + 回喂重试）→ 拿锁落库。
#[tauri::command]
pub async fn generate_word_card(
    state: State<'_, AppState>,
    word_id: i64,
) -> AppResult<ai_cards::AiCardDto> {
    let ctx = {
        let conn = state.conn()?;
        let word = words::get(&conn, word_id)?;
        let marks = words::marks(&conn, word_id)?;
        WordContext {
            text: word.text,
            wrong_chars: marks.wrong_chars,
            wrong_count: marks.wrong_count,
        }
    };
    let dir = state.data_dir.clone();
    let cfg = join(
        tauri::async_runtime::spawn_blocking(move || config::load_required(&dir)).await,
    )?;
    let (content, model) = join(
        tauri::async_runtime::spawn_blocking(move || client::generate_content(&cfg, &ctx)).await,
    )?;
    let conn = state.conn()?;
    ai_cards::upsert(&conn, word_id, "mnemonic", &content, &model)
}
```

- [ ] **Step 2: 注册**

`src-tauri/src/commands/mod.rs` 加：`pub mod ai;`

`src-tauri/src/lib.rs` 的 `generate_handler!` 列表末尾（`sync_repair_state` 之后）追加：

```rust
            commands::ai::get_ai_config,
            commands::ai::save_ai_config,
            commands::ai::clear_ai_config,
            commands::ai::test_ai_config,
            commands::ai::get_word_card,
            commands::ai::generate_word_card,
```

- [ ] **Step 3: 编译 + 全量后端测试**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets`
Expected: 全部 PASS、clippy 无新警告（命令层是薄编排，逻辑都在已测的 domain/repo/ai 里，符合 ADR-9）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/ai.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: AI 讲解六命令（生成锁编排：拿锁读→放锁调→拿锁写）"
```

---

### Task 7: 前端类型与 ipc 方法

**Files:**
- Modify: `src/types.ts`（`Settings` 接口之后）
- Modify: `src/lib/ipc.ts`（import type 列表 + api 对象「同步」注释块之前）

**Interfaces:**
- Consumes: Task 6 的六个命令名与 DTO 形状
- Produces: `CardContent` / `AiCard` / `AiConfigView` 类型 + `api.getAiConfig / saveAiConfig / clearAiConfig / testAiConfig / getWordCard / generateWordCard`

- [ ] **Step 1: types.ts 追加**

```ts
/** AI 讲解卡内容（对齐 Rust domain::ai_card::CardContent） */
export interface CardContent {
  /** 部件拆解（如 碧 → 王/白/石），0~4 个 */
  decomposition: string[];
  /** 记忆口诀，孩子念的顺口溜 */
  mnemonic: string;
  /** 易混字辨析（给家长看的内容） */
  confusables: { char: string; hint: string }[];
  /** 例句 1~2 条，每条包含词语本身 */
  sentences: string[];
}

export interface AiCard {
  id: number;
  wordId: number;
  kind: string;
  content: CardContent;
  model: string;
  createdAt: string;
  updatedAt: string;
}

/** AI 配置的掩码视图：key 原值永不回传 */
export interface AiConfigView {
  configured: boolean;
  baseUrl: string;
  model: string;
  keyMask: string;
}
```

- [ ] **Step 2: ipc.ts 追加**

import type 列表加 `AiCard, AiConfigView,`（按字母序插入）。api 对象里（`// 同步` 区块之前）：

```ts
  // AI 讲解卡（默认关闭；配置了 .ai-config.json 才会联网）
  getAiConfig: () => invoke<AiConfigView>("get_ai_config"),
  saveAiConfig: (baseUrl: string, model: string, apiKey: string | null) =>
    invoke<AiConfigView>("save_ai_config", { baseUrl, model, apiKey }),
  clearAiConfig: () => invoke<AiConfigView>("clear_ai_config"),
  testAiConfig: () => invoke<string>("test_ai_config"),
  getWordCard: (wordId: number) => invoke<AiCard | null>("get_word_card", { wordId }),
  generateWordCard: (wordId: number) => invoke<AiCard>("generate_word_card", { wordId }),
```

- [ ] **Step 3: 类型检查**

Run: `npm run typecheck`
Expected: PASS（tsc --noEmit 无错误）

- [ ] **Step 4: Commit**

```bash
git add src/types.ts src/lib/ipc.ts
git commit -m "feat: 前端 AI 类型与 ipc 方法"
```

---

### Task 8: `AiSettings` 组件与设置页挂载

**Files:**
- Create: `src/components/AiSettings.tsx`
- Modify: `src/pages/Settings.tsx`（import + 一个 section 的挂载点）

**Interfaces:**
- Consumes: Task 7 的 api 方法；`ConfirmModal`；`Notice["kind"]`
- Produces: `export default function AiSettings({ notify }: { notify: (kind: "ok" | "error", text: string) => void })`

- [ ] **Step 1: 写组件**

```tsx
import { useEffect, useState } from "react";
import ConfirmModal from "./ConfirmModal";
import { api, errorMessage } from "../lib/ipc";
import type { AiConfigView } from "../types";

/** 设置页「AI 讲解」区块：OpenAI 兼容配置（默认关闭，配置后复习页才有「生成讲解」按钮）。
 *  API Key 只存本机数据目录（.ai-config.json），界面永远只显示掩码；留空 = 不修改。 */
export default function AiSettings({ notify }: { notify: (kind: "ok" | "error", text: string) => void }) {
  const [view, setView] = useState<AiConfigView | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [askClear, setAskClear] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const v = await api.getAiConfig();
        setView(v);
        setBaseUrl(v.baseUrl);
        setModel(v.model);
      } catch (e) {
        notify("error", "AI 配置读取失败：" + errorMessage(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const v = await api.saveAiConfig(baseUrl.trim(), model.trim(), apiKey.trim() || null);
      setView(v);
      setApiKey("");
      notify("ok", "AI 配置已保存");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const test = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const reply = await api.testAiConfig();
      notify("ok", "连接正常，模型回复：" + reply);
    } catch (e) {
      notify("error", "连接失败：" + errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const v = await api.clearAiConfig();
      setView(v);
      setBaseUrl("");
      setModel("");
      setApiKey("");
      notify("ok", "AI 配置已清除，应用回到纯离线状态");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return null; // 读取中不占位，避免闪一块空区块

  const inputCls =
    "mt-1 w-full rounded-md border border-stone-200 px-2.5 py-1.5 text-sm text-stone-700 focus:border-amber-400 focus:outline-none";

  return (
    <section className="mt-5 rounded-xl border border-stone-200 bg-white p-5">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-sm font-medium text-stone-700">AI 讲解（可选）</h2>
        <span className={view.configured ? "text-xs text-emerald-600" : "text-xs text-stone-400"}>
          {view.configured ? "已启用" : "默认关闭"}
        </span>
      </div>
      <p className="mt-2 text-xs leading-5 text-stone-500">
        配置后可在错题的笔画复习页生成「巧记讲解卡」（部件拆解、记忆口诀、例句、易混字辨析）。
        生成时会把<strong>词语本身</strong>发送到你配置的服务，除此之外不发送任何数据；
        内容缓存在本机，生成一次之后离线也能看。不配置则完全离线，与现在一样。
      </p>
      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        <label className="block text-xs font-medium text-stone-600">
          服务地址（Base URL）
          <input
            className={inputCls}
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://api.deepseek.com 或 https://open.bigmodel.cn/api/paas/v4"
          />
        </label>
        <label className="block text-xs font-medium text-stone-600">
          模型名
          <input
            className={inputCls}
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="deepseek-chat / glm-4-flash 等"
          />
        </label>
        <label className="block text-xs font-medium text-stone-600 sm:col-span-2">
          API Key
          <input
            type="password"
            className={inputCls}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={
              view.configured
                ? `已保存 ${view.keyMask}（留空不修改）`
                : "sk-…（只存在本机数据目录，不上传）"
            }
          />
        </label>
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={busy}
          className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-50"
        >
          保存配置
        </button>
        <button
          onClick={() => void test()}
          disabled={!view.configured || busy}
          className="rounded-md border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-40"
        >
          测试连接
        </button>
        {view.configured && (
          <button
            onClick={() => setAskClear(true)}
            className="px-1 text-xs text-stone-400 hover:text-rose-600"
          >
            清除配置
          </button>
        )}
      </div>

      {askClear && (
        <ConfirmModal
          title="清除 AI 配置？"
          body={
            "清除后复习页不再出现「生成讲解」按钮，应用回到纯离线。\nAPI Key 会从数据目录删除；已生成的讲解卡仍保留在本机。"
          }
          confirmText="清除"
          cancelText="再想想"
          onCancel={() => setAskClear(false)}
          onConfirm={() => {
            setAskClear(false);
            void clear();
          }}
        />
      )}
    </section>
  );
}
```

- [ ] **Step 2: 挂到 Settings.tsx**

读取当前 `src/pages/Settings.tsx`（注意：工作区可能有未提交改动，以实际内容为准）：

1. 顶部加 `import AiSettings from "../components/AiSettings";`
2. 找到「笔画复习」section 的 `</section>` 结束标签与「数据与备份」section 之间，插入一行：

```tsx
      <AiSettings notify={notify} />
```

（`notify` 来自 Settings 组件里现成的 `useNotice()`。）

- [ ] **Step 3: 类型检查 + 构建**

Run: `npm run typecheck`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/components/AiSettings.tsx src/pages/Settings.tsx
git commit -m "feat: 设置页 AI 讲解配置区块（key 掩码、测试连接、清除确认）"
```

---

### Task 9: `MnemonicCard` 组件与复习页集成

**Files:**
- Create: `src/components/MnemonicCard.tsx`
- Modify: `src/pages/StrokeReview.tsx`

**Interfaces:**
- Consumes: Task 7 的 api/类型；现有 `speak_text`
- Produces: 复习页田字格卡下方的「✨ 巧记」卡（组件签名如下）

```ts
interface MnemonicCardProps {
  word: Mistake;
  card: AiCard | null;
  aiReady: boolean;       // AI 已配置（无缓存卡时才显示「生成」按钮）
  masked: boolean;        // 「先自己写」遮蔽中：整卡隐藏（口诀=答案提示）
  generating: boolean;
  autoSpeak: boolean;     // 展开时自动读口诀（与翻词自动朗读共用偏好）
  onGenerate: () => void;
  onSpeak: (text: string) => void;
}
```

- [ ] **Step 1: 写组件**

```tsx
import { useEffect, useState } from "react";
import type { AiCard, Mistake } from "../types";

interface MnemonicCardProps {
  word: Mistake;
  card: AiCard | null;
  /** AI 已配置（无缓存卡时才显示「生成」按钮）；没配置也可能有同步来的缓存卡 */
  aiReady: boolean;
  /** 「先自己写」遮住笔画中：口诀与部件等于答案提示，整卡隐藏 */
  masked: boolean;
  generating: boolean;
  autoSpeak: boolean;
  onGenerate: () => void;
  onSpeak: (text: string) => void;
}

/** 部件积木块的循环配色（沿用应用暖色系，不引新色板） */
const BLOCK_BG = ["bg-amber-100", "bg-emerald-100", "bg-sky-100", "bg-rose-100"];

/** 复习页「✨ 巧记」卡：孩子先看（部件积木 → 大字口诀 → 例句），家长补充（易混字辨析） */
export default function MnemonicCard({
  word,
  card,
  aiReady,
  masked,
  generating,
  autoSpeak,
  onGenerate,
  onSpeak,
}: MnemonicCardProps) {
  const [open, setOpen] = useState(false);
  const c = card?.content ?? null;

  // 展开时自动读口诀：二年级识字量有限，听觉是主通道；与「自动朗读」共用一个偏好
  useEffect(() => {
    if (open && autoSpeak && c) onSpeak(c.mnemonic);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (masked) return null;
  if (!c && !aiReady) return null;

  // 没有缓存卡：一个按钮直接触发生成
  if (!c) {
    return (
      <button
        onClick={onGenerate}
        disabled={generating}
        className="rounded-full border border-amber-300 bg-amber-50 px-5 py-2 text-sm text-amber-700 hover:bg-amber-100 disabled:cursor-wait disabled:opacity-60"
      >
        {generating ? "讲解生成中，稍等…" : "✨ 生成巧记讲解"}
      </button>
    );
  }

  const wordChars = new Set([...word.text]);
  return (
    <div className="w-full rounded-2xl border border-amber-200 bg-amber-50/60 px-6 py-4">
      <div className="flex items-center justify-between">
        <button
          onClick={() => setOpen(!open)}
          className="text-sm font-medium text-amber-700 hover:text-amber-800"
        >
          ✨ 巧记 {open ? "▴" : "▾"}
        </button>
        <div className="flex items-center gap-2">
          <button
            onClick={() => onSpeak(c.mnemonic)}
            title="用系统语音读一遍口诀"
            className="rounded-md border border-amber-200 bg-white px-2.5 py-1 text-xs text-stone-600 hover:bg-amber-50"
          >
            🔊 读口诀
          </button>
          <button
            onClick={onGenerate}
            disabled={generating}
            title="重新生成（会覆盖当前内容）"
            className="rounded-md px-2 py-1 text-xs text-stone-400 hover:text-amber-700 disabled:opacity-50"
          >
            {generating ? "生成中…" : "重新生成"}
          </button>
        </div>
      </div>
      {open && (
        <div className="mt-3 flex flex-col gap-3">
          {c.decomposition.length >= 2 && (
            <div className="flex items-end justify-center gap-2">
              {c.decomposition.map((part, i) => (
                <span
                  key={i}
                  className={
                    "flex h-14 w-14 items-center justify-center rounded-xl text-3xl text-stone-700 " +
                    BLOCK_BG[i % BLOCK_BG.length]
                  }
                >
                  {part}
                </span>
              ))}
              <span className="pb-1 text-xs text-stone-400">合起来就是它</span>
            </div>
          )}
          <p className="text-center text-2xl leading-relaxed text-stone-800">
            {[...c.mnemonic].map((ch, i) => (
              <span
                key={i}
                className={wordChars.has(ch) ? "rounded bg-amber-200/80 px-0.5" : ""}
              >
                {ch}
              </span>
            ))}
          </p>
          {c.sentences.map((s, i) => (
            <p key={i} className="text-center text-base text-stone-600">
              {s.split(word.text).map((part, j, arr) => (
                <span key={j}>
                  {part}
                  {j < arr.length - 1 && (
                    <u className="decoration-amber-500 decoration-2 underline-offset-4">
                      {word.text}
                    </u>
                  )}
                </span>
              ))}
            </p>
          ))}
          {c.confusables.length > 0 && (
            <div className="rounded-lg bg-white/70 px-3 py-2">
              <p className="text-[11px] text-stone-400">👤 给家长：孩子容易写混的字</p>
              <ul className="mt-1 flex flex-col gap-0.5">
                {c.confusables.map((f, i) => (
                  <li key={i} className="text-xs text-stone-600">
                    {f.char}：{f.hint}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: 集成进 StrokeReview.tsx（以当前文件实际内容为准）**

1. import 区加：

```tsx
import MnemonicCard from "../components/MnemonicCard";
import type { AiCard } from "../types";
```

（`Mistake` 已在 import type 列表里。）

2. state 区（`autoSpeak` 附近）加：

```tsx
  // AI 讲解卡：aiReady 决定「生成」按钮；card 可能来自同步快照（未配置 AI 也显示）
  const [aiReady, setAiReady] = useState(false);
  const [card, setCard] = useState<AiCard | null>(null);
  const [generating, setGenerating] = useState(false);
```

3. 初次加载的 `useEffect` 里，`Promise.all` 增加一项 `api.getAiConfig()`：

```tsx
        const [list, cfg, aiCfg] = await Promise.all([
          api.listMistakes(groupId),
          api.getSettings(),
          api.getAiConfig(),
        ]);
        setAiReady(aiCfg.configured);
```

4. 现有 `speak` 回调改为通用版（读词与读口诀共用）：

```tsx
  const speakText = useCallback(
    async (text: string) => {
      try {
        await api.speakText(text);
      } catch (e) {
        // 朗读失败不该打断复习，但也不能无声无息让家长误以为孩子听过音了
        notify("error", "朗读失败：" + errorMessage(e));
      }
    },
    [notify],
  );

  const speak = useCallback(() => {
    if (word) void speakText(word.text);
  }, [word, speakText]);
```

（原 `speak` 的自动朗读 effect 与调用点不变，只是内部走 `speakText`。）

5. 翻词时取当前词的缓存卡（本地查询，快）；失败静默——没有卡不该报错：

```tsx
  useEffect(() => {
    setCard(null);
    const id = word?.id;
    if (id == null) return;
    void api
      .getWordCard(id)
      .then(setCard)
      .catch(() => {});
  }, [word?.id]);
```

6. 生成回调：

```tsx
  const generate = useCallback(async () => {
    if (!word || generating) return;
    setGenerating(true);
    try {
      setCard(await api.generateWordCard(word.id));
    } catch (e) {
      notify("error", "讲解生成失败：" + errorMessage(e));
    } finally {
      setGenerating(false);
    }
  }, [word, generating, notify]);
```

7. 渲染：田字格卡片的容器 `<div className="flex min-h-72 w-full ...">…</div>` 之后、键盘提示 `<p className="text-xs text-stone-400">` 之前插入：

```tsx
          <MnemonicCard
            word={word}
            card={card}
            aiReady={aiReady}
            masked={masked}
            generating={generating}
            autoSpeak={autoSpeak}
            onGenerate={() => void generate()}
            onSpeak={(t) => void speakText(t)}
          />
```

- [ ] **Step 3: 类型检查**

Run: `npm run typecheck && npm run build`
Expected: PASS

- [ ] **Step 4: 手工验证（可选但推荐）**

Run: `npm run tauri dev`（与 cargo test 串行，别同时跑）
验证点：① 未配置 AI 时复习页与现在完全一致；② 设置页配好 AI 后，错题复习页出现「✨ 生成巧记讲解」，生成后展开四层内容；③ 「先自己写」遮住时整卡消失；④ 「读口诀」出声。

- [ ] **Step 5: Commit**

```bash
git add src/components/MnemonicCard.tsx src/pages/StrokeReview.tsx
git commit -m "feat: 复习页巧记讲解卡（孩子视角分层：积木→口诀→例句→给家长辨析）"
```

---

### Task 10: 全量验证与文档（ADR-20、wiki、AGENTS）

**Files:**
- Modify: `wiki/decisions.md`、`wiki/architecture.md`、`wiki/data-model.md`、`wiki/frontend.md`、`wiki/overview.md`、`AGENTS.md`

**Interfaces:**
- Consumes: Task 1-9 的最终实现
- Produces: 文档与实现一致（ADR-20、命令数 47/五组、ai_cards 表、新组件）

- [ ] **Step 1: 三件套全量验证**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets`，然后仓库根 `npm run typecheck`
Expected: cargo test 全绿（136 + 新增 ≈ 20 个）、clippy 无新警告、tsc 无错误

- [ ] **Step 2: wiki/decisions.md 追加 ADR-20**

在文件末尾（ADR-19 之后）追加：

```markdown
## ADR-20 AI 讲解卡：显式配置的 OpenAI 兼容客户端 + 内容缓存表

- **结论**：错题复习页可按需生成「巧记讲解卡」（部件拆解、口诀、例句、易混字辨析）。AI 服务为 OpenAI 兼容 chat completions（DeepSeek / 智谱 / 硅基流动等任填），配置存数据目录 `.ai-config.json`（0600；凭据不进 `settings` 表，key 原值不回传前端，只回掩码）；生成内容存 `ai_cards` 表（随整库快照同步），按词缓存、生成一次离线复用
- **理由**：「怎么记住这个字」是无法预计算的个性化内容，是 LLM 强项而家长最缺。缓存 + 显式配置让默认安装保持零联网——与 ADR-17 的 MySQL 同步同构：又一个「可选的外挂」，不是产品定位的转向
- **红线**：LLM 只产内容，**永不写学习进度**（`word_progress` 不变，讲解不是进度）；输出必须过 `domain/ai_card.rs::validate`（术语黑名单、例句含目标词、长度上限、无换行）才入库；讲解卡在「先自己写」遮蔽模式下整卡隐藏（口诀=答案提示）；面向孩子的内容层（积木/口诀/例句）与给家长的辨析层分开呈现
- **实现**：`commands/ai.rs` 六命令（生成走「拿锁读词→放锁等 HTTP→拿锁落库」，`MutexGuard` 不跨 await）；`ai/config.rs` + `ai/client.rs`（reqwest rustls，30s 超时，校验失败回喂重试一次）；词文本被编辑时清卡（对齐 ADR-16 清手工错字的先例）；新依赖仅 `reqwest`
- **边界**：Phase 2 在线 TTS（智谱 GLM-TTS 等，`/audio/speech`）将复用同一配置文件与 opt-in 边界，届时另立 ADR-21 修订 ADR-19 的「不引联网引擎」
```

- [ ] **Step 3: 其余 wiki 与 AGENTS 对齐**

- `wiki/architecture.md`：IPC 契约一段改为「共 47 个命令，分五组：`commands/library.rs`（词表 11）、`commands/quiz.rs`（默写/错题 14）、`commands/system.rs`（设置/备份 9）、`commands/sync.rs`（跨设备同步 7）、`commands/ai.rs`（AI 讲解 6，默认关闭）」；分层图加一行 `ai/  # OpenAI 兼容客户端与配置（ADR-20）`
- `wiki/data-model.md`：按该页现有格式补 `ai_cards` 表说明（字段、`(word_id, kind)` 唯一、级联删、改文清卡、随快照同步）
- `wiki/frontend.md`：StrokeReview 一行补「AI 巧记讲解卡（`components/MnemonicCard.tsx`，配置 AI 后可按需生成，遮蔽模式整卡隐藏，展开可自动读口诀）」；组件表加 `MnemonicCard` / `AiSettings` 两行；Settings 一行补「AI 讲解配置区块」
- `wiki/overview.md`：「跨设备同步是可选的外挂」段落附近补一句「AI 讲解同理：只有在设置里配置了 OpenAI 兼容服务才会联网，且只发送词语本身（ADR-20）」
- `AGENTS.md`：目录树 `src-tauri/src/` 下补 `ai/  # AI 讲解卡客户端与配置（ADR-20，默认关闭）`；「commands/ library.rs（11）/ quiz.rs（14）/ system.rs（9）/ sync.rs（7）（薄封装，共 41 个命令）」改为「…sync.rs（7）/ ai.rs（6）（薄封装，共 47 个命令）」
- 各页 frontmatter 的 `updated` 改为当日日期

- [ ] **Step 4: Commit**

```bash
git add wiki/decisions.md wiki/architecture.md wiki/data-model.md wiki/frontend.md wiki/overview.md AGENTS.md
git commit -m "docs: ADR-20 与 wiki/AGENTS 登记 AI 讲解卡"
```

---

### Task 11: 干净子 agent 审查（AGENTS.md 审查流程）

**Files:**
- Modify: `.workbuddy/memory/2026-11-07.md`（记录审查结论）

**Interfaces:**
- Consumes: Task 1-10 的全部改动与验证结果
- Produces: 审查报告 → 主会话逐条判定 → 修复（带复现测试）→ 复审 → memory 记录

- [ ] **Step 1: 起一个全新子 agent 审查（只读，不改代码）**

prompt 必须自包含，包含：① `git status --short` + `git diff bdf5bcc..HEAD --stat` 的输出（bdf5bcc 是本计划第一个提交的前基线）；② 要求读全部改动文件的全文而非只看 diff；③ 硬约束清单（AGENTS.md「硬约束」一节 + ADR-3/10/14/16/17/18/19/20）；④ 已跑通的验证（Task 10 Step 1 的命令与结果）；⑤ 输出格式：每条问题给 `文件:行号` + 触发路径 + 严重度（P1 阻断 / P2 建议）+ 最小修法；⑥ 明确「只读，不改代码；没有问题就直说未发现问题」。

- [ ] **Step 2: 主会话逐条判定**

真问题 → 修复并补一条能复现的单测；误报 → 记下理由。下一轮审查 prompt 里显式列出「已判定为非问题：X，理由 Y」。

- [ ] **Step 3: 修复后重跑三件套，再起新的干净子 agent 复审**

收敛条件：某轮既无 P1 也无「本轮修复新引入」的 P2。剩余 P2 若判定可接受，在 prompt 与提交说明里写明「已知取舍」。

- [ ] **Step 4: 把每轮结论写入 `.workbuddy/memory/2026-11-07.md`**

格式：发现几条、哪几条是真问题、怎么修的、哪几条判为误报及理由。

- [ ] **Step 5: Commit**

```bash
git add .workbuddy/memory/2026-11-07.md
git commit -m "docs: AI 讲解卡审查记录（N 轮收敛）"
```
