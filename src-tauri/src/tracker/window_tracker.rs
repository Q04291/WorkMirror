// WorkMirror - Window activity tracker.
//
// Responsibilities:
//   - Poll the active window once per second (interval configurable).
//   - Classify the application into a category (IDE, browser, comms, ...).
//   - Detect idle periods (no window change for 5 consecutive minutes).
//   - Persist every completed activity record immediately to the
//     encrypted SQLite database.
//   - Run the tracking loop in a dedicated Tokio task.
//   - Load / save category rules from / to a local JSON config file.
//
// Platform idle detection:
//   - Windows   -> GetLastInputInfo (winapi)
//   - macOS     -> CGEventSourceSecondsSinceLastEvent (CoreGraphics)
//   - Linux/X11 -> XScreenSaverQueryInfo (x11 crate)
//   - Other     -> no idle detection (always considered active)

use crate::db::{Activity, Database, DbError};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default poll interval in seconds.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 1;

/// Idle threshold in seconds (5 minutes).
const IDLE_THRESHOLD_SECS: u64 = 300;

/// Name of the category rules config file stored alongside the database.
const CATEGORY_CONFIG_FILE: &str = "category_rules.json";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Tracker-level errors.
#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("Failed to get active window: {0}")]
    ActiveWindow(String),

    #[error("Database error: {0}")]
    Database(#[from] DbError),

    #[error("Config error: {0}")]
    Config(String),
}

// ---------------------------------------------------------------------------
// Category configuration
// ---------------------------------------------------------------------------

/// Serializable category rules mapping category_name -> list of keywords.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryConfig {
    pub rules: HashMap<String, Vec<String>>,
}

impl Default for CategoryConfig {
    fn default() -> Self {
        let mut rules = HashMap::new();

        rules.insert(
            "IDE".into(),
            vec![
                "vscode".into(), "cursor".into(), "intellij".into(), "idea".into(),
                "webstorm".into(), "goland".into(), "pycharm".into(), "clion".into(),
                "vim".into(), "nvim".into(), "neovim".into(), "emacs".into(),
                "sublime".into(), "xcode".into(), "android studio".into(),
            ],
        );

        rules.insert(
            "browser".into(),
            vec![
                "chrome".into(), "firefox".into(), "safari".into(), "edge".into(),
                "brave".into(), "opera".into(), "arc".into(), "vivaldi".into(),
                "chromium".into(),
            ],
        );

        rules.insert(
            "comms".into(),
            vec![
                "wechat".into(), ("\u{5fae}\u{4fe1}").into(), "discord".into(), "slack".into(),
                "telegram".into(), "teams".into(), "microsoft teams".into(),
                ("\u{9489}\u{9489}").into(), "dingtalk".into(), ("\u{98de}\u{4e66}").into(), "lark".into(),
                "qq".into(), "whatsapp".into(), "signal".into(),
            ],
        );

        rules.insert(
            "terminal".into(),
            vec![
                "iterm2".into(), "windows terminal".into(), "gnome-terminal".into(),
                "alacritty".into(), "kitty".into(), "wezterm".into(),
                "foot".into(), "rxvt".into(), "konsole".into(),
                "xterm".into(), "warp".into(),
            ],
        );

        rules.insert(
            "music".into(),
            vec![
                "spotify".into(), "netease".into(), ("\u{7f51}\u{6613}\u{4e91}").into(),
                ("\u{0071}\u{0071}\u{97f3}\u{4e50}").into(), "apple music".into(), "music".into(),
            ],
        );

        CategoryConfig { rules }
    }
}

// ---------------------------------------------------------------------------
// Category rules I/O
// ---------------------------------------------------------------------------

/// Load category rules from the JSON config file.
/// Falls back to the built-in defaults if the file does not exist.
pub fn load_category_config() -> CategoryConfig {
    let path = category_config_path();
    if !path.exists() {
        return CategoryConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => CategoryConfig::default(),
    }
}

/// Persist category rules to the JSON config file.
pub fn save_category_config(cfg: &CategoryConfig) {
    if let Some(parent) = category_config_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(category_config_path(), content);
    }
}

fn category_config_path() -> PathBuf {
    dirs::data_dir()
        .map(|p| p.join("workmirror").join(CATEGORY_CONFIG_FILE))
        .unwrap_or_else(|| PathBuf::from(CATEGORY_CONFIG_FILE))
}

// ---------------------------------------------------------------------------
// Idle detection (platform-specific)
// ---------------------------------------------------------------------------

/// Returns the number of seconds since the last user input event.
/// Returns `None` if the platform is not supported.
fn idle_seconds() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        idle_seconds_windows()
    }

    #[cfg(target_os = "macos")]
    {
        idle_seconds_macos()
    }

    #[cfg(all(target_os = "linux"))]
    {
        idle_seconds_x11()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn idle_seconds_windows() -> Option<u64> {
    use std::mem;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::winuser::LASTINPUTINFO;

    unsafe {
        let mut lii: LASTINPUTINFO = mem::zeroed();
        lii.cbSize = mem::size_of::<LASTINPUTINFO>() as DWORD;
        if winapi::um::winuser::GetLastInputInfo(&mut lii) != 0 {
            let now = winapi::um::sysinfoapi::GetTickCount();
            let idle_ms = now.wrapping_sub(lii.dwTime);
            Some(idle_ms as u64 / 1000)
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
fn idle_seconds_macos() -> Option<u64> {
    extern "C" {
        fn CGEventSourceSecondsSinceLastEvent(source_id: u32, event_type: u32) -> f64;
    }
    const KCG_ANY_EVENT: u32 = std::u32::MAX;
    unsafe {
        let secs = CGEventSourceSecondsSinceLastEvent(0, KCG_ANY_EVENT);
        if secs >= 0.0 {
            Some(secs as u64)
        } else {
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn idle_seconds_x11() -> Option<u64> {
    use x11::xlib;
    use x11::xss;

    unsafe {
        let display = xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return None;
        }
        let root = xlib::XDefaultRootWindow(display);
        let mut info: xss::XScreenSaverInfo = std::mem::zeroed();
        let result = xss::XScreenSaverQueryInfo(display, root, &mut info);
        xlib::XCloseDisplay(display);
        if result != 0 {
            Some(info.idle as u64 / 1000)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Window tracker
// ---------------------------------------------------------------------------

/// The core activity tracker.
///
/// Typical usage:
///
/// ```ignore
/// let db = Database::new()?;
/// let tracker = WindowTracker::new(Arc::new(StdMutex::new(db)));
/// tracker.start().await;
/// // ... application runs ...
/// tracker.stop().await;
/// ```
pub struct WindowTracker {
    /// Shared database handle.
    db: Arc<StdMutex<Database>>,
    /// Flag; `true` while the tracking loop is running.
    running: Arc<AtomicBool>,
    /// Poll interval in seconds.
    poll_interval: Arc<StdMutex<u64>>,
    /// Latest captured activity, available for the frontend to query.
    current: Arc<StdMutex<Option<Activity>>>,
    /// Cached category rules (hot-reload not yet implemented).
    _category_rules: Arc<DashMap<String, Vec<String>>>,
    /// Timestamp of the last window switch, used for idle detection.
    last_window_switch: Arc<StdMutex<Instant>>,
    /// When the last break reminder was sent.
    last_break_reminder: Arc<StdMutex<Instant>>,
    /// Consecutive work minutes accumulated since last break reminder.
    consecutive_work_minutes: Arc<AtomicI64>,
    /// Current pending reminder text (None if none pending).
    current_reminder: Arc<StdMutex<Option<String>>>,
    /// Break reminder interval in minutes (default 50).
    break_interval_minutes: Arc<AtomicI64>,
}

impl WindowTracker {
    /// Create a new tracker bound to the given database.
    pub fn new(db: Arc<StdMutex<Database>>) -> Self {
        let cfg = load_category_config();
        let rules = Arc::new(DashMap::new());
        for (cat, keywords) in cfg.rules {
            rules.insert(cat, keywords);
        }

        WindowTracker {
            db,
            running: Arc::new(AtomicBool::new(false)),
            poll_interval: Arc::new(StdMutex::new(DEFAULT_POLL_INTERVAL_SECS)),
            current: Arc::new(StdMutex::new(None)),
            _category_rules: rules,
            last_window_switch: Arc::new(StdMutex::new(Instant::now())),
            last_break_reminder: Arc::new(StdMutex::new(Instant::now())),
            consecutive_work_minutes: Arc::new(AtomicI64::new(0)),
            current_reminder: Arc::new(StdMutex::new(None)),
            break_interval_minutes: Arc::new(AtomicI64::new(50)),
        }
    }

    /// Set a custom poll interval (in seconds).
    pub fn set_poll_interval(&self, secs: u64) {
        if let Ok(mut interval) = self.poll_interval.lock() {
            *interval = secs.max(1);
        }
    }

    /// Start the tracking loop in a background Tokio task.
    ///
    /// If tracking is already running, this is a no-op.
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running.
        }

        let running = Arc::clone(&self.running);
        let db = Arc::clone(&self.db);
        let poll_interval = Arc::clone(&self.poll_interval);
        let current = Arc::clone(&self.current);
        let last_switch = Arc::clone(&self.last_window_switch);
        let last_reminder = Arc::clone(&self.last_break_reminder);
        let consec_work = Arc::clone(&self.consecutive_work_minutes);
        let current_reminder = Arc::clone(&self.current_reminder);
        let break_interval = Arc::clone(&self.break_interval_minutes);

        tokio::spawn(async move {
            let mut previous_app: Option<String> = None;
            let mut ticks_since_break: u64 = 0;

            while running.load(Ordering::SeqCst) {
                let interval = {
                    poll_interval
                        .lock()
                        .map(|i| Duration::from_secs(*i))
                        .unwrap_or(Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS))
                };

                let now = Instant::now();

                // Fetch the currently active window.
                let active = match get_active_window_info() {
                    Ok(info) => info,
                    Err(_) => {
                        tokio::time::sleep(interval).await;
                        continue;
                    }
                };

                // Check idle: if the current app hasn't changed for 5
                // minutes and the system reports no input, we're idle.
                let idle_limit = Duration::from_secs(IDLE_THRESHOLD_SECS);
                let is_idle = if let Some(last) = previous_app.as_ref() {
                    *last == active.app_name
                        && last_switch.lock().map(|s| s.elapsed() >= idle_limit).unwrap_or(false)
                        && idle_seconds().map(|s| s >= IDLE_THRESHOLD_SECS).unwrap_or(false)
                } else {
                    false
                };

                if is_idle {
                    // Idle - reset work tracking, skip recording.
                    consec_work.store(0, Ordering::Relaxed);
                    ticks_since_break = 0;
                    tokio::time::sleep(interval).await;
                    continue;
                }

                // Detect window switch.
                let switched = match previous_app.as_ref() {
                    Some(prev) => prev != &active.app_name,
                    None => true,
                };

                if switched {
                    // Record the previous activity before switching.
                    if let Ok(db_lock) = db.lock() {
                        let cat = classify_app(&active.process_name, &active.app_name);
                        let activity = Activity {
                            id: None,
                            timestamp: chrono_now(),
                            window_title: active.title.clone(),
                            process_name: active.process_name.clone(),
                            duration_seconds: interval.as_secs() as i64,
                            category: cat,
                        };
                        let _ = db_lock.insert_activity(&activity);

                        // Update `current` for `get_current_activity()`.
                        if let Ok(mut cur) = current.lock() {
                            let mut a = activity.clone();
                            a.id = None;
                            *cur = Some(a);
                        }
                    }

                    if let Ok(mut st) = last_switch.lock() {
                        *st = now;
                    }
                }

                // --- Break reminder logic ---
                // Accumulate consecutive work time every tick (not just on switch).
                let interval_secs = interval.as_secs();
                ticks_since_break += interval_secs;
                let interval_min = break_interval.load(Ordering::Relaxed).max(1);
                let total_work_min = ticks_since_break / 60;
                consec_work.store(total_work_min as i64, Ordering::Relaxed);

                if total_work_min >= interval_min as u64 {
                    // Check if we already showed a reminder recently (within last 2 minutes)
                    let should_remind = last_reminder.lock()
                        .map(|lr| lr.elapsed() >= Duration::from_secs(120))
                        .unwrap_or(true);

                    if should_remind {
                        let texts = [
                            "你已经连续工作 50 分钟了，起来走走喝杯水 🌿",
                            "休息时间到！试试站起来伸展一下",
                            "今天已经高效工作 1 小时了，干得不错，记得休息 ✨",
                            "专注工作值得称赞，但现在该让眼睛和肩膀放松一下了 ☕",
                            "起来走走，看看窗外，给自己 5 分钟放空时间 🌻",
                        ];
                        let idx = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as usize) % texts.len();
                        let text = texts[idx].to_string();

                        if let Ok(mut rm) = current_reminder.lock() {
                            *rm = Some(text);
                        }
                        if let Ok(mut lr) = last_reminder.lock() {
                            *lr = Instant::now();
                        }
                    }

                    // Reset the counter regardless so it starts counting fresh.
                    ticks_since_break = 0;
                    consec_work.store(0, Ordering::Relaxed);
                }

                previous_app = Some(active.app_name);
                tokio::time::sleep(interval).await;
            }
        });
    }

    /// Signal the tracking loop to stop.
    ///
    /// The loop will exit after its current iteration completes (within
    /// at most one poll interval).
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Return `true` if the tracking loop is currently running.
    pub fn is_tracking(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Return the most recently captured activity, if any.
    pub fn get_current_activity(&self) -> Option<Activity> {
        self.current.lock().ok()?.clone()
    }

    /// Return the current pending reminder text, if any.
    /// Returns `Some(text)` if a reminder is waiting to be shown,
    /// or `None` if nothing is pending.
    pub fn check_reminder(&self) -> Option<String> {
        self.current_reminder.lock().ok()?.clone()
    }

    /// Mark the current reminder as handled (dismiss it).
    pub fn dismiss_reminder(&self) {
        if let Ok(mut rm) = self.current_reminder.lock() {
            *rm = None;
        }
    }

    /// Set the break reminder interval in minutes (default: 50).
    pub fn set_break_interval(&self, minutes: u64) {
        let m = minutes.max(1);
        self.break_interval_minutes.store(m as i64, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Active window info
// ---------------------------------------------------------------------------

/// Lightweight struct for window information.
#[derive(Debug, Clone)]
struct ActiveWindowInfo {
    title: String,
    process_name: String,
    app_name: String,
}

/// Retrieve the currently active window via `active-win-pos-rs`.
fn get_active_window_info() -> Result<ActiveWindowInfo, TrackerError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        match active_win_pos_rs::get_active_window() {
            Ok(win) => Ok(ActiveWindowInfo {
                title: win.title,
                process_name: win.process_path.to_string_lossy().to_string(),
                app_name: win.app_name,
            }),
            Err(e) => Err(TrackerError::ActiveWindow(format!("{:?}", e))),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(TrackerError::ActiveWindow("unsupported platform".into()))
    }
}

// ---------------------------------------------------------------------------
// App classification
// ---------------------------------------------------------------------------

/// Classify an application into a category based on keywords.
fn classify_app(process_name: &str, app_name: &str) -> String {
    let cfg = load_category_config();
    classify_app_with_config(process_name, app_name, &cfg)
}

/// Same as `classify_app` but uses the provided config instead of
/// reading from disk.  Useful for testing with predictable results.
fn classify_app_with_config(
    process_name: &str,
    app_name: &str,
    cfg: &CategoryConfig,
) -> String {
    let combined = format!(
        "{} {}",
        process_name.to_lowercase(),
        app_name.to_lowercase()
    );

    for (category, keywords) in &cfg.rules {
        for kw in keywords {
            if combined.contains(&kw.to_lowercase()) {
                return category.clone();
            }
        }
    }

    "other".to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return an ISO-8601 timestamp string for the current moment.
fn chrono_now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a WindowTracker backed by an in-memory database.
    fn create_tracker() -> (WindowTracker, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let db = Database::open_memory().expect("in-memory db");
        let db_arc = Arc::new(StdMutex::new(db));
        let tracker = WindowTracker::new(db_arc);
        (tracker, tmp)
    }

    // ------------------------------------------------------------------
    // Start / stop
    // ------------------------------------------------------------------

    #[test]
    fn initial_state_not_tracking() {
        let (tracker, _tmp) = create_tracker();
        assert!(!tracker.is_tracking());
    }

    #[tokio::test]
    async fn start_and_stop() {
        let (tracker, _tmp) = create_tracker();
        tracker.start().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(tracker.is_tracking());

        tracker.stop().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!tracker.is_tracking());
    }

    #[tokio::test]
    async fn double_start_is_idempotent() {
        let (tracker, _tmp) = create_tracker();
        tracker.start().await;
        tracker.start().await;
        assert!(tracker.is_tracking());
        tracker.stop().await;
    }

    // ------------------------------------------------------------------
    // Poll interval
    // ------------------------------------------------------------------

    #[test]
    fn set_poll_interval() {
        let (tracker, _tmp) = create_tracker();
        tracker.set_poll_interval(5);
        let interval = tracker.poll_interval.lock().unwrap();
        assert_eq!(*interval, 5);
    }

    #[test]
    fn poll_interval_minimum_one() {
        let (tracker, _tmp) = create_tracker();
        tracker.set_poll_interval(0);
        let interval = tracker.poll_interval.lock().unwrap();
        assert_eq!(*interval, 1);
    }

    // ------------------------------------------------------------------
    // Classification (using defaults, not disk config)
    // ------------------------------------------------------------------

    #[test]
    fn classify_ide_vscode() {
        let cfg = CategoryConfig::default();
        let cat = classify_app_with_config("vscode.exe", "Visual Studio Code", &cfg);
        assert_eq!(cat, "IDE");
    }

    #[test]
    fn classify_browser_chrome() {
        let cfg = CategoryConfig::default();
        let cat = classify_app_with_config("chrome.exe", "Google Chrome", &cfg);
        assert_eq!(cat, "browser");
    }

    #[test]
    fn classify_comms_wechat() {
        let cfg = CategoryConfig::default();
        let cat = classify_app_with_config("WeChat.exe", "WeChat", &cfg);
        assert_eq!(cat, "comms");
    }

    #[test]
    fn classify_terminal_wt() {
        let cfg = CategoryConfig::default();
        let cat = classify_app_with_config("WindowsTerminal.exe", "Windows Terminal", &cfg);
        assert_eq!(cat, "terminal");
    }

    #[test]
    fn classify_unknown_falls_to_other() {
        let cfg = CategoryConfig::default();
        let cat = classify_app_with_config("some_random_app.exe", "Unknown", &cfg);
        assert_eq!(cat, "other");
    }

    // ------------------------------------------------------------------
    // Category config (load / save / defaults)
    // ------------------------------------------------------------------

    #[test]
    fn default_config_has_all_categories() {
        let cfg = CategoryConfig::default();
        assert!(cfg.rules.contains_key("IDE"));
        assert!(cfg.rules.contains_key("browser"));
        assert!(cfg.rules.contains_key("comms"));
        assert!(cfg.rules.contains_key("terminal"));
        assert!(cfg.rules.contains_key("music"));
    }

    #[test]
    fn save_and_load_config() {
        let mut rules = HashMap::new();
        rules.insert("test".into(), vec!["myapp".into()]);
        let cfg = CategoryConfig { rules };

        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("category_rules.json");
        std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).expect("write");

        let content = std::fs::read_to_string(&path).expect("read");
        let loaded: CategoryConfig = serde_json::from_str(&content).expect("parse");
        assert!(loaded.rules.contains_key("test"));
        assert_eq!(loaded.rules["test"], vec!["myapp"]);
    }
}
