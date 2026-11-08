---
title: 项目 wiki 索引
updated: 2026-11-05
tags: [index]
---

# 生词默写 · 项目 wiki

给人和 LLM 读的项目记忆。一页一主题，短、准、可跳转。

## 阅读约定

- 每页顶部有 frontmatter：`title / updated / tags / related`，其中 `updated` 是内容最后核对日期
- 每页正文不超过约 150 行，超过就拆分
- 断言尽量附「文件:符号」方便跳转，例如 `src-tauri/src/repo/sessions.rs:submit_review`
- 页面之间用相对链接互连；新增页面必须在本索引登记

## 索引

| 页面 | 一句话 | 什么时候读 |
|---|---|---|
| [overview.md](./overview.md) | 产品定位、用户、目标与非目标 | 第一次接触项目 |
| [architecture.md](./architecture.md) | 分层、运行时状态、IPC 契约、目录职责 | 要动后端结构 |
| [data-model.md](./data-model.md) | 表结构、字段语义、不变量、迁移策略 | 要改数据或加字段 |
| [quiz-flow.md](./quiz-flow.md) | 抽题 → 默写 → 批改 → 错题本 的完整状态流转 | 要改默写/批改逻辑 |
| [frontend.md](./frontend.md) | 路由、页面职责、组件与样式约定、应用图标 | 要改界面或换图标 |
| [decisions.md](./decisions.md) | 已成定局的设计取舍及原因 | 想「为什么不做 X」 |
| [testing.md](./testing.md) | 测试清单、怎么加测试 | 改完要验证 |
| [release.md](./release.md) | 一条命令发版、CI 打包矩阵、Release 说明生成规则 | 发版或改 CI |
| [updater.md](./updater.md) | 自动升级全链路、签名密钥、latest.json 格式、平台坑 | 改自动升级或排查发版签名 |
| [pitfalls.md](./pitfalls.md) | 踩过的坑与规避方式 | 遇到怪问题 |
| [roadmap.md](./roadmap.md) | 已完成里程碑与 V2 候选 | 规划下一步 |
| [glossary.md](./glossary.md) | 术语表（分组/词条/会话/出本…） | 读代码前对齐概念 |
| [sync.md](./sync.md) | 自有 MySQL 整库快照同步、CAS 冲突、pending 启动替换 | 配置或排查多机同步 |
| [tts-local-models.md](./tts-local-models.md) | 本地 TTS 语音包（已落地，ADR-21） | 调整语音包目录 / 排查本地朗读问题时 |

## 当前状态

M1–M5 全部完成：词表（多级分组 + 拖动调整结构）、默写、批改、错题本、历史、设置、手动备份、打印复习单、NSIS 安装包、可选跨设备同步。
后端 136 个单测通过（macOS，另有 1 个需真实服务器的集成测试默认忽略）；前端 `tsc --noEmit` + `vite build` 通过。详见 [roadmap.md](./roadmap.md)。
