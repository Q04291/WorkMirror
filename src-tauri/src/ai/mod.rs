// WorkMirror - Local AI analysis engine.
//
// Module layout:
//   - `ollama_client` - async HTTP client for a local Ollama instance.
//   - `analyzer`      - high-level functions that query the DB and call the
//                         AI to produce daily / weekly reports.
//
// If Ollama is not running the client returns a clear error so the UI can
// show a friendly message.  The analyzer degrades gracefully: when the AI
// is unavailable it still returns statistics-only summaries without failing.

pub mod ollama_client;
pub mod analyzer;

pub use analyzer::{AnalysisError, Analyzer, DailyNote, DailySummary, WeeklyReport};
pub use ollama_client::{AiError, generate};
