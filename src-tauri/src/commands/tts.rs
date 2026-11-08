//! 语音包命令：目录列表 / 下载（后台线程）/ 取消 / 删除。
//! 朗读本身走既有 `speak_text` / `stop_speak`（见 system.rs），本文件不碰音频。

use crate::error::{AppError, AppResult};
use crate::tts::download;
use crate::tts::models;
use crate::AppState;
use serde::Serialize;
use tauri::State;

/// 设置页「语音包」卡片数据
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsModelView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_mb: u64,
    pub license_note: Option<String>,
    pub installed: bool,
    /// 进行中的下载（页面刷新后恢复进度条用）
    pub downloading: Option<download::ActiveInfo>,
}

#[tauri::command]
pub fn list_tts_models(state: State<'_, AppState>) -> AppResult<Vec<TtsModelView>> {
    let active = state.tts.snapshot();
    Ok(models::MODELS
        .iter()
        .map(|m| TtsModelView {
            id: m.id.to_string(),
            name: m.name.to_string(),
            description: m.description.to_string(),
            size_mb: m.size_mb,
            license_note: m.license_note.map(str::to_string),
            installed: models::is_installed(m, &state.data_dir),
            downloading: if active.as_ref().is_some_and(|a| a.model_id == m.id) {
                active.clone()
            } else {
                None
            },
        })
        .collect())
}

/// 启动后台下载线程后立即返回；进度经 tts-download-progress 事件推送
#[tauri::command]
pub fn download_tts_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    let spec = models::find(&id).ok_or_else(|| AppError::msg("未知语音包"))?;
    let cancel = state.tts.start(&id)?;
    let data_dir = state.data_dir.clone();
    let registry = state.tts.clone();
    std::thread::Builder::new()
        .name(format!("tts-download-{id}"))
        .spawn(move || download::run(app, spec, cancel, data_dir, registry))
        .map_err(|e| AppError::msg(format!("启动下载失败：{e}")))?;
    Ok(())
}

#[tauri::command]
pub fn cancel_tts_download(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if state.tts.cancel(&id) {
        Ok(())
    } else {
        Err(AppError::msg("没有正在下载的语音包"))
    }
}

/// 删除已下载的语音包（前端先过 ConfirmModal）；正在下载的同名包不许删
#[tauri::command]
pub fn delete_tts_model(state: State<'_, AppState>, id: String) -> AppResult<()> {
    models::find(&id).ok_or_else(|| AppError::msg("未知语音包"))?;
    if state.tts.snapshot().is_some_and(|a| a.model_id == id) {
        return Err(AppError::msg("这个语音包正在下载，请先取消"));
    }
    let dir = models::model_dir(&state.data_dir, &id);
    if !dir.is_dir() {
        return Err(AppError::msg("这个语音包还没有下载过"));
    }
    // 顺带清掉可能残留的临时目录 / 归档
    let root = models::model_root(&state.data_dir);
    for suffix in [".staging", ".tmp"] {
        let _ = std::fs::remove_dir_all(root.join(format!("{id}{suffix}")));
    }
    let _ = std::fs::remove_file(root.join(format!("{id}.tar.bz2")));
    let _ = std::fs::remove_file(root.join(format!("{id}.tar.bz2.part")));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
