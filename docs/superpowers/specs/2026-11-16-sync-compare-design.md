# 2026-11-16 跨设备同步：设置页显示本机 vs 云端状态（sync_compare）

## 背景与目标

设置页的「跨设备同步」此前只显示本机状态（上次同步时间、是否有待应用快照），**完全不查云端**。
用户换机/两台电脑轮流用时，不知道当前本地数据是领先还是落后、该点「推送到云端」还是「从云端拉取」。

目标：状态框直接给出结论（一致 / 本地领先→建议推送 / 本地落后→建议拉取 / 两边都改过等），
并附云端详情（版本、最后推送的设备、更新时间）。

## 方案选型

- **A（采纳）：轻量「头部」查询**。云端表只 `SELECT version, OCTET_LENGTH(data), device_id, device_name, app_version, UNIX_TIMESTAMP(updated_at)`，不下载几 MB 的快照 blob。判定信号用「本机记录的 `remote_version` vs 云端当前 version」+「本机 `content_hash` vs `last_synced_hash`」（口径与 `decide_pull` 完全一致）。
- B（否决）：复用 `fetch()` 拉整个快照做校验后判定——每次打开设置页都传整库、落盘校验，太重。
- C（否决）：只记本地脏标记、不查云端——发现不了另一台设备推送过（落后），不满足核心诉求。

## 分类规则（classify_sync，纯函数）

| 判定 | kind | 提示方向 |
|---|---|---|
| 有待应用快照 | `pending` | 重启后生效（直接短路，不联网） |
| 云端没数据 | `noCloud` | 建议先推送 |
| 云端版本 < 本机记录 | `cloudBehind` | 云端可能被重置过，谨慎选择 |
| 版本相等、本地改过 | `localAhead` | 建议推送 |
| 版本相等、本地没改 | `synced` | 一致，无需操作 |
| 云端更新、本地没改 | `behind` | 建议拉取 |
| 云端更新、本地也改过 | `diverged` | 需选择保留哪边（都会先确认+备份） |

从未同步过的机器（`last_synced_hash` 为空）沿用 `decide_pull` 口径：空库 → `behind`（新设备直接拉取）、
非空库 → `diverged`（首次同步必须选边）。刻意**不**用云端行的 `hash` 列做判定（列可能被手工改过，
与「判定只用校验过的快照元数据」的既有哲学一致）。

## 关键实现点

- `SnapshotStore` trait 加 `head()`；`MysqlStore` 用轻量 SQL 实现，`FakeStore`（测试）同步实现并带 `head_count` 计数（钉住「pending 时不联网」）。
- `prepare_compare`（持有连接的命令线程里算哈希/空库/pending/状态）+ `perform_compare`（spawn_blocking 网络查询），与 push/pull 同一套路；`MutexGuard` 不跨 await。
- `updated_at` 用 `UNIX_TIMESTAMP()` 取 epoch 秒：DATETIME 无时区，`DATE_FORMAT` 拼 `Z` 等于谎称 UTC，服务器跑本地时间时显示差 8 小时；前端 `formatEpoch` 按本机时区格式化。
- 前端 `Settings.tsx`：同步卡片加状态框（彩色结论 + 云端详情 + 刷新按钮）；启用且无 `warning` 时自动查一次，`sync` 状态每次刷新（保存配置/推送/拉取后）自动重查；`comparingRef` 防重入；查询失败只显示错误文案不禁用任何功能。
- 命令 `sync_compare` 注册进 `lib.rs`，同步命令 7 → 8；wiki/sync.md 增「状态对比」一节。

## 验证

- `cargo test`：251 passed / 0 failed / 4 ignored（sync 63 个，含新增 6 个：分类全分支、文案方向、端到端三态流转、从未同步两分支、空云端、pending 不联网）。
- `cargo clippy --all-targets`：仅剩 `src/tts/download.rs:522` 存量 unused_mut（非本次引入）。
- `npm run typecheck`：通过。
- 真实 MySQL 集成测试 `live_mysql_roundtrip` 顺带补了 `head()` 断言（默认忽略，需 `DICTATION_SYNC_*` 环境变量）。
