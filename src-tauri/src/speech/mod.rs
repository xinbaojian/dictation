#![allow(dead_code)]

use crate::error::AppResult;

/// TTS 抽象：朗读走系统自带离线语音（macOS `say` / Windows SAPI / Linux `spd-say`）。
/// speak 异步启动、立即返回；再次 speak 或 stop 会先停掉上一句。
pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str) -> AppResult<()>;
    /// 停止当前朗读（没有在播则无操作）
    fn stop(&self);
}

/// 本地语音包合成（ADR-21）：按模型朗读，失败由上层路由兜底到系统语音。
/// 独立于 `Speaker` 是因为本地合成需要知道「用哪个语音包」。
pub trait LocalVoice: Send + Sync {
    fn speak_model(&self, model_id: &str, text: &str) -> AppResult<()>;
    fn stop(&self);
}

pub mod local;
pub mod service;

pub use local::LocalTts;
pub use service::SpeechService;

pub struct NullSpeaker;

impl Speaker for NullSpeaker {
    fn speak(&self, _text: &str) -> AppResult<()> {
        Ok(())
    }

    fn stop(&self) {}
}

#[cfg(target_os = "macos")]
mod say;
#[cfg(target_os = "macos")]
pub use say::SaySpeaker as SystemSpeaker;

#[cfg(target_os = "windows")]
mod sapi;
#[cfg(target_os = "windows")]
pub use sapi::SapiSpeaker as SystemSpeaker;

#[cfg(all(unix, not(target_os = "macos")))]
mod spd;
#[cfg(all(unix, not(target_os = "macos")))]
pub use spd::SpdSpeaker as SystemSpeaker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_speaker_is_noop() {
        let s = NullSpeaker;
        assert!(s.speak("你好").is_ok());
        s.stop();
    }
}
