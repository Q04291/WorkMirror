// WorkMirror — Window activity tracker.
//
// Exposes `WindowTracker` — the core background loop that polls the
// active window, classifies the application, detects idle periods, and
// persists every activity record directly to the encrypted SQLite DB.

pub mod window_tracker;

pub use window_tracker::WindowTracker;
