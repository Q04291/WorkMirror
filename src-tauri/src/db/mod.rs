// WorkMirror — Encrypted SQLite database layer.
//
// All window_title and process_name fields are encrypted with AES-256-GCM
// before being written to disk, and decrypted on read.
// Non-sensitive fields (duration, category, timestamps) are stored in
// cleartext for efficient querying.

pub mod database;

pub use database::{Activity, Database, DbError, Stats};
