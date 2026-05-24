// WorkMirror — Consecutive active-day streak tracker.
//
// Tracks how many consecutive days the user has been active (has at
// least one activity record).  A "streak" is counted from today backward:
//
// - If today has activity → streak = 1 + count of consecutive previous
//   days that also have activity.
// - If today has no activity but yesterday does → streak is preserved
//   (the user simply hasn't started yet today).
// - If both today and yesterday lack activity → streak resets to 0.

use crate::db::{Database, DbError};
use chrono::NaiveDate;

/// Pure-logic streak tracker.
///
/// All methods are synchronous and operate on the shared database.
pub struct StreakTracker;

impl StreakTracker {
    /// Return the current streak (consecutive active days ending at the
    /// latest available date).
    ///
    /// Behaviour:
    /// - If today has records → count backwards from today.
    /// - If today has none but yesterday does → preserve the streak.
    /// - If neither today nor yesterday has records → return 0.
    pub fn get_streak(db: &Database) -> Result<u32, DbError> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let today_has_data = Self::date_has_data(db, &today)?;

        if today_has_data {
            // Today counts as 1, then count consecutive previous days
            // starting from yesterday.
            let yesterday = Self::days_ago(1);
            Ok(Self::count_consecutive(db, &yesterday)? + 1)
        } else {
            // Check yesterday.
            let yesterday = Self::days_ago(1);
            let yesterday_has_data = Self::date_has_data(db, &yesterday)?;
            if yesterday_has_data {
                // Preserve the streak (user just hasn't started today yet).
                Self::count_consecutive(db, &yesterday)
            } else {
                // No activity today or yesterday → reset.
                Ok(0)
            }
        }
    }

    /// Update the streak based on today's data.  This is a logical
    /// no-op because the streak is computed dynamically from the
    /// activities table — there is no persistent streak counter.
    ///
    /// This method exists for API consistency and future persistence.
    pub fn update_streak(_db: &Database) -> Result<(), DbError> {
        // Currently a no-op: streak is always derived from raw activity
        // data.  In the future we could cache the value in the config
        // table for performance.
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Check whether the given date (YYYY-MM-DD) has at least one
    /// activity record.
    fn date_has_data(db: &Database, date: &str) -> Result<bool, DbError> {
        let activities = db.get_activities_by_date(date)?;
        Ok(!activities.is_empty())
    }

    /// Count consecutive active days going backwards from `start_date`
    /// (inclusive).
    ///
    /// Stops at the first day with no activity records.
    fn count_consecutive(db: &Database, start_date: &str) -> Result<u32, DbError> {
        let mut count: u32 = 0;

        // Parse the starting date.
        let mut current = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Local::now().date_naive());

        loop {
            let date_str = current.format("%Y-%m-%d").to_string();
            if Self::date_has_data(db, &date_str)? {
                count += 1;
                current -= chrono::Duration::days(1);
            } else {
                break;
            }
        }

        Ok(count)
    }

    /// Return the date `n` days ago as `YYYY-MM-DD`.
    fn days_ago(n: i64) -> String {
        (chrono::Local::now() - chrono::Duration::days(n))
            .format("%Y-%m-%d")
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Activity, Database};

    fn insert_activity_on_date(db: &Database, date: &str) {
        let act = Activity {
            id: None,
            timestamp: format!("{date}T12:00:00"),
            window_title: "test window".into(),
            process_name: "test.exe".into(),
            duration_seconds: 60,
            category: "test".into(),
        };
        db.insert_activity(&act).expect("insert activity");
    }

    #[test]
    fn empty_db_returns_zero() {
        let db = Database::open_memory().expect("in-memory db");
        let streak = StreakTracker::get_streak(&db).expect("get_streak");
        // No data at all — streak should be 0.
        assert_eq!(streak, 0);
    }

    #[test]
    fn only_today_returns_one() {
        let db = Database::open_memory().expect("in-memory db");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        insert_activity_on_date(&db, &today);

        let streak = StreakTracker::get_streak(&db).expect("get_streak");
        assert_eq!(streak, 1, "expected 1, got {streak}");
    }

    #[test]
    fn two_consecutive_days() {
        let db = Database::open_memory().expect("in-memory db");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        insert_activity_on_date(&db, &today);
        insert_activity_on_date(&db, &yesterday);

        let streak = StreakTracker::get_streak(&db).expect("get_streak");
        assert!(streak >= 2, "expected at least 2, got {streak}");
    }

    #[test]
    fn gap_resets_streak() {
        let db = Database::open_memory().expect("in-memory db");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let three_days_ago = (chrono::Local::now() - chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();

        insert_activity_on_date(&db, &today);
        insert_activity_on_date(&db, &three_days_ago);

        // Gap: 3 days ago has data, but day before yesterday and yesterday don't.
        let streak = StreakTracker::get_streak(&db).expect("get_streak");
        // The gap should break the streak at yesterday → only today counts.
        assert_eq!(streak, 1, "expected 1 (today only), got {streak}");
    }

    #[test]
    fn update_streak_is_noop() {
        let db = Database::open_memory().expect("in-memory db");
        // Should succeed without panicking.
        StreakTracker::update_streak(&db).expect("update_streak");
    }

    #[test]
    fn date_has_data_true() {
        let db = Database::open_memory().expect("in-memory db");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        insert_activity_on_date(&db, &today);
        assert!(StreakTracker::date_has_data(&db, &today).expect("check"));
    }

    #[test]
    fn date_has_data_false() {
        let db = Database::open_memory().expect("in-memory db");
        assert!(
            !StreakTracker::date_has_data(&db, "2099-01-01")
                .expect("check")
        );
    }

    #[test]
    fn count_consecutive_from_yesterday() {
        let db = Database::open_memory().expect("in-memory db");
        let _today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let day_before = (chrono::Local::now() - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();

        insert_activity_on_date(&db, &yesterday);
        insert_activity_on_date(&db, &day_before);

        let count = StreakTracker::count_consecutive(&db, &yesterday)
            .expect("count");
        assert_eq!(count, 2, "expected 2 consecutive days, got {count}");
    }

    #[test]
    fn get_streak_preserves_when_today_empty_and_yesterday_has_data() {
        let db = Database::open_memory().expect("in-memory db");
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let day_before = (chrono::Local::now() - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();

        // Only yesterday and the day before have data, not today.
        insert_activity_on_date(&db, &yesterday);
        insert_activity_on_date(&db, &day_before);

        let streak = StreakTracker::get_streak(&db).expect("get_streak");
        // Yesterday and day before are consecutive → streak = 2.
        assert_eq!(streak, 2, "expected 2, got {streak}");
    }
}
