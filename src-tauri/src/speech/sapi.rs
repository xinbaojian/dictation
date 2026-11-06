//! Windows 的 SAPI 实现（`ISpVoice`）：系统自带、离线；中文版 Windows 默认带中文语音（Huihui 等）。
//!
//! - `SPF_ASYNC`：Speak 立即返回，朗读在 SAPI 内部线程进行；
//! - `SPF_PURGEBEFORESPEAK`：同一条 voice 实例再次 Speak / 空文本 Speak 即清掉上一句（停止）；
//! - 选中文语音：默认 voice 的 token → `GetCategory()` → `EnumTokens("Language=804")`
//!   （804 = zh-CN 的 LCID；404 = zh-TW、C04 = zh-HK 兜底）；
//! - COM 线程模型：**CoInitializeEx 以线程为单位**，而 `spawn_blocking` 的 tokio 阻塞线程池
//!   会回收/新建线程，所以不能假设「初始化过一次整池都在 MTA」——每次 speak / stop 入口
//!   都各自调 `ensure_com()`（幂等，已初始化返回 S_FALSE 同样放行）。
//!   voice 实例只会在已进 MTA 的线程上创建与使用，跨线程共享走 `ZhVoice` 的 unsafe impl Send。
//!   特意**不调 CoUninitialize**：池线程随进程存活，反初始化会让缓存实例的 apartment 消失。
//!
//! 无法在 macOS 上运行验证：本文件靠 `cargo check --target x86_64-pc-windows-msvc`
//! （配合只含 windows crate 的最小检查工程，绕开 tauri 依赖树里 ring 的交叉编译 C 工具链问题）
//! + CI 的 Windows 构建兜底。

use crate::error::{AppError, AppResult};
use crate::speech::Speaker;
use std::sync::Mutex;
use windows::core::PCWSTR;
use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

/// 中文语音按 LCID（十六进制属性值）优先级：简体(804) → 繁体台湾(404) → 繁体香港(C04)
const ZH_LCIDS: &[&str] = &["804", "404", "C04"];

/// COM 接口指针默认不是 Send；这里跨线程共享是安全的——每个使用它的线程都先经过
/// `ensure_com()` 进了 MTA（见文件头注释），COM 的 apartment 规则保证了 MTA 对象的多线程直接调用。
struct ZhVoice(ISpVoice);
unsafe impl Send for ZhVoice {}

pub struct SapiSpeaker {
    /// 复用同一条 voice：SetVoice 选好的中文语音只设一次，PURGEBEFORESPEAK 的「停上一句」也依赖同一实例
    voice: Mutex<Option<ZhVoice>>,
}

impl SapiSpeaker {
    pub fn new() -> Self {
        Self {
            voice: Mutex::new(None),
        }
    }
}

impl Default for SapiSpeaker {
    fn default() -> Self {
        Self::new()
    }
}

/// 空结尾 UTF-16，给需要 PCWSTR 的 SAPI 参数用
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 保证当前线程已在 MTA。幂等：本线程已初始化时返回 S_FALSE，同样放行；
/// 被别的模型（STA）占用的线程不能拿裸 COM 指针硬用，直接报错。
fn ensure_com() -> AppResult<()> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(AppError::msg(format!("朗读组件初始化失败（COM {hr:?}）")));
    }
    Ok(())
}

impl Speaker for SapiSpeaker {
    fn speak(&self, text: &str) -> AppResult<()> {
        ensure_com()?;
        let mut guard = self
            .voice
            .lock()
            .map_err(|_| AppError::msg("朗读组件状态异常"))?;
        if guard.is_none() {
            *guard = Some(create_zh_voice()?);
        }
        let voice = guard.as_ref().unwrap().0.clone();
        drop(guard);

        let text_w = wide(text);
        unsafe {
            voice
                .Speak(
                    PCWSTR(text_w.as_ptr()),
                    (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32,
                    None,
                )
                .map_err(|e| AppError::msg(format!("朗读失败：{e}")))?;
        }
        Ok(())
    }

    fn stop(&self) {
        if ensure_com().is_err() {
            return;
        }
        let Ok(guard) = self.voice.lock() else { return };
        if let Some(v) = guard.as_ref() {
            // 空文本 + PURGEBEFORESPEAK = SAPI 的「清空朗读队列」用法
            unsafe {
                let _ = v.0.Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None);
            }
        }
    }
}

/// 建 voice 实例并选一个中文语音；找不到中文语音时报可操作的中文错误。
fn create_zh_voice() -> AppResult<ZhVoice> {
    ensure_com()?;
    unsafe {
        let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)
            .map_err(|e| AppError::msg(format!("系统语音服务不可用：{e}")))?;

        // Voices 类别从「当前默认语音」的 token 反查（SpObjectTokenCategory 没有可直接
        // CoCreateInstance 的 coclass，这是 windows-rs 里拿到类别实例最省事的路）。
        // 拿不到默认语音的机器多半是根本没装语音包，指引家长去装而不是「系统坏了」
        let category = voice
            .GetVoice()
            .map_err(|_| AppError::msg("未检测到系统语音：请在系统设置里安装语音包（设置 → 时间和语言 → 语音）"))?
            .GetCategory()
            .map_err(|_| AppError::msg("未检测到系统语音：请在系统设置里安装语音包（设置 → 时间和语言 → 语音）"))?;

        for lcid in ZH_LCIDS {
            let req = wide(&format!("Language={lcid}"));
            let Ok(tokens) = category.EnumTokens(PCWSTR(req.as_ptr()), PCWSTR::null()) else {
                continue;
            };
            let mut count = 0u32;
            if tokens.GetCount(&mut count).is_err() || count == 0 {
                continue;
            }
            if let Ok(token) = tokens.Item(0) {
                if voice.SetVoice(&token).is_ok() {
                    return Ok(ZhVoice(voice));
                }
            }
        }
    }
    Err(AppError::msg(
        "这台电脑还没有中文语音，无法朗读：设置 → 时间和语言 → 语音 → 添加语音，安装「中文（中国）」语音包即可",
    ))
}
