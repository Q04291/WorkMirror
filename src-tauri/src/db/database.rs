// WorkMirror — Encrypted SQLite database layer.
//
// All sensitive fields (window_title, process_name) are encrypted with
// the AES-256-GCM module (`security::crypto`) before being written to
// disk, and decrypted on read.  Non-sensitive fields (duration,
// category, timestamps) are stored in cleartext for efficient querying.

use crate::security::crypto;
use dirs::data_dir;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Database-layer error.  Wraps I/O, SQLite, and crypto errors with
/// contextual messages.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("I/O error ({context}): {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("Crypto error: {0}")]
    Crypto(#[from] crypto::SecurityError),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single activity record as stored in the database.
///
/// Sensitive fields: `window_title` and `process_name` are encrypted.
/// Non-sensitive: `timestamp`, `duration_seconds`, `category`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: Option<i64>,
    pub timestamp: String,
    pub window_title: String,
    pub process_name: String,
    pub duration_seconds: i64,
    pub category: String,
}

/// Aggregated usage statistics for a date range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    /// Total active time in seconds.
    pub total_active_time: i64,
    /// Time spent on "deep work" applications (e.g. IDEs) in seconds.
    pub deep_work_time: i64,
    /// Per-application breakdown: app_name → total_seconds.
    pub app_breakdown: HashMap<String, i64>,
    /// Number of window switches during the period.
    pub switch_count: i64,
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// The encrypted SQLite database.
pub struct Database {
    conn: Connection,
    /// Cached path so we know where to look for the DB file.
    _db_path: PathBuf,
}

impl Database {
    /// Open (or create) the database and ensure all tables exist.
    ///
    /// The database file is stored at `{app_data_dir}/workmirror/data.db`.
    /// All necessary parent directories are created on first run.
    pub fn new() -> Result<Self, DbError> {
        let data_dir = data_dir()
            .ok_or_else(|| DbError::Io {
                context: "cannot determine application data directory".into(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "data_dir is None"),
            })?;

        let db_dir = data_dir.join("workmirror");
        fs::create_dir_all(&db_dir).map_err(|e| DbError::Io {
            context: format!("failed to create datadir `{}`", db_dir.display()),
            source: e,
        })?;

        let db_path = db_dir.join("data.db");
        let conn = Connection::open(&db_path).map_err(|e| {
            DbError::Sqlite(format!("failed to open database `{}`: {e}", db_path.display()))
        })?;

        // Enable WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|e| DbError::Sqlite(format!("pragma: {e}")))?;

        let db = Database {
            conn,
            _db_path: db_path,
        };
        db.create_tables()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    ///
    /// The database is ephemeral — data is lost when the connection closes.
    pub fn open_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory().map_err(|e| {
            DbError::Sqlite(format!("failed to open in-memory DB: {e}"))
        })?;

        let db = Database {
            conn,
            _db_path: PathBuf::from(":memory:"),
        };
        db.create_tables()?;
        Ok(db)
    }

    // -----------------------------------------------------------------------
    // Table creation
    // -----------------------------------------------------------------------

    fn create_tables(&self) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction().map_err(|e| {
            DbError::Sqlite(format!("begin transaction: {e}"))
        })?;

        tx.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS activities (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp       TEXT    NOT NULL,
                window_title    BLOB   NOT NULL,
                process_name    BLOB   NOT NULL,
                duration_seconds INTEGER NOT NULL DEFAULT 0,
                category        TEXT   NOT NULL DEFAULT 'other'
            );

            CREATE INDEX IF NOT EXISTS idx_activities_timestamp
                ON activities(timestamp);

            CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .map_err(|e| DbError::Sqlite(format!("create tables: {e}")))?;

        tx.commit()
            .map_err(|e| DbError::Sqlite(format!("commit: {e}")))?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // CRUD – Activity
    // -----------------------------------------------------------------------

    /// Insert a new activity record.
    ///
    /// The `window_title` and `process_name` fields are automatically
    /// encrypted before being written to the database.
    pub fn insert_activity(&self, activity: &Activity) -> Result<(), DbError> {
        let encrypted_title = crypto::encrypt(activity.window_title.as_bytes())?;
        let encrypted_process = crypto::encrypt(activity.process_name.as_bytes())?;

        let tx = self.conn.unchecked_transaction().map_err(|e| {
            DbError::Sqlite(format!("begin transaction: {e}"))
        })?;

        tx.execute(
            "INSERT INTO activities (timestamp, window_title, process_name, duration_seconds, category)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                activity.timestamp,
                encrypted_title,
                encrypted_process,
                activity.duration_seconds,
                activity.category,
            ],
        )
        .map_err(|e| DbError::Sqlite(format!("insert: {e}")))?;

        tx.commit()
            .map_err(|e| DbError::Sqlite(format!("commit: {e}")))?;

        Ok(())
    }

    /// Retrieve all activities for a given date.
    ///
    /// `date` should be in the format `YYYY-MM-DD`.  The field is matched
    /// against the `timestamp` column with a `LIKE` prefix query.
    /// Encrypted fields are decrypted before being returned.
    pub fn get_activities_by_date(&self, date: &str) -> Result<Vec<Activity>, DbError> {
        let pattern = format!("{date}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, timestamp, window_title, process_name, duration_seconds, category
                 FROM activities
                 WHERE timestamp LIKE ?1
                 ORDER BY timestamp ASC",
            )
            .map_err(|e| DbError::Sqlite(format!("prepare: {e}")))?;

        let rows = stmt
            .query_map(params![pattern], |row| {
                let id: i64 = row.get(0)?;
                let ts: String = row.get(1)?;
                let encrypted_title: Vec<u8> = row.get(2)?;
                let encrypted_process: Vec<u8> = row.get(3)?;
                let dur: i64 = row.get(4)?;
                let cat: String = row.get(5)?;

                // Decrypt the sensitive fields.
                let title_bytes = crypto::decrypt(&encrypted_title)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let process_bytes = crypto::decrypt(&encrypted_process)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

                let window_title = String::from_utf8(title_bytes)
                    .unwrap_or_else(|_| "<invalid utf-8>".to_string());
                let process_name = String::from_utf8(process_bytes)
                    .unwrap_or_else(|_| "<invalid utf-8>".to_string());

                Ok(Activity {
                    id: Some(id),
                    timestamp: ts,
                    window_title,
                    process_name,
                    duration_seconds: dur,
                    category: cat,
                })
            })
            .map_err(|e| DbError::Sqlite(format!("query_map: {e}")))?;

        let mut activities = Vec::new();
        for row in rows {
            activities.push(row.map_err(|e| DbError::Sqlite(format!("row read: {e}")))?);
        }
        Ok(activities)
    }

    // -----------------------------------------------------------------------
    // Statistics
    // -----------------------------------------------------------------------

    /// Compute aggregated statistics for the date range `[start, end]`.
    ///
    /// Both dates should be in `YYYY-MM-DD` format.  The result includes
    /// total active time, approximate deep-work time, per-application
    /// breakdown, and the number of window switches.
    pub fn get_date_range_stats(&self, start: &str, end: &str) -> Result<Stats, DbError> {
        // Total active time and switch count in one query.
        let (total_active_time, switch_count): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(duration_seconds), 0),
                        MAX(0, COUNT(*) - 1)
                 FROM activities
                 WHERE timestamp >= ?1 AND timestamp < ?2",
                params![format!("{start}T00:00:00"), format!("{end}T23:59:59")],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| DbError::Sqlite(format!("stats query: {e}")))?;

        // Per-app breakdown: we need to decrypt process_name for each record.
        // A more efficient approach would store a pre-classified category
        // column, but for correctness we decrypt here.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT process_name, duration_seconds
                 FROM activities
                 WHERE timestamp >= ?1 AND timestamp < ?2",
            )
            .map_err(|e| DbError::Sqlite(format!("prepare breakdown: {e}")))?;

        let rows = stmt
            .query_map(
                params![format!("{start}T00:00:00"), format!("{end}T23:59:59")],
                |row| {
                    let encrypted: Vec<u8> = row.get(0)?;
                    let dur: i64 = row.get(1)?;
                    // Decrypt inline; on failure fall back to "unknown".
                    let name = crypto::decrypt(&encrypted)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or_else(|| "<unknown>".to_string());
                    Ok((name, dur))
                },
            )
            .map_err(|e| DbError::Sqlite(format!("breakdown rows: {e}")))?;

        let mut app_breakdown: HashMap<String, i64> = HashMap::new();
        let mut deep_work_time: i64 = 0;
        for row in rows {
            let (name, dur) = row.map_err(|e| DbError::Sqlite(format!("row: {e}")))?;
            *app_breakdown.entry(name.clone()).or_insert(0) += dur;

            // Heuristic: apps with "idea", "code", "studio", "terminal",
            // "vim", "emacs" in the name count as deep work.
            let lower = name.to_lowercase();
            if lower.contains("idea")
                || lower.contains("code")
                || lower.contains("studio")
                || lower.contains("terminal")
                || lower.contains("vim")
                || lower.contains("emacs")
                || lower.contains("neovim")
            {
                deep_work_time += dur;
            }
        }

        Ok(Stats {
            total_active_time,
            deep_work_time,
            app_breakdown,
            switch_count,
        })
    }

    // -----------------------------------------------------------------------
    // Configuration table helpers
    // -----------------------------------------------------------------------

    /// Read a config value by key.  Returns `None` if the key does not exist.
    pub fn get_config(&self, key: &str) -> Result<Option<String>, DbError> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| DbError::Sqlite(format!("get_config: {e}")))?;
        Ok(result)
    }

    /// Write (or overwrite) a config value.
    pub fn set_config(&self, key: &str, value: &str) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction().map_err(|e| {
            DbError::Sqlite(format!("begin transaction: {e}"))
        })?;

        tx.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| DbError::Sqlite(format!("set_config: {e}")))?;

        tx.commit()
            .map_err(|e| DbError::Sqlite(format!("commit: {e}")))?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Danger zone
    // -----------------------------------------------------------------------

    /// Wipe all activity data and config entries.
    ///
    /// This is a destructive operation and should be confirmed by the user
    /// before being called.
    pub fn clear_all(&self) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction().map_err(|e| {
            DbError::Sqlite(format!("begin transaction: {e}"))
        })?;

        tx.execute_batch(
            "DELETE FROM activities; DELETE FROM config;",
        )
        .map_err(|e| DbError::Sqlite(format!("clear_all: {e}")))?;

        tx.commit()
            .map_err(|e| DbError::Sqlite(format!("commit: {e}")))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (Database, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("test.db");

        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS activities (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp       TEXT    NOT NULL,
                window_title    BLOB    NOT NULL,
                process_name    BLOB    NOT NULL,
                duration_seconds INTEGER NOT NULL DEFAULT 0,
                category        TEXT    NOT NULL DEFAULT 'other'
            );
            CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .expect("create tables");

        let db = Database {
            conn,
            _db_path: db_path,
        };
        (db, dir)
    }

    // ------------------------------------------------------------------
    // Insert + query
    // ------------------------------------------------------------------

    #[test]
    fn insert_and_query_by_date() {
        let (db, _tmp) = create_test_db();

        let act = Activity {
            id: None,
            timestamp: "2026-05-24T10:00:00".into(),
            window_title: "WorkMirror - main.rs".into(),
            process_name: "code.exe".into(),
            duration_seconds: 120,
            category: "dev".into(),
        };
        db.insert_activity(&act).expect("insert");

        let results = db
            .get_activities_by_date("2026-05-24")
            .expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].window_title, "WorkMirror - main.rs");
        assert_eq!(results[0].process_name, "code.exe");
        assert_eq!(results[0].duration_seconds, 120);
        assert_eq!(results[0].category, "dev");
    }

    #[test]
    fn query_empty_date() {
        let (db, _tmp) = create_test_db();
        let results = db.get_activities_by_date("2099-01-01").expect("query");
        assert!(results.is_empty());
    }

    #[test]
    fn multiple_activities_same_date() {
        let (db, _tmp) = create_test_db();

        for i in 0..5 {
            let act = Activity {
                id: None,
                timestamp: format!("2026-05-24T{:02}:00:00", 9 + i),
                window_title: format!("Window {i}"),
                process_name: "test.exe".into(),
                duration_seconds: 60 * (i + 1),
                category: "test".into(),
            };
            db.insert_activity(&act).expect("insert");
        }

        let results = db.get_activities_by_date("2026-05-24").expect("query");
        assert_eq!(results.len(), 5);
        // Ordered by timestamp ASC.
        assert!(results[0].timestamp < results[4].timestamp);
    }

    // ------------------------------------------------------------------
    // Statistics
    // ------------------------------------------------------------------

    #[test]
    fn stats_empty_range() {
        let (db, _tmp) = create_test_db();
        let stats = db
            .get_date_range_stats("2026-01-01", "2026-01-02")
            .expect("stats");
        assert_eq!(stats.total_active_time, 0);
        assert_eq!(stats.switch_count, 0);
        assert!(stats.app_breakdown.is_empty());
    }

    #[test]
    fn stats_with_data() {
        let (db, _tmp) = create_test_db();

        let activities = vec![
            Activity {
                id: None,
                timestamp: "2026-05-24T09:00:00".into(),
                window_title: "WorkMirror - lib.rs".into(),
                process_name: "code.exe".into(),
                duration_seconds: 3600,
                category: "dev".into(),
            },
            Activity {
                id: None,
                timestamp: "2026-05-24T10:00:00".into(),
                window_title: "Chrome".into(),
                process_name: "chrome.exe".into(),
                duration_seconds: 1800,
                category: "browser".into(),
            },
            Activity {
                id: None,
                timestamp: "2026-05-24T10:30:00".into(),
                window_title: "Terminal".into(),
                process_name: "WindowsTerminal.exe".into(),
                duration_seconds: 600,
                category: "terminal".into(),
            },
        ];

        for a in &activities {
            db.insert_activity(a).expect("insert");
        }

        let stats = db
            .get_date_range_stats("2026-05-24", "2026-05-24")
            .expect("stats");

        assert_eq!(stats.total_active_time, 3600 + 1800 + 600);
        // code.exe and WindowsTerminal.exe count as deep work.
        assert_eq!(stats.deep_work_time, 3600 + 600);
        // 3 activities → 2 switches.
        assert_eq!(stats.switch_count, 2);
        assert_eq!(stats.app_breakdown.len(), 3);
    }

    // ------------------------------------------------------------------
    // Config
    // ------------------------------------------------------------------

    #[test]
    fn config_round_trip() {
        let (db, _tmp) = create_test_db();

        db.set_config("theme", "dark").expect("set");
        db.set_config("poll_interval", "5").expect("set");

        assert_eq!(db.get_config("theme").expect("get"), Some("dark".into()));
        assert_eq!(
            db.get_config("poll_interval").expect("get"),
            Some("5".into())
        );
    }

    #[test]
    fn config_nonexistent_key() {
        let (db, _tmp) = create_test_db();
        assert_eq!(
            db.get_config("nonexistent").expect("get"),
            None
        );
    }

    #[test]
    fn config_overwrite() {
        let (db, _tmp) = create_test_db();
        db.set_config("key", "v1").expect("set v1");
        db.set_config("key", "v2").expect("set v2");
        assert_eq!(db.get_config("key").expect("get"), Some("v2".into()));
    }

    // ------------------------------------------------------------------
    // Clear
    // ------------------------------------------------------------------

    #[test]
    fn clear_all_removes_data() {
        let (db, _tmp) = create_test_db();

        db.set_config("k", "v").expect("set");
        let act = Activity {
            id: None,
            timestamp: "2026-05-24T12:00:00".into(),
            window_title: "Test".into(),
            process_name: "test".into(),
            duration_seconds: 10,
            category: "test".into(),
        };
        db.insert_activity(&act).expect("insert");

        db.clear_all().expect("clear");

        assert_eq!(db.get_activities_by_date("2026-05-24").expect("q").len(), 0);
        assert_eq!(db.get_config("k").expect("get"), None);
    }

    // ------------------------------------------------------------------
    // Encrypted storage verification
    // ------------------------------------------------------------------

    #[test]
    fn encrypted_fields_are_not_cleartext() {
        let (db, _tmp) = create_test_db();

        let act = Activity {
            id: None,
            timestamp: "2026-05-24T08:00:00".into(),
            window_title: "SecretProject".into(),
            process_name: "confidential.exe".into(),
            duration_seconds: 30,
            category: "work".into(),
        };
        db.insert_activity(&act).expect("insert");

        // Read raw bytes from the DB to confirm encryption.
        let raw_title: Vec<u8> = db
            .conn
            .query_row(
                "SELECT window_title FROM activities WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("raw read");

        // The raw bytes should NOT contain the plaintext string.
        let raw_str = String::from_utf8_lossy(&raw_title);
        assert!(
            !raw_str.contains("SecretProject"),
            "plaintext must not be visible in the database"
        );
        // The encrypted blob should be longer than the plaintext (nonce +
        // ciphertext + tag).
        assert!(raw_title.len() > 12);
    }

    // ------------------------------------------------------------------
    // In-memory database
    // ------------------------------------------------------------------

    #[test]
    fn in_memory_works() {
        let db = Database::open_memory().expect("in-memory");

        let act = Activity {
            id: None,
            timestamp: "2026-05-24T00:00:00".into(),
            window_title: "in-memory test".into(),
            process_name: "mem.exe".into(),
            duration_seconds: 1,
            category: "test".into(),
        };
        db.insert_activity(&act).expect("insert");

        let results = db.get_activities_by_date("2026-05-24").expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].window_title, "in-memory test");
    }

    // ------------------------------------------------------------------
    // Large batch insert
    // ------------------------------------------------------------------

    #[test]
    fn batch_insert_100_records() {
        let (db, _tmp) = create_test_db();

        for i in 0..100 {
            let act = Activity {
                id: None,
                timestamp: format!("2026-05-24T{:02}:00:00", i % 24),
                window_title: format!("Window {i}"),
                process_name: "batch.exe".into(),
                duration_seconds: 60,
                category: "batch".into(),
            };
            db.insert_activity(&act).expect("insert");
        }

        let results = db.get_activities_by_date("2026-05-24").expect("query");
        assert_eq!(results.len(), 100);
    }
}
