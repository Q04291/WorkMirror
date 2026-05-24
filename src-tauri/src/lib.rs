// WorkMirror — Application entry point & module wiring.
//
// Initialises the encrypted database, window tracker, AI analyzer, and
// registers all Tauri commands.  On shutdown the tracker is stopped
// gracefully.

pub mod commands;
pub mod security;
pub mod db;
pub mod tracker;
pub mod ai;
pub mod reporter;
pub mod streak;

use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // -------------------------------------------------------------------
    // Module initialisation
    // -------------------------------------------------------------------

    // 1. Encrypted SQLite database.
    let database = db::Database::new().expect("Failed to initialise database");
    let db = Arc::new(Mutex::new(database));

    // 2. Window activity tracker (shares the database Arc).
    let tracker = tracker::WindowTracker::new(Arc::clone(&db));
    let tracker_arc = Arc::new(tracker);

    // 3. AI analysis engine (shares the database Arc, defaults to local
    //    Ollama at http://localhost:11434).
    let analyzer = ai::Analyzer::new(Arc::clone(&db), None);
    let analyzer_arc = Arc::new(analyzer);

    // -------------------------------------------------------------------
    // Tauri application builder
    // -------------------------------------------------------------------

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Expose shared state to commands via `app.try_state()`.
        .manage(db)
        .manage(tracker_arc)
        .manage(analyzer_arc)
        // Register all Tauri commands.
        .invoke_handler(tauri::generate_handler![
            // Tracking lifecycle
            commands::start_tracking,
            commands::stop_tracking,
            commands::get_tracking_status,
            // Reports & summaries
            commands::get_daily_summary,
            commands::generate_daily_note,
            commands::get_weekly_report,
            commands::get_current_activity,
            // Statistics
            commands::get_app_stats,
            // Settings
            commands::update_settings,
            commands::get_settings,
            // Export & data management
            commands::export_report,
            commands::clear_all_data,
            // Health
            commands::health_check,
            // Break reminder
            commands::check_reminder,
            commands::dismiss_reminder,
        ])
        .setup(|app| {
            // Optional: restore poll_interval from config on startup.
            if let Some(db_state) = app.try_state::<Arc<Mutex<db::Database>>>() {
                if let Ok(guard) = db_state.lock() {
                    if let Ok(Some(interval)) = guard.get_config("poll_interval") {
                        if let Ok(secs) = interval.parse::<u64>() {
                            drop(guard);
                            if let Some(tracker_state) = app.try_state::<Arc<tracker::WindowTracker>>()
                            {
                                tracker_state.set_poll_interval(secs);
                            }
                        }
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
