use crate::domain::text::{self, SplitResult};
use crate::error::AppResult;
use crate::repo::groups::{self, GroupDto};
use crate::repo::words::{self, ImportResult, WordDto};
use crate::AppState;
use tauri::State;

// ---------- 分组 ----------

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<GroupDto>> {
    let conn = state.conn()?;
    groups::list(&conn)
}

#[tauri::command]
pub fn create_group(
    state: State<'_, AppState>,
    parent_id: Option<i64>,
    code: String,
    name: String,
) -> AppResult<GroupDto> {
    let conn = state.conn()?;
    groups::create(&conn, parent_id, &code, &name)
}

#[tauri::command]
pub fn update_group(
    state: State<'_, AppState>,
    id: i64,
    code: String,
    name: String,
) -> AppResult<GroupDto> {
    let conn = state.conn()?;
    groups::update(&conn, id, &code, &name)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: i64) -> AppResult<groups::DeleteResult> {
    let conn = state.conn()?;
    groups::delete(&conn, id)
}

// ---------- 词条 ----------

/// deep = true 时包含所有子分组的词条（打印整册用）
#[tauri::command]
pub fn list_words(
    state: State<'_, AppState>,
    group_id: i64,
    deep: Option<bool>,
) -> AppResult<Vec<WordDto>> {
    let conn = state.conn()?;
    if deep.unwrap_or(false) {
        words::list_deep(&conn, group_id)
    } else {
        words::list(&conn, group_id)
    }
}

#[tauri::command]
pub fn add_word(
    state: State<'_, AppState>,
    group_id: i64,
    text: String,
    note: Option<String>,
) -> AppResult<WordDto> {
    let conn = state.conn()?;
    words::add(&conn, group_id, &text, note.as_deref())
}

#[tauri::command]
pub fn update_word(
    state: State<'_, AppState>,
    id: i64,
    text: String,
    note: Option<String>,
) -> AppResult<WordDto> {
    let conn = state.conn()?;
    words::update(&conn, id, &text, note.as_deref())
}

#[tauri::command]
pub fn delete_word(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = state.conn()?;
    words::delete(&conn, id)
}

// ---------- 粘贴导入 ----------

/// 仅解析、不入库：给弹窗做实时预览，保证前后端只有一套切分规则
#[tauri::command]
pub fn preview_words(text: String) -> SplitResult {
    text::split_words(&text)
}

#[tauri::command]
pub fn import_words(
    state: State<'_, AppState>,
    group_id: i64,
    items: Vec<String>,
) -> AppResult<ImportResult> {
    let conn = state.conn()?;
    words::import(&conn, group_id, &items)
}
