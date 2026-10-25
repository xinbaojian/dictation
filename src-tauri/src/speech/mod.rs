#![allow(dead_code)]

use crate::error::AppResult;

/// TTS 抽象：V1 只有空实现，V2 接入系统语音（SAPI / edge-tts）时新增实现并替换注入。
/// 之所以先立这个 trait，是为了让命令层与前端不必因为 TTS 而改动接口形状。
pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str) -> AppResult<()>;
}

pub struct NullSpeaker;

impl Speaker for NullSpeaker {
    fn speak(&self, _text: &str) -> AppResult<()> {
        Ok(())
    }
}
