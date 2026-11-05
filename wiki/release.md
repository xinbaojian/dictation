---
title: 发版流程
updated: 2026-11-05
tags: [release, ci]
related: [pitfalls.md, testing.md]
---

# 发版流程

一条命令发版；GitHub Actions 打包全平台安装包，Release 说明由两个 tag 之间的提交记录整理而成。

## 一条命令

```bash
make release v0.2.2
```

`Makefile:release` 依次做：

1. `scripts/bump-version.mjs` 改 5 处版本号（`package.json` / `package-lock.json` / `Cargo.toml` / `Cargo.lock` / `tauri.conf.json`）
2. 有改动则提交 `chore: release X.Y.Z`；版本号已是目标值就跳过提交，避免空提交卡住
3. 打 tag 并推送分支与 tag → 触发 `.github/workflows/release.yml`

打包矩阵：Windows NSIS（x64 / x86 / arm64）、macOS universal dmg、Linux deb + AppImage（x86_64 / aarch64）。
产物由 `release` job 汇总，用 `softprops/action-gh-release` 发布。

## 自动升级产物

每个 build job 都注入 `TAURI_SIGNING_PRIVATE_KEY`（仓库 Secret），`createUpdaterArtifacts: true`
会在安装包旁产出 `.sig`，macOS 另外产出 `Dictation.app.tar.gz`（更新包本体）。
`release` job 在下载完 artifacts 后跑 `scripts/gen-updater-manifest.mjs`，生成 `latest.json`
并连同 `.app.tar.gz` / `.sig` 一起上传 —— 清单里的 url 指向这些资产，漏传就会 404。

完整说明（密钥保管、平台映射、离线行为、便携版坑）见 [updater.md](./updater.md)。
**没有配置 Secret 时发版会失败**：清单脚本拒绝产出空 `platforms`，这是故意的，防止发出无法升级的版本。

## Release 说明怎么生成

`release` job 里跑 `scripts/gen-release-notes.mjs <tag> release-notes.md`，输出交给 `body_path`。
脚本做的事：

| 步骤 | 规则 |
|---|---|
| 取提交区间 | `git describe --tags --abbrev=0 <tag>^` 找上一个 tag，取 `<prev>..<tag>`；找不到就当作首次发版 |
| 过滤 | `--no-merges` 去掉合并提交，再排除 `chore: release X.Y.Z` 这种纯改版本号的噪音 |
| 分组 | `feat` → 新功能、`fix` → 问题修复、`ci`/`build` → 构建 / 持续集成、`docs` → 文档、其余 → 维护 / 其他 |
| 排版 | 剥掉 `feat:` 这类前缀，中文描述原样保留，每条附短 sha 的 commit 链接 |
| 兜底 | 区间内没有有效提交时写「本次仅版本号更新，无功能改动。」 |
| 末尾 | 脚本补 `**Full Changelog**: .../compare/<prev>...<tag>`，与原来 GitHub 自动生成的那行一致 |

所以 **commit message 的前缀决定它落在哪一节**，发版前留意别写成没有前缀的一句话，否则会归到「维护 / 其他」。

### 为什么不用 GitHub 自带的 generate_release_notes

`generate_release_notes: true` 只汇总**合并过的 Pull Request**，直接 push 的 commit 一条都不列；
本仓库全靠直接 push，结果说明里只有 Full Changelog 一行。新建 `.github/release.yml` 配 `changelog.categories`
也解决不了——那份配置只管 PR 的分类与排除。详见 [pitfalls.md](./pitfalls.md) 第 20 条。

### 两个必须记住的前提

- `actions/checkout@v4` 在 `release` job 里必须带 `fetch-depth: 0`。tag 触发的默认是浅克隆（只有 1 个 commit），
  找不到上一个 tag，`git describe` 会失败。
- **改过 `scripts/gen-release-notes.mjs` 或 `release.yml` 后要先提交并推送，再打 tag**。
  `make release` 只 `git add` 那 5 个版本号文件，未提交的脚本不在 CI 里，生成步骤会直接失败、发不出包。

## 本地预览 / 补写历史 Release

```bash
# 预览某次发版会写出什么（不改动任何文件）
node scripts/gen-release-notes.mjs v0.2.1 /tmp/notes.md

# 只改已发布 Release 的说明文字，不动附件
node scripts/gen-release-notes.mjs v0.2.1 /tmp/notes.md
gh release edit v0.2.1 --notes-file /tmp/notes.md -R xinbaojian/dictation
```

脚本从 `GITHUB_REPOSITORY`（Actions 内置）或本地 `origin` 推导仓库地址来拼 commit 链接，
两者都没有时只显示短 sha、不生成链接。
