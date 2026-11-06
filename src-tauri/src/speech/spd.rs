//! Linux 的 `spd-say` 实现（speech-dispatcher，主流发行版的桌面语音服务）。
//!
//! spd-say 把文本交给 speech-dispatcher 守护进程排队后立即退出，所以：
//! - 「停止」和「先停上一句再读」都不能靠 kill 子进程，要用 `spd-say -S` 清空队列；
//! - 子进程要 `output()` 等退出再走，`spawn()` 不管会留僵尸。

use crate::error::{AppError, AppResult};
use crate::speech::Speaker;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

/// 探测成功才缓存：没装 speech-dispatcher 时每次给出可操作的提示，
/// 用户装好后不用重启应用（与 say.rs 的缓存策略一致）
static AVAILABLE: OnceLock<()> = OnceLock::new();

/// 「清队列 + 提交新句」是两个子进程、天然不原子：快速翻词时两次 speak 在
/// 阻塞线程池上并发，交错会把两个词都排进队列连读。用一把锁把 purge+submit
/// 串成临界区（macOS 的 kill+spawn 持 child 锁、SAPI 的 PURGE+新句在同一次
/// Speak 调用里，都天然原子，只有这里需要显式加锁）。
pub struct SpdSpeaker {
    gate: Mutex<()>,
}

impl SpdSpeaker {
    pub fn new() -> Self {
        Self {
            gate: Mutex::new(()),
        }
    }
}

impl Default for SpdSpeaker {
    fn default() -> Self {
        Self::new()
    }
}

/// 清空朗读队列；守护进程不在了也只能忽略
fn purge_queue() {
    let _ = Command::new("spd-say").arg("-S").output();
}

impl Speaker for SpdSpeaker {
    fn speak(&self, text: &str) -> AppResult<()> {
        if AVAILABLE.get().is_none() {
            detect().map_err(AppError::msg)?;
            let _ = AVAILABLE.set(());
        }
        let _gate = self
            .gate
            .lock()
            .map_err(|_| AppError::msg("朗读组件状态异常"))?;
        // 与 macOS / Windows 的统一语义：读新句子前先停掉上一句，
        // 否则翻过的词全在 speech-dispatcher 里排队连读
        purge_queue();
        Command::new("spd-say")
            .arg(text)
            .output()
            .map_err(|e| AppError::msg(format!("无法启动系统朗读（spd-say）：{e}")))?;
        Ok(())
    }

    fn stop(&self) {
        if let Ok(_gate) = self.gate.lock() {
            purge_queue();
        }
    }
}

fn detect() -> Result<(), String> {
    Command::new("spd-say")
        .arg("--version")
        .output()
        .map_err(|_| "未安装 speech-dispatcher（spd-say），无法朗读；可用系统包管理器安装".to_string())
        .map(|_| ())
}
