//! 本地 TTS 语音包：模型目录（[models]）与按需下载（[download]）。
//!
//! 模型不随安装包分发：全部来自 k2-fsa/sherpa-onnx 的 GitHub Releases
//! （与自动升级同一下载通道），下载一次存数据目录 `tts-model/<id>/`，
//! 之后永久离线。运行时（sherpa-onnx 静态库）随安装包，见 ADR-21。

pub mod download;
pub mod models;
