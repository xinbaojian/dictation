---
title: 测试
updated: 2026-10-25
tags: [testing]
related: [architecture.md, quiz-flow.md]
---

# 测试

## 怎么跑

```bash
cd src-tauri && cargo test   # 49 个单测，秒级
npm run typecheck            # 前端类型（无单测框架，靠 strict + 类型）
npm run build                # 构建即验证
```

前端没有测试框架：页面逻辑薄、以调用后端为主，靠 `tsc` 严格模式 + 手动走查兜底。**不要**为了覆盖率引入 vitest，除非开始写复杂纯函数。

## 覆盖清单

| 模块 | 测什么 |
|---|---|
| `domain/text.rs`（8） | 换行/中英逗号/顿号/分号切分、去重计数、剥离 `1.` 序号、剔除纯符号、保留拉丁字母、全角空格 trim、空输入 |
| `domain/sampler.rs`（5） | 跨组去重只留一条、按分组过滤、空分组数组=全部、三种范围的 status 过滤、非法 scope 报错 |
| `repo/groups.rs`（3） | 自动编号与排序、删组级联删词、空名拒绝 |
| `repo/words.rs`（5） | trim 后入库、重复词友好报错、批量导入跳过已存在、改名撞车报错、删除 |
| `repo/word_progress.rs`（4） | 错→learning、新词对→mastered、learning 需连对 2 次、答错清零 streak |
| `repo/sessions.rs`（7） | 建会话顺序与条目、空池报错、批改写回词条状态、禁止重复提交、进度与 skip 持久化、active 查询、错词重开 |
| `repo/backup.rs`（2） | 导出→导入往返（含进度恢复）、坏文件报错 |

## 加测试的位置

- 纯逻辑 → 和实现同文件 `#[cfg(test)]`
- 需要数据库 → `crate::db::open_memory()`（内存库 + 全量迁移）
- 需要临时目录 → `std::env::temp_dir()` 下自建目录，测完删掉（见 `backup.rs` 测试）

## 改了这些必须补/改测试

抽题规则、切分规则、状态机（出本条件）、批改写回、备份格式。其余 UI 调整不需要。
