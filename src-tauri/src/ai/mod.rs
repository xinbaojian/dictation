//! AI 讲解卡（OpenAI 兼容）：config 管凭据文件，client 管对话调用。
//! 默认不启用；只有数据目录里存在 .ai-config.json 才会有联网行为（ADR-20）。
// pub mod client; // Task 5 建 client.rs 后放开
pub mod config;
