---
title: 踩坑记录
updated: 2026-10-25
tags: [pitfalls]
related: [testing.md, architecture.md]
---

# 踩坑记录

每条都是真实遇到过的，改代码时注意。

## 1. 用 bash 的 sqlite3 写中文会存成非法 UTF-8

Windows 控制台是 GBK，`sqlite3 db "INSERT ... '春天'"` 会把 GBK 字节写进库，读出来报：

```
Conversion error from type Text at index 2, invalid utf-8 sequence of 1 bytes
```

**做法**：写 UTF-8 的 `.sql` 文件，`sqlite3 db < file.sql`；或者干脆用应用界面录入。
应用本身没问题——这条只影响手工造数据。

## 2. sqlite3 CLI 默认不启用外键

`PRAGMA foreign_keys` 默认 OFF，`DELETE FROM groups` 不会级联删 `words`，留下孤儿行。
应用内初始化时已 `PRAGMA foreign_keys = ON`。手工清理时要补一句：

```sql
DELETE FROM words WHERE group_id NOT IN (SELECT id FROM groups);
```

## 3. rustup 正在更新时 cargo 会诡异失败

现象：`error: the 'rustc.exe' binary ... is not applicable to the toolchain`，但 `rustc --version` 又能跑。
原因：另一个进程在跑 `rustup update`，`.rustup/toolchains/.../bin` 被清空中。
**做法**：等它跑完再编译，不要急着重装工具链。

## 4. 连点「下一个」会跳词

`setIndex(next)` 是异步的，两次点击读到同一个 `index`。
已用 `advancing` ref 做同步锁（`pages/Quiz.tsx`）。改这段别把锁去掉。

## 5. Vite HMR 不会重跑 useEffect

改前端后如果数据没刷新，别怀疑后端——Fast Refresh 保留了 state，effect 不会重跑。
**做法**：重启 `tauri dev`，或改动 `index.css` 之类触发整页 reload。

## 6. window.confirm 在 WebView 里不可靠

统一用 `components/ConfirmModal.tsx`。

## 7. Tailwind 4 没有 tailwind.config.js

主题写在 `src/index.css` 的 ```theme` 块。新增自定义色/字体走那里，不要再建 config。

## 8. 打印样式不生效先检查层级

```media print` 里要让 `html/body/#root` 高度 auto、`overflow visible`，否则长清单只打一页。
`.no-print` 用在侧栏、工具栏；`.print-row` 防跨页断行。

## 9. cargo test 与 tauri dev 抢 target 锁

会卡在 `Blocking waiting for file lock`。串行执行即可。
