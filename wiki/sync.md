---
title: 跨设备同步（自有 MySQL 快照）
updated: 2026-11-05
tags: [sync, database, mysql, tailscale]
related: [data-model.md, decisions.md, architecture.md]
---

# 跨设备同步（自有 MySQL 快照）

## 一句话

本地 SQLite 永远是**唯一主库**。用户在设置里显式配置自己的 MySQL 后，联网只是「上传/下载一个整库快照」；
未配置时程序完全离线，行为与 V1 一致。见 [decisions.md](./decisions.md) ADR-17。

## 为什么不做行级同步

| 行级双向复制必须解决 | 快照方案 |
|---|---|
| 0001 里 4 张表都是自增主键，跨机合并必撞 id | 整库覆盖，不需要全局 ID |
| `wrong_count / correct_streak / status` 用 last-write-wins 会抹掉另一台的进度 | 整库覆盖，进度天然一致 |
| `ON DELETE CASCADE` 需墓碑、需冲突时钟 | 不需要 |
| `sessions.config_json` 存的分组 id 跨机无效 | 随整库一起走 |
| 离线写队列 + 重放 | 离线只写本地库，联网后整库传 |
| DB 账号密码分发到每台客户端 | 只给一张表的 SELECT/UPDATE 最小权限账号 |

代价：**同一时刻只应有一台设备推进数据**；两台都改过时程序会先询问，不自动合并。
这对「家里两台电脑轮流陪孩子默写」的场景足够。

## 数据流

```
推送：本地 dictation.db ──VACUUM INTO──> 单文件快照 ──> MySQL dictation_snapshot 一行（CAS）
拉取：MySQL ──> 校验快照 ──> data_dir/pending.db ──> 下次启动 db::init 替换主库
```

- 快照用 `VACUUM INTO`（SQLite ≥3.27）生成**已 checkpoint 的单文件**，不带 `-wal` / `-shm`。
  绝不能 `fs::copy` 正在运行的库。
- 替换发生在 `db/mod.rs::init` 里、`Connection::open` **之前**，避开 `Mutex<Connection>` 持有中的库文件。
- 覆盖前自动把旧库完整复制到 `backups/sync/before-sync-<ts>.db`；失败不阻塞启动，错误写进 `.sync-last-error.txt`。

## 服务端

一张表，固定只有一行 `id=1`：

```sql
CREATE TABLE dictation_snapshot (
  id TINYINT PRIMARY KEY,
  data LONGBLOB NOT NULL,
  version INT UNSIGNED NOT NULL DEFAULT 0,
  device_id VARCHAR(64) NOT NULL DEFAULT '',
  device_name VARCHAR(64) NOT NULL DEFAULT '',
  hash CHAR(64) NOT NULL DEFAULT '',
  app_version VARCHAR(32) NOT NULL DEFAULT '',
  user_version INT NOT NULL DEFAULT 0,
  exported_at VARCHAR(32) NOT NULL DEFAULT '',
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
INSERT INTO dictation_snapshot (id, data, version) VALUES (1, X'', 0);
```

推送靠 CAS 乐观锁，`affected_rows = 0` 即分叉：

```sql
UPDATE dictation_snapshot SET data=?, version=version+1, ... WHERE id=1 AND version=?;
```

账号建议 `IDENTIFIED WITH mysql_native_password`（MySQL 8.0 的 `caching_sha2_password`
在纯 TCP 非 TLS 通道首次认证需 RSA 公钥交换，Rust 驱动兼容更麻烦），只授
`GRANT SELECT, UPDATE ON <库>.*`。**绝不要给客户端 root。**

网络：MySQL 只监听 Tailscale 网段，公网端口应在云平台安全组收紧。
Tailscale 解封装后走 `tailscale0`，不经安全组，收紧公网不影响同步。
链路已由 WireGuard 加密，客户端 `mysql` crate 用默认 features（不含 TLS）即可。

## 客户端落盘文件（都在 `data_dir`）

| 文件 | 内容 | 说明 |
|---|---|---|
| `.sync-credentials.json` | 配置 + 密码 | Unix 权限 600；**密码永不进 `settings` 表、永不回传前端**，`sync_status` 只给 `hasPassword` |
| `.sync-state.json` | deviceId / lastSyncedHash / lastSyncAt / lastSyncFrom / remoteVersion | 本机私有。**损坏时严格报错**，推送/拉取被拦住；`sync_status` 降级返回 `warning` 让设置页仍能打开，绝不静默重置（否则 remoteVersion 归零会导致每次推送误判冲突） |
| `pending.db` | 拉下来待应用的快照 | 下次启动替换主库后删除；也可用「放弃待应用快照」主动清掉。替换前会重算本机哈希，与拉取时不一致就作废（见下） |
| `.sync-pending-state.json` | 拉取时暂存的「同步完成后的状态」+ 拉取那一刻本机的内容哈希 | **只有 `apply_pending` 成功替换库之后**才提交到 `.sync-state.json`；否则本地库还是旧的却提前对齐了云端版本，会让未重启就推送绕过 CAS。若替换成功但写状态失败，下次启动会补提交（前提：本机库确实已不是拉取前那份，见下）|
| `.sync-last-error.txt` | 上一次 pending 替换失败原因 | **由 `sync_status.lastError` 回显到设置页**，用户可点「清除错误提示」；不会被下次启动自动清掉，避免用户以为快照已生效 |
| `backups/sync/before-sync-*.db`、`remote-before-overwrite-*.db` | 覆盖前的完整留档（每种前缀只留最近 5 份） | 应用内无恢复入口，回滚需手工替换 `dictation.db` |
| `backups/sync/pending-rejected-*.db` | 被作废的待应用快照 | 校验失败 / 缺配套状态 / 拉取后本机又改过时留档 |
| `backups/sync/pending-on-reset-*.db` | 「重置同步状态」时一并作废的待应用快照 | 低频；与配套状态一起留档 |
| `backups/sync/sync-pending-state-rejected-*.json` | 补提交时被作废的待提交状态 | 主库并非那份快照（如快照被手工删除）时留档 |
| `backups/sync/sync-pending-state-orphan-*.json` | 重置同步状态时隔离的孤儿待提交状态 | 防止 commit_pending_state 把刚重置的版本号写回去 |
| `backups/sync/sync-state-corrupt-*.json` | 被重置的损坏状态文件 | 隔离留档，不删除 |

`backups/sync/` 下**所有前缀**（含上表全部）都按 `prune_sync_backups` 各留最近 5 份。

同步相关的留档一律放 `backups/sync/` 子目录：`backup::list` 只列 `backups/` 顶层的 `.json`
导出备份，混在一起会让坏状态文件出现在「数据与备份」列表里（点开必报格式错误）。

`.sync-credentials.json` / `.sync-state.json` / `.sync-pending-state.json` 一律走
「临时文件 → fsync → rename」原子写，中途崩溃不会留下半截 JSON。
状态文件损坏时 `sync_status` 降级返回 `warning + stateCorrupt`（设置页仍能打开，推送/拉取被暂停），
用户确认后可用「重置同步状态」把坏文件隔离到 `backups/sync/` 并重建；`remoteVersion` 归零后下一次同步会先走确认流程，
不会静默覆盖云端。

`content_hash`（`repo/sync.rs`）对 `TABLES` 里的业务表（groups / words / sessions / session_items）
`SELECT *` 逐行逐列算 sha256：自动适配列变化，但**新增业务表必须同步加进 `TABLES`**。
刻意排除 `_sync_meta`（同步元数据）与 `settings`（字号 / 遮罩秒数 / 错题本出本阈值）——
两台机器字号不同不该被算成本机内容有改动，否则每次拉取都会弹覆盖确认。
注意 `settings` 含 `streak_to_master`（出本阈值），所以**替换前会先把本机的 settings 写进 `pending.db` 再 rename**，
「换库」与「保留本机偏好」是同一个原子动作，另一台机器的偏好不会改掉出本规则。
它用来判断「本地自上次同步后有没有改过」，也随快照写入 `_sync_meta.content_hash` 做传输校验。

## 冲突规则

| 场景 | 行为 |
|---|---|
| 推送时云端版本已被别的设备推进 | CAS 返回 `affected_rows=0` → 返回 `needsConfirm`，询问是否强制覆盖 |
| 强制推送（含空库强推） | 只要用户确认覆盖（`force=true`），先在 `backups/sync/remote-before-overwrite-*.db` 留一份云端快照再写 |
| 拉取时本地与云端都偏离上次同步点 | 返回 `needsConfirm`，确认后才写 pending |
| 本机从没同步过（`lastSyncedHash` 为空）且是空库 | 视为全新第二台，直接拉取，不弹确认 |
| 本机从没同步过但已经录过词 | 按「有改动」处理，必须先确认 |
| 本机是空库、云端有数据（删库重建后直接推送） | 返回 `needsConfirm`：空库推送会把云端和另一台设备一起清空 |
| 没有 `pending.db` 但本机库和「拉取时那份」一模一样 | 视为上次替换没成功，**不补提交**状态（否则下次推送会静默覆盖云端），把待提交状态作废并写 `lastError` |
| 拉取之后、重启之前本机又被改过 | 启动替换前重算本机哈希，与拉取时不一致就**作废这次 pending**（写 `lastError`），宁可重来也不覆盖新写的进度 |
| 快照 `user_version` 高于本程序 | 直接拒绝，提示先升级 |
| 快照 `user_version` 低于本程序 | 返回 `needsConfirm`：旧快照里没有新列（如错字标记）的取值，迁移只会补默认值 |
| 主状态版本已高于待应用快照 | 作废这份 pending（本机之后推送过，落地会把数据回退） |
| 快照 `integrity_check` 失败 / 元数据哈希不符 | 直接拒绝，绝不写进 pending |
| 两边内容哈希相同 | 不替换文件，只把 `remoteVersion / lastSyncedHash / lastSyncAt / lastSyncFrom` 对齐 |
| 本地已有 `pending.db` | 推送与拉取**都直接拒绝**，必须先重启完成替换 |

本机库读不出来时按**错误码**分流（不要用 `PRAGMA integrity_check` 反推：库被锁住时它同样报错）：

| 错误 | 判定 | 行为 |
|---|---|---|
| `SQLITE_BUSY` / `LOCKED` / 其它未知错误 | 暂时读不出来，数据还在 | 保留 pending，下次启动重试 |
| `SQLITE_NOTADB` / `CORRUPT` / 「no such table」 | 本机没有可用数据 | 直接把下载好的快照换上去（坏库留在 `backups/sync/before-sync-*.db`），并在设置页写明原因 |

后者是同步存在的首要用途——从云端救回一份能用的库；前者绝不能覆盖，否则「点个拉取继续默写」的新进度会被悄悄换掉。

拉取得到的是 `pending`，**必须重启应用才生效**；设置页会显示提示并给「立即重启」按钮。
没重启就继续默写也没关系：`apply_pending` 发现本机内容已经不是拉取时那份，会作废 pending 并提示重新拉取。

## 命令与代码位置

| 命令 | 作用 |
|---|---|
| `sync_status` | 状态（不含密码）+ 是否有 pending |
| `save_sync_config` | 保存配置；`password=null` 表示沿用旧密码 |
| `test_sync_connection` | 连一次 `SELECT VERSION()` |
| `sync_push` / `sync_pull` | `force` 表示用户已确认覆盖 |
| `sync_discard_pending` | 放弃未应用的 `pending.db`（有 pending 时连带清待提交状态，并清失败提示），不动主库与主状态 |
| `sync_repair_state` | 隔离损坏的 `.sync-state.json` 到 `backups/sync/` 并重建，仅 `stateCorrupt` 时可用 |

共 7 个同步命令。`sync_status` 的返回值含 `warning` / `lastError` / `stateCorrupt`：
本机同步文件损坏时降级展示，替换失败的原因单独回显，都不让一整个设置页卡在「加载中…」。
设置页把状态读取与其他设置拆开单独取，并对放弃快照 / 重置状态做二次确认。

- `repo/sync.rs`：`SnapshotStore` trait / `MysqlStore`、`VACUUM INTO`、`content_hash`、
  `.sync-state.json`、CAS 与分叉判定、`apply_pending`。换 HTTP API 只需换 `SnapshotStore` 实现。
- `commands/sync.rs`：薄层；网络 I/O 一律 `async fn` + `spawn_blocking`，`MutexGuard` 绝不跨 await。
- `db/mod.rs::init`：**每次启动无条件**调用 `apply_pending`（有无 pending 都要跑：补提交状态；替换成功时清错误文件；顺带清理崩溃留下的临时文件）。
- `pages/Settings.tsx`：「跨设备同步」卡片 + 分叉 `ConfirmModal`。

## 配置步骤

1. 服务器建独立库、最小权限账号、快照表 + 占位行（见上方 DDL）。
2. 每台电脑在「设置 → 跨设备同步」填 Tailscale 地址 / 端口 / 库名 / 账号 / 密码 / 本机名称，勾「启用」。
3. 先「测试连接」，再「保存配置」。
4. 第一台点「推送到云端」；另一台点「从云端拉取」，重启后生效。
5. 以后换电脑：用完推、开机拉。两台都改过时会弹确认，别盲目覆盖。
6. **换机拷贝数据目录前先重启一次**，确认设置页没有「有下载好的快照待应用」——
   否则 `pending.db` 会跟着目录被拷到新机，新机首跑就把刚拷过去的主库换成那份云端快照。
   同理，拷过去之后再改本机数据也没用：`apply_pending` 只跟拉取那一刻的哈希比。

## 验证

```bash
cd src-tauri && cargo test                 # 136 个单测（含 sync 的 53 个，拉取/推送用假 store 覆盖）
# 真实服务器集成测试，默认忽略；跑完会把云端行恢复成空快照 + version 0
DICTATION_SYNC_HOST=100.x.y.z DICTATION_SYNC_PORT=4407 \
DICTATION_SYNC_DB=dictation_sync DICTATION_SYNC_USER=dictation \
DICTATION_SYNC_PASSWORD=... cargo test live_mysql_roundtrip -- --ignored --nocapture
```

## 边界与坑

- 快照 blob 会进 MySQL 的 ROW binlog；磁盘紧张时别在云端留多份历史版本，表里只保留一行覆盖写。
- `max_allowed_packet` 默认可能是 4MB；本方案服务端已确认 64MB，客户端也显式声明 64MB。
- Windows 便携版数据在 exe 同级、macOS 在 `~/Library/Application Support/com.dictation.desktop`
  （硬约束 7）；同步凭据与状态都跟着 `data_dir` 走，不要另写路径。
- 整库恢复是**数据目录级操作**，不算「改进度」，不违反硬约束 1；但同步绝不能成为业务写入口。
- 替换前必须 `PRAGMA wal_checkpoint(TRUNCATE)` **且返回 busy=0** 才删 `-wal/-shm`，否则已提交事务会丢；
  `std::fs::rename` 在 Windows 同样会覆盖已存在目标文件，因此**不要先删旧库**（那会造出「删库成功但改名失败」的空窗）。
  失败分两类：校验类 / 缺配套状态 / 拉取后又改动过 → 把 pending 挪进 `backups/sync/pending-rejected-*.db` 并作废；
  库被其他连接占用、备份或改名失败这类瞬时错误 → **保留** pending，下次启动重试（设置页提示里会写明）。
- 凭据文件只在类 Unix 下设 600；Windows 便携版数据目录在 exe 同级，需自行控制该目录的访问权限。
- 快照用 `VACUUM INTO` 在持有 `Mutex<Connection>` 的命令线程内完成，期间其它命令排队。
  本应用数据量很小（词条级别，库通常 < 5 MB），耗时毫秒级；真到上百 MB 才需要把快照搬进 `spawn_blocking`。
- 升级程序跑迁移会加列、改变 `content_hash`：`db::init` 在迁移前记一份哈希，若它正好等于 `lastSyncedHash`
  就把基准挪到迁移后的值，避免升级后第一次拉取被误判成「本机有改动」。
- 默认不启用时，唯一的联网行为仍是自动升级检查。
