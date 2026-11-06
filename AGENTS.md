# AGENTS.md

给接手这个仓库的 AI / 新人的操作手册。项目知识的权威来源是 [wiki/](./wiki/README.md)，本文件只放「怎么干活」的硬信息。

## 这是什么

给二年级孩子的**学习小工具集合**（单机离线桌面应用）：一级顶栏切换工具，当前含「生词默写」（家长读词、孩子手写、家长批改；程序负责抽题、防偷看、记录错题）与「口算练习」（三数两步逆推口算卷，出题打印）两个工具。

- Rust + Tauri 2 + SQLite（rusqlite，bundled）
- React 18 + TypeScript + Vite + Tailwind CSS 4
- 数据只在本机 SQLite，无账号、无官方云；**默认唯一联网行为**是自动升级访问 GitHub Release；用户在设置里显式配置自有 MySQL 后才增加同步请求（两种情况下离线都不影响使用），详见 [wiki/sync.md](./wiki/sync.md)

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
cd src-tauri && cargo test # 后端单测（数目随提交增长，以输出为准；另有 1 个需真实服务器的集成测试默认忽略）
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
src-tauri/src/db/       连接 + WAL + 迁移 + 启动时 pending 快照替换
                        （数据目录里还有同步用的 .sync-credentials.json / .sync-state.json /
                         .sync-pending-state.json / pending.db / backups/sync/，见 wiki/sync.md）
src-tauri/src/domain/   text.rs 粘贴切分、sampler.rs 抽题（纯函数，带单测）
src-tauri/src/repo/     groups / words / sessions / word_progress / settings / backup / sync
src-tauri/src/commands/ library.rs（11）/ quiz.rs（14）/ system.rs（9）/ sync.rs（7）（薄封装，共 41 个命令）
src-tauri/src/speech/   TTS 抽象 trait Speaker（V1 只有 NullSpeaker）
src/pages/              Pick / Quiz / Review / Mistakes / StrokeReview / History / Settings / Wordbook / PrintSheet；math/（口算：MathSheet + example + math-sheet.css）
src/tools/registry.ts   工具注册表：顶栏一级菜单（TOOLS）与各工具二级菜单（NAV_BY_TOOL）、Route 联合类型
src/lib/ipc.ts          全部自定义后端命令集中于此，类型来自 src/types.ts
src/lib/strokes.ts      笔顺数据加载（分片缓存，本地 public/hanzi/，不联网）
src/lib/updater.ts      自动升级状态机（插件 API：check / downloadAndInstall）
scripts/gen-updater-manifest.mjs  CI 里把各平台 .sig 汇总成 Release 的 latest.json
scripts/sync-hanzi-data.mjs       把 hanzi-writer-data 打成 256 个分片写进 public/hanzi/（predev/prebuild 自动跑）
public/hanzi/           笔画数据分片，生成物（.gitignore 掉了），别手改也别提交；约 33MB
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
10. **同步只搬运整库快照，绝不做行级复制**：本地 SQLite 永远是唯一主库；云端只存一行 `LONGBLOB + version`，推送走 CAS 乐观锁，拉取写 `pending.db` 并在下次启动、打开连接之前替换（备份旧库 + 清 WAL）。同步凭据（含密码）绝不进 `settings` 表、绝不回传前端；网络命令必须 `async fn` + `spawn_blocking`，`MutexGuard` 不跨 await。整库恢复属数据目录级操作，不算违反第 1 条，但同步不能成为业务写入口。详见 [wiki/sync.md](./wiki/sync.md) 与 ADR-17。
11. **单实例插件必须第一个注册**（`tauri-plugin-single-instance`）：第二个进程启动时会 `wal_checkpoint` 甚至替换同一个 SQLite 库，和同步的 pending 替换叠加会让旧实例写进被 unlink 的 inode（静默丢数据）。第二次启动只把已有窗口提到前台，绝不开第二个进程。别删这个插件，也别把它挪到其它 `.plugin()` 之后。
12. **笔画数据必须随包发，不许改回 CDN**：数据由 `scripts/sync-hanzi-data.mjs` 打成 256 个分片放 `public/hanzi/`（生成物，不进库），`predev` / `prebuild` 自动跑。取数据的 URL 一律用站点根（`import.meta.env.BASE_URL`）拼，**别用 `document.baseURI`**——见 wiki/pitfalls.md 第 24 条。默认纯离线是产品定义，理由见 ADR-18。

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
| 打印 / 字帖 | `pages/PrintSheet.tsx`（每行一个词：示范字 → 描红 → 空格，选项存 localStorage）+ `components/StrokeGlyph.tsx`（用笔顺数据把字画成 SVG，不依赖楷体）+ `components/GridLines.tsx`（方格辅助线，**必须用 SVG 描边，CSS 背景打印不出**）+ `index.css` 的 `.ge` 与 `@media print` |
| 分组树拖动 | `repo/groups.rs::move_to`（后端）+ `pages/Wordbook.tsx` 的 `targetFor / zoneOf`（落点判定） |
| 自动升级 | `src/lib/updater.ts` + `Settings.tsx` / `App.tsx`；清单与平台映射在 `scripts/gen-updater-manifest.mjs` |
| 跨设备同步 | `repo/sync.rs`（`SnapshotStore` / 快照 / CAS / pending）+ `commands/sync.rs` + `db/mod.rs::init` + `Settings.tsx`；见 [wiki/sync.md](./wiki/sync.md) |
| 笔顺动画 / 笔画复习 | `src/components/StrokeWord.tsx` + `src/lib/strokes.ts` + `pages/StrokeReview.tsx`；数据换代只需升 `hanzi-writer-data` 并让脚本重刷（`.stamp` 里的 FORMAT 一起改） |

## 已知坑

- **不要用 bash 里的 `sqlite3` 命令行写中文数据**：Windows 控制台是 GBK，直接写会存进非法 UTF-8，读出来报 `invalid utf-8 sequence`。要写就用 UTF-8 的 `.sql` 文件 `sqlite3 db < file.sql`。
- `sqlite3` CLI 默认 `foreign_keys=OFF`，删分组不会级联删词；应用内是 ON 的。
- 开发时 `cargo test` 和 `tauri dev` 会争 target 锁，串行跑。
- **打印要过 `src/lib/print.ts`，别直接调 `window.print()`**：WebView2（Windows）实现了它，但 macOS WKWebView / Linux WebKitGTK 里是**静默无操作**（Tauri issue #3066）。工具函数按平台分流：Windows 原生打印；其余平台把「打印元素 + 全量 CSS」拼成独立 HTML 经 `print_html` 命令写临时文件、用默认浏览器打开并自动调起打印。打印样式靠 `index.css` 里的 `@media print` + `.no-print`。
- 会话推进有 `advancing` ref 同步锁，别换成普通 state，会连点跳词。
- **WebView2 的 HTML5 拖放要先关 Tauri 的文件拖放**：`tauri.conf.json` 窗口配置里 `dragDropEnabled: false`，否则 `dragstart / dragover / drop` 一个都不触发，还不报错。
- **Windows 便携版升级后会「丢数据」**：updater 装的是 NSIS 版，数据目录从 exe 同级回落到 `%APPDATA%`；升级前先导出备份（wiki/pitfalls.md 第 21 条）。
- **别把 `dictation.db` 放进网盘 / Syncthing 同步目录**：WAL 模式下 `-wal` / `-shm` 的文件锁跨机失效，冲突副本还会让人挑错文件。要跨机就用快照同步；导出快照必须用 `VACUUM INTO`，不能 `fs::copy` 运行中的库。

## 代码审查流程（改完必须走，直到审不出问题）

**核心规则：审查者必须是「干净上下文」的子 agent。** 主会话自己审自己的改动一定有作者滤镜，上一轮定下的结论也会污染判断；而 resume 同一个子 agent 同样会带上它自己的历史结论。所以**每一轮都新建一个子 agent**，只给它仓库 + 本轮改动范围 + 验证结果。

1. **先跑验证，再起审查**：`cd src-tauri && cargo test`、`cargo clippy --all-targets`、`npm run typecheck`。审查者要的是可复现的基线，测试红着去审等于浪费一轮。
2. **起一个全新子 agent**（`Agent`，`subagent_type: general-purpose`，一次性任务，不复用），prompt 必须自包含：
   - 要审的文件清单（`git status --short` + `git diff --stat` 的结果），明确要求读全文而不是只看 diff；
   - 项目硬约束（本文件「硬约束」一节 + 相关 wiki 页面路径），让它可以判定什么叫「违反约定」；
   - 已跑通的验证命令与结果，避免它再花时间跑一遍；
   - 输出格式：**每条问题给 `文件:行号` + 触发路径（什么操作序列会踩到）+ 后果严重度（P1 阻断 / P2 建议）+ 最小修法**；
   - 明确要求：**只读，不改代码**；没有问题就直说「未发现问题」，不要为了凑数编造。
3. **主 agent 逐条判定**（这是唯一有权改代码的一方）：
   - 真问题 → 修，并补一条能复现它的单测（修不带测试等于没修，下一轮还会被审出来）；
   - 误报 → 记下理由。**下一轮 prompt 里显式列出「已判定为非问题：X，理由 Y」**，否则新审查者会重复提同一条。
4. **修复后重新跑第 1 步，再起一个新的干净子 agent 复审。**
5. **收敛条件**：某一轮既没有 P1，也没有「本轮修复新引入」的 P2，才算审干净。剩余 P2 若判定为可接受，必须在 prompt 与提交说明里写明「已知取舍」，不能默默留着。
6. **把每轮结论落到 `.workbuddy/memory/YYYY-MM-DD.md`**：发现了几条、哪几条是真问题、怎么修的、哪几条判为误报及理由。下一个会话的人（或 AI）靠这个避免重复踩。

反面教材（都要避免）：

- 主会话「我已经仔细看过了」——自己审自己；
- 一问一答式续审同一个子 agent——它的旧结论会变成新的偏见；
- 审查者顺手改了代码——改动没有经过判定，也没人给它补测试；
- 只看 diff 不看上下文——漏掉「调用方怎么用」「这个分支在真实启动路径上可达吗」。

## 文档

- 需求：[REQUIREMENTS.md](./REQUIREMENTS.md)
- 项目 wiki（架构、数据模型、流程、决策、坑）：[wiki/README.md](./wiki/README.md)
- 新增知识请写进 wiki 对应页面，并在 `wiki/README.md` 索引里登记；**不要**在这里堆细节。
