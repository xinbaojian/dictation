use crate::ai::{client, config};
use crate::ai::config::{AiConfig, AiConfigView};
use crate::domain::ai_card::WordContext;
use crate::error::{AppError, AppResult};
use crate::repo::{ai_cards, words};
use crate::AppState;
use tauri::State;

/// 网络/文件 I/O 挪去阻塞线程池的统一包装（与 commands/sync.rs 同款）
fn join<T, E>(r: Result<AppResult<T>, E>) -> AppResult<T> {
    match r {
        Ok(v) => v,
        Err(_) => Err(AppError::msg("AI 任务异常，请重试")),
    }
}

#[tauri::command]
pub fn get_ai_config(state: State<'_, AppState>) -> AiConfigView {
    config::view(&state.data_dir)
}

/// `apiKey = None` 表示沿用已保存的 key（前端密钥框留空即不修改）；
/// `maxOutputTokens = None` 是显式的「不限制」（表单即真相，见 config::save）
#[tauri::command]
pub fn save_ai_config(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
    max_output_tokens: Option<u32>,
) -> AppResult<AiConfigView> {
    config::save(&state.data_dir, &base_url, &model, api_key, max_output_tokens)?;
    Ok(config::view(&state.data_dir))
}

/// 配置文件不存在时 clear 内部也返回 Ok：目标是「回到未配置状态」；真 I/O 失败要透传给用户
#[tauri::command]
pub fn clear_ai_config(state: State<'_, AppState>) -> AppResult<AiConfigView> {
    config::clear(&state.data_dir)?;
    Ok(config::view(&state.data_dir))
}

/// 测连接不落盘（与 test_sync_connection 同思路）：`base_url` 传了表单值就测表单值，
/// 没传（空）就测已保存的配置。`api_key` 空则回退磁盘已存 key，两者皆无报「请填写 API Key」；
/// `max_output_tokens` 同理回退已存值（与 apiKey 的「留空=沿用」语义一致）。
#[tauri::command]
pub async fn test_ai_config(
    state: State<'_, AppState>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    max_output_tokens: Option<u32>,
) -> AppResult<String> {
    let dir = state.data_dir.clone();
    join(tauri::async_runtime::spawn_blocking(move || {
        let cfg = match base_url.map(|b| b.trim().trim_end_matches('/').to_string()) {
            Some(b) if !b.is_empty() => {
                // base_url / model 与 config::save 同样 trim 归一，但不写盘
                let model = model.unwrap_or_default().trim().to_string();
                let saved = config::load(&dir)?;
                let api_key = match api_key {
                    Some(k) if !k.trim().is_empty() => k.trim().to_string(),
                    _ => saved.as_ref().map(|c| c.api_key.clone()).unwrap_or_default(),
                };
                if api_key.is_empty() {
                    return Err(AppError::msg("请填写 API Key"));
                }
                let max_output_tokens =
                    max_output_tokens.or_else(|| saved.and_then(|c| c.max_output_tokens));
                AiConfig { base_url: b, api_key, model, max_output_tokens }
            }
            _ => config::load_required(&dir)?,
        };
        client::test_connection(&cfg)
    })
    .await)
}

#[tauri::command]
pub fn get_word_card(
    state: State<'_, AppState>,
    word_id: i64,
) -> AppResult<Option<ai_cards::AiCardDto>> {
    let conn = state.conn()?;
    ai_cards::get(&conn, word_id, "mnemonic")
}

/// 生成/重新生成讲解卡。锁编排（MutexGuard 绝不跨 await）：
/// 拿锁读词与错字上下文 → 放锁 → 无锁等 HTTP（60s 超时 + 回喂重试）→ 拿锁落库。
#[tauri::command]
pub async fn generate_word_card(
    state: State<'_, AppState>,
    word_id: i64,
) -> AppResult<ai_cards::AiCardDto> {
    let ctx = {
        let conn = state.conn()?;
        let word = words::get(&conn, word_id)?;
        let marks = words::marks(&conn, word_id)?;
        WordContext {
            text: word.text,
            wrong_chars: marks.wrong_chars,
            wrong_count: marks.wrong_count,
            whole_word_wrong: marks.whole_word_wrong,
        }
    };
    let dir = state.data_dir.clone();
    let cfg = join(
        tauri::async_runtime::spawn_blocking(move || config::load_required(&dir)).await,
    )?;
    let (content, model) = join(
        tauri::async_runtime::spawn_blocking(move || client::generate_content(&cfg, &ctx)).await,
    )?;
    let conn = state.conn()?;
    ai_cards::upsert(&conn, word_id, "mnemonic", &content, &model)
}
