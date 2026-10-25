use crate::domain::sampler::{self, Scope};
use crate::error::AppResult;
use crate::repo::sessions::{self, ReviewResult, SessionDto, SessionSummaryDto, WrongMark};
use crate::repo::words::{self, MistakeDto};
use crate::repo::word_progress;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn count_candidates(
    state: State<'_, AppState>,
    group_ids: Vec<i64>,
    scope: String,
) -> AppResult<i64> {
    let conn = state.conn()?;
    let scope = Scope::parse(&scope)?;
    Ok(sampler::collect(&conn, &group_ids, scope)?.len() as i64)
}

#[tauri::command]
pub fn create_session(
    state: State<'_, AppState>,
    group_ids: Vec<i64>,
    scope: String,
    limit: i64,
    shuffle: bool,
) -> AppResult<SessionDto> {
    let conn = state.conn()?;
    let scope = Scope::parse(&scope)?;
    let id = sessions::create(&conn, &group_ids, scope, limit, shuffle)?;
    sessions::get(&conn, id)
}

#[tauri::command]
pub fn get_session(state: State<'_, AppState>, id: i64) -> AppResult<SessionDto> {
    let conn = state.conn()?;
    sessions::get(&conn, id)
}

#[tauri::command]
pub fn active_session(state: State<'_, AppState>) -> AppResult<Option<SessionDto>> {
    let conn = state.conn()?;
    sessions::active(&conn)
}

#[tauri::command]
pub fn update_session_progress(
    state: State<'_, AppState>,
    session_id: i64,
    current_seq: i64,
    skip_item_id: Option<i64>,
) -> AppResult<()> {
    let conn = state.conn()?;
    sessions::update_progress(&conn, session_id, current_seq, skip_item_id)
}

#[tauri::command]
pub fn finish_session(state: State<'_, AppState>, id: i64) -> AppResult<SessionDto> {
    let conn = state.conn()?;
    sessions::finish_quiz(&conn, id)
}

#[tauri::command]
pub fn abandon_session(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = state.conn()?;
    sessions::abandon(&conn, id)
}

#[tauri::command]
pub fn submit_review(
    state: State<'_, AppState>,
    session_id: i64,
    wrong_items: Vec<WrongMark>,
) -> AppResult<ReviewResult> {
    let conn = state.conn()?;
    sessions::submit_review(&conn, session_id, &wrong_items)
}

#[tauri::command]
pub fn retry_session(state: State<'_, AppState>, id: i64) -> AppResult<SessionDto> {
    let conn = state.conn()?;
    let new_id = sessions::retry_wrong_words(&conn, id)?;
    sessions::get(&conn, new_id)
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>, limit: i64) -> AppResult<Vec<SessionSummaryDto>> {
    let conn = state.conn()?;
    sessions::list(&conn, limit)
}

// ---------- 错题本 ----------

#[tauri::command]
pub fn list_mistakes(
    state: State<'_, AppState>,
    group_id: Option<i64>,
) -> AppResult<Vec<MistakeDto>> {
    let conn = state.conn()?;
    words::list_mistakes(&conn, group_id)
}

#[tauri::command]
pub fn set_word_mastered(
    state: State<'_, AppState>,
    word_id: i64,
    mastered: bool,
) -> AppResult<()> {
    let conn = state.conn()?;
    word_progress::set_mastered(&conn, word_id, mastered)
}
