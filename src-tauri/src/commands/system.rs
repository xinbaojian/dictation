use crate::error::{AppError, AppResult};
use crate::repo::backup::{self, BackupInfo, ImportSummary};
use crate::repo::settings::{self, SettingsDto};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<SettingsDto> {
    let conn = state.conn()?;
    settings::load(&conn)
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, value: SettingsDto) -> AppResult<SettingsDto> {
    let conn = state.conn()?;
    settings::save(&conn, &value)
}

#[tauri::command]
pub fn create_backup(state: State<'_, AppState>, dest_dir: Option<String>) -> AppResult<String> {
    let conn = state.conn()?;
    backup::create(&conn, &state.data_dir, dest_dir.as_deref())
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> AppResult<Vec<BackupInfo>> {
    backup::list(&state.data_dir)
}

#[tauri::command]
pub fn import_backup(state: State<'_, AppState>, path: String) -> AppResult<ImportSummary> {
    let conn = state.conn()?;
    backup::import(&conn, &path)
}

#[tauri::command]
pub fn open_data_dir(state: State<'_, AppState>) -> AppResult<()> {
    let dir = state.data_dir.clone();
    // 目录一般在初始化时即创建，这里兜底一下（用户运行中手动删掉后点开）
    std::fs::create_dir_all(&dir)?;
    // 各平台打开文件夹的命令不同：Windows=explorer，macOS=open，Linux=xdg-open
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map_err(|e| AppError::msg(format!("无法打开数据目录：{e}")))?;
    Ok(())
}

#[tauri::command]
pub fn data_dir_path(state: State<'_, AppState>) -> String {
    state.data_dir.to_string_lossy().to_string()
}
