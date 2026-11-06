---
title: 自动升级（GitHub Release）
updated: 2026-11-06
tags: [updater, release, ci]
related: [release.md, architecture.md, pitfalls.md]
---

# 自动升级

新版安装包发布在 GitHub Release；程序启动时自己检查（可在「设置 → 版本与更新」关闭，默认开启，关闭后只能手动检查），家长点「下载并安装」即可升级。
**学习数据仍只在本机 SQLite**，联网只发生在两处：GET `latest.json`、下载安装包。

## 全链路

```
make release v0.2.2
  → CI 构建 6 个 target（带私钥签名，产出 .sig / macOS 的 .app.tar.gz）
  → release job：gen-release-notes.mjs → gen-updater-manifest.mjs → 上传安装包 + latest.json
  → 客户端启动 3 秒后 GET https://github.com/xinbaojian/dictation/releases/latest/download/latest.json
  → 插件比对 version > 当前版本 → 横幅提示（默写 / 批改 / 打印 / 设置页不弹）
  → 用户点「下载并安装」→ 校验 minisign 签名 → 安装 → 重启后生效
```

端点写死在 `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`，指向 GitHub 的
`releases/latest/download/` —— 这意味着**最新 Release 必须是正式版**（非 draft / prerelease），否则客户端会拿到 404。

## 改哪里

| 想做什么 | 改哪里 |
|---|---|
| 换更新源 / 换公钥 | `src-tauri/tauri.conf.json` 的 `plugins.updater` |
| 改检查时机、提示样式、进度显示 | `src/lib/updater.ts`（状态机）+ `src/App.tsx`（横幅/红点）+ `src/pages/Settings.tsx`（卡片） |
| 改「启动时自动检查更新」开关 | `repo/settings.rs::SettingsDto.auto_check_updates`（默认 true）+ `Settings.tsx` 开关（即时保存）+ `updater.ts` 启动 effect 的闸门 |
| 改更新说明的渲染（新增语法 / 排版） | `src/components/Markdown.tsx`；外部链接走 `src/lib/external.ts` |
| 改 release 说明的生成（分组 / 链接） | `scripts/gen-release-notes.mjs` |
| 改清单内容或平台映射 | `scripts/gen-updater-manifest.mjs` |
| 改签名产物上传 | `.github/workflows/release.yml` 的 `files:` |
| 版本号 / 当前版本显示 | `scripts/bump-version.mjs` 同步 5 处；`commands/system.rs::app_version` 读 `CARGO_PKG_VERSION` |

## 签名密钥（最容易出事的地方）

- 密钥对由 `./node_modules/.bin/tauri signer generate -w ~/.tauri/dictation.key --ci` 生成（本项目已生成，无密码）。
- **公钥**在 `tauri.conf.json`，随每个客户端发布；**私钥**只在 `~/.tauri/dictation.key`（权限 600）和
  GitHub 仓库 Secret `TAURI_SIGNING_PRIVATE_KEY` 里。`.gitignore` 已排除 `*.key` / `.env*`，**绝不提交私钥**。
- 本地 `make build` 会自动带上 `~/.tauri/dictation.key`；换成别的密钥用 `make build SIGN_KEY=/path/to/key`。
- CI 三个 build job 都把 Secret 注入 `TAURI_SIGNING_PRIVATE_KEY`；缺私钥就没有 `.sig`，
  `gen-updater-manifest.mjs` 会拒绝生成空清单，发版直接失败（防止发出无法升级的版本）。
- **私钥丢了 = 再也发不出能被老客户端接受的更新**；换公钥等于让所有已安装旧版失去升级能力。
  备份私钥，别轮换。若真要轮换，必须先用旧密钥签一个「过渡到新公钥」的版本并让足够多用户升级。
- `Makefile` 和 CI 都显式传 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（本密钥无密码，值就是空字符串）：
  Tauri CLI 用**私钥路径**签名时会交互式索要密码，不给这个变量本地会卡在 `Password:`、CI 会卡住。
  将来换成带密码的密钥，把密码写进同名 Secret / 环境变量即可，不用改 workflow。

## 静态清单 latest.json

`release` job 用 `scripts/gen-updater-manifest.mjs <tag> artifacts latest.json` 生成：

```json
{
  "version": "0.2.2",
  "notes": "<release-notes.md 全文>",
  "pub_date": "2026-11-05T14:00:00.000Z",
  "platforms": {
    "windows-x86_64": { "signature": "<.sig 内容>", "url": "https://github.com/.../Dictation_0.2.2_x64-setup.exe" },
    "darwin-aarch64": { "signature": "...", "url": "https://github.com/.../Dictation.app.tar.gz" }
  }
}
```

- 平台键是 `OS-ARCH`（`windows-x86_64/i686/aarch64`、`darwin-x86_64/aarch64`、`linux-x86_64/aarch64`）。
  macOS universal 一份 `.app.tar.gz` 写两个键；updater 插件先找 `{os}-{arch}-{bundle_type}`
  再回退 `{os}-{arch}`，所以裸 `darwin-aarch64` 键能用。
- 各平台的更新包：Windows = NSIS `.exe`、macOS = `.app.tar.gz`（**不是 .dmg**）、Linux = `.AppImage`；
  签名文件就是「包名 + `.sig`」，由 `bundle.createUpdaterArtifacts: true` 生成。
  **macOS 还要求 bundles 列表里有 `app`**（CI 是 `--bundles app --bundles dmg`），只打 dmg 不产
  `.app.tar.gz`（[pitfalls.md](./pitfalls.md) 第 23 条，v0.3.0 / v0.4.0 曾因此中招）。
- **Tauri 先校验整份 JSON 再比版本号**：任一列出的平台缺 `url` 或 `signature`，所有客户端检查都会失败。
  脚本对缺失平台**直接报错退出**（6 个平台来自 release job 的 needs 依赖，理应齐全；
  2026-11-06 前的版本只 WARN，导致发布过没有 darwin 键的清单）。
- `url` 指向的资产必须随 Release 一起上传 —— `files:` 里 `*.app.tar.gz` 不能漏，否则清单有效但下载 404。
- 本地验证（不需要真 Release）：
  `GITHUB_REPOSITORY=xinbaojian/dictation node scripts/gen-updater-manifest.mjs v0.2.2 <模拟 artifacts> /tmp/latest.json`

## 前端行为

`src/lib/updater.ts::useAutoUpdate` 管状态机（`idle/checking/available/downloading/installing/ready/error/dev`）：

- 启动 3 秒后静默检查，超时 8 秒；**离线、GitHub 不可达、旧 Release 没 `latest.json` 一律静默忽略**，不弹错。
  受设置项「启动时自动检查更新」控制（`settings` 表 `app.autoCheckUpdates`，默认开启）：开关关着就完全不发请求。
  设置读取失败按默认「开」处理，与后端 `serde(default)` 口径一致——宁可多查一次（反正静默失败），不能让老用户升级后收不到更新提示。
  老版本存量的设置 JSON 没有该字段，靠 `SettingsDto` 结构体级 `serde(default)` + 手写 `Default`（true）补齐，有单测兜底。
- `Update` 对象持有 Rust 侧资源，用 ref 保活，重新检查前先 `close()`；`busyRef` 挡住连点。
- 「忽略此版本」写 `localStorage["dictation.skipUpdateVersion"]`，只隐藏横幅，设置页仍可更新；手动检查会重新展示。
- 开发模式（`import.meta.env.DEV`）不检查，避免开发时误装更新包。
- Windows 调 `downloadAndInstall` 后安装器会退出应用；macOS / Linux 需重启，`ready` 状态给「立即重启」按钮，
  走 `commands/system.rs::restart_app`（`AppHandle::restart()` 会立刻结束进程，前端 invoke 报错属正常，已 catch）。

## 平台限制与坑

1. **0.2.1 及更早的版本没有 updater**，必须先手动装一次带本功能的版本，之后才能自动升级。
2. **Windows 便携版**：`make install` 拷的是裸 `dictation.exe`，数据目录在 exe 同级；updater 下载的是
   NSIS 安装包，升级后会变成安装版、数据目录可能回落 `%APPDATA%`，看起来像「数据丢了」。
   这类用户升级前先在「设置 → 数据与备份」导出备份。macOS 无此问题（数据目录恒在
   `~/Library/Application Support/com.dictation.desktop`）。
3. **macOS 装在 `/Applications`** 且由管理员安装时，普通用户可能无权替换 `.app`，更新会失败；
   `make install` 默认装 `~/Applications`，没有这个问题。
4. `latest.json` 里 `notes` 是 Markdown 原文（`release-notes.md` 全文），前端用
   `src/components/Markdown.tsx` 渲染成排版（块级：标题 / 段落 / 有序·无序列表 / 引用 / 分割线 / 围栏代码块；行内：粗体 / 斜体 / 行内代码 / 链接）；
   **不引第三方 markdown 库、也不用 `dangerouslySetInnerHTML`**——notes 来自网络，按 HTML 解释等于让远端内容在 WebView 里执行。
   链接点击走 `src/lib/external.ts` → `tauri-plugin-opener` 交给系统浏览器。**必须 `preventDefault`**：
   本项目没配 `on_navigation`，Tauri 默认不拦导航，`<a href>` 会把主窗口直接导航走（白屏、也没有后退可回）；
   `window.open` / `target="_blank"` 同样指望不上。另外 `[文字](url)` 与裸链接都只放行 http/https，且括号按深度配平
   （`.../Foo_(bar)` 不会被截断）；
   权限只开了 `opener:allow-open-url` + `opener:allow-default-urls`（默认集还带 `reveal_item_in_dir`，用不上就不给）。
   **两条缺一不可**：`allow-open-url` 只开 `open_url` 命令、本身不带 scope，URL 白名单来自 `allow-default-urls`
   的 glob（`http://*` / `https://*` / `mailto:*` / `tel:*`），只剩前者会让所有链接静默 `ForbiddenUrl`（界面只弹「打不开链接」）。
   注意 scope 本身含 `mailto:` / `tel:`，http(s) 这条限制实际由 `external.ts` 维护；scope 里写死的协议前缀
   是大小写敏感的（`HTTPS://…` 会被判 `ForbiddenUrl`），所以 `external.ts` 在调用前把协议头转成了小写。
5. 改过 `gen-updater-manifest.mjs` / `release.yml` 要先提交推送再打 tag —— `make release` 只
   `git add` 那 5 个版本号文件（同 [release.md](./release.md) 的前提）。
6. 本机 `make dmg` 会因 `hdiutil` 输出变化失败（[pitfalls.md](./pitfalls.md) 第 17 条），与更新功能无关；
   验证签名产物用 `--bundles app`：`bundle/macos/Dictation.app.tar.gz` 与 `.sig` 会照常生成。
   失败后记得 `hdiutil detach` 残留卷并删 `rw.*.dmg`。
7. 直接跑 `npm run tauri build` 而不带私钥，打包到最后会失败并报
   `A public key has been found, but no private key`（配置里有公钥却没有可签名的私钥）。
   用 `make build`（自动读本机密钥）或显式 `TAURI_SIGNING_PRIVATE_KEY=...`，别为了绕过去把公钥删掉。
8. **macOS 更新包只有 bundles 含 `app` 才会产出**：`--bundles dmg` 不产 `.app.tar.gz` / `.sig`，
   清单会缺 darwin 键、macOS 客户端检查更新报 fallback platforms 错（第 23 条坑）。
   CI 与本地验证见上文「静态清单」一节。
