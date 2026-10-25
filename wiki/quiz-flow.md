---
title: 默写流程与状态流转
updated: 2026-10-25
tags: [flow, domain]
related: [data-model.md, decisions.md]
---

# 默写流程与状态流转

## 一、抽题（`domain/sampler.rs::collect`）

1. 取所有 `is_archived = 0` 的词，按 `groups.sort_order, groups.id, words.id` 排序
2. 过滤分组（`group_ids` 为空 = 全部分组，错题专项常用）
3. 过滤范围：`all` 全要 / `unlearned` 排除 mastered / `mistake` 只要 learning
4. **跨组按 text 去重**，保留排序靠前的那条
5. 随机洗牌（`rand` Fisher–Yates）→ 截取前 N（N=0 表示全部）

候选为空时返回带病因的中文提示（"错题本里还没有词…"等），不是干巴巴的报错。

## 二、建会话（`repo/sessions.rs::create`）

- 写入 `sessions`（`status='running'`，`config_json` 快照）+ 逐条写 `session_items`（`seq` 从 1 递增）
- 事务内完成，前端拿到的是完整 `SessionDto`（含 items），可以直接进默写页

## 三、默写（`pages/Quiz.tsx`）

| 操作 | 行为 |
|---|---|
| 点击词语区 / 空格 | 揭示词语，`maskSeconds` 秒后自动遮罩 |
| 下一个（空格） | `update_session_progress(id, index+1, null)` |
| 不会（Enter） | `update_session_progress(id, index+1, item.id)` → 该 item `skip_marked=1` |
| 上一个（←） | 仅本地回看，不回退 `current_seq` |
| 退出（Esc） | 保留 `running`，回首页显示「继续默写」横幅 |
| 最后一个 + 完成 | `finish_session` → `status='reviewing'` |

并发保护：`advancing` ref 同步锁，防止连点跳词。

恢复：进入页面时 `index = min(current_seq, total-1)`。历史遗留的 `reviewing` 会话在首页显示「去批改」而不是重新抽题。

## 四、批改（`pages/Review.tsx` → `repo/sessions.rs::submit_review`）

1. 前端默认全对，`skip_marked` 的词预置为错
2. 提交时逐条写 `result`，并调用 `word_progress::mark_wrong / mark_correct`
3. 会话落 `status='finished'`、`wrong_total`、`finished_at`
4. 返回 `{ wrong, correct, graduated }`，`graduated` 是本次移出错题本的词数，前端直接提示

提交状态不是 `reviewing` 时拒绝（防重复提交）。

## 五、错题本出本（`repo/word_progress.rs`）

```
批改=错 → status='learning', wrong_count+1, correct_streak=0
批改=对 → correct_streak+1
        new       → mastered（一次就对）
        learning 且 correct_streak>=N → mastered 并清零（N 默认 2）
```

`mark_correct` 返回「这次是否出本」，用于统计。手动「已掌握」等价于直接置 `mastered`；词表里「标错」则置回 `learning`（反向操作）。

## 六、只练本次错词

`retry_session` 取出该会话 `result='wrong'` 的 word_id（去重）→ 新建 `mode='mistake'` 会话，标签显示「错题本」。

## 边界

- 词条从错题本出本后，选「全部词」仍会抽到（期末复习需要）
- 跨组去重的副作用：同一词在 A、B 两组的进度互相独立，抽到的那条被练，另一条保持 `new`。分组统计的「未掌握」可能略高于实际
