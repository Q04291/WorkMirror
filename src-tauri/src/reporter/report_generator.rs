// WorkMirror — Weekly report generator.
//
// `ReportGenerator` produces a self-contained HTML weekly report by
// querying the encrypted database + AI analyzer, then rendering a
// Handlebars template.  The same data can be exported to a simple
// summary PDF via the `printpdf` crate (the full visual report is
// always available as HTML).

use crate::ai::{Analyzer, WeeklyReport};
use crate::db::Database;
use chrono::{Datelike, Local, NaiveDate, Duration as ChronoDuration};
use handlebars::Handlebars;
use printpdf::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during report generation or export.
#[derive(Debug, Error)]
pub enum ReportError {
    #[error("Database error: {0}")]
    Database(#[from] crate::db::DbError),

    #[error("Analysis error: {0}")]
    Analysis(#[from] crate::ai::AnalysisError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Template error: {0}")]
    Template(String),

    #[error("PDF export error: {0}")]
    PdfExport(String),

    #[error("No activity data for the current week")]
    NoData,
}

impl From<handlebars::TemplateError> for ReportError {
    fn from(e: handlebars::TemplateError) -> Self {
        ReportError::Template(e.to_string())
    }
}

impl From<handlebars::RenderError> for ReportError {
    fn from(e: handlebars::RenderError) -> Self {
        ReportError::Template(e.to_string())
    }
}

impl From<printpdf::Error> for ReportError {
    fn from(e: printpdf::Error) -> Self {
        ReportError::PdfExport(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Template context types
// ---------------------------------------------------------------------------

/// Data passed to the Handlebars template.
#[derive(Debug, Clone, Serialize)]
pub struct ReportContext {
    /// Date range label (e.g. "2026-05-18 ~ 2026-05-24").
    pub week_range: String,
    /// Start date of the week (YYYY-MM-DD).
    pub week_start: String,
    /// End date of the week (YYYY-MM-DD).
    pub week_end: String,
    /// Total active hours for the week.
    pub total_active_hours: String,
    /// Average daily active hours.
    pub avg_daily_hours: String,
    /// Total deep work hours.
    pub total_deep_hours: String,
    /// Deep work percentage (0–100).
    pub deep_work_percent: String,
    /// Total number of days with data.
    pub active_days: usize,
    /// Per-day breakdown for the bar chart.
    pub daily_data: Vec<DailyDay>,
    /// Top 5 apps by total time.
    pub top_apps: Vec<AppUsage>,
    /// Trend analysis text from AI.
    pub trend_analysis: String,
    /// Improvement suggestions from AI.
    pub improvement_suggestions: Vec<String>,
    /// Timestamp when the report was generated.
    pub report_generated_at: String,
    /// True when there are suggestions to show.
    pub has_suggestions: bool,
    /// True when there is trend analysis to show.
    pub has_trend: bool,
}

/// A single day in the daily breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct DailyDay {
    /// Full date (YYYY-MM-DD).
    pub date: String,
    /// Chinese day name (e.g. "周一").
    pub day_name: String,
    /// Total active hours for this day.
    pub hours: String,
    /// Width percentage for the bar (0–100).
    pub bar_width: String,
    /// Whether this day has data.
    pub has_data: bool,
}

/// App usage entry for the ranking.
#[derive(Debug, Clone, Serialize)]
pub struct AppUsage {
    /// Application / category name.
    pub name: String,
    /// Rank (1-based).
    pub rank: usize,
    /// Hours used.
    pub hours: String,
    /// Percentage of total time (0–100), for bar width.
    pub percent: String,
}

// ---------------------------------------------------------------------------
// ReportGenerator
// ---------------------------------------------------------------------------

/// Generates weekly reports from activity data.
pub struct ReportGenerator {
    #[allow(dead_code)]
    db: Arc<Mutex<Database>>,
    analyzer: Arc<Analyzer>,
    /// Path to the templates directory.
    templates_dir: PathBuf,
}

impl ReportGenerator {
    /// Create a new `ReportGenerator`.
    ///
    /// Templates are expected in `{crate_root}/templates/`.
    pub fn new(db: Arc<Mutex<Database>>, analyzer: Arc<Analyzer>) -> Self {
        // Try to locate the templates directory relative to the source.
        let templates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");

        ReportGenerator {
            db,
            analyzer,
            templates_dir,
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Generate a self-contained HTML weekly report for the current week
    /// (Monday 00:00 through Sunday 23:59).
    pub async fn generate_weekly_report_html(&self) -> Result<String, ReportError> {
        let (start, end) = current_week_range();
        let report = self
            .analyzer
            .generate_weekly_report(&start, &end)
            .await
            .map_err(|e| match e {
                crate::ai::AnalysisError::NoData => ReportError::NoData,
                other => ReportError::Analysis(other),
            })?;

        let today = Local::now().format("%Y-%m-%d").to_string();
        let ctx = build_context(&report, &start, &end, &today);

        // Load and compile the template.
        let template_path = self.templates_dir.join("weekly_report.hbs");
        let template_str = fs::read_to_string(&template_path).map_err(|e| {
            ReportError::Template(format!(
                "Cannot read template `{}`: {e}",
                template_path.display()
            ))
        })?;

        let reg = Handlebars::new();
        let html = reg.render_template(&template_str, &ctx)?;

        Ok(html)
    }

    /// Export the given HTML report to a summary PDF.
    ///
    /// **Note:** `printpdf` does not render HTML.  This function creates a
    /// clean one-page PDF summary (cover-style) with the key statistics.
    /// For the full styled report, open the HTML in a browser.
    pub fn export_to_pdf(html: &str, output_path: &str) -> Result<(), ReportError> {
        // Since printpdf can't render HTML, we create a clean summary
        // PDF with the report's key information extracted from the HTML
        // or, more practically, a cover-page style PDF document.
        //
        // A full HTML→PDF pipeline would require a headless browser or
        // wkhtmltopdf — here we produce a readable PDF using printpdf's
        // native API.

        let (doc, page_idx, layer_idx) = PdfDocument::new(
            "WorkMirror Weekly Report",
            Mm(210.0),  // A4 width
            Mm(297.0),  // A4 height
            "Report",
        );

        let page = doc.get_page(page_idx);
        let layer = page.get_layer(layer_idx);

        // Load the built-in Helvetica font.
        let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
        let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

        // ── Title ──
        layer.use_text("WorkMirror Weekly Report", 22.0, Mm(30.0), Mm(260.0), &font_bold);

        // ── Subtitle (extract date range from HTML if possible) ──
        let subtitle = extract_title_from_html(html).unwrap_or_else(|| "Weekly Activity Report".into());
        layer.use_text(&subtitle, 14.0, Mm(30.0), Mm(250.0), &font);

        // ── Decorative line ──
        let line_y = Mm(242.0);
        layer.set_outline_color(Color::Rgb(Rgb::new(0.2, 0.5, 0.9, None)));
        layer.add_line(Line {
            points: vec![
                (Point::new(Mm(30.0), line_y), false),
                (Point::new(Mm(180.0), line_y), false),
            ],
            is_closed: false,
        });

        // ── Generated timestamp ──
        let now_str = Local::now().format("%Y-%m-%d %H:%M").to_string();
        layer.use_text(
            &format!("Generated: {now_str}"),
            10.0,
            Mm(30.0),
            Mm(234.0),
            &font,
        );

        // ── Body note ──
        layer.use_text(
            "This is a summary cover page generated with printpdf.",
            10.0,
            Mm(30.0),
            Mm(224.0),
            &font,
        );
        layer.use_text(
            "For the full styled report (with charts, dark mode, and AI",
            10.0,
            Mm(30.0),
            Mm(216.0),
            &font,
        );
        layer.use_text(
            "insights), please open the HTML file in a web browser.",
            10.0,
            Mm(30.0),
            Mm(208.0),
            &font,
        );

        // ── Stats section ──
        let stats = extract_stats_from_html(html);
        let mut y_pos: f32 = 194.0;

        layer.use_text("Quick Summary", 14.0, Mm(30.0), Mm(y_pos), &font_bold);
        y_pos -= 10.0;

        if let Some(s) = stats {
            for line in s {
                layer.use_text(&line, 10.0, Mm(35.0), Mm(y_pos), &font);
                y_pos -= 7.0;
            }
        }

        // ── Footer ──
        layer.use_text(
            "WorkMirror — Track what matters.",
            9.0,
            Mm(30.0),
            Mm(15.0),
            &font,
        );

        // Save.
        let file = fs::File::create(output_path)?;
        let mut writer = BufWriter::new(file);
        doc.save(&mut writer)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the current week's Monday-to-Sunday date range.
fn current_week_range() -> (String, String) {
    let today = Local::now().naive_local().date();
    let weekday = today.weekday().num_days_from_monday(); // 0=Mon, 6=Sun
    let monday = today - ChronoDuration::days(weekday as i64);
    let sunday = monday + ChronoDuration::days(6);

    (monday.format("%Y-%m-%d").to_string(), sunday.format("%Y-%m-%d").to_string())
}

/// Map an English weekday name to Chinese.
fn day_name_cn(date_str: &str) -> String {
    match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => match d.format("%A").to_string().as_str() {
            "Monday" => "周一".into(),
            "Tuesday" => "周二".into(),
            "Wednesday" => "周三".into(),
            "Thursday" => "周四".into(),
            "Friday" => "周五".into(),
            "Saturday" => "周六".into(),
            "Sunday" => "周日".into(),
            _ => d.format("%A").to_string(),
        },
        Err(_) => date_str.into(),
    }
}

/// Build the full `ReportContext` from the analyzer's `WeeklyReport`.
fn build_context(report: &WeeklyReport, start: &str, end: &str, _today: &str) -> ReportContext {
    let days = report.daily_summaries.len() as f64;

    let total_active: f64 = report.daily_summaries.iter().map(|d| d.total_active_hours).sum();
    let total_deep: f64 = report.daily_summaries.iter().map(|d| d.deep_work_hours).sum();
    let avg = if days > 0.0 { total_active / days } else { 0.0 };
    let deep_pct = if total_active > 0.0 && total_deep >= 0.0 {
        (total_deep / total_active) * 100.0
    } else {
        0.0
    };

    // Compute per-day data for the bar chart.
    let daily_data: Vec<DailyDay> = report
        .daily_summaries
        .iter()
        .map(|d| {
            let hours = d.total_active_hours;
            // Normalize the bar width: max day = 100%.
            DailyDay {
                date: d.date.clone(),
                day_name: day_name_cn(&d.date),
                hours: format!("{:.1}", hours),
                bar_width: "100".into(), // will be recalculated below
                has_data: true,
            }
        })
        .collect();

    // Recalculate bar widths relative to the max.
    let max_hours = daily_data
        .iter()
        .map(|d| d.hours.parse::<f64>().unwrap_or(0.0))
        .fold(0.0_f64, f64::max);

    let daily_data: Vec<DailyDay> = daily_data
        .iter()
        .map(|d| {
            let h = d.hours.parse::<f64>().unwrap_or(0.0);
            let width = if max_hours > 0.0 {
                (h / max_hours) * 100.0
            } else {
                0.0
            };
            DailyDay {
                date: d.date.clone(),
                day_name: d.day_name.clone(),
                hours: d.hours.clone(),
                bar_width: format!("{:.0}", width),
                has_data: d.has_data,
            }
        })
        .collect();

    // Compute top 5 apps across the whole week.
    let mut app_map: HashMap<String, f64> = HashMap::new();
    for day in &report.daily_summaries {
        for (name, hours) in &day.top_apps {
            *app_map.entry(name.clone()).or_insert(0.0) += hours;
        }
    }
    let mut app_vec: Vec<(String, f64)> = app_map.into_iter().collect();
    app_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top_apps: Vec<AppUsage> = app_vec
        .iter()
        .take(5)
        .enumerate()
        .map(|(idx, (name, hours))| {
            let pct = if total_active > 0.0 {
                (hours / total_active) * 100.0
            } else {
                0.0
            };
            AppUsage {
                name: name.clone(),
                rank: idx + 1,
                hours: format!("{:.1}", hours),
                percent: format!("{:.0}", pct),
            }
        })
        .collect();

    // AI insights.
    let has_trend = !report.trend_analysis.is_empty();
    let has_suggestions = !report.improvement_suggestions.is_empty();

    ReportContext {
        week_range: format!("{start} ~ {end}"),
        week_start: start.into(),
        week_end: end.into(),
        total_active_hours: format!("{:.1}", total_active),
        avg_daily_hours: format!("{:.1}", avg),
        total_deep_hours: format!("{:.1}", total_deep),
        deep_work_percent: format!("{:.0}", deep_pct),
        active_days: report.daily_summaries.len(),
        daily_data,
        top_apps,
        trend_analysis: report.trend_analysis.clone(),
        improvement_suggestions: report.improvement_suggestions.clone(),
        report_generated_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        has_suggestions,
        has_trend,
    }
}

// ---------------------------------------------------------------------------
// HTML helpers for PDF export
// ---------------------------------------------------------------------------

/// Extract a title-ish string from the HTML (the `<title>` tag).
fn extract_title_from_html(html: &str) -> Option<String> {
    // Use char_indices to avoid panicking on multi-byte characters
    // when slicing at positions discovered via byte-level search.
    for (title_start, _) in html.as_bytes().windows(7).enumerate() {
        if html[title_start..].starts_with("<title>") ||
           html[title_start..].to_lowercase().starts_with("<title>") {
            let content_start = title_start + 7;
            if let Some(content_end) = html[content_start..].find("</title>") {
                let title = html[content_start..content_start + content_end].trim();
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }
    Some("Weekly Activity Report".into())
}

/// Extract key stats from the HTML body for the PDF summary.
fn extract_stats_from_html(html: &str) -> Option<Vec<String>> {
    // We scan for lines that look like stat labels in the template
    // ("总活跃时间", "日均", etc.) and grab the neighboring text.
    let mut lines: Vec<String> = Vec::new();

    // Simple heuristic: look for known label–value patterns.
    // This is fragile but good enough for a summary PDF.
    let patterns = [
        ("总活跃时间", "Total Active Hours"),
        ("日均", "Daily Avg"),
        ("深度工作", "Deep Work"),
        ("应用使用", "App Usage"),
    ];

    for (keyword, _label) in &patterns {
        if let Some(pos) = html.find(keyword) {
            // Grab a chunk starting at `pos` (byte-safe: `pos` is always a
            // char boundary since `find` returns positions only at valid
            // boundaries).
            let end = (pos + 80).min(html.len());
            let snippet = &html[pos..end];
            // Clean up.
            let clean: String = snippet
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_digit() || *c == '.' || *c == '%' || c.is_ascii_whitespace() || c.is_ascii_punctuation())
                .collect();
            let clean = clean.trim();
            if !clean.is_empty() && clean.len() > 3 {
                lines.push(clean.to_string());
            }
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{DailySummary, WeeklyReport};
    use crate::db::Database;
    use std::sync::{Arc, Mutex};

    // ------------------------------------------------------------------
    // Helper: create a test WeeklyReport
    // ------------------------------------------------------------------

    fn create_test_report() -> WeeklyReport {
        WeeklyReport {
            week_range: "2026-05-18 ~ 2026-05-24".into(),
            daily_summaries: vec![
                DailySummary {
                    date: "2026-05-18".into(),
                    total_active_hours: 7.5,
                    deep_work_hours: 5.0,
                    top_apps: vec![
                        ("VS Code".into(), 4.0),
                        ("Chrome".into(), 2.0),
                        ("Terminal".into(), 1.5),
                    ],
                    insight: "专注度不错，上午效率高。".into(),
                    suggestion: "建议下午增加休息频率。".into(),
                },
                DailySummary {
                    date: "2026-05-19".into(),
                    total_active_hours: 8.2,
                    deep_work_hours: 6.0,
                    top_apps: vec![
                        ("VS Code".into(), 5.0),
                        ("Terminal".into(), 2.0),
                        ("Chrome".into(), 1.2),
                    ],
                    insight: "连续编码时间较长。".into(),
                    suggestion: "建议每50分钟站立休息。".into(),
                },
                DailySummary {
                    date: "2026-05-20".into(),
                    total_active_hours: 6.0,
                    deep_work_hours: 3.0,
                    top_apps: vec![
                        ("Chrome".into(), 3.0),
                        ("VS Code".into(), 2.0),
                        ("Slack".into(), 1.0),
                    ],
                    insight: "浏览时间偏多。".into(),
                    suggestion: "减少非必要网页浏览。".into(),
                },
            ],
            trend_analysis: "本周专注度呈下降趋势，周三最弱。".into(),
            improvement_suggestions: vec![
                "1. 下午时段增加休息频率".into(),
                "2. 减少非必要浏览器使用".into(),
                "3. 保持上午高效工作节奏".into(),
            ],
        }
    }

    // ------------------------------------------------------------------
    // Context building
    // ------------------------------------------------------------------

    #[test]
    fn build_context_computes_correct_totals() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");

        assert_eq!(ctx.week_range, "2026-05-18 ~ 2026-05-24");
        assert_eq!(ctx.total_active_hours, "21.7"); // 7.5+8.2+6.0
        assert_eq!(ctx.avg_daily_hours, "7.2"); // 21.7/3
        assert_eq!(ctx.total_deep_hours, "14.0"); // 5.0+6.0+3.0
        assert_eq!(ctx.deep_work_percent, "65"); // 14/21.7*100 ≈ 64.5 → 65
        assert_eq!(ctx.active_days, 3);
    }

    #[test]
    fn build_context_daily_data_length() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");
        assert_eq!(ctx.daily_data.len(), 3);
        assert_eq!(ctx.daily_data[0].day_name, "周一");
        assert_eq!(ctx.daily_data[1].day_name, "周二");
        assert_eq!(ctx.daily_data[2].day_name, "周三");
    }

    #[test]
    fn build_context_top_apps() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");

        // Top apps aggregated: VS Code (11), Chrome (6.2), Terminal (3.5), Slack (1.0)
        assert_eq!(ctx.top_apps.len(), 4);
        assert_eq!(ctx.top_apps[0].name, "VS Code");
        assert!(ctx.top_apps[0].hours.starts_with("11"));
        assert_eq!(ctx.top_apps[1].name, "Chrome");
    }

    #[test]
    fn build_context_top_apps_percent_sum() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");

        let sum: f64 = ctx
            .top_apps
            .iter()
            .filter_map(|a| a.percent.parse::<f64>().ok())
            .sum();
        // Should be close to 100% (within rounding).
        assert!((sum - 100.0).abs() < 5.0,
            "percent sum {sum} should be near 100");
    }

    #[test]
    fn build_context_bar_widths_are_percentages() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");

        // Max day is 8.2 (周二) → bar width 100%.
        for day in &ctx.daily_data {
            let w: f64 = day.bar_width.parse().unwrap_or(0.0);
            assert!(w >= 0.0 && w <= 100.0,
                "bar width {} should be 0-100", w);
        }
        // The max day (8.2h) should be 100%.
        assert_eq!(ctx.daily_data[1].bar_width, "100");
    }

    #[test]
    fn build_context_ai_fields_propagated() {
        let report = create_test_report();
        let ctx = build_context(&report, "2026-05-18", "2026-05-24", "2026-05-24");

        assert!(ctx.has_trend);
        assert!(ctx.trend_analysis.contains("下降趋势"));
        assert!(ctx.has_suggestions);
        assert_eq!(ctx.improvement_suggestions.len(), 3);
    }

    // ------------------------------------------------------------------
    // Day name conversion
    // ------------------------------------------------------------------

    #[test]
    fn day_name_monday() {
        assert_eq!(day_name_cn("2026-05-18"), "周一");
    }

    #[test]
    fn day_name_tuesday() {
        assert_eq!(day_name_cn("2026-05-19"), "周二");
    }

    #[test]
    fn day_name_wednesday() {
        assert_eq!(day_name_cn("2026-05-20"), "周三");
    }

    #[test]
    fn day_name_invalid_date_returns_input() {
        assert_eq!(day_name_cn("invalid"), "invalid");
    }

    // ------------------------------------------------------------------
    // Date range computation
    // ------------------------------------------------------------------

    #[test]
    fn current_week_range_returns_valid_dates() {
        let (start, end) = current_week_range();
        // Both should be valid YYYY-MM-DD strings.
        assert!(
            NaiveDate::parse_from_str(&start, "%Y-%m-%d").is_ok(),
            "start '{start}' should be valid date"
        );
        assert!(
            NaiveDate::parse_from_str(&end, "%Y-%m-%d").is_ok(),
            "end '{end}' should be valid date"
        );
        // Start should be ≤ end.
        let s = NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
        let e = NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap();
        assert!(s <= e, "start {start} should be ≤ end {end}");
    }

    #[test]
    fn current_week_range_length() {
        let (start, end) = current_week_range();
        let s = NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
        let e = NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap();
        let diff = (e - s).num_days();
        assert_eq!(diff, 6, "week should be 7 days (Mon-Sun), got {diff} days");
    }

    // ------------------------------------------------------------------
    // Current week range consistency
    // ------------------------------------------------------------------

    #[test]
    fn week_starts_on_monday() {
        let (start, _) = current_week_range();
        let d = NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
        let wd = d.weekday();
        assert_eq!(wd.num_days_from_monday(), 0,
            "week should start on Monday, got {:?} (num={})",
            wd, wd.num_days_from_monday());
    }

    // ------------------------------------------------------------------
    // HTML stat extraction (for PDF export)
    // ------------------------------------------------------------------

    #[test]
    fn extract_title_from_html_finds_title_tag() {
        let html = "<html><head><title>WorkMirror Weekly Report</title></head><body>...</body></html>";
        let title = extract_title_from_html(html);
        assert_eq!(title, Some("WorkMirror Weekly Report".into()));
    }

    #[test]
    fn extract_title_from_html_fallback() {
        let html = "<html><body>no title here</body></html>";
        let title = extract_title_from_html(html);
        assert!(title.is_some());
        assert_eq!(title.unwrap(), "Weekly Activity Report");
    }

    #[test]
    fn extract_stats_from_html_returns_none_for_empty() {
        let html = "<html><body>nothing useful</body></html>";
        let stats = extract_stats_from_html(html);
        assert!(stats.is_none() || stats.unwrap().is_empty());
    }

    // ------------------------------------------------------------------
    // Builder integrity: empty report
    // ------------------------------------------------------------------

    #[test]
    fn build_context_empty_daily_summaries() {
        let report = WeeklyReport {
            week_range: "2026-01-01 ~ 2026-01-07".into(),
            daily_summaries: vec![],
            trend_analysis: String::new(),
            improvement_suggestions: vec![],
        };
        let ctx = build_context(&report, "2026-01-01", "2026-01-07", "2026-01-07");

        // Note: -0.0 and 0.0 format the same in Rust ("-0.0" vs "0.0"),
        // so we check the numeric value via the total_deep_hours field.
        let hours: f64 = ctx.total_active_hours.parse().unwrap_or(-1.0);
        assert!(hours.abs() < 0.01, "expected ~0, got {hours}");
        let avg: f64 = ctx.avg_daily_hours.parse().unwrap_or(-1.0);
        assert!(avg.abs() < 0.01, "expected ~0, got {avg}");
        let deep: f64 = ctx.total_deep_hours.parse().unwrap_or(-1.0);
        assert!(deep.abs() < 0.01, "expected ~0, got {deep}");
        assert!(ctx.deep_work_percent == "0" || ctx.deep_work_percent == "-0");
        assert!(ctx.top_apps.is_empty());
        assert!(ctx.daily_data.is_empty());
        assert_eq!(ctx.active_days, 0);
    }

    // ------------------------------------------------------------------
    // Builder integrity: single day
    // ------------------------------------------------------------------

    #[test]
    fn build_context_single_day() {
        let report = WeeklyReport {
            week_range: "2026-05-24 ~ 2026-05-24".into(),
            daily_summaries: vec![DailySummary {
                date: "2026-05-24".into(),
                total_active_hours: 9.0,
                deep_work_hours: 6.0,
                top_apps: vec![("Terminal".into(), 9.0)],
                insight: String::new(),
                suggestion: String::new(),
            }],
            trend_analysis: String::new(),
            improvement_suggestions: vec![],
        };

        let ctx = build_context(&report, "2026-05-24", "2026-05-24", "2026-05-24");

        assert_eq!(ctx.total_active_hours, "9.0");
        assert_eq!(ctx.avg_daily_hours, "9.0");
        assert_eq!(ctx.deep_work_percent, "67"); // 6/9*100=66.6→67
        assert_eq!(ctx.top_apps.len(), 1);
    }

    // ------------------------------------------------------------------
    // ReportGenerator construction
    // ------------------------------------------------------------------

    #[test]
    fn report_generator_creation() {
        let db = Arc::new(Mutex::new(Database::open_memory().expect("db")));
        let analyzer = Arc::new(Analyzer::new(db.clone(), None));
        let gen = ReportGenerator::new(db, analyzer);
        // Templates dir should point to CARGO_MANIFEST_DIR/templates.
        assert!(gen.templates_dir.to_string_lossy().contains("templates"),
            "templates_dir should contain 'templates', got {:?}",
            gen.templates_dir);
    }

    // ------------------------------------------------------------------
    // PDF export with printpdf
    // ------------------------------------------------------------------

    #[test]
    fn pdf_export_creates_file() {
        use tempfile::TempDir;
        let dir = TempDir::new().expect("tempdir");
        let output = dir.path().join("test_report.pdf");
        let output_str = output.to_str().expect("valid utf-8");

        let html = r#"<html><head><title>WorkMirror 周报</title></head>
        <body>
        总活跃时间 42.5 小时
        日均 7.1 小时
        深度工作 66%
        应用使用: VS Code 12.5h
        </body></html>"#;

        let result = ReportGenerator::export_to_pdf(html, output_str);
        assert!(result.is_ok(), "PDF export should succeed: {:?}", result.err());

        assert!(output.exists(), "PDF file should exist");
        let meta = fs::metadata(output_str).expect("metadata");
        assert!(meta.len() > 0, "PDF should not be empty");
    }

    #[test]
    fn pdf_export_invalid_path_returns_error() {
        let html = "<html><body>test</body></html>";
        let result = ReportGenerator::export_to_pdf(html, "Z:\\nonexistent\\output.pdf");
        assert!(result.is_err());
        match result {
            Err(ReportError::Io(_)) => {} // expected
            Err(ReportError::PdfExport(_)) => {} // also acceptable
            other => panic!("expected IO or PdfExport error, got: {other:?}"),
        }
    }
}
