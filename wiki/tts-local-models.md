---
title: 本地开源 TTS 语音包
updated: 2026-11-08
tags: [tts, adr-21]
related: [decisions.md, frontend.md, updater.md]
---

# 本地开源 TTS 语音包

**状态：2026-11-08 已落地（ADR-21）。** 模型不打包、设置页按需下载、系统朗读兜底；本页调研结论已实现，历史调研记录留档在下方。

## 实现后的实际形状（与调研的差异）

- ~~`sherpa-rs`~~：社区 crate 已废弃，改用**官方 `sherpa-onnx` crate 2.0.1**（静态链接预编译库，无需 cmake）
- 语音包目录 4 个（kokoro-int8-v1.1 / matcha-zh-baker+vocos / melo-zh-en / kokoro-v1.1），aishell3（8kHz）与 fanchen-C（HF-only 国内难访问）未上目录
- 播放管线：rodio（独立音频线程）+ 按内容哈希缓存 `tts-cache/`（64MB 淘汰），未等 Phase 2 直接建好
- 多音字 pinyin-pro 校正暂不做（`GenerationConfig.tokens` 已支持拼音输入，是将来挂钩点）；端到端验收用忽略测试 `kokoro_int8_end_to_end`

---

## 调研留档（2026-11-07，暂缓开发时的结论）

**状态（2026-11-07 时）：调研完成，暂缓开发。** 若 Phase 2（云端在线 TTS，见 ADR-20 边界段）落地后仍有离线高音质需求，按本页结论接入。（2026-11-08 已按此结论实现，见上方与 ADR-21。）

## 一句话结论

可行且与本项目架构高度契合：**sherpa-onnx 运行时 + 中文语音模型，按需下载一次、存数据目录、之后永久离线**——与 AI 讲解卡「生成一次离线复用」的 opt-in 哲学同构（ADR-20）。sherpa-onnx 官方文档有 [Tauri 桌面应用章节与示例](https://k2-fsa.github.io/sherpa/onnx/tauri/index.html)。

## 候选模型（官方 RTF 基准，树莓派 4 实测，桌面 CPU 快 5~10 倍）

| 模型 | 中文 | 体积 | RPi4 RTF@4线程 | 定位 |
|---|---|---|---|---|
| matcha-icefall-zh-baker | 女声 | 73MB | 0.39 | 入门首选：小、快、质量中上 |
| kokoro-multi-lang-v1.1 | 中英 103 音色 | 311MB | 3.19 | 质量最好，大而慢 |
| vits-zh-hf-fanchen-C | 187 音色 | 116MB | 1.60 | 多音色备选 |
| vits-melo-tts-zh_en | 中英 | 163MB | 2.52 | 中英混读 |
| aishell3 | 174 音色 | 30MB | 0.16 | 最快最小，音质偏老 |

RTF<1 即快于实时；桌面估算：一个词/短句合成 matcha 近乎即时、kokoro <1s——满足「翻词读词」节奏。

## 为什么选 sherpa-onnx

1. **中文前端（文本→音素、lexicon、分词）内建在 C++ 运行时**——用 `ort` 裸跑 ONNX 得在 Rust 重写中文 G2P（kokoro 的 misaki 前端），工作量不可比
2. Rust 集成两条路：sherpa-onnx 官方 Rust API；社区 crate `sherpa-rs`（MIT，0.6.8 带 `tts` 模块与 WAV 写出，2026-10 仍活跃）
3. CPU-only 无需 GPU；macOS / Windows / Linux 三平台一致；Piper 语音也可转进来跑
4. 模型许可证：kokoro 为 Apache-2.0，其余需在下载页逐个核对后才能分发

## 将来的集成形状（与现有架构对齐）

- 下载一次→离线复用：设置页「语音包」区，模型存数据目录 `tts-model/`（不进安装包、不进 git），断网永久可用
- `speech/` 的 `Speaker` trait 加第三实现：朗读引擎三选一（系统离线 / 云端在线 / 本地模型），合成走 `spawn_blocking`，播放与音频缓存管线复用 Phase 2 设计（rodio + 按内容哈希缓存）
- 安装包不膨胀：73~311MB 的语音包按需下载（对比 ADR-18 笔顺数据 33MB 是「必须随包」的决策）

## 落地前必须验证

1. **多音字准确度（最关键）**：vits/matcha 靠 lexicon 消歧、kokoro 靠自带中文前端；二年级词表是封闭集合，可全表合成逐词试听验收；`src/lib/pinyin.ts`（pinyin-pro 按词消歧，ADR-19）可做兜底校正
2. Windows 构建链：`sherpa-rs-sys` 需编 C++（cmake），CI Windows 矩阵需验证一次
3. sherpa-rs 文档率低（0.65%），遇坑可退 sherpa-onnx 官方 C API
4. 音质预期管理：本地 kokoro 与云端 GLM-TTS 仍有差距（云端情感韵律更强），定位是「离线高音质档」而非替代云端

## 建议节奏

Phase 2（云端在线 TTS）先行，但「朗读引擎」设置直接做成三选一的形状；本地模型作为 Phase 3 增量（管线全复用）。动手前先做概念验证：下载 matcha-zh-baker（73MB）对全词表合成试听，验收多音字与延迟后再定默认语音包。
