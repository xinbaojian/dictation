# 错题本编辑 + 批量加入 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 错题本支持行内编辑（改文本 + 重标错字）与批量粘贴加入（分隔符切分，已在错题本的跳过）。

**Architecture:** 后端两个新命令 `add_mistakes` / `update_mistake`（repo 层单事务，复用 `add_mistake` / `update` 既有语义）；前端 `AddMistakeModal` 加双模式开关（单个/批量），新 `EditMistakeModal` 组件接入 `Mistakes.tsx` 操作列。设计文档：`docs/superpowers/specs/2026-11-18-mistake-batch-edit-design.md`。

**Tech Stack:** Rust (rusqlite) + React 18 + TypeScript，无新依赖。

## Global Constraints

- 学习进度（`status / wrong_count / correct_streak`）只能经 `repo/word_progress.rs` 修改——本次两命令均不得直接 UPDATE 这些列。
- 错题本等价于 `words.status = 'learning'`，不建新表。
- 前端不拼 SQL，一切 IPC 走 `src/lib/ipc.ts`；类型加 `src/types.ts`。
- `update_mistake` 里调 `words::update` 时必须先把原 note 读出来传回去，**传 `None` 会清空备注**。
- 时间写库一律 SQLite `strftime('%Y-%m-%dT%H:%M:%fZ','now')`（本次无需新写时间字段）。
- 错误统一 `AppError` 中文消息直出 UI。

---

### Task 1: 后端 `add_mistakes`（repo + 命令 + ipc 端到端）

**Files:**
- Modify: `src-tauri/src/repo/words.rs`（`add_mistake` 函数后新增；`WordDto` 后新增 `BatchMistakeResult`；tests 内新增用例）
- Modify: `src-tauri/src/commands/quiz.rs:111-121`（`add_mistake` 命令后新增）
- Modify: `src-tauri/src/lib.rs:87`（注册表 `add_mistake,` 后加一行）
- Modify: `src/lib/ipc.ts:73`（`addMistake` 后新增）
- Modify: `src/types.ts:48`（`ImportResult` 后新增 `BatchMistakeResult`）

**Interfaces:**
- Consumes: `words::add_mistake(&Connection, i64, &str, &[i64]) -> AppResult<WordDto>`（既有）；`normalize_word`（既有）。
- Produces: `words::add_mistakes(&Connection, i64, &[String]) -> AppResult<BatchMistakeResult>`；命令 `add_mistakes(state, group_id, texts)`；前端 `api.addMistakes(groupId: number, texts: string[]) => Promise<BatchMistakeResult>`；类型 `BatchMistakeResult { added: number; skippedExisting: number }`。Task 4 的批量提交依赖 `api.addMistakes`。

- [ ] **Step 1: 写失败测试**（`src-tauri/src/repo/words.rs` tests 模块末尾，`add_mistake_reuses_existing_word_and_merges_chars` 之后）

```rust
    #[test]
    fn add_mistakes_creates_and_reuses() {
        let (conn, gid) = setup();
        add(&conn, gid, "春天", None).unwrap();
        let r = add_mistakes(&conn, gid, &["春天".into(), "夏天".into(), "秋天".into()]).unwrap();
        assert_eq!(r.added, 3, "两个新词建词条、一个已有词条复用，都标错");
        assert_eq!(r.skipped_existing, 0);
        let m = list_mistakes(&conn, None).unwrap();
        assert_eq!(m.len(), 3);
        // 已有词条复用：错误次数从 0 涨到 1，词条 id 不变
        let reused = m.iter().find(|x| x.text == "春天").unwrap();
        assert_eq!(reused.wrong_count, 1);
        assert!(m.iter().find(|x| x.text == "夏天").is_some());
    }

    #[test]
    fn add_mistakes_skips_learning_without_counting() {
        let (conn, gid) = setup();
        add_mistake(&conn, gid, "春天", &[]).unwrap();
        let before = list_mistakes(&conn, None).unwrap()[0].wrong_count;
        let r =
            add_mistakes(&conn, gid, &["春天".into(), "夏天".into()]).unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(r.skipped_existing, 1);
        let after = list_mistakes(&conn, None)
            .unwrap()
            .into_iter()
            .find(|x| x.text == "春天")
            .unwrap()
            .wrong_count;
        assert_eq!(before, after, "已在错题本的词跳过，错误次数不虚增");
    }

    #[test]
    fn add_mistakes_rejects_blank_word_atomically() {
        let (conn, gid) = setup();
        let err = add_mistakes(&conn, gid, &["春天".into(), "  ".into()]).unwrap_err();
        assert!(err.to_string().contains("词语不能为空"));
        assert!(
            list(&conn, gid).unwrap().is_empty(),
            "事务回滚：第一条不能留下半批数据"
        );
        assert!(
            list_mistakes(&conn, None).unwrap().is_empty(),
            "错题本也不能有残留"
        );
    }

    #[test]
    fn add_mistakes_blank_input_ok() {
        let (conn, gid) = setup();
        let r = add_mistakes(&conn, gid, &[]).unwrap();
        assert_eq!(r.added, 0);
        assert_eq!(r.skipped_existing, 0);
    }
```

注：空输入（`texts = []`）返回 `added=0` 合法——前端提交按钮已禁用，这里是兜底行为钉住，不报错。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test add_mistakes`
Expected: 编译错误 `cannot find function add_mistakes`（以及 `BatchMistakeResult` 未定义）。

- [ ] **Step 3: 实现**（`src-tauri/src/repo/words.rs`：`WordDto` 定义后加类型，`add_mistake` 函数后加函数）

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMistakeResult {
    pub added: i64,
    pub skipped_existing: i64,
}
```

```rust
/// 批量把词语加进错题本，单事务。
///
/// 已在该分组错题本（status='learning'）的词跳过且不重复计错误次数——
/// 手工加入是「标记为错题」的幂等操作，重复粘贴同一份清单不虚增 wrong_count。
/// 其余词逐条复用 `add_mistake` 语义（词条存在则复用 + mark_wrong，否则新建）。
pub fn add_mistakes(
    conn: &Connection,
    group_id: i64,
    texts: &[String],
) -> AppResult<BatchMistakeResult> {
    let tx = conn.unchecked_transaction()?;
    let mut added = 0i64;
    let mut skipped_existing = 0i64;
    for raw in texts {
        let text = normalize_word(raw);
        if text.is_empty() {
            return Err(AppError::msg("词语不能为空"));
        }
        let already: Option<i64> = tx
            .query_row(
                "SELECT id FROM words
                 WHERE group_id = ?1 AND text = ?2 AND status = 'learning'",
                params![group_id, &text],
                |r| r.get(0),
            )
            .optional()?;
        if already.is_some() {
            skipped_existing += 1;
            continue;
        }
        add_mistake(&tx, group_id, &text, &[])?;
        added += 1;
    }
    tx.commit()?;
    Ok(BatchMistakeResult { added, skipped_existing })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test add_mistakes`
Expected: 4 个新用例 PASS（连同既有 `add_mistake_*` 用例）。

- [ ] **Step 5: 命令层 + 注册 + ipc + 类型**

`src-tauri/src/commands/quiz.rs`，`add_mistake` 命令之后：

```rust
/// 批量把词语加进错题本；已在错题本（learning）的词跳过、不重复计数
#[tauri::command]
pub fn add_mistakes(
    state: State<'_, AppState>,
    group_id: i64,
    texts: Vec<String>,
) -> AppResult<words::BatchMistakeResult> {
    let conn = state.conn()?;
    words::add_mistakes(&conn, group_id, &texts)
}
```

`src-tauri/src/lib.rs` 注册表，`commands::quiz::add_mistake,` 之后加：

```rust
            commands::quiz::add_mistakes,
```

`src/lib/ipc.ts`，`addMistake` 之后：

```ts
  /** 批量加入错题本；已在错题本的词后端跳过不重复计数 */
  addMistakes: (groupId: number, texts: string[]) =>
    invoke<BatchMistakeResult>("add_mistakes", { groupId, texts }),
```

（文件顶部 import type 列表补 `BatchMistakeResult`。）

`src/types.ts`，`ImportResult` 之后：

```ts
export interface BatchMistakeResult {
  added: number;
  skippedExisting: number;
}
```

- [ ] **Step 6: 验证 + 提交**

Run: `cd src-tauri && cargo test` 和 `npm run typecheck`
Expected: 全部 PASS / 无错误。

```bash
git add src-tauri/src/repo/words.rs src-tauri/src/commands/quiz.rs src-tauri/src/lib.rs src/lib/ipc.ts src/types.ts
git commit -m "feat(quiz): add_mistakes 批量加入错题本（跳过已存在、单事务）"
```

---

### Task 2: 后端 `update_mistake`（repo + 命令 + ipc 端到端）

**Files:**
- Modify: `src-tauri/src/repo/words.rs`（Task 1 的 `add_mistakes` 后新增函数；tests 新增用例）
- Modify: `src-tauri/src/commands/quiz.rs`（Task 1 的 `add_mistakes` 命令后新增）
- Modify: `src-tauri/src/lib.rs`（`commands::quiz::add_mistakes,` 后注册）
- Modify: `src/lib/ipc.ts`（`addMistakes` 后新增）
- Modify: `src-tauri/src/commands/quiz.rs` 顶部如需补 `WordDto` import（已有 `words::WordDto`，无需）

**Interfaces:**
- Consumes: `words::update(&Connection, i64, &str, Option<&str>)`（既有——文本变化时清 wrong_chars、删 AI 卡、重名报错）；`words::get`、`format_char_indices`（既有）。
- Produces: `words::update_mistake(&Connection, i64, &str, &[i64]) -> AppResult<WordDto>`；命令 `update_mistake(state, word_id, text, wrong_chars)`；前端 `api.updateMistake(wordId: number, text: string, wrongChars: number[]) => Promise<Word>`。Task 3 的编辑弹窗依赖它。

- [ ] **Step 1: 写失败测试**（`words.rs` tests，Task 1 用例之后）

```rust
    #[test]
    fn update_mistake_only_chars_keeps_progress() {
        let (conn, gid) = setup();
        let w = add_mistake(&conn, gid, "春天", &[]).unwrap();
        update_mistake(&conn, w.id, "春天", &[0]).unwrap();
        let m = list_mistakes(&conn, None).unwrap();
        assert_eq!(m[0].wrong_chars, vec![0]);
        assert_eq!(m[0].wrong_count, 1, "编辑不是又错一次，不推进度");
        // status 仍是 learning（还在错题本）
        assert_eq!(get(&conn, w.id).unwrap().status, "learning");
    }

    #[test]
    fn update_mistake_text_change_rewrites_chars_and_clears_ai_cards() {
        let (conn, gid) = setup();
        let w = add_mistake(&conn, gid, "春天", &[1]).unwrap();
        crate::repo::ai_cards::upsert(
            &conn,
            w.id,
            "mnemonic",
            &crate::domain::ai_card::CardContent {
                decomposition: vec![],
                mnemonic: "口诀".into(),
                confusables: vec![],
                sentences: vec![],
            },
            "m",
        )
        .unwrap();
        update_mistake(&conn, w.id, "春风", &[0, 9]).unwrap();
        let m = list_mistakes(&conn, None).unwrap();
        assert_eq!(m[0].wrong_chars, vec![0], "改文本后重标，越界下标 9 丢弃");
        assert!(
            crate::repo::ai_cards::get(&conn, w.id, "mnemonic").unwrap().is_none(),
            "讲解卡针对旧字形，改文本后应删除"
        );
    }

    #[test]
    fn update_mistake_name_conflict_rolls_back() {
        let (conn, gid) = setup();
        let a = add_mistake(&conn, gid, "春天", &[0]).unwrap();
        add(&conn, gid, "夏天", None).unwrap();
        let err = update_mistake(&conn, a.id, "夏天", &[1]).unwrap_err();
        assert!(err.to_string().contains("已有「夏天」"));
        // 回滚：wrong_chars 不能残留半改状态
        let m = list_mistakes(&conn, None)
            .unwrap()
            .into_iter()
            .find(|x| x.id == a.id)
            .unwrap();
        assert_eq!(m.wrong_chars, vec![0], "报错时事务回滚，手工标错保持原样");
    }

    #[test]
    fn update_mistake_preserves_note() {
        let (conn, gid) = setup();
        let w = add(&conn, gid, "春天", Some("课文第 4 课")).unwrap();
        add_mistake(&conn, gid, "春天", &[]).unwrap();
        update_mistake(&conn, w.id, "春天", &[0]).unwrap();
        assert_eq!(get(&conn, w.id).unwrap().note.as_deref(), Some("课文第 4 课"));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test update_mistake`
Expected: 编译错误 `cannot find function update_mistake`。

- [ ] **Step 3: 实现**（`words.rs`，`add_mistakes` 函数后）

```rust
/// 编辑错题词条：改文本（复用 `update` 语义：清失效错字下标、删作废 AI 卡、
/// 同组重名报错）后覆盖写手工标错字。不动 status / wrong_count / correct_streak。
pub fn update_mistake(
    conn: &Connection,
    word_id: i64,
    text: &str,
    chars: &[i64],
) -> AppResult<WordDto> {
    let tx = conn.unchecked_transaction()?;
    // update 的 note 参数传 None 会清空备注，必须先读回原值传回去
    let note = get(&tx, word_id)?.note;
    update(&tx, word_id, text, note.as_deref())?;
    let text = normalize_word(text);
    let len = text.chars().count() as i64;
    let valid: Vec<i64> = chars.iter().copied().filter(|i| *i >= 0 && *i < len).collect();
    tx.execute(
        "UPDATE words SET wrong_chars = ?1 WHERE id = ?2",
        params![format_char_indices(&valid), word_id],
    )?;
    let dto = get(&tx, word_id)?;
    tx.commit()?;
    Ok(dto)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test update_mistake`
Expected: 4 个新用例 PASS。

- [ ] **Step 5: 命令层 + 注册 + ipc**

`src-tauri/src/commands/quiz.rs`，Task 1 的 `add_mistakes` 命令后：

```rust
/// 编辑错题词条：改文本 + 覆盖写手工标错字；不动学习进度
#[tauri::command]
pub fn update_mistake(
    state: State<'_, AppState>,
    word_id: i64,
    text: String,
    wrong_chars: Vec<i64>,
) -> AppResult<WordDto> {
    let conn = state.conn()?;
    words::update_mistake(&conn, word_id, &text, &wrong_chars)
}
```

`src-tauri/src/lib.rs`，`commands::quiz::add_mistakes,` 后加：

```rust
            commands::quiz::update_mistake,
```

`src/lib/ipc.ts`，`addMistakes` 后：

```ts
  /** 编辑错题词条：改文本 + 重标「不会写的字」，不动学习进度 */
  updateMistake: (wordId: number, text: string, wrongChars: number[]) =>
    invoke<Word>("update_mistake", { wordId, text, wrongChars }),
```

- [ ] **Step 6: 验证 + 提交**

Run: `cd src-tauri && cargo test` 和 `npm run typecheck`
Expected: 全部 PASS / 无错误。

```bash
git add src-tauri/src/repo/words.rs src-tauri/src/commands/quiz.rs src-tauri/src/lib.rs src/lib/ipc.ts
git commit -m "feat(quiz): update_mistake 编辑错题词条（改文本+重标错字，保 note）"
```

---

### Task 3: 前端 `EditMistakeModal` + 错题本行内编辑

**Files:**
- Create: `src/components/EditMistakeModal.tsx`
- Modify: `src/pages/Mistakes.tsx`（import、state、操作列、弹窗渲染）

**Interfaces:**
- Consumes: `api.updateMistake(wordId, text, wrongChars)`（Task 2）；`Mistake` 类型（`src/types.ts`，含 `wrongChars` 显示值）；`Modal` 组件（`title / subtitle / footer / onClose`）。
- Produces: `<EditMistakeModal word={Mistake} onSubmit={(text, chars) => Promise<void>} onClose={() => void} />`。

- [ ] **Step 1: 新建 `src/components/EditMistakeModal.tsx`**

```tsx
import { useState } from "react";
import Modal from "./Modal";
import type { Mistake } from "../types";

interface EditMistakeModalProps {
  word: Mistake;
  /** 成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (text: string, chars: number[]) => Promise<void>;
  onClose: () => void;
}

export default function EditMistakeModal({ word, onSubmit, onClose }: EditMistakeModalProps) {
  const [text, setText] = useState(word.text);
  const [chars, setChars] = useState<Set<number>>(new Set(word.wrongChars));
  const [busy, setBusy] = useState(false);

  const trimmed = text.trim();
  const glyphs = [...trimmed];

  const toggle = (index: number) => {
    setChars((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const submit = async () => {
    if (!trimmed || busy) return;
    setBusy(true);
    try {
      await onSubmit(trimmed, [...chars].sort((a, b) => a - b));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title="编辑错题"
      subtitle="改词语文字后，之前标的字会失效，需要重新点"
      width="max-w-md"
      onClose={onClose}
      footer={
        <>
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            取消
          </button>
          <button
            onClick={submit}
            disabled={!trimmed || busy}
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            保存
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">词语</span>
          <input
            value={text}
            autoFocus
            onChange={(e) => {
              setText(e.target.value);
              // 字变了，之前标的下标就失效了
              setChars(new Set());
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
            }}
            className="w-full rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
          />
        </label>
        {glyphs.length > 0 && (
          <div>
            <span className="mb-1.5 block text-xs font-medium text-stone-500">不会写的字（可不点）</span>
            <div className="flex flex-wrap gap-1.5">
              {glyphs.map((ch, i) => (
                <button
                  key={i}
                  onClick={() => toggle(i)}
                  className={
                    "rounded-md border px-2.5 py-1 text-sm " +
                    (chars.has(i)
                      ? "border-rose-300 bg-rose-100 text-rose-800"
                      : "border-stone-200 bg-white text-stone-700 hover:border-stone-300")
                  }
                >
                  {ch}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
```

- [ ] **Step 2: `Mistakes.tsx` 接入**

1. import 区（`AddMistakeModal` 之后）加：

```tsx
import EditMistakeModal from "../components/EditMistakeModal";
```

2. state 区（`addOpen` 之后）加：

```tsx
  const [editing, setEditing] = useState<Mistake | null>(null);
```

3. `master` 函数后加提交回调：

```tsx
  const editMistake = async (word: Mistake, text: string, chars: number[]) => {
    try {
      await api.updateMistake(word.id, text, chars);
      setEditing(null);
      await reload(filter);
      notify("ok", "「" + text + "」已更新");
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };
```

4. 操作列（`<td className="py-2.5 pr-8 text-right">` 内，「已掌握」按钮**之前**）加：

```tsx
                    <button
                      onClick={() => setEditing(word)}
                      className="rounded px-2 py-1 text-xs text-stone-400 hover:bg-stone-100 hover:text-stone-700"
                    >
                      编辑
                    </button>
```

5. `{addOpen && (` 块之前渲染弹窗：

```tsx
      {editing && (
        <EditMistakeModal
          word={editing}
          onSubmit={(text, chars) => editMistake(editing, text, chars)}
          onClose={() => setEditing(null)}
        />
      )}
```

- [ ] **Step 3: 验证 + 提交**

Run: `npm run typecheck`
Expected: 无错误。

```bash
git add src/components/EditMistakeModal.tsx src/pages/Mistakes.tsx
git commit -m "feat(quiz): 错题本行内编辑——改文本+重标错字弹窗"
```

---

### Task 4: 前端 `AddMistakeModal` 双模式（单个 / 批量）

**Files:**
- Modify: `src/components/AddMistakeModal.tsx`（全文件重写，下方给出完整内容）
- Modify: `src/pages/Mistakes.tsx`（props 传批量回调）

**Interfaces:**
- Consumes: `api.previewWords(text) => Promise<SplitResult>`（既有命令 `preview_words`，类型含 `words / duplicates / dropped`）；`api.addMistakes`（Task 1）。
- Produces: `AddMistakeModalProps` 新增 `onSubmitBatch: (groupId: number, texts: string[]) => Promise<void>`；`SplitResult` 需在 `src/types.ts` 已存在（核对：已有 `words / duplicates / dropped` 字段，直接用）。

- [ ] **Step 1: 重写 `src/components/AddMistakeModal.tsx`**

```tsx
import { useEffect, useState } from "react";
import Modal from "./Modal";
import { api } from "../lib/ipc";
import type { Group, SplitResult } from "../types";

interface AddMistakeModalProps {
  groups: Group[];
  defaultGroupId: number | null;
  /** 单个模式提交；成功与否由调用方处理：返回即复位按钮，错误用 Toast 提示 */
  onSubmit: (groupId: number, text: string, chars: number[]) => Promise<void>;
  /** 批量模式提交（texts 已切分去重） */
  onSubmitBatch: (groupId: number, texts: string[]) => Promise<void>;
  onClose: () => void;
}

type Mode = "single" | "batch";

export default function AddMistakeModal({
  groups,
  defaultGroupId,
  onSubmit,
  onSubmitBatch,
  onClose,
}: AddMistakeModalProps) {
  const [mode, setMode] = useState<Mode>("single");
  const [groupId, setGroupId] = useState<number>(defaultGroupId ?? groups[0]?.id ?? 0);
  const [text, setText] = useState("");
  const [chars, setChars] = useState<Set<number>>(new Set());
  const [batchText, setBatchText] = useState("");
  const [preview, setPreview] = useState<SplitResult | null>(null);
  const [busy, setBusy] = useState(false);

  const trimmed = text.trim();
  const glyphs = [...trimmed];

  const toggle = (index: number) => {
    setChars((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  // 批量输入防抖预览：与抽题页 count 查询同节奏
  useEffect(() => {
    if (mode !== "batch") return;
    if (!batchText.trim()) {
      setPreview(null);
      return;
    }
    let alive = true;
    const timer = setTimeout(() => {
      api
        .previewWords(batchText)
        .then((r) => {
          if (alive) setPreview(r);
        })
        .catch(() => {
          if (alive) setPreview(null);
        });
    }, 120);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [mode, batchText]);

  const submitSingle = async () => {
    if (!trimmed || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmit(groupId, trimmed, [...chars].sort((a, b) => a - b));
    } finally {
      setBusy(false);
    }
  };

  const submitBatch = async () => {
    const words = preview?.words ?? [];
    if (words.length === 0 || !groupId || busy) return;
    setBusy(true);
    try {
      await onSubmitBatch(groupId, words);
    } finally {
      setBusy(false);
    }
  };

  const batchCount = preview?.words.length ?? 0;

  return (
    <Modal
      title="加入错题本"
      subtitle="批量模式粘贴整段词语，按空格、逗号等分隔"
      width="max-w-md"
      onClose={onClose}
      footer={
        <>
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            取消
          </button>
          {mode === "single" ? (
            <button
              onClick={submitSingle}
              disabled={!trimmed || !groupId || busy}
              className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
            >
              加入
            </button>
          ) : (
            <button
              onClick={submitBatch}
              disabled={batchCount === 0 || !groupId || busy}
              className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
            >
              加入 {batchCount > 0 ? batchCount + " 个词" : ""}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        <div className="flex gap-1 rounded-lg bg-stone-100 p-1">
          {(
            [
              { key: "single", label: "单个" },
              { key: "batch", label: "批量" },
            ] as { key: Mode; label: string }[]
          ).map((m) => (
            <button
              key={m.key}
              onClick={() => setMode(m.key)}
              className={
                "flex-1 rounded-md px-3 py-1.5 text-sm transition " +
                (mode === m.key
                  ? "bg-white font-medium text-stone-800 shadow-sm"
                  : "text-stone-500 hover:text-stone-700")
              }
            >
              {m.label}
            </button>
          ))}
        </div>
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">分组</span>
          <select
            value={groupId}
            onChange={(e) => setGroupId(Number(e.target.value))}
            className="w-full rounded-md border border-stone-200 bg-white px-3 py-2 text-sm text-stone-700 outline-none focus:border-stone-400"
          >
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.path}
              </option>
            ))}
          </select>
        </label>
        {mode === "single" ? (
          <>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-stone-500">词语</span>
              <input
                value={text}
                autoFocus
                onChange={(e) => {
                  setText(e.target.value);
                  // 字变了，之前标的下标就失效了
                  setChars(new Set());
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void submitSingle();
                }}
                placeholder="如：春天"
                className="w-full rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
              />
            </label>
            {glyphs.length > 0 && (
              <div>
                <span className="mb-1.5 block text-xs font-medium text-stone-500">
                  不会写的字（可不点）
                </span>
                <div className="flex flex-wrap gap-1.5">
                  {glyphs.map((ch, i) => (
                    <button
                      key={i}
                      onClick={() => toggle(i)}
                      className={
                        "rounded-md border px-2.5 py-1 text-sm " +
                        (chars.has(i)
                          ? "border-rose-300 bg-rose-100 text-rose-800"
                          : "border-stone-200 bg-white text-stone-700 hover:border-stone-300")
                      }
                    >
                      {ch}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </>
        ) : (
          <>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-stone-500">
                词语（空格、逗号、顿号、换行都行）
              </span>
              <textarea
                value={batchText}
                autoFocus
                onChange={(e) => setBatchText(e.target.value)}
                placeholder={"如：春天 秋天，\n月亮、太阳"}
                rows={4}
                className="w-full resize-none rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
              />
            </label>
            {preview && (
              <div>
                <span className="mb-1.5 block text-xs font-medium text-stone-500">
                  共 {batchCount} 个词
                  {preview.duplicates > 0 ? `，输入里重复 ${preview.duplicates} 个已去重` : ""}
                  {preview.dropped > 0 ? `，无效 ${preview.dropped} 条已过滤` : ""}
                </span>
                <div className="max-h-32 overflow-y-auto rounded-md border border-stone-100 bg-stone-50/60 px-3 py-2">
                  <div className="flex flex-wrap gap-1.5">
                    {preview.words.map((w, i) => (
                      <span
                        key={i}
                        className="rounded-md border border-stone-200 bg-white px-2 py-0.5 text-sm text-stone-700"
                      >
                        {w}
                      </span>
                    ))}
                  </div>
                </div>
                <p className="mt-1.5 text-xs text-stone-400">
                  已在错题本里的词加入时会自动跳过；需要标「不会写的字」的词，加入后点「编辑」补
                </p>
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
```

- [ ] **Step 2: `Mistakes.tsx` 加批量提交回调**

`addMistake` 函数后加：

```tsx
  const addMistakeBatch = async (groupId: number, texts: string[]) => {
    try {
      const r = await api.addMistakes(groupId, texts);
      setAddOpen(false);
      await reload(filter);
      setGroups(await api.listGroups());
      notify(
        "ok",
        `已加入 ${r.added} 个词` +
          (r.skippedExisting > 0 ? `，${r.skippedExisting} 个已在错题本跳过` : ""),
      );
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };
```

`<AddMistakeModal ...>` 加 prop：

```tsx
          onSubmitBatch={addMistakeBatch}
```

- [ ] **Step 3: 验证 + 提交**

Run: `npm run typecheck`
Expected: 无错误。

```bash
git add src/components/AddMistakeModal.tsx src/pages/Mistakes.tsx
git commit -m "feat(quiz): 加入错题本支持批量粘贴（分隔符切分预览、跳过已存在）"
```

---

### Task 5: 文档同步 + 全量验证 + 审查

**Files:**
- Modify: `wiki/frontend.md`（错题本行）
- Modify: `wiki/data-model.md`（`wrong_chars` 行）
- Modify: `AGENTS.md`（目录节命令计数 `quiz.rs（14）`→`（16）`、`共 58 个命令`→`共 60 个`）

- [ ] **Step 1: 文档更新**

`wiki/frontend.md` 错题本行，在「手工加词并点字标错」后补（保持原行内链接与格式，只扩文字）：

> …**手工加词并点字标错**（`AddMistakeModal`，单个/批量双模式：批量粘贴按分隔符切分预览、已在错题本的跳过）**、行内编辑改文本+重标错字**（`EditMistakeModal`）**、进笔画复习…

`wiki/data-model.md` 的 `wrong_chars` 行末尾补：

> ；可经 `update_mistake` 编辑覆盖（改文本时旧下标自动清空重标）

`AGENTS.md` 目录节：

- `quiz.rs（14）` → `quiz.rs（16）`
- `共 58 个命令` → `共 60 个命令`

- [ ] **Step 2: 全量验证**

```bash
cd src-tauri && cargo test
cargo clippy --all-targets
cd .. && npm run typecheck
```

Expected: 测试全 PASS（基线 251+ 新增 8 个用例）；clippy 无新增警告（存量 `src/tts/download.rs:522` 除外）；typecheck 通过。

- [ ] **Step 3: 起干净审查子 agent（按 AGENTS.md 审查流程）**

prompt 自包含：文件清单（`git status --short` + `git diff af1b74a..HEAD --stat`，af1b74a 是功能起点前最后一个提交）、硬约束、验证结果、输出格式（`文件:行号` + 触发路径 + P1/P2 + 最小修法）、只读。已知取舍（spec「已知取舍」三条 + note 保留逻辑）写进 prompt 防重复提。

- [ ] **Step 4: 按审查结论修复/判定，收敛后写 memory + 提交**

`.workbuddy/memory/2026-11-18.md` 追加本轮条目（问题数、真问题修法、误报理由、验证基线）。

```bash
git add wiki/frontend.md wiki/data-model.md AGENTS.md .workbuddy/ 2>/dev/null || git add wiki/frontend.md wiki/data-model.md AGENTS.md
git commit -m "docs: 错题本编辑+批量加入——wiki 与 AGENTS.md 命令计数同步"
```

（`.workbuddy/` 在 .gitignore 里，add 不进也不报错；命令以实际为准。）

---

## Self-Review 记录

- Spec 覆盖：编辑（Task 2/3）、批量（Task 1/4）、文档（Task 5）、测试清单全部落到具体用例 ✓
- 类型一致性：`BatchMistakeResult { added, skippedExisting }`（serde camelCase）在 Task 1 定义、Task 4 消费 `r.added / r.skippedExisting` ✓；`api.updateMistake(wordId, text, wrongChars)` Task 2 定义、Task 3 消费 ✓
- 陷阱已内联：update 传 None 清 note（Task 2 Step 3 注释 + 测试钉住）；事务回滚原子性（Task 1/2 测试钉住）
