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

主题写在 `src/index.css` 的 `@theme` 块。新增自定义色/字体走那里，不要再建 config。

## 8. 打印样式不生效先检查层级

```media print` 里要让 `html/body/#root` 高度 auto、`overflow visible`，否则长清单只打一页。
`.no-print` 用在侧栏、工具栏；`.print-row` 防跨页断行。

## 9. cargo test 与 tauri dev 抢 target 锁

会卡在 `Blocking waiting for file lock`。串行执行即可。

## 10. 开发模式的数据在 target 目录里

便携化的代价：`"cargo run"` / `"tauri dev"` 的 `current_exe()` 是 `src-tauri/target/debug/dictation.exe`，库就落在那里。
`cargo clean` / `"make clean"` 会连库一起删掉。

**做法**：开发时别存真实数据；录词请用安装后的正式版本（数据在程序同级目录）。

## 11. 便携模式的数据会随卸载删除

数据在程序同级目录，NSIS 卸载会删掉整个安装目录（含 `dictation.db`）。
**做法**：卸载/换机前先在「设置 → 数据与备份」导出备份，或整个文件夹拷走。

## 12. 删了数据库，数据却又回来了

两个原因叠在一起：

1. 便携化迁移时把 `%APPDATA%` 的旧库**复制**过来，却保留了原文件，而「目标不存在就从旧位置搬」的判断每次启动都执行 —— 用户删库后又被搬回来
2. 只删 `dictation.db` 而留下 `-wal`，SQLite 打开时可能把旧页恢复出来

**已修**：`db/mod.rs` 在初始化后写 `.dictation-initialized` 标记，只有「没库 + 没标记」才搬旧数据；库不存在时顺带清理孤立的 `-wal / -shm`。
后来者注意：任何「自动恢复/自动迁移」都必须是一次性的，别留一个每次启动都会触发的复活路径。

## 13. Windows 上 HTML5 拖放要先关掉 Tauri 的文件拖放

Tauri 在 Windows 默认接管 webview 的拖放事件（用来做「把文件拖进窗口」），前端 `draggable` 的 `dragstart / dragover / drop` 一个都不会触发，表现是「拖不动、没反应、也不报错」。

**做法**：`tauri.conf.json` 的窗口配置里加 `"dragDropEnabled": false`（词表左侧树的拖动调整就靠它）。
代价是失去了「拖文件进窗口」的能力——本应用不需要（导入走粘贴和备份弹窗）。

顺带两条拖放本身的坑：

- `dragover` 里不 `preventDefault()` 就不会有 `drop`，看起来像「松手没反应」
- `dragleave` 在移到子节点时也会触发，判断「真的离开了」要用 `currentTarget.contains(relatedTarget)`，否则指示器一直闪
