use crate::ai::{client, config};
use crate::ai::config::AiConfigView;
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

/// `apiKey = None` 表示沿用已保存的 key（前端密钥框留空即不修改）
#[tauri::command]
pub fn save_ai_config(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> AppResult<AiConfigView> {
    config::save(&state.data_dir, &base_url, &model, api_key)?;
    Ok(config::view(&state.data_dir))
}

#[tauri::command]
pub fn clear_ai_config(state: State<'_, AppState>) -> AiConfigView {
    // 清除不存在也算成功：目的是「回到未配置状态」
    let _ = config::clear(&state.data_dir);
    config::view(&state.data_dir)
}

#[tauri::command]
pub async fn test_ai_config(state: State<'_, AppState>) -> AppResult<String> {
    let dir = state.data_dir.clone();
    join(tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load_required(&dir)?;
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
/// 拿锁读词与错字上下文 → 放锁 → 无锁等 HTTP（30s 超时 + 回喂重试）→ 拿锁落库。
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
