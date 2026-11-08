use crate::error::{AppError, AppResult};
use crate::repo::sync::write_private;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = ".ai-config.json";

/// 输出上限的本地校验上限：防手滑多打几个零；服务端模型真实上限由 provider 自己校验
pub const MAX_OUTPUT_TOKENS_LIMIT: u32 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// 输出上限（max_tokens）：None = 请求不带该字段，交服务端默认；思考型模型的
    /// 推理 token 也算输出预算，硬编码小值会把回答拦腰截断（finish_reason=length）。
    /// 旧配置文件没有此字段，serde default 兜住视为不限制。
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

/// 回传前端的视图：key 只给掩码，原值绝不离开后端
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigView {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub key_mask: String,
    pub max_output_tokens: Option<u32>,
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

pub fn load(dir: &Path) -> AppResult<Option<AiConfig>> {
    let path = config_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn load_required(dir: &Path) -> AppResult<AiConfig> {
    load(dir)?.ok_or_else(|| AppError::msg("请先在设置里配置 AI 服务"))
}

/// `api_key = None` 或空白表示沿用已保存的 key（前端密钥框留空即「不修改」）。
/// 与 apiKey 不同，`max_output_tokens = None` 是显式的「不限制」：
/// 前端表单即真相（数字框显示的就是当前值），保存永远覆盖旧值。
pub fn save(
    dir: &Path,
    base_url: &str,
    model: &str,
    api_key: Option<String>,
    max_output_tokens: Option<u32>,
) -> AppResult<()> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let model = model.trim().to_string();
    if base_url.is_empty() || model.is_empty() {
        return Err(AppError::msg("请填写服务地址和模型名"));
    }
    if let Some(n) = max_output_tokens {
        if n == 0 || n > MAX_OUTPUT_TOKENS_LIMIT {
            return Err(AppError::msg(format!(
                "最大输出 tokens 要在 1~{MAX_OUTPUT_TOKENS_LIMIT} 之间（留空 = 不限制）"
            )));
        }
    }
    let existing = load(dir).unwrap_or(None);
    let api_key = match api_key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => existing.map(|c| c.api_key).unwrap_or_default(),
    };
    if api_key.is_empty() {
        return Err(AppError::msg("请填写 API Key（只存在本机数据目录，不会上传）"));
    }
    let cfg = AiConfig { base_url, model, api_key, max_output_tokens };
    write_private(&config_path(dir), serde_json::to_string_pretty(&cfg)?.as_bytes())
}

pub fn clear(dir: &Path) -> AppResult<()> {
    let path = config_path(dir);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// 读失败（文件损坏等）一律视为未配置：设置页仍可重新保存覆盖修复
pub fn view(dir: &Path) -> AiConfigView {
    match load(dir) {
        Ok(Some(c)) => AiConfigView {
            configured: true,
            base_url: c.base_url,
            model: c.model,
            key_mask: mask_key(&c.api_key),
            max_output_tokens: c.max_output_tokens,
        },
        _ => AiConfigView {
            configured: false,
            base_url: String::new(),
            model: String::new(),
            key_mask: String::new(),
            max_output_tokens: None,
        },
    }
}

/// 掩码：保留头 3 尾 4，中间打码；短 key 全打码
fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}***{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ai-cfg-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_view_masks_key() {
        let dir = tmp_dir();
        save(
            &dir,
            "https://api.deepseek.com/",
            "deepseek-chat",
            Some("sk-1234567890abcd".into()),
            None,
        )
        .unwrap();
        let v = view(&dir);
        assert!(v.configured);
        assert_eq!(v.base_url, "https://api.deepseek.com", "尾部斜杠要归一");
        assert_eq!(v.model, "deepseek-chat");
        assert_eq!(v.key_mask, "sk-***abcd");
        assert_eq!(v.max_output_tokens, None);
        assert!(!serde_json::to_string(&v).unwrap().contains("1234567890"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_key_keeps_old_one() {
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-oldkey12345".into()), None).unwrap();
        save(&dir, "https://x.example", "m2", None, None).unwrap();
        let cfg = load(&dir).unwrap().unwrap();
        assert_eq!(cfg.api_key, "sk-oldkey12345");
        assert_eq!(cfg.model, "m2");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_without_any_key_errors() {
        let dir = tmp_dir();
        let err = save(&dir, "https://x.example", "m", None, None).unwrap_err();
        assert!(err.to_string().contains("API Key"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_then_unconfigured() {
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-abcdef123456".into()), Some(100000)).unwrap();
        clear(&dir).unwrap();
        assert!(!view(&dir).configured);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_reports_unconfigured_and_can_be_repaired() {
        let dir = tmp_dir();
        std::fs::write(config_path(&dir), b"{broken").unwrap();
        assert!(!view(&dir).configured);
        save(&dir, "https://x.example", "m", Some("sk-goodkey12345".into()), None).unwrap();
        assert!(view(&dir).configured);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-secret12345".into()), None).unwrap();
        let mode = std::fs::metadata(config_path(&dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "凭据文件必须 0600");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn old_config_without_max_tokens_loads_as_unlimited() {
        let dir = tmp_dir();
        // 旧版本写的文件没有 maxOutputTokens 字段：serde default 兜住，视为不限制
        std::fs::write(
            config_path(&dir),
            br#"{"baseUrl":"https://x.example","apiKey":"sk-oldkey12345","model":"m"}"#,
        )
        .unwrap();
        let cfg = load(&dir).unwrap().unwrap();
        assert_eq!(cfg.api_key, "sk-oldkey12345");
        assert_eq!(cfg.max_output_tokens, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_rejects_invalid_max_tokens() {
        let dir = tmp_dir();
        for bad in [0u32, MAX_OUTPUT_TOKENS_LIMIT + 1] {
            let err = save(&dir, "https://x.example", "m", Some("sk-abcdef123456".into()), Some(bad))
                .unwrap_err();
            assert!(err.to_string().contains("最大输出 tokens"), "bad = {bad}");
        }
        // 校验失败不落盘
        assert!(!config_path(&dir).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_persists_max_tokens_and_none_means_unlimited() {
        let dir = tmp_dir();
        save(&dir, "https://x.example", "m", Some("sk-abcdef123456".into()), Some(100000)).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().max_output_tokens, Some(100000));
        assert_eq!(view(&dir).max_output_tokens, Some(100000));
        // 清空表单再存 = 显式回到不限制（不是沿用旧值）
        save(&dir, "https://x.example", "m", None, None).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().max_output_tokens, None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
