---
title: 测试
updated: 2026-11-05
tags: [testing]
related: [architecture.md, quiz-flow.md, sync.md]
---

# 测试

## 怎么跑

```bash
cd src-tauri && cargo test   # 136 个单测，秒级；另有 1 个 ignored 的真实服务器集成测试
npm run typecheck            # 前端类型（无单测框架，靠 strict + 类型）
npm run build                # 构建即验证
```

真实服务器集成测试（凭据只走环境变量，跑完会把云端行恢复成空快照 + version 0）：

```bash
DICTATION_SYNC_HOST=100.x.y.z DICTATION_SYNC_PORT=4407 \
DICTATION_SYNC_DB=dictation_sync DICTATION_SYNC_USER=dictation \
DICTATION_SYNC_PASSWORD=... cargo test live_mysql_roundtrip -- --ignored --nocapture
```

前端没有测试框架：页面逻辑薄、以调用后端为主，靠 `tsc` 严格模式 + 手动走查兜底。**不要**为了覆盖率引入 vitest，除非开始写复杂纯函数。

## 覆盖清单

| 模块 | 测什么 |
|---|---|
| `domain/text.rs`（9） | 换行/中英逗号/顿号/分号切分、去重计数、剥离 `1.` 序号、剔除纯符号、保留拉丁字母、全角空格 trim、空输入 |
| `domain/sampler.rs`（9） | 跨组去重只留一条、按分组过滤、空分组数组=全部、三种范围的 status 过滤、非法 scope 报错；连字排序：共享字的词必须相邻、无共享字仍是全排列、0/1 个词的边界 |
| `repo/groups.rs`（10） | 自动编号与排序、路径与子孙统计、删组级联删词、空名拒绝；拖动：换父级带子树、同级重排、跨父级顺序、自定义编号保留、拖进自己子孙被拒、未知分组被拒 |
| `repo/words.rs`（12） | trim 后入库、重复词友好报错、批量导入跳过已存在、改名撞车报错、删除；错题本按字下标聚合、多次标错合并升序、整词错不带下标、词条改短后越界下标丢弃；手工加错题（新词/已有词复用、下标合并去重、改名清空） |
| `repo/word_progress.rs`（4） | 错→learning、新词对→mastered、learning 需连对 2 次、答错清零 streak |
| `repo/sessions.rs`（28） | 建会话顺序与条目、空池报错、批改写回词条状态、错字下标去重升序、禁止重复提交、进度与 skip 持久化、active 查询、错词重开、删记录级联删条目且不动词条进度；连字偏好开启后不漏词、默认偏好（除连字）全开、全关等于纯随机；上一场词只排后不删词（题量不缩水）、新词优先于已会词、含弱字的词（含跨词共享字）优先；同批优先不重复用字、字不够时兜底补满、去重不跨档挤掉高优先级词、题量为 0 或超过候选数时保留全部；手工标的错字进弱字池；同一字在多个词里错时优先（去重后按词数判定）、同一词重复错不抬高计数 |
| `repo/backup.rs`（3） | 导出→导入往返（含进度恢复）、嵌套分组往返、坏文件报错 |
| `repo/sync.rs`（53） | 内容哈希稳定/随数据变化、快照往返与 `_sync_meta` 校验、拒绝更高 `user_version`、凭据 600 且不回传配置、设备 ID 稳定、pending 替换留完整备份、坏 pending 挪走并留错误痕迹；**回归**：已同步过的库仍可再次导出快照、pending 状态只在替换成功后提交、pending 期间禁止推送/拉取、无 pending 启动时补提交孤立状态、补提交不会回退更新版本的状态、放弃 pending 不重置主状态也不误删孤儿状态、坏状态文件降级展示但推送/拉取仍严格拒绝、坏凭据文件可重新保存修复、端口 0 被拒；替换失败原因回显且不被启动自动清除、损坏状态可隔离留档后重建、私有文件原子写不留临时垃圾 |
| `db`（8） | 可写目录探测（非 macOS）/ macOS 恒用 `app_data_dir`、旧库只搬一次、无标记才搬、孤儿 WAL 清理、删库不复活；旧 identifier 目录定位与迁移优先级、备份只补不覆盖 |

## 加测试的位置

- 纯逻辑 → 和实现同文件 `#[cfg(test)]`
- 需要数据库 → `crate::db::open_memory()`（内存库 + 全量迁移）
- 需要临时目录 → `std::env::temp_dir()` 下自建目录，测完删掉（见 `backup.rs` 测试）
- 需要真实 MySQL → `#[ignore]` + 环境变量，见 `repo/sync.rs::live_mysql_roundtrip`

## 改了这些必须补/改测试

抽题规则、切分规则、状态机（出本条件）、批改写回、备份格式、**同步快照与冲突判定**。其余 UI 调整不需要。
