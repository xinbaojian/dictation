use crate::error::{AppError, AppResult};
use crate::repo::sync::{self, SyncConfigDto, SyncOutcome, SyncStatus};
use crate::AppState;
use tauri::State;

/// 网络 I/O 一律丢到 blocking 线程池，避免卡住 WebView 的命令线程。
fn join<T, E>(r: Result<AppResult<T>, E>) -> AppResult<T> {
    match r {
        Ok(v) => v,
        // 不依赖 runtime 的 JoinError 具体类型
        Err(_) => Err(AppError::msg("同步任务异常，请重试")),
    }
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    sync::status(&state.data_dir)
}

/// 密码只在写入时出现；`password = None` 表示沿用已保存的密码。
#[tauri::command]
pub fn save_sync_config(
    state: State<'_, AppState>,
    config: SyncConfigDto,
    password: Option<String>,
) -> AppResult<SyncStatus> {
    sync::save_credentials(&state.data_dir, &config, password)?;
    sync::status(&state.data_dir)
}

/// 测连接时不落盘：`config` 有值就用它直接测，避免「测一个错端口把可用凭据覆盖掉」。
#[tauri::command]
pub async fn test_sync_connection(
    state: State<'_, AppState>,
    config: Option<SyncConfigDto>,
    password: Option<String>,
) -> AppResult<String> {
    let dir = state.data_dir.clone();
    join(tauri::async_runtime::spawn_blocking(move || {
        sync::test_connection(&dir, config, password)
    })
    .await)
}

#[tauri::command]
pub async fn sync_push(state: State<'_, AppState>, force: bool) -> AppResult<SyncOutcome> {
    let dir = state.data_dir.clone();
    // conn 只在这个块里持有，绝不跨 await
    let plan = {
        let conn = state.conn()?;
        sync::prepare_push(&conn, &dir)
    }?;
    join(tauri::async_runtime::spawn_blocking(move || sync::perform_push(plan, force)).await)
}

/// 主动放弃已下载但还没应用的快照：清掉 pending.db / 待提交状态 / 错误痕迹，
/// 让被残留 pending 锁住的同步可以恢复。不动主库，也不碰 .sync-state.json。
#[tauri::command]
pub fn sync_discard_pending(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    sync::discard_pending(&state.data_dir)?;
    sync::status(&state.data_dir)
}

/// 状态文件损坏时，用户确认后隔离旧文件并重建状态（remoteVersion 归零，后续同步会走确认流程）。
#[tauri::command]
pub fn sync_repair_state(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    sync::repair_state(&state.data_dir)
}

#[tauri::command]
pub async fn sync_pull(state: State<'_, AppState>, force: bool) -> AppResult<SyncOutcome> {
    let dir = state.data_dir.clone();
    let plan = {
        let conn = state.conn()?;
        sync::prepare_pull(&conn, &dir)
    }?;
    join(tauri::async_runtime::spawn_blocking(move || sync::perform_pull(plan, force)).await)
}
