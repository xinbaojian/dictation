---
title: 架构
updated: 2026-11-07
tags: [architecture, rust, tauri]
related: [data-model.md, frontend.md, decisions.md, sync.md]
---

# 架构

## 分层

```
React 页面 ──invoke──> commands/*.rs ──> repo/*.rs ──> rusqlite ──> SQLite
                            │                  │
                            │                  └──> 自有 MySQL（仅快照同步，可选）
                            ├──> domain/*.rs（纯逻辑：切分、抽题、状态机）
                            └──> ai/  # OpenAI 兼容客户端与配置（ADR-20）
```

- `domain/`：无 I/O、不依赖 Tauri，输入输出都是普通结构，全部可单测（`math.rs` 口算出题引擎：三数两步逆推，同 seed 同卷）
- `repo/`：唯一写 SQL 的地方，函数签名接收 `&Connection`
- `commands/`：只做「拿锁 → 调 repo → 返回 DTO」，不含业务规则
- 前端：`src/lib/ipc.ts` 是唯一出口，命令名与 Rust 一一对应

## 运行时状态

`AppState`（`src-tauri/src/lib.rs`）持有三项：

| 字段 | 作用 |
|---|---|
| `db: Mutex<Connection>` | 单连接 + WAL。单机单用户，不需要连接池 |
| `data_dir: PathBuf` | 程序同级目录（不可写则回落 `app_data_dir()`），库、备份与同步状态都放这里 |
| `speech: SpeechService` | 朗读路由（ADR-21）：系统语音 / 本地语音包二选一，本地缺失或失败自动兜底系统 |
| `tts: Arc<Downloads>` | 语音包下载注册表：全局同时只允许一个任务，快照供设置页恢复进度条 |

命令拿连接统一走 `AppState::conn()`，锁中毒会返回中文错误而不是 panic。

## 错误契约

`AppError`（`src-tauri/src/error.rs`）序列化为**字符串**，前端拿到的就是可以直接显示的中文：`该分组中已有「春天」`。因此前端只需要 `errorMessage(e)` 一次转换。

## IPC 契约

- 命令参数/返回值用 `#[serde(rename_all = "camelCase")]`，JS 侧 camelCase，Rust 侧 snake_case，由 Tauri 自动转换
- 返回类型必须是可 Serialize 的 DTO，禁止把 `rusqlite::Row` 之类的内部类型漏出去
- 共 55 个命令：`commands/library.rs`（词表 11）、`commands/quiz.rs`（默写/错题 14）、`commands/system.rs`（设置/备份/朗读 11）、`commands/sync.rs`（跨设备同步 7）、`commands/ai.rs`（AI 讲解 6，默认关闭）、`commands/tts.rs`（语音包 4，ADR-21），另有工具命令 `commands/math.rs`（口算 1）与 `commands/print.rs`（打印 1）

## 数据库连接约定

- 打开后立刻 `PRAGMA journal_mode = WAL` 与 `PRAGMA foreign_keys = ON`
- 迁移用 `PRAGMA user_version` 递增，脚本只能用一次（详见 [data-model.md](./data-model.md)）
- 批量写入用 `unchecked_transaction()`（拿到的是 `&Connection`，不是 `&mut`）
- 同步快照的替换发生在 `db::init` 打开连接**之前**（`repo/sync.rs::apply_pending`）；网络命令用 `async fn` + `spawn_blocking`，`MutexGuard` 绝不跨 await。详见 [sync.md](./sync.md)

## TTS 朗读

ADR-19（系统离线语音）+ ADR-21（本地语音包）两层：

- `speech/`：`Speaker` trait + `SystemSpeaker`（macOS say / Windows SAPI / Linux spd-say）；`LocalTts`（sherpa-onnx 引擎懒加载 + `tts-cache/` 哈希缓存 + 独立音频线程 rodio 播放）；`SpeechService` 路由——设置选 `local` 且模型完好才走本地，**任何失败静默兜底系统语音**
- `tts/`：`models.rs` 语音包目录常量表（下载源 / 必需文件 / 引擎配置构建）与 `download.rs`（后台下载线程 + 进度事件 + 断点续传 + 解压落位）
- 命令：`speak_text`（可选 `modelId` 试听）/ `stop_speak` + `commands/tts.rs` 四条（列表 / 下载 / 取消 / 删除）

## 前端结构

见 [frontend.md](./frontend.md)：单文件路由（`App.tsx` 的 `Route` 联合类型），无 react-router。
