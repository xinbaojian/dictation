---
title: 术语表
updated: 2026-11-04
tags: [glossary]
related: [data-model.md, quiz-flow.md]
---

# 术语表

| 术语 | 含义 | 对应实现 |
|---|---|---|
| 分组 | 一个课本单元/课，词条的容器 | `groups` |
| 词条 | 一个要默写的词 | `words` |
| 新词 | 从没练过的词，`status='new'` | `words.status` |
| 错题 | 被批改为错的词，`status='learning'` | 错题本 ≡ 这个状态 |
| 已会 | 已掌握，`status='mastered'`，选「全部词」仍会被抽到 | `words.status` |
| 出本 | 错题连续答对够次数后自动变回已会 | `mark_correct` 返回 true |
| 连对 | `correct_streak`，答错归零 | `words.correct_streak` |
| 会话 | 一次完整的默写（抽题→批改） | `sessions` |
| 题序 | 本次会话内第几个词 | `session_items.seq` |
| 进度 | 已经推进到第几个词，用于恢复 | `sessions.current_seq` |
| 遮罩 | 默写页默认遮住词语，揭示后自动盖回 | `maskSeconds` |
| 不会 | 默写时点「不会」，批改页预置为错 | `skip_marked` |
| 范围 | 抽题过滤：全部 / 仅未掌握 / 错题本 | `domain::Scope` |
| 弱字 | 批改时点过「不会写」的字，跨词共享：错「春天」的「天」，「天空」也算弱 | `session_items.wrong_chars` → `weak_chars` |
| 抽词优先级 | 建会话时的档位排序：反复错的字 > 含弱字 > 新词 > 其余 | `repo::sessions::prioritize` |
| 反复错字 | 同一个字在 ≥2 个不同词里都被标错，说明掌握差 | `sessions::weak_char_counts` |
| 同批去重 | 一场默写里尽量不共用一个字（只在同一档内取舍） | `repo::sessions::pick_diverse` |
| 抽词偏好 | 抽题页多选：先练错过的字 / 新词优先 / 上次写过的排后 / 同批不重字 / 连字；全不选 = 纯随机 | `SampleOptions` |
| 手工错字 | 错题本里直接加词时标的字，存在词条上而不是会话里 | `words.wrong_chars` |
| 专项默写 | 只从错题本抽题的一轮 | `mode='mistake'` |
| 复习单 | 打印出来的 A4 田字格纸 | `PrintSheet.tsx` |
| 默写单 / 抄写单 | 打印单的两种形态：不含词语 / 含词语 | `showWord` |
