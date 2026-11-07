use crate::ai::config::AiConfig;
use crate::domain::ai_card::{self, CardContent, ChatMessage, WordContext};
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// OpenAI 兼容 chat/completions 请求体（纯函数，便于单测）
pub fn build_body(model: &str, messages: &[ChatMessage]) -> Value {
    json!({
        "model": model,
        "messages": messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>(),
        "temperature": 0.7,
        "max_tokens": 500,
        "stream": false,
    })
}

/// 发请求并抽出 choices[0].message.content。30 秒超时；只被 spawn_blocking 调用。
fn post_chat(cfg: &AiConfig, body: &Value) -> AppResult<String> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::msg(format!("初始化 AI 客户端失败：{e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(body)
        .send()
        .map_err(|e| AppError::msg(format!("连接 AI 服务失败：{e}（请检查网络与地址）")))?;
    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| AppError::msg(format!("读取 AI 响应失败：{e}")))?;
    if !status.is_success() {
        let brief: String = text.chars().take(200).collect();
        return Err(AppError::msg(format!("AI 服务返回错误（HTTP {status}）：{brief}")));
    }
    let v: Value = serde_json::from_str(&text)?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::msg("AI 返回内容为空，请重试或换个模型"))
}

/// 设置页「测试连接」：最小请求验证连通与鉴权
pub fn test_connection(cfg: &AiConfig) -> AppResult<String> {
    let body = json!({
        "model": cfg.model,
        "messages": [{ "role": "user", "content": "请只回复四个字：连接正常" }],
        "max_tokens": 16,
        "temperature": 0,
        "stream": false,
    });
    let reply = post_chat(cfg, &body)?;
    let brief: String = reply.chars().take(50).collect();
    Ok(brief)
}

/// 生成讲解卡内容：调用 → domain 校验 → 不合格把病因回喂重试一次。
/// 只做 I/O 编排；校验规则全在 domain::ai_card（有单测）。
pub fn generate_content(cfg: &AiConfig, ctx: &WordContext) -> AppResult<(CardContent, String)> {
    let mut messages = ai_card::build_messages(ctx);
    let raw = post_chat(cfg, &build_body(&cfg.model, &messages))?;
    match ai_card::validate(&raw, &ctx.text) {
        Ok(content) => Ok((content, cfg.model.clone())),
        Err(first) => {
            messages.push(ChatMessage { role: "assistant".into(), content: raw });
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "你上次的输出不合格：{first}。请严格按要求的 JSON 格式重新输出，只输出 JSON 本身。"
                ),
            });
            let raw2 = post_chat(cfg, &build_body(&cfg.model, &messages))?;
            let content = ai_card::validate(&raw2, &ctx.text).map_err(|e| {
                AppError::msg(format!("生成的内容不合格式（{e}），请重试或换个模型"))
            })?;
            Ok((content, cfg.model.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_carries_model_and_messages() {
        let msgs = vec![
            ChatMessage { role: "system".into(), content: "s".into() },
            ChatMessage { role: "user".into(), content: "u".into() },
        ];
        let b = build_body("deepseek-chat", &msgs);
        assert_eq!(b["model"], "deepseek-chat");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "u");
        assert_eq!(b["max_tokens"], 500);
        assert_eq!(b["stream"], false);
    }

    /// 需要真实服务：设 AI_BASE_URL / AI_KEY / AI_MODEL 后
    /// `cargo test real_provider -- --ignored` 手动跑（与 sync 的真实服务器测试同规格）
    #[test]
    #[ignore = "需要真实 API Key"]
    fn real_provider_generates_valid_card() {
        let cfg = AiConfig {
            base_url: std::env::var("AI_BASE_URL").unwrap(),
            api_key: std::env::var("AI_KEY").unwrap(),
            model: std::env::var("AI_MODEL").unwrap(),
        };
        let ctx = WordContext { text: "碧绿".into(), wrong_chars: vec![0], wrong_count: 3, whole_word_wrong: 0 };
        let (content, _) = generate_content(&cfg, &ctx).unwrap();
        assert!(!content.mnemonic.is_empty());
        assert!(content.sentences.iter().all(|s| s.contains("碧绿")));
    }
}
