# AGENTS.md

给接手这个仓库的 AI / 新人的操作手册。项目知识的权威来源是 [wiki/](./wiki/README.md)，本文件只放「怎么干活」的硬信息。

## 这是什么

家长给二年级孩子做生词默写的**单机离线**桌面应用：家长读词、孩子手写、家长批改；程序负责抽题、防偷看、记录错题。

- Rust + Tauri 2 + SQLite（rusqlite，bundled）
- React 18 + TypeScript + Vite + Tailwind CSS 4
- 无网络、无账号、无云端，数据只在本机 SQLite

## 命令

```bash
npm install
make                       # 打包 + 部署到 D:/StudyApp/dictation
npm run tauri dev          # 开发运行（Vite 1420 + Rust）
npm run typecheck          # 前端类型检查（tsc --noEmit）
npm run build              # 前端构建（typecheck + vite build）
cd src-tauri && cargo test # 后端单测，当前 43 个
cd src-tauri && cargo build
npm run tauri build        # 出 NSIS 安装包
```

产物：`src-tauri/target/release/bundle/nsis/Dictation_0.1.0_x64-setup.exe`

## 目录

```
src-tauri/migrations/   0001 表结构、0002 会话进度、0003 错字标记、0004 分组树（只增不改，user_version 递增）
src-tauri/src/db/       连接 + WAL + 迁移
src-tauri/src/domain/   text.rs 粘贴切分、sampler.rs 抽题（纯函数，带单测）
src-tauri/src/repo/     groups / words / sessions / word_progress / settings / backup
src-tauri/src/commands/ library.rs / quiz.rs / system.rs（薄封装，共 29 个命令）
src-tauri/src/speech/   TTS 抽象 trait Speaker（V1 只有 NullSpeaker）
src/pages/              Pick / Quiz / Review / Mistakes / History / Settings / Wordbook / PrintSheet
src/lib/ipc.ts          全部后端调用集中于此，类型来自 src/types.ts
```

## 硬约束（改代码前必须知道）

1. **学习进度只能通过 `repo/word_progress.rs` 修改**。命令层、会话层都不得直接写 `status / wrong_count / correct_streak`。这是为了以后拆「多孩子」时把进度迁到 `word_progress` 表。
2. **错题本不建表**，等价于 `words.status = 'learning'`。别加 `mistakes` 表，会不同步。
3. **一场会话内词不重复**由 `UNIQUE(session_id, word_id)` 兜底；顺序由 `seq` 记录，别用内存下标当唯一真相。
4. **拼音字段不存在**，不要顺手加回来（产品已排除）。导入解析也不处理拼音。
5. **分组是树**（`groups.parent_id`）：选父分组就等于选它全部子孙，抽题/打印/统计都靠 `sampler::expand_group_ids` 展开；别在别处再写一套遍历。
6. **前端不拼 SQL、不绕过 `src/lib/ipc.ts`**。新增后端能力 = 加命令 + 加 ipc 方法 + 加类型。

## 代码约定

- Rust：`domain` 纯逻辑（可单测、不依赖 Tauri）→ `repo` 读写 → `commands` 只做参数转换与锁。
- 错误统一 `AppError`，会序列化成中文提示直接显示给用户，例如「该分组中已有「春天」」。
- 时间一律用 SQLite 的 `strftime('%Y-%m-%dT%H:%M:%fZ','now')`，前端 `new Date()` 解析，不要引入 chrono。
- 前端 camelCase 对应 Rust snake_case（Tauri 自动转换）：`group_id` ← `groupId`。
- 破坏性操作走自建 `ConfirmModal`，不要用 `window.confirm`（WebView 里不可靠）。

## 常见任务

| 想做什么 | 改哪里 |
|---|---|
| 加一个后端能力 | `repo/` 加函数 → `commands/` 加 `#[tauri::command]` → `lib.rs` 注册 → `ipc.ts` 加方法 → `types.ts` 加类型 |
| 加表/改表 | 新增 `migrations/000N_xxx.sql` + `db/mod.rs` 的 `MIGRATIONS` 数组追加（绝不改历史文件） |
| 加设置项 | `repo/settings.rs` 的 `SettingsDto`（带默认值）→ `commands/system.rs` 已有 get/save → `pages/Settings.tsx` |
| 打印相关 | `pages/PrintSheet.tsx` + `index.css` 里的 `.tian` 与 ```media print` |

## 已知坑

- **不要用 bash 里的 `sqlite3` 命令行写中文数据**：Windows 控制台是 GBK，直接写会存进非法 UTF-8，读出来报 `invalid utf-8 sequence`。要写就用 UTF-8 的 `.sql` 文件 `sqlite3 db < file.sql`。
- `sqlite3` CLI 默认 `foreign_keys=OFF`，删分组不会级联删词；应用内是 ON 的。
- 开发时 `cargo test` 和 `tauri dev` 会争 target 锁，串行跑。
- `window.print()` 在 WebView2 可用；打印样式靠 `index.css` 里的 ```media print` + `.no-print`。
- 会话推进有 `advancing` ref 同步锁，别换成普通 state，会连点跳词。

## 文档

- 需求：[REQUIREMENTS.md](./REQUIREMENTS.md)
- 项目 wiki（架构、数据模型、流程、决策、坑）：[wiki/README.md](./wiki/README.md)
- 新增知识请写进 wiki 对应页面，并在 `wiki/README.md` 索引里登记；**不要**在这里堆细节。
