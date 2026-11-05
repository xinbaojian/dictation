---
title: 自动升级（GitHub Release）
updated: 2026-11-05
tags: [updater, release, ci]
related: [release.md, architecture.md, pitfalls.md]
---

# 自动升级

新版安装包发布在 GitHub Release；程序启动时自己检查，家长点「下载并安装」即可升级。
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
  macOS universal 一份 `.app.tar.gz` 写两个键。
- 各平台的更新包：Windows = NSIS `.exe`、macOS = `.app.tar.gz`（**不是 .dmg**）、Linux = `.AppImage`；
  签名文件就是「包名 + `.sig`」，由 `bundle.createUpdaterArtifacts: true` 生成。
- **Tauri 先校验整份 JSON 再比版本号**：任一列出的平台缺 `url` 或 `signature`，所有客户端检查都会失败。
  脚本对缺失平台只告警、不写半条记录；一个平台都没有则报错退出。
- `url` 指向的资产必须随 Release 一起上传 —— `files:` 里 `*.app.tar.gz` 不能漏，否则清单有效但下载 404。
- 本地验证（不需要真 Release）：
  `GITHUB_REPOSITORY=xinbaojian/dictation node scripts/gen-updater-manifest.mjs v0.2.2 <模拟 artifacts> /tmp/latest.json`

## 前端行为

`src/lib/updater.ts::useAutoUpdate` 管状态机（`idle/checking/available/downloading/installing/ready/error/dev`）：

- 启动 3 秒后静默检查，超时 8 秒；**离线、GitHub 不可达、旧 Release 没 `latest.json` 一律静默忽略**，不弹错。
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
4. `latest.json` 里 `notes` 是 Markdown 原文，前端按纯文本 `whitespace-pre-wrap` 展示，不渲染 HTML。
5. 改过 `gen-updater-manifest.mjs` / `release.yml` 要先提交推送再打 tag —— `make release` 只
   `git add` 那 5 个版本号文件（同 [release.md](./release.md) 的前提）。
6. 本机 `make dmg` 会因 `hdiutil` 输出变化失败（[pitfalls.md](./pitfalls.md) 第 17 条），与更新功能无关；
   验证签名产物用 `--bundles app`：`bundle/macos/Dictation.app.tar.gz` 与 `.sig` 会照常生成。
   失败后记得 `hdiutil detach` 残留卷并删 `rw.*.dmg`。
7. 直接跑 `npm run tauri build` 而不带私钥，打包到最后会失败并报
   `A public key has been found, but no private key`（配置里有公钥却没有可签名的私钥）。
   用 `make build`（自动读本机密钥）或显式 `TAURI_SIGNING_PRIVATE_KEY=...`，别为了绕过去把公钥删掉。
