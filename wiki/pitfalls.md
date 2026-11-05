---
title: 踩坑记录
updated: 2026-11-05
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

## 14. make 在 PowerShell / cmd 里会悄悄退回 cmd.exe

Makefile 里写 `SHELL := /bin/bash` 对 Windows 版 make **无效**：它只认 Windows 路径，找不到就静默退回 `cmd.exe`，不报错。于是：

- `mkdir -p "$(INSTALL_DIR)"` 变成 cmd 的 `mkdir`，在仓库里建出一个叫 `-p` 的目录，第二次跑就报「子目录或文件 -p 已经存在」
- `cp / rm / ls / awk` 之所以还能跑，只是因为 PATH 里恰好有 Git 的 coreutils —— 换台机器就全崩

**已修**：Makefile 顶部按 `$(OS)` 把 Git 自带的 bash 显式指给 `SHELL`：

- 优先 8.3 短路径 `C:/PROGRA~1/Git/bin/bash.exe`——`$(wildcard)` 会按空格切分模式，带空格的路径匹配不到
- 8.3 被关掉时兜底用全路径，make 4.4 能正确处理带空格的 `SHELL`
- **不要**用 `C:\Windows\System32\bash.exe` 或 `WindowsApps\bash.exe`：那是 WSL 的，`D:/...` 这类路径语义完全不同

排查手法：`make` 里加一句 `@echo "SHELL=$(SHELL)"`，正常应打印 bash 路径；打印 `sh.exe` 或空就是没生效。

## 15. 抽词优先级：先洗牌，再做稳定排序

「写错过的字 → 新词 → 其余」只是 `sessions::prioritize` 里的一次 `sort_by_key`，三条红线：

- 必须**稳定排序**（`sort_by_key`）。换成 `sort_unstable_by_key` 会把同档内洗好的随机顺序打乱，每次抽题变成固定顺序
- 必须排在洗牌之后。先排序再洗牌等于优先级白做
- **不许改成 `filter`**（只留高优先级词）。可抽数量会凭空变少，「题量 20」在 20 个词的分组里可能只抽出 10 个，用户会以为题量设置失效
- 同批去重（`pick_diverse`）只能在**同一档内**取舍：跨档挑字会把弱字词、新词挤掉，等于用「字不重复」换掉了更重要的复习优先级

弱字池来自 `session_items.wrong_chars` 和 `words.wrong_chars`（逗号分隔的字符下标），解析时一定按 `text.chars()` 的字符下标取，不能按字节；词条改短后越界下标要丢掉，否则会提前一个不相干的字。

## 16. 手工错字挂在词条上，改词必须清空

`words.wrong_chars` 是「当前词条」的错字下标，改文本时不清空就会高亮到别的字（`words::update` 已处理）。它和批改记录 `session_items.wrong_chars` 是两处来源：列表展示、弱字池、备份都要取并集，只读一处会漏掉另一种。

## 17. macOS 上 dmg 打包失败：bundle_dmg.sh 找不到临时挂载卷

现象：`npm run tauri -- build --bundles dmg`（含 `make` / `make dmg`）在本机 release 编译与 `Dictation.app` 都成功，最后一步 `bundle_dmg.sh` 报 9 次 `grep: bad regex '/dev/disk8s': brackets ([ ]) not balanced`，再报 `interstitial disk image was not found`。稳定复现，重试无效。

- **原因**：`@tauri-apps/cli` 自带的 `bundle_dmg.sh`（生成在 `target/release/bundle/dmg/`）里 `find_mount_dir` 靠 `grep -E "${DEV_NAME}s"` 从 `hdiutil info` 里找挂载点；本机 hdiutil 已把 `create / resize / attach` 标为 deprecated 且输出格式有变，这一步解析不到卷
- **与图标和项目代码无关**：`.app` 本身完好，`Contents/Resources/icon.icns` 与 `src-tauri/icons/icon.icns` 逐像素一致，`Info.plist::CFBundleIconFile` 指向它
- **失败会留垃圾**：一个挂载卷 `/Volumes/dmg.XXXXXX` + `bundle/macos/rw.<pid>.*.dmg`。先 `hdiutil detach /Volumes/dmg.XXXXXX`，再删 `rw.*.dmg`，不清理会污染下次构建
- **临时绕过**：只出 `.app` 用 `--bundles app`；dmg 交给 GitHub Actions 的 macOS runner，或升级 CLI 后再验。别手改 `target/` 里那个脚本，下次构建会被覆盖

## 18. 改 bundle identifier 会换掉整个数据目录

`tauri.conf.json` 的 `identifier` 不只是 macOS 的 `CFBundleIdentifier`：Tauri 的 `app_data_dir()` 直接把它当最后一级目录名（`~/Library/Application Support/<identifier>` / `%APPDATA%\<identifier>`）。为修掉「identifier 以 `.app` 结尾」的构建警告，把 `com.dictation.app` 改成 `com.dictation.desktop` 后，老用户的新目录是空的，看起来像数据全丢了。

**做法**：`db/mod.rs::migration_sources` 把旧 identifier 目录排在迁移源第一位，`init` 在「没库 + 没 marker」时把 `dictation.db`（含 WAL/SHM）和 `backups/` 一起复制到新目录；只复制、不覆盖、不删旧文件，可回退。以后**不要再改 identifier**；真要改，得把新名字同时加进迁移源，并保持一次性迁移的语义（见第 12 条）。

## 19. macOS 上 cargo test 编不过：测试引用了被 cfg 裁掉的函数

`is_writable` 带 `#[cfg(not(target_os = "macos"))]`（macOS 根本不走便携目录分支），但 `writable_dir_is_used` 测试没加同样的 cfg，macOS 上直接报 `E0425: cannot find function is_writable`。Windows 上正常，所以很容易漏。

**做法**：平台专属函数的测试加同名 cfg；macOS 分支另补一个断言 `resolve_dir` 恒返回 `app_data_dir` 的测试，两边都有覆盖。

## 20. GitHub 自动生成的 Release Notes 里没有 commit message

现象：`make release v0.2.1` 发出来后，Release 说明只剩一行 `**Full Changelog**: v0.2.0...v0.2.1`，明明区间内有 3 个提交却一条都没列。

- **原因**：`generate_release_notes: true` 只汇总**合并过的 Pull Request**（按 PR 标签分类），直接 push 的 commit 一律不列；本仓库全靠直接 push。新建 `.github/release.yml` 配 `changelog.categories` 也没用——那份配置只管 PR 的分类与排除
- **做法**：`.github/workflows/release.yml` 的 `release` job 改用 `scripts/gen-release-notes.mjs` 自己 `git log <上一个 tag>..<tag>` 整理正文，按 `feat/fix/ci/docs/其他` 分组、过滤 `chore: release` 噪音提交，再用 `body_path` 发布
- **配套**：`actions/checkout@v4` 必须带 `fetch-depth: 0`。tag 触发的默认浅克隆只有 1 个 commit，`git describe --tags --abbrev=0 <tag>^` 找不到上一个 tag
- **别漏**：改完脚本或 workflow 要先提交推送再打 tag，`make release` 只 `git add` 那 5 个版本号文件

详见 [release.md](./release.md)。

## 21. 加了自动升级后，Windows 便携版升级会「丢数据」

`make install` 部署的是裸 `dictation.exe`，按硬约束第 7 条数据目录在 exe 同级；而 updater 下载的是
NSIS 安装包，装到 `Program Files` 后数据目录会回落到 `%APPDATA%\com.dictation.desktop`，旧库不在那儿，
用户看到的是空词表（数据其实没被删）。

**做法**：升级前在「设置 → 数据与备份」导出备份，装完再导入；不要改 updater 去指向便携版 exe
（没有签名产物、也没有安装流程）。macOS 数据目录恒在 `~/Library/Application Support/com.dictation.desktop`，
不受影响。密钥与清单的其它坑见 [updater.md](./updater.md)。
