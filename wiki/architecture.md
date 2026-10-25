---
title: 架构
updated: 2026-10-25
tags: [architecture, rust, tauri]
related: [data-model.md, frontend.md, decisions.md]
---

# 架构

## 分层

```
React 页面 ──invoke──> commands/*.rs ──> repo/*.rs ──> rusqlite ──> SQLite
                            │
                            └──> domain/*.rs（纯逻辑：切分、抽题、状态机）
```

- `domain/`：无 I/O、不依赖 Tauri，输入输出都是普通结构，全部可单测
- `repo/`：唯一写 SQL 的地方，函数签名接收 `&Connection`
- `commands/`：只做「拿锁 → 调 repo → 返回 DTO」，不含业务规则
- 前端：`src/lib/ipc.ts` 是唯一出口，命令名与 Rust 一一对应

## 运行时状态

`AppState`（`src-tauri/src/lib.rs`）持有三项：

| 字段 | 作用 |
|---|---|
| `db: Mutex<Connection>` | 单连接 + WAL。单机单用户，不需要连接池 |
| `data_dir: PathBuf` | 程序同级目录（不可写则回落 `app_data_dir()`），库与备份都放这里 |
| `speaker: Arc<dyn Speaker>` | TTS 预留位，V1 注入 `NullSpeaker` |

命令拿连接统一走 `AppState::conn()`，锁中毒会返回中文错误而不是 panic。

## 错误契约

`AppError`（`src-tauri/src/error.rs`）序列化为**字符串**，前端拿到的就是可以直接显示的中文：`该分组中已有「春天」`。因此前端只需要 `errorMessage(e)` 一次转换。

## IPC 契约

- 命令参数/返回值用 `#[serde(rename_all = "camelCase")]`，JS 侧 camelCase，Rust 侧 snake_case，由 Tauri 自动转换
- 返回类型必须是可 Serialize 的 DTO，禁止把 `rusqlite::Row` 之类的内部类型漏出去
- 共 30 个命令，分三组：`commands/library.rs`（词表 11）、`commands/quiz.rs`（默写/错题 12）、`commands/system.rs`（设置/备份 7）

## 数据库连接约定

- 打开后立刻 `PRAGMA journal_mode = WAL` 与 `PRAGMA foreign_keys = ON`
- 迁移用 `PRAGMA user_version` 递增，脚本只能用一次（详见 [data-model.md](./data-model.md)）
- 批量写入用 `unchecked_transaction()`（拿到的是 `&Connection`，不是 `&mut`）

## TTS 预留

`trait Speaker { fn speak(&self, text: &str) -> AppResult<()> }` 已注入 `AppState`，V1 没有任何调用点。
V2 落地方式：新增 `SapiSpeaker` 实现 + 在 `commands` 加一条 `speak` 命令 + 默写页启用「朗读」按钮。**不需要改表结构**。

## 前端结构

见 [frontend.md](./frontend.md)：单文件路由（`App.tsx` 的 `Route` 联合类型），无 react-router。
