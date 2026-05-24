// WorkMirror — Tauri command bridge layer.
//
// Every command receives an `AppHandle` and extracts shared state via
// `try_state()`.  Errors are returned as `String` so the frontend gets
// user-friendly messages without exposing Rust error internals.

use crate::ai::Analyzer;
use crate::db::Database;
use crate::streak::StreakTracker;
use crate::tracker::WindowTracker;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri::Manager;

// ---------------------------------------------------------------------------
// Tracking lifecycle
// ---------------------------------------------------------------------------

/// Start the background window-activity tracker.
#[tauri::command]
pub async fn start_tracking(app: AppHandle) -> Result<(), String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;
    tracker.start().await;
    Ok(())
}

/// Gracefully stop the tracker.  The current iteration finishes before
/// the loop exits (at most one poll interval).
#[tauri::command]
pub async fn stop_tracking(app: AppHandle) -> Result<(), String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;
    tracker.stop().await;
    Ok(())
}

/// Returns `true` if the tracker loop is currently active.
#[tauri::command]
pub async fn get_tracking_status(app: AppHandle) -> Result<bool, String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;
    Ok(tracker.is_tracking())
}

// ---------------------------------------------------------------------------
// Reports & summaries
// ---------------------------------------------------------------------------

/// Generate a daily summary for the given date (YYYY-MM-DD).
///
/// Returns a JSON object with statistics and AI-generated insight (when
/// Ollama is available).
#[tauri::command]
pub async fn get_daily_summary(
    app: AppHandle,
    date: String,
) -> Result<serde_json::Value, String> {
    let analyzer = app
        .try_state::<Arc<Analyzer>>()
        .ok_or_else(|| "Analyzer not initialized".to_string())?;
    let summary = analyzer
        .generate_daily_summary(&date)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(summary).map_err(|e| e.to_string())
}

/// Generate a warm daily note for the given date (YYYY-MM-DD).
///
/// Returns a JSON object with activity stats and AI-generated personal
/// messages.  Streak days are computed automatically.
#[tauri::command]
pub async fn generate_daily_note(
    app: AppHandle,
    date: String,
) -> Result<serde_json::Value, String> {
    let analyzer = app
        .try_state::<Arc<Analyzer>>()
        .ok_or_else(|| "Analyzer not initialized".to_string())?;

    // Compute streak from the database.
    let streak_days = {
        let db = app
            .try_state::<Arc<Mutex<Database>>>()
            .ok_or_else(|| "Database not initialized".to_string())?;
        let db = db.lock().map_err(|e| format!("Lock error: {e}"))?;
        StreakTracker::get_streak(&db).map_err(|e| e.to_string())?
    };

    let note = analyzer
        .generate_daily_note(&date, streak_days)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(note).map_err(|e| e.to_string())
}

/// Generate a weekly report for the current week (Monday → Sunday).
#[tauri::command]
pub async fn get_weekly_report(app: AppHandle) -> Result<serde_json::Value, String> {
    let analyzer = app
        .try_state::<Arc<Analyzer>>()
        .ok_or_else(|| "Analyzer not initialized".to_string())?;

    // Calculate Monday 00:00 to Sunday 23:59 of the current ISO week.
    let now = chrono::Local::now();
    let weekday = now
        .format("%u")
        .to_string()
        .parse::<u32>()
        .unwrap_or(7);
    let days_from_monday = weekday.saturating_sub(1);
    let monday = now - chrono::Duration::days(days_from_monday as i64);
    let sunday = monday + chrono::Duration::days(6);

    let start = monday.format("%Y-%m-%d").to_string();
    let end = sunday.format("%Y-%m-%d").to_string();

    let report = analyzer
        .generate_weekly_report(&start, &end)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Return the most recently captured activity (or `null` if nothing has
/// been captured yet).
#[tauri::command]
pub async fn get_current_activity(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;

    match tracker.get_current_activity() {
        Some(activity) => {
            let value = serde_json::to_value(activity).map_err(|e| e.to_string())?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Statistics & data
// ---------------------------------------------------------------------------

/// Return the top-N applications by total usage time (today).
///
/// Each entry contains `name`, `total_seconds`, and `total_hours`.
#[tauri::command]
pub async fn get_app_stats(app: AppHandle, limit: usize) -> Result<Vec<serde_json::Value>, String> {
    let db = app
        .try_state::<Arc<Mutex<Database>>>()
        .ok_or_else(|| "Database not initialized".to_string())?;
    let db = db.lock().map_err(|e| format!("Lock error: {e}"))?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let stats = db
        .get_date_range_stats(&today, &today)
        .map_err(|e| e.to_string())?;

    let mut apps: Vec<serde_json::Value> = stats
        .app_breakdown
        .into_iter()
        .map(|(name, secs)| {
            serde_json::json!({
                "name": name,
                "total_seconds": secs,
                "total_hours": (secs as f64 / 3600.0 * 100.0).round() / 100.0,
            })
        })
        .collect();

    // Sort descending by total_seconds.
    apps.sort_by(|a, b| {
        b["total_seconds"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["total_seconds"].as_i64().unwrap_or(0))
    });
    apps.truncate(limit.max(1));

    Ok(apps)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Persist one or more configuration values.
///
/// `settings` is a JSON object — each key-value pair is written to the
/// `config` table (upsert semantics).
#[tauri::command]
pub async fn update_settings(app: AppHandle, settings: String) -> Result<(), String> {
    let db = app
        .try_state::<Arc<Mutex<Database>>>()
        .ok_or_else(|| "Database not initialized".to_string())?;
    let db = db.lock().map_err(|e| format!("Lock error: {e}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&settings).map_err(|e| format!("Invalid settings JSON: {e}"))?;

    if let Some(obj) = parsed.as_object() {
        for (key, value) in obj {
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            db.set_config(key, &val_str)
                .map_err(|e| format!("Failed to save `{key}`: {e}"))?;

            // Apply poll_interval immediately to the tracker.
            if key == "poll_interval" {
                if let Ok(secs) = val_str.parse::<u64>() {
                    if let Some(tracker) = app.try_state::<Arc<WindowTracker>>() {
                        tracker.set_poll_interval(secs);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Return all known configuration values as a JSON string.
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<String, String> {
    let db = app
        .try_state::<Arc<Mutex<Database>>>()
        .ok_or_else(|| "Database not initialized".to_string())?;

    let settings = {
        let db = db.lock().map_err(|e| format!("Lock error: {e}"))?;
        let known_keys = [
            "theme",
            "poll_interval",
            "ollama_url",
            "sync_enabled",
            "language",
        ];

        let mut settings = serde_json::Map::new();
        for key in &known_keys {
            if let Ok(Some(value)) = db.get_config(key) {
                settings.insert(key.to_string(), serde_json::Value::String(value));
            }
        }
        settings
    };

    serde_json::to_string(&settings).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Export & data management
// ---------------------------------------------------------------------------

/// Export activity data in the requested format.
///
/// Supported formats:
/// - `"json"` — full weekly report as pretty-printed JSON
/// - `"csv"`  — comma-separated values (date, hours, insight)
///
/// Returns the report as a string that the frontend can save or display.
#[tauri::command]
pub async fn export_report(app: AppHandle, format: String) -> Result<String, String> {
    // Release any database lock before the async analyzer call.
    {
        let _ = app.try_state::<Arc<Mutex<Database>>>();
    }

    let analyzer = app
        .try_state::<Arc<Analyzer>>()
        .ok_or_else(|| "Analyzer not initialized".to_string())?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let week_ago = (chrono::Local::now() - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();

    let report = analyzer
        .generate_weekly_report(&week_ago, &today)
        .await
        .map_err(|e| e.to_string())?;

    match format.to_lowercase().as_str() {
        "json" => serde_json::to_string_pretty(&report).map_err(|e| e.to_string()),
        "csv" => {
            let mut csv = String::from("date,total_hours,deep_work_hours,category,insight\n");
            for day in &report.daily_summaries {
                let clean_insight = day
                    .insight
                    .replace(',', " ")
                    .replace('\n', " ")
                    .replace('\r', "");
                let top_app = day
                    .top_apps
                    .first()
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("");
                csv.push_str(&format!(
                    "{},{:.2},{:.2},{},{}\n",
                    day.date,
                    day.total_active_hours,
                    day.deep_work_hours,
                    top_app,
                    clean_insight
                ));
            }
            Ok(csv)
        }
        other => Err(format!("Unsupported export format: `{other}`. Use `json` or `csv`.")),
    }
}

/// Delete all activity records and configuration entries.
///
/// **Destructive** — this cannot be undone.  The frontend should prompt
/// for confirmation before calling this command.
#[tauri::command]
pub async fn clear_all_data(app: AppHandle) -> Result<(), String> {
    let db = app
        .try_state::<Arc<Mutex<Database>>>()
        .ok_or_else(|| "Database not initialized".to_string())?;
    let db = db.lock().map_err(|e| format!("Lock error: {e}"))?;
    db.clear_all().map_err(|e| format!("Clear failed: {e}"))
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Simple health-check endpoint.
///
/// Returns the state of each major subsystem so the frontend can diagnose
/// connectivity issues.
#[tauri::command]
pub async fn health_check(app: AppHandle) -> Result<serde_json::Value, String> {
    let db_ok = app.try_state::<Arc<Mutex<Database>>>().is_some();
    let tracker_ok = app.try_state::<Arc<WindowTracker>>().is_some();
    let analyzer_ok = app.try_state::<Arc<Analyzer>>().is_some();

    let tracking = tracker_ok
        && app
            .try_state::<Arc<WindowTracker>>()
            .map(|t| t.is_tracking())
            .unwrap_or(false);

    Ok(serde_json::json!({
        "status": if db_ok && tracker_ok && analyzer_ok { "ok" } else { "degraded" },
        "database": db_ok,
        "tracker": tracker_ok,
        "tracking": tracking,
        "analyzer": analyzer_ok,
    }))
}

// ---------------------------------------------------------------------------
// Break reminder
// ---------------------------------------------------------------------------

/// Check if there's a pending break reminder. Returns the reminder text
/// or `None` if nothing is pending.
#[tauri::command]
pub async fn check_reminder(app: AppHandle) -> Result<Option<String>, String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;
    Ok(tracker.check_reminder())
}

/// Dismiss (clear) the current break reminder.
#[tauri::command]
pub async fn dismiss_reminder(app: AppHandle) -> Result<(), String> {
    let tracker = app
        .try_state::<Arc<WindowTracker>>()
        .ok_or_else(|| "Tracker not initialized".to_string())?;
    tracker.dismiss_reminder();
    Ok(())
}
