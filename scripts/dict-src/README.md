# 字典源数据（vendor）

`xinhua.json`：新华字典字级数据精简版，键是单字，值 `{ r?: 部首, y?: 释义 }`。

- 上游：https://github.com/pwxcoo/chinese-xinhua 的 `data/word.json`（16142 条）
- 精简规则：只留单字条目；部首仅当原始 `radicals` 恰好一个字符；释义去首尾空白保留原文。
- 抓取时间：2026-11-14。上游数据为社区抓取整理、无明确授权，**仅作个人学习用途**；
  若日后收到侵权通知应删除并改换数据源。
- 重新生成：见仓库根 `docs/superpowers/plans/2026-11-14-lookup-tool.md` Task 1 的命令。
- 消费方：`scripts/sync-dict-data.mjs`（打成 public/dict/ 分片）。
