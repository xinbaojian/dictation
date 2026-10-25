---
title: 前端结构
updated: 2026-10-25
tags: [frontend, react]
related: [architecture.md, decisions.md]
---

# 前端结构

技术：React 18 + TypeScript（strict, noUnusedLocals）+ Vite + Tailwind 4（```tailwindcss/vite`，无 config 文件，主题在 `src/index.css` 的 ```theme`）。

## 路由

没有 react-router。`App.tsx` 里一个联合类型 `Route` 驱动整屏切换：

```ts
{ name: "pick" | "wordbook" | "mistakes" | "history" | "settings" }
| { name: "quiz"; sessionId: number }
| { name: "review"; sessionId: number }
| { name: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string }
```

- 默写中 / 批改中，侧栏高亮仍停在「默写」
- Quiz / Review 用 `key={sessionId}` 强制重建，避免上一场的状态残留

## 页面职责

| 页面 | 文件 | 关键点 |
|---|---|---|
| 抽题配置 | `pages/Pick.tsx` | 分组多选、范围、题量、可抽数量（120ms 防抖）；顶部「继续上次」横幅 |
| 默写 | `pages/Quiz.tsx` | 进入即自动显示、到点遮罩、键盘快捷键、进度落库、`advancing` 锁 |
| 批改 | `pages/Review.tsx` | 网格点错、`skip_marked` 预置、提交后 0.9s 跳转 |
| 错题本 | `pages/Mistakes.tsx` | 分组筛选、连对进度条（读设置）、专项默写、打印 |
| 记录 | `pages/History.tsx` | 50 条列表 + 详情弹窗 + 只练本次错词 |
| 设置 | `pages/Settings.tsx` | 遮罩/连对/字号、备份导出导入、打开数据目录 |
| 词表 | `pages/Wordbook.tsx` | 分组栏（词数/错题/最近练习）、行内编辑、粘贴导入、标记错题 |
| 打印 | `pages/PrintSheet.tsx` | A4 排版 + 田字格，`window.print()` |

## 通用组件

| 组件 | 用途 |
|---|---|
| `components/Modal.tsx` | 遮罩弹窗骨架，Esc 关闭 |
| `components/ConfirmModal.tsx` | 危险操作确认，支持左下的第三个动作（如「放弃这次」） |
| `components/Toast.tsx` | `useNotice()` 返回 `{notice, notify}`，4 秒自动消失 |
| `components/GroupModal.tsx` / `PasteImportModal.tsx` | 分组表单 / 粘贴解析预览（预览走后端 `preview_words`） |

## 约定

- 后端调用全部走 `src/lib/ipc.ts`；错误统一 `errorMessage(e)`
- 时间展示用 `formatTime`（MM-DD HH:mm）与 `practiceLabel`（今天/昨天/N 天前练过）
- 破坏性操作一律自建弹窗，不用 `window.confirm`
- 状态机与业务流程放页面内，不引状态库（无 redux/zustand）
- 打印相关类名：`no-print`（打印时隐藏）、`print-page`（去阴影去宽度）、`print-row`（避免跨页断开）、`tian`（田字格）

## 容易忘的

Quiz 进入每个词会 `setRevealed(true)`，再由 `[revealed, settings, index]` 的定时器到点遮罩；`maskSeconds = 0` 表示一直显示。批改页的错字标记是 `Map<itemId, Set<字下标>>`，空集合 = 整词错。
