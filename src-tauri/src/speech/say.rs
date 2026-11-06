//! macOS 的 `say` 子进程实现：系统自带、离线、支持中文语音（需系统安装）。
//!
//! 不经 shell、文本只作为独立参数传给 `say`，没有注入面（与 `commands/system.rs::open_data_dir`
//! 直接 spawn 的先例同款）。朗读是异步的：spawn 后立即返回，翻词时先 kill 上一句。

use crate::error::{AppError, AppResult};
use crate::speech::Speaker;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// 探测成功的中文语音缓存（只缓存 Ok(Some)）：`say -v ?` 要起子进程，不该每次朗读都跑。
/// 探测失败 / 没装中文语音**不缓存**——家长按提示装好语音后不用重启应用，翻词就能读出来。
static VOICE: OnceLock<String> = OnceLock::new();

/// 取中文语音；探测失败（say 不可用 / 没装中文语音）时每次重试，成功才落缓存
fn current_voice() -> Result<Option<String>, String> {
    if let Some(name) = VOICE.get() {
        return Ok(Some(name.clone()));
    }
    let detected = detect_voice()?;
    if let Some(name) = &detected {
        let _ = VOICE.set(name.clone());
    }
    Ok(detected)
}

pub struct SaySpeaker {
    /// 正在朗读的 say 子进程；翻词 / stop 时 kill 掉即可中断
    child: Mutex<Option<Child>>,
}

impl SaySpeaker {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
        }
    }
}

impl Default for SaySpeaker {
    fn default() -> Self {
        Self::new()
    }
}

impl Speaker for SaySpeaker {
    fn speak(&self, text: &str) -> AppResult<()> {
        let voice = current_voice().map_err(AppError::msg)?;
        let Some(voice) = voice.as_ref() else {
            return Err(AppError::msg(
                "这台 Mac 还没有中文语音，无法朗读：系统设置 → 辅助功能 → 朗读内容 → 系统声音，选一个中文声音即可",
            ));
        };

        let mut guard = self.child.lock().map_err(|_| AppError::msg("朗读组件状态异常"))?;
        kill_child(&mut guard);
        // spawn 不等待：say 边合成边播，函数立即返回，朗读在后台进行
        let child = Command::new("say")
            .arg("-v")
            .arg(voice)
            .arg(text)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::msg(format!("无法启动系统朗读（say）：{e}")))?;
        *guard = Some(child);
        Ok(())
    }

    fn stop(&self) {
        if let Ok(mut guard) = self.child.lock() {
            kill_child(&mut guard);
        }
    }
}

/// kill 并回收子进程，避免留下僵尸
fn kill_child(guard: &mut Option<Child>) {
    if let Some(mut c) = guard.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// 探测一个简体中文语音（zh_CN 优先，繁体兜底）。Err(消息) 表示 say 不可用。
/// 实测（macOS 14）输出行格式：`Eddy (中文（中国大陆）)     zh_CN    # 你好！我叫Eddy。`
fn detect_voice() -> Result<Option<String>, String> {
    let out = Command::new("say")
        .arg("-v")
        .arg("?")
        .output()
        .map_err(|e| format!("无法查询系统语音列表（say）：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(pick_zh_voice(parse_voices(&text)))
}

/// 简体（zh_CN / zh_SG）优先；没有简体时用繁体（zh_TW / zh_HK）兜底。
/// **兜底必须限定繁体**：`say -v ?` 的英语语音排在 zh 之前，不限语言的话第一个
/// en_US 会抢占兜底位，`say -v Albert 中文` 用英语语音合成中文会静默无声，
/// 而且不会走到「没有中文语音」的指引错误（港台家庭只装繁体是现实场景）。
fn pick_zh_voice(voices: Vec<VoiceEntry>) -> Option<String> {
    let mut fallback: Option<String> = None;
    for voice in voices {
        if voice.locale.starts_with("zh_CN") || voice.locale.starts_with("zh_SG") {
            return Some(voice.name);
        }
        if fallback.is_none() && (voice.locale.starts_with("zh_TW") || voice.locale.starts_with("zh_HK")) {
            fallback = Some(voice.name);
        }
    }
    fallback
}

pub struct VoiceEntry {
    pub name: String,
    pub locale: String,
}

/// 解析 `say -v ?` 的输出：行 = 语音名（可含空格与括号）+ locale + `#` 例句，
/// 各版 macOS 的 locale 都在 `#` 之前（`#` 后是朗读示例，直接丢弃）。
/// 按空白切分后，最后一个「形如 zh_CN / en-US 的短语言码」当 locale，其余整体当语音名。
pub fn parse_voices(output: &str) -> Vec<VoiceEntry> {
    let mut list = Vec::new();
    for line in output.lines() {
        let head = line.split('#').next().unwrap_or("").trim_end();
        let mut fields = head.split_whitespace();
        let Some(first) = fields.next() else { continue };
        // 从右往左找 locale 字段，剩下的都是语音名（名字本身可含空格/括号）
        let rest: Vec<&str> = fields.collect();
        let mut name_parts = vec![first];
        let mut locale = String::new();
        for f in rest {
            // locale 形如 zh_CN / zh-CN / en_US，最长不过 7 个字符；限长可避免把带连字符的语音名误判成 locale
            let is_locale = f.len() <= 7
                && (f.contains('_') || f.contains('-'))
                && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if is_locale && locale.is_empty() {
                locale = f.to_string();
            } else {
                name_parts.push(f);
            }
        }
        if locale.is_empty() {
            continue;
        }
        list.push(VoiceEntry {
            name: name_parts.join(" "),
            locale,
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS 14 实测格式：名字带括号、locale 用下划线、行尾 `#` 例句
    #[test]
    fn parses_modern_say_output() {
        let out = "Eddy (中文（中国大陆）)     zh_CN    # 你好！我叫Eddy。\n\
                   Eddy (中文（台湾）)       zh_TW    # 你好，我叫Eddy。\n\
                   Albert              en_US    # Hello! My name is Albert.\n";
        let zh: Vec<_> = parse_voices(out)
            .into_iter()
            .filter(|v| v.locale.starts_with("zh"))
            .collect();
        assert_eq!(zh.len(), 2);
        assert_eq!(zh[0].name, "Eddy (中文（中国大陆）)");
        assert_eq!(zh[0].locale, "zh_CN");
    }

    /// 语音名可含空格（Ting Ting / Bad News 都是真实存在的 voice），locale 在 `#` 之前
    #[test]
    fn parses_voice_names_containing_spaces() {
        let out = "Ting Ting           zh_CN    # 你好，我叫婷婷。\n\
                   Bad News            en_US    # The light you see...\n";
        let zh: Vec<_> = parse_voices(out)
            .into_iter()
            .filter(|v| v.locale.starts_with("zh"))
            .collect();
        assert_eq!(zh.len(), 1);
        assert_eq!(zh[0].name, "Ting Ting");
        assert_eq!(zh[0].locale, "zh_CN");
    }

    #[test]
    fn skips_lines_without_locale() {
        assert!(parse_voices("没有语言字段的行").is_empty());
        assert!(parse_voices("").is_empty());
    }

    /// 只有繁体语音的机器（港台家庭）：选繁体兜底，不能静默无声
    #[test]
    fn picks_traditional_when_no_simplified() {
        let voices = parse_voices(
            "Albert              en_US    # Hello\n\
             Meijia             zh_TW    # 你好，我是美佳。\n",
        );
        assert_eq!(pick_zh_voice(voices).as_deref(), Some("Meijia"));
    }

    /// 只有英语语音：宁可不朗读（报「没有中文语音」指引），也别被英语抢占兜底位
    #[test]
    fn picks_none_when_no_chinese_at_all() {
        let voices = parse_voices(
            "Albert              en_US    # Hello\n\
             Alice              it_IT    # Ciao\n",
        );
        assert_eq!(pick_zh_voice(voices), None);
    }

    /// 英语排在 zh 之前（`say -v ?` 实际顺序）也不能抢占繁体兜底位
    #[test]
    fn english_does_not_steal_fallback_slot() {
        let voices = parse_voices(
            "Albert              en_US    # Hello\n\
             Sinji              zh_HK    # 你好。\n",
        );
        assert_eq!(pick_zh_voice(voices).as_deref(), Some("Sinji"));
    }

    /// 简繁并存：简体优先
    #[test]
    fn prefers_simplified_over_traditional() {
        let voices = parse_voices(
            "Meijia             zh_TW    # 你好\n\
             Tingting           zh_CN    # 你好\n",
        );
        assert_eq!(pick_zh_voice(voices).as_deref(), Some("Tingting"));
    }

    /// 探测 + 真机朗读一条龙（会发声，默认忽略；本机验证用 `cargo test -- --ignored`）
    #[test]
    #[ignore]
    fn detect_voice_finds_chinese() {
        match detect_voice() {
            Ok(Some(name)) => {
                let out = Command::new("say")
                    .arg("-v")
                    .arg(&name)
                    .arg("-o")
                    .arg("/tmp/dictation-say-test.aiff")
                    .arg("你好，雪花")
                    .output()
                    .expect("say");
                assert!(out.status.success(), "say 合成失败：{out:?}");
                let _ = std::fs::remove_file("/tmp/dictation-say-test.aiff");
            }
            Ok(None) => panic!("本机没有中文语音"),
            Err(e) => panic!("{e}"),
        }
    }
}
