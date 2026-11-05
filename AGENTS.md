# AGENTS.md

给接手这个仓库的 AI / 新人的操作手册。项目知识的权威来源是 [wiki/](./wiki/README.md)，本文件只放「怎么干活」的硬信息。

## 这是什么

家长给二年级孩子做生词默写的**单机离线**桌面应用：家长读词、孩子手写、家长批改；程序负责抽题、防偷看、记录错题。

- Rust + Tauri 2 + SQLite（rusqlite，bundled）
- React 18 + TypeScript + Vite + Tailwind CSS 4
- 数据只在本机 SQLite，无账号、无云端；**唯一联网行为**是自动升级时访问 GitHub Release（离线不影响使用）

## 沟通交流

- 使用中文

## 命令

```bash
npm install
make                       # 打包 + 部署（Windows 默认 D:/StudyApp/dictation，macOS 默认 ~/Applications）
make dmg                   # macOS：只出 .dmg 安装包（等价于 make build）
make build SIGN_KEY=/path/to/dictation.key  # 指定 updater 签名私钥（默认 ~/.tauri/dictation.key）
npm run tauri dev          # 开发运行（Vite 1420 + Rust）
npm run typecheck          # 前端类型检查（tsc --noEmit）
npm run build              # 前端构建（typecheck + vite build）
npm run icons              # 改完 app-icon.svg 后，重新生成全平台图标
cd src-tauri && cargo test # 后端单测，当前 83 个（macOS；平台条件测试有 1 个差异）
cd src-tauri && cargo build
npm run tauri build        # 出安装包；需先设 TAURI_SIGNING_PRIVATE_KEY，推荐用 make build 自动带上
```

产物（Windows）：`src-tauri/target/release/bundle/nsis/Dictation_0.1.0_x64-setup.exe`
产物（macOS）：`src-tauri/target/release/bundle/dmg/Dictation_0.1.0_aarch64.dmg`（`make install` 会把它拷到 `~/Applications/Dictation.app`；要装到 `/Applications` 用 `make install INSTALL_DIR=/Applications`，需 sudo）
另外 `bundle.createUpdaterArtifacts: true` 会产出 `*.sig` 与 macOS 的 `Dictation.app.tar.gz`，详见 [wiki/updater.md](./wiki/updater.md)。

`make` 在 Git Bash / PowerShell / cmd 里都能跑：Makefile 顶部按 `OS` 环境变量把 Git 自带的 bash 指给 `SHELL`（仅 Windows 分支设置）。**别删这段**——Windows 版 make 不认 `/bin/bash`，会静默退回 cmd，`mkdir -p` 会在仓库里建出一个叫 `-p` 的目录（见 wiki/pitfalls.md 第 14 条）。macOS / Linux 用系统默认 shell，无需该段。

## 目录

```
app-icon.svg            应用图标唯一矢量源；改完跑 npm run icons
app-icon.png            1024 预览图（由 SVG 渲染），别手改；src-tauri/icons/ 全部是生成物
src-tauri/migrations/   0001 表结构、0002 会话进度、0003 错字标记、0004 分组树、0005 手工错字（只增不改，user_version 递增）
src-tauri/src/db/       连接 + WAL + 迁移
src-tauri/src/domain/   text.rs 粘贴切分、sampler.rs 抽题（纯函数，带单测）
src-tauri/src/repo/     groups / words / sessions / word_progress / settings / backup
src-tauri/src/commands/ library.rs / quiz.rs / system.rs（薄封装，共 33 个命令）
src-tauri/src/speech/   TTS 抽象 trait Speaker（V1 只有 NullSpeaker）
src/pages/              Pick / Quiz / Review / Mistakes / History / Settings / Wordbook / PrintSheet
src/lib/ipc.ts          全部自定义后端命令集中于此，类型来自 src/types.ts
src/lib/updater.ts      自动升级状态机（插件 API：check / downloadAndInstall）
scripts/gen-updater-manifest.mjs  CI 里把各平台 .sig 汇总成 Release 的 latest.json
```

## 硬约束（改代码前必须知道）

1. **学习进度只能通过 `repo/word_progress.rs` 修改**。命令层、会话层都不得直接写 `status / wrong_count / correct_streak`。这是为了以后拆「多孩子」时把进度迁到 `word_progress` 表。
2. **错题本不建表**，等价于 `words.status = 'learning'`。别加 `mistakes` 表，会不同步。
3. **一场会话内词不重复**由 `UNIQUE(session_id, word_id)` 兜底；顺序由 `seq` 记录，别用内存下标当唯一真相。
4. **拼音字段不存在**，不要顺手加回来（产品已排除）。导入解析也不处理拼音。
5. **分组是树**（`groups.parent_id`）：选父分组就等于选它全部子孙，抽题/打印/统计都靠 `sampler::expand_group_ids` 展开；别在别处再写一套遍历。拖动改结构只走 `repo/groups.rs::move_to`（换父级 + 同级 `sort_order` 重排 + 数字编号重排，一个事务）。
6. **前端不拼 SQL、不绕过 `src/lib/ipc.ts`**。新增后端能力 = 加命令 + 加 ipc 方法 + 加类型。
7. **数据目录位置**：Windows / Linux 优先放「可执行文件同级目录」（绿色便携，拷走文件夹即可换电脑）；**macOS 一律放 `~/Library/Application Support/com.dictation.desktop`**——`.app` 包更新时会被整体替换，放包内（`Dictation.app/Contents/MacOS`）会随更新丢数据。`src-tauri/src/db/mod.rs::resolve_dir` 已用 `cfg(target_os = "macos")` 隔离，别改回「放 exe 同级」。
8. **identifier 别再改**：它同时是 Tauri `app_data_dir()` 的最后一级目录名，改了等于换数据目录，老用户会以为数据丢了。从 `com.dictation.app` 换过来的旧库／备份由 `db/mod.rs::migration_sources` 一次性搬入（只复制不覆盖）；真要再改，必须把新名字加进迁移源并保持「只搬一次」。
9. **updater 签名私钥绝不入库、公钥别随便换**：私钥只在 `~/.tauri/dictation.key` 与 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`；公钥写死在 `tauri.conf.json`，换公钥等于让所有已装旧版失去升级能力。CI 缺私钥时 `gen-updater-manifest.mjs` 会主动让发版失败，别改成「跳过」。

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
| 分组树拖动 | `repo/groups.rs::move_to`（后端）+ `pages/Wordbook.tsx` 的 `targetFor / zoneOf`（落点判定） |
| 自动升级 | `src/lib/updater.ts` + `Settings.tsx` / `App.tsx`；清单与平台映射在 `scripts/gen-updater-manifest.mjs` |

## 已知坑

- **不要用 bash 里的 `sqlite3` 命令行写中文数据**：Windows 控制台是 GBK，直接写会存进非法 UTF-8，读出来报 `invalid utf-8 sequence`。要写就用 UTF-8 的 `.sql` 文件 `sqlite3 db < file.sql`。
- `sqlite3` CLI 默认 `foreign_keys=OFF`，删分组不会级联删词；应用内是 ON 的。
- 开发时 `cargo test` 和 `tauri dev` 会争 target 锁，串行跑。
- `window.print()` 在 WebView2 可用；打印样式靠 `index.css` 里的 ```media print` + `.no-print`。
- 会话推进有 `advancing` ref 同步锁，别换成普通 state，会连点跳词。
- **WebView2 的 HTML5 拖放要先关 Tauri 的文件拖放**：`tauri.conf.json` 窗口配置里 `dragDropEnabled: false`，否则 `dragstart / dragover / drop` 一个都不触发，还不报错。
- **Windows 便携版升级后会「丢数据」**：updater 装的是 NSIS 版，数据目录从 exe 同级回落到 `%APPDATA%`；升级前先导出备份（wiki/pitfalls.md 第 21 条）。

## 文档

- 需求：[REQUIREMENTS.md](./REQUIREMENTS.md)
- 项目 wiki（架构、数据模型、流程、决策、坑）：[wiki/README.md](./wiki/README.md)
- 新增知识请写进 wiki 对应页面，并在 `wiki/README.md` 索引里登记；**不要**在这里堆细节。
