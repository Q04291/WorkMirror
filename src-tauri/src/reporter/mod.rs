// WorkMirror — Report generation module.
//
// Produces weekly report HTML using Handlebars templates, and exports
// summary PDFs via the `printpdf` crate.
//
// Layout:
//   - `report_generator` — `ReportGenerator` struct + helper types

pub mod report_generator;

pub use report_generator::{ReportGenerator, ReportError};
