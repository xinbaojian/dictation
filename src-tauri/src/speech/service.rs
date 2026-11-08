//! 朗读路由：按设置选引擎，本地语音包失败 / 缺失时**自动落回系统朗读**（最终兜底）。
//!
//! 兜底是静默的（只打日志）：自动朗读跟着孩子翻词走，任何弹窗都会打断节奏；
//! 真正的病因（语音包损坏等）在设置页「试听」时才显式暴露。

use crate::error::AppResult;
use crate::speech::{LocalVoice, Speaker};
use crate::tts::models;
use std::path::PathBuf;
use std::sync::Arc;

pub struct SpeechService {
    pub system: Arc<dyn Speaker>,
    pub local: Arc<dyn LocalVoice>,
    /// 「模型是否完好」的判定：生产环境查磁盘（required 文件在位）；测试注入假实现
    ready: Arc<dyn Fn(&str) -> bool + Send + Sync>,
}

impl Clone for SpeechService {
    fn clone(&self) -> Self {
        Self {
            system: self.system.clone(),
            local: self.local.clone(),
            ready: self.ready.clone(),
        }
    }
}

impl SpeechService {
    pub fn new(system: Arc<dyn Speaker>, local: Arc<dyn LocalVoice>, data_dir: PathBuf) -> Self {
        let ready = Arc::new(move |id: &str| models::find(id).is_some_and(|m| models::is_installed(m, &data_dir)));
        Self { system, local, ready }
    }

    /// 朗读一句话。
    /// - `forced`：设置页「试听」指定的模型，无视引擎设置（听的就是这个包）
    /// - `engine` / `configured`：settings 里的 ttsEngine 与 ttsModelId
    pub fn speak(&self, engine: &str, forced: Option<&str>, configured: Option<&str>, text: &str) -> AppResult<()> {
        let (use_local, model) = resolve(engine, forced, configured);
        if use_local {
            let Some(id) = model else {
                eprintln!("引擎选了本地语音包但没有选择模型，走系统语音");
                return self.system.speak(text);
            };
            if (self.ready)(id) {
                match self.local.speak_model(id, text) {
                    Ok(()) => return Ok(()),
                    // 最终兜底：本地合成失败（引擎坏了 / 合成异常）不打扰孩子，直接系统朗读
                    Err(e) => eprintln!("本地朗读失败，已自动改用系统朗读：{e}"),
                }
            } else {
                eprintln!("本地语音包未就绪（可能被删除或未下完），已自动改用系统朗读");
            }
        }
        self.system.speak(text)
    }

    /// 停掉两种引擎的当前朗读
    pub fn stop(&self) {
        self.local.stop();
        self.system.stop();
    }
}

/// 引擎选择（纯函数，便于单测）：
/// 试听指定模型 > 引擎设置；引擎选 local 但没配模型 → 回系统。
fn resolve<'a>(engine: &str, forced: Option<&'a str>, configured: Option<&'a str>) -> (bool, Option<&'a str>) {
    if let Some(id) = forced {
        return (true, Some(id));
    }
    if engine == "local" {
        (true, configured)
    } else {
        (false, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 计数假 Speaker：可以配置成失败
    struct CountingSystem {
        ok: bool,
        calls: AtomicUsize,
    }
    impl Speaker for CountingSystem {
        fn speak(&self, _text: &str) -> AppResult<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.ok { Ok(()) } else { Err(AppError::msg("系统也失败")) }
        }
        fn stop(&self) {}
    }

    struct CountingLocal {
        fail: bool,
        calls: MutexVec,
    }
    // 简单包装，避免在每个测试里重复
    type MutexVec = std::sync::Mutex<Vec<String>>;
    impl LocalVoice for CountingLocal {
        fn speak_model(&self, model_id: &str, _text: &str) -> AppResult<()> {
            self.calls.lock().unwrap().push(model_id.to_string());
            if self.fail { Err(AppError::msg("本地合成失败")) } else { Ok(()) }
        }
        fn stop(&self) {}
    }

    /// 生成「永远返回 v」的就绪判定（返回类型注解让闭包拿到正确的 HRTB 签名）
    fn const_ready(v: bool) -> Arc<dyn Fn(&str) -> bool + Send + Sync> {
        Arc::new(move |_: &str| v)
    }

    fn service(local_fail: bool, ready: bool) -> (SpeechService, Arc<CountingSystem>, Arc<CountingLocal>) {
        let system = Arc::new(CountingSystem { ok: true, calls: AtomicUsize::new(0) });
        let local = Arc::new(CountingLocal { fail: local_fail, calls: std::sync::Mutex::new(vec![]) });
        (
            SpeechService {
                system: system.clone(),
                local: local.clone(),
                ready: const_ready(ready),
            },
            system,
            local,
        )
    }

    #[test]
    fn system_engine_never_touches_local() {
        let (svc, sys, loc) = service(false, true);
        svc.speak("system", None, Some("matcha-zh-baker"), "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 1);
        assert!(loc.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn local_engine_uses_configured_model() {
        let (svc, sys, loc) = service(false, true);
        svc.speak("local", None, Some("matcha-zh-baker"), "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 0, "本地成功不该动系统");
        assert_eq!(*loc.calls.lock().unwrap(), vec!["matcha-zh-baker".to_string()]);
    }

    /// 用户的核心要求：本地引擎失败 → 系统朗读兜底，不抛错
    #[test]
    fn local_failure_falls_back_to_system() {
        let (svc, sys, loc) = service(true, true);
        svc.speak("local", None, Some("matcha-zh-baker"), "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 1, "失败要兜底到系统");
        assert_eq!(loc.calls.lock().unwrap().len(), 1);
    }

    /// 模型目录不在位（被删 / 没下完）也兜底
    #[test]
    fn local_not_ready_falls_back_to_system() {
        let (svc, sys, loc) = service(false, false);
        svc.speak("local", None, Some("matcha-zh-baker"), "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 1);
        assert!(loc.calls.lock().unwrap().is_empty());
    }

    /// 引擎选 local 但没选模型：回系统，不算错误
    #[test]
    fn local_engine_without_model_uses_system() {
        let (svc, sys, _loc) = service(false, true);
        svc.speak("local", None, None, "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 1);
    }

    /// 设置页试听：无视当前引擎设置，直接用指定模型
    #[test]
    fn forced_model_overrides_engine() {
        let (svc, sys, loc) = service(false, true);
        svc.speak("system", Some("kokoro-int8-v1.1"), Some("matcha-zh-baker"), "词").unwrap();
        assert_eq!(sys.calls.load(Ordering::SeqCst), 0);
        assert_eq!(*loc.calls.lock().unwrap(), vec!["kokoro-int8-v1.1".to_string()]);
    }

    #[test]
    fn resolve_matrix() {
        assert_eq!(resolve("system", None, Some("m")), (false, None));
        assert_eq!(resolve("local", None, Some("m")), (true, Some("m")));
        assert_eq!(resolve("local", None, None), (true, None));
        assert_eq!(resolve("system", Some("k"), Some("m")), (true, Some("k")));
        assert_eq!(resolve("别的", None, None), (false, None), "未知引擎值当系统处理");
    }
}
