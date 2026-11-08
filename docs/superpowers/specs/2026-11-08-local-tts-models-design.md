# 本地 TTS 语音包（按需下载 + 系统朗读兜底）设计

日期：2026-11-08 ｜ 分支：`feature/local-tts-models` ｜ 依据：`wiki/tts-local-models.md` 调研 + 用户确认的三项决策

## 目标

- TTS 模型**不打包进安装包**：设置页「语音包」区自行选择下载，存数据目录 `tts-model/<id>/`，下载一次永久离线。
- 朗读引擎二选一：**系统语音（默认，零下载）** / **本地语音包**；本地引擎缺模型或合成失败时**自动落回系统朗读**（最终兜底）。
- sherpa-onnx 运行时随安装包（静态链接），模型按需下载。

## 用户确认的决策

1. 模型清单 4 个（全部 GitHub Releases 直连，与 updater 同通道）：kokoro-int8-v1.1（约 120MB，推荐）、matcha-zh-baker（73MB + 51MB vocoder）、melo-tts-zh_en（163MB）、kokoro-v1.1 完整版（311MB）。
2. 下载源：GitHub 直连 + `.part` 断点续传（Range 请求），不做第三方镜像与本地导入。
3. 默认引擎保持「系统语音」，用户手动切换。

## 技术选型（较 wiki 调研的变化）

- **`sherpa-rs` 已废弃**（上游已有官方 Rust API），改用官方 **`sherpa-onnx` crate 2.0.1**（Apache-2.0）：默认静态链接，build.rs 自动从 GitHub Releases 下载平台预编译库，**无需 cmake/C++ 工具链**；可用 `SHERPA_ONNX_ARCHIVE_DIR` 离线覆盖。
- 播放：`rodio`（cpal 后端，SamplesBuffer 直放 f32/i16，无需解码 feature）；缓存：`hound` 读写 WAV。
- 解压：`bzip2` + `tar` crate 纯 Rust 解 `.tar.bz2`；tar 条目拒绝 `..`/绝对路径（路径穿越防护）。

## 后端形状

```
speech/            trait Speaker 不变；SystemSpeaker 不变
  local.rs         LocalTts：懒加载 OfflineTts（Mutex，按模型目录建引擎）、
                   synth(text)->(Vec<f32>, u32)、按内容哈希缓存（tts-cache/，64MB 上限按 mtime 淘汰）、
                   rodio OutputStream 常驻 + Sink 播放/停止；实现 Speaker trait
  service.rs       SpeechService::speak：engine=local 且模型完好 → local.speak，Err → system.speak 兜底；
                   engine=system → system.speak；stop 停两者
tts/
  models.rs        const 目录：id、名称、体积、许可证备注、下载源列表（TarBz2 / File 两类）、
                   每模型构建 OfflineTtsConfig 的纯函数、required_files 校验清单、默认 sid
  download.rs      后台线程下载（reqwest blocking）：进度事件 tts-download-progress、AtomicBool 取消、
                   .part + Range 续传（206 续写 / 200 重下）、解压到 .tmp 后原子 rename、删归档
commands/tts.rs    list_tts_models / download_tts_model / cancel_tts_download / delete_tts_model
speak_text         加可选 modelId 参数（设置页按模型「试听」，忽略当前引擎设置）
settings           ttsEngine（"system"|"local"，默认 system）+ ttsModelId（Option，须在目录内）
                   ——同步拉取替换时本机 settings 保留，现有机制已覆盖
```

模型目录数据（解压后文件名以 PoC 实测为准，required_files 校验兜底）：

| id | 源 | 关键文件 | 默认 sid |
|---|---|---|---|
| kokoro-int8-zh-en-v1.1 | tts-models/kokoro-int8-multi-lang-v1_1.tar.bz2 | model.onnx, voices.bin, tokens.txt, lexicon-zh/us-en.txt, dict/ | 47 (zf_xiaoxiao) |
| matcha-zh-baker | tts-models/matcha-icefall-zh-baker.tar.bz2 + vocoder-models/vocos-22khz-univ.onnx（裸文件） | model-steps-3.onnx, vocos-22khz-univ.onnx, lexicon.txt, tokens.txt, dict/, *.fst | 0 |
| melo-zh-en | tts-models/vits-melo-tts-zh_en.tar.bz2 | model.onnx, lexicon.txt, tokens.txt, dict/, *.fst | 0 |
| kokoro-zh-en-v1.1 | tts-models/kokoro-multi-lang-v1_1.tar.bz2 | 同 int8 | 47 |

## 前端形状

- `types.ts`/`ipc.ts`：TtsModelView、下载事件 payload、4 个新命令 + speakText 可选 modelId。
- `Settings.tsx`「朗读」区扩展：引擎单选（系统语音 / 本地语音包）；模型卡片（说明/体积/许可证备注 + 下载/取消/删除[ConfirmModal]/试听 + 进度条）；选 local 未装模型时提示「暂用系统语音兜底」。事件 `listen("tts-download-progress")` 驱动进度。
- 复习页零改动（speak_text 自动走路由）。

## 错误处理与边界

- 下载失败/取消：删 `.part` 保留与否——保留（续传），取消也保留以便重试；错误事件带中文消息。
- 引擎创建失败 / 合成失败 / 模型目录被删：speak 路由兜底系统语音，不向复习页抛错（自动朗读场景不打断孩子）。
- 删除正在使用的模型：后端允许删，路由兜底；UI 提示。
- 同时只允许一个下载任务（第二个请求直接报「已有下载任务」）。

## 测试

- 单测：目录完整性（id 唯一、URL 合法）、tar 路径穿越拒绝、Range 头构造、续传分支、缓存键稳定与淘汰、settings round-trip 与 legacy 默认、路由兜底（mock Speaker 计数）、解压落位（测试内构造小 tar.bz2）。
- 忽略集成（--ignored，需网络）：下载 kokoro-int8 → 建引擎 → 合成非空样本。
- `cargo test` + `cargo clippy --all-targets` + `npm run typecheck`；按 AGENTS.md 走干净子 agent 审查。

## 已知取舍

- 安装包增大（静态链 sherpa-onnx + onnxruntime，估 +20~40MB）：运行时必须随包。
- 不做归档 sha256 校验（发布源即官方 GitHub；引擎创建失败会兜底并报错）；不做 pinyin-pro 多音字校正（先试听验收，wiki 路线）；不做云端在线 TTS（设置形状为将来三选一留位）。
- matcha 数据集为非商用许可，UI 在该模型卡片注明。
