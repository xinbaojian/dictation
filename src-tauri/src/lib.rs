mod ai;
mod commands;
mod db;
mod domain;
mod error;
mod repo;
mod speech;
mod tts;

use error::{AppError, AppResult};
use rusqlite::Connection;
use speech::{LocalTts, SpeechService, SystemSpeaker};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::Manager;

pub struct AppState {
    db: Mutex<Connection>,
    data_dir: PathBuf,
    /// 语音包下载注册表（全局同时只允许一个任务）
    tts: Arc<tts::download::Downloads>,
    /// 朗读路由：系统 / 本地语音包，本地失败自动兜底系统（ADR-21）
    speech: SpeechService,
}

impl AppState {
    pub(crate) fn conn(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|_| AppError::msg("数据库连接异常，请重启应用"))
    }
}

pub fn run() {
    tauri::Builder::default()
        // 必须第一个注册：第二个进程会在启动时 checkpoint / 替换同一个 SQLite 库，
        // 与同步的 pending 替换叠加会导致旧实例的写入落进被 unlink 的文件里（静默丢数据）。
        // 这里改成把已有窗口提到前台，而不是开第二个进程。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // 更新端点与公钥在 tauri.conf.json；离线时只在前端主动 check 才会联网
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 只用于打开更新说明里的链接（前端限定 http/https，且必须用户点击）
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let (conn, data_dir) = db::init(app.handle())?;
            let speech = SpeechService::new(
                Arc::new(SystemSpeaker::new()),
                Arc::new(LocalTts::new(data_dir.clone())),
                data_dir.clone(),
            );
            app.manage(AppState {
                db: Mutex::new(conn),
                data_dir,
                tts: Arc::new(tts::download::Downloads::default()),
                speech,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::list_groups,
            commands::library::create_group,
            commands::library::update_group,
            commands::library::delete_group,
            commands::library::move_group,
            commands::library::list_words,
            commands::library::add_word,
            commands::library::update_word,
            commands::library::delete_word,
            commands::library::preview_words,
            commands::library::import_words,
            commands::quiz::count_candidates,
            commands::quiz::create_session,
            commands::quiz::get_session,
            commands::quiz::active_session,
            commands::quiz::update_session_progress,
            commands::quiz::finish_session,
            commands::quiz::abandon_session,
            commands::quiz::submit_review,
            commands::quiz::retry_session,
            commands::quiz::list_sessions,
            commands::quiz::delete_session,
            commands::quiz::list_mistakes,
            commands::quiz::add_mistake,
            commands::math::generate_math_sheet,
            commands::print::print_html,
            commands::quiz::set_word_mastered,
            commands::system::get_settings,
            commands::system::save_settings,
            commands::system::create_backup,
            commands::system::list_backups,
            commands::system::import_backup,
            commands::system::open_data_dir,
            commands::system::data_dir_path,
            commands::system::app_version,
            commands::system::restart_app,
            commands::system::speak_text,
            commands::system::stop_speak,
            commands::sync::sync_status,
            commands::sync::save_sync_config,
            commands::sync::test_sync_connection,
            commands::sync::sync_push,
            commands::sync::sync_pull,
            commands::sync::sync_discard_pending,
            commands::sync::sync_repair_state,
            commands::ai::get_ai_config,
            commands::ai::save_ai_config,
            commands::ai::clear_ai_config,
            commands::ai::test_ai_config,
            commands::ai::get_word_card,
            commands::ai::generate_word_card,
            commands::tts::list_tts_models,
            commands::tts::download_tts_model,
            commands::tts::cancel_tts_download,
            commands::tts::delete_tts_model,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
