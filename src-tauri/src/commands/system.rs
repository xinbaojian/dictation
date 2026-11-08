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

/// 当前程序版本号，与 Cargo.toml / tauri.conf.json 由 publish 流程同步（scripts/bump-version.mjs）
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 安装完更新后重启应用。restart() 会立刻结束当前进程，前端 invoke 收不到返回值属正常。
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// 朗读一段文本（笔画复习页读词语 / 设置页试听）。异步：启动朗读后立即返回。
/// 引擎由 settings.ttsEngine 决定（系统 / 本地语音包，本地失败自动兜底系统，ADR-21）；
/// `model_id` 供设置页「试听」指定语音包，无视当前引擎设置。
/// 子进程 / COM / ONNX 推理都不能在 async 线程上直接做，照同步命令的规矩挪去阻塞线程池。
#[tauri::command]
pub async fn speak_text(
    state: State<'_, AppState>,
    text: String,
    model_id: Option<String>,
) -> AppResult<()> {
    let (engine, configured) = {
        let conn = state.conn()?;
        let s = settings::load(&conn)?;
        (s.tts_engine, s.tts_model_id)
    };
    let speech = state.speech.clone();
    tauri::async_runtime::spawn_blocking(move || {
        speech.speak(&engine, model_id.as_deref(), configured.as_deref(), &text)
    })
    .await
    .map_err(|_| AppError::msg("朗读没有完成"))?
}

/// 停止当前朗读（翻词 / 退出复习页时调用）；本地与系统两个引擎都停
#[tauri::command]
pub async fn stop_speak(state: State<'_, AppState>) -> AppResult<()> {
    let speech = state.speech.clone();
    tauri::async_runtime::spawn_blocking(move || speech.stop())
        .await
        .map_err(|_| AppError::msg("停止朗读失败"))
}
