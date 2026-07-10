//! Relative-time formatting, faithful to git's `show_date_relative` (date.c).
//!
//! Pure: takes `now` and `then` (unix seconds) so it is fully deterministic in tests.

/// Format `then` relative to `now`, matching `git log`'s `%cr` output.
///
/// Examples: "just now" is not used by git; the smallest bucket is "N seconds ago".
pub fn relative_time(now: i64, then: i64) -> String {
    if now < then {
        return "in the future".to_string();
    }
    let diff = now - then;

    // Seconds.
    if diff < 90 {
        return unit(diff, "second");
    }
    // Minutes.
    let diff = (diff + 30) / 60;
    if diff < 90 {
        return unit(diff, "minute");
    }
    // Hours.
    let diff = (diff + 30) / 60;
    if diff < 36 {
        return unit(diff, "hour");
    }
    // Days.
    let diff = (diff + 12) / 24;
    if diff < 14 {
        return unit(diff, "day");
    }
    // Weeks (up to ~10).
    if diff < 70 {
        return unit((diff + 3) / 7, "week");
    }
    // Months (up to ~12).
    if diff < 365 {
        return unit((diff + 15) / 30, "month");
    }
    // Years and months (up to ~5 years).
    if diff < 1825 {
        let total_months = (diff * 12 * 2 + 365) / (365 * 2);
        let years = total_months / 12;
        let months = total_months % 12;
        if months > 0 {
            return format!("{}, {} ago", count(years, "year"), count(months, "month"));
        }
        return unit(years, "year");
    }
    // Plain years.
    unit((diff + 183) / 365, "year")
}

/// "1 second ago" / "5 seconds ago".
fn unit(n: i64, noun: &str) -> String {
    format!("{} ago", count(n, noun))
}

/// "1 year" / "5 years" (no "ago" suffix — used to compose "years, months").
fn count(n: i64, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;

    // LOG-02: relative dates computed from now/then.
    #[test]
    fn log_02_seconds_and_singular_plural() {
        assert_eq!(relative_time(1000, 1000), "0 seconds ago");
        assert_eq!(relative_time(1001, 1000), "1 second ago");
        assert_eq!(relative_time(1010, 1000), "10 seconds ago");
    }

    #[test]
    fn log_02_minutes() {
        // 89 seconds still counts as seconds; at 90 it rounds to minutes.
        assert_eq!(relative_time(89, 0), "89 seconds ago");
        assert_eq!(relative_time(90, 0), "2 minutes ago"); // (90+30)/60 = 2
        assert_eq!(relative_time(5 * MIN, 0), "5 minutes ago");
    }

    #[test]
    fn log_02_hours() {
        // Faithful to git: hours only appear at >= 90 minutes, so 1 hour is still "60 minutes ago"
        // and the smallest hour bucket is "2 hours ago".
        assert_eq!(relative_time(HOUR, 0), "60 minutes ago");
        assert_eq!(relative_time(90 * MIN, 0), "2 hours ago");
        assert_eq!(relative_time(2 * HOUR, 0), "2 hours ago");
    }

    #[test]
    fn log_02_days_weeks_months() {
        assert_eq!(relative_time(3 * DAY, 0), "3 days ago");
        assert_eq!(relative_time(3 * 7 * DAY, 0), "3 weeks ago");
        assert_eq!(relative_time(90 * DAY, 0), "3 months ago");
    }

    #[test]
    fn log_02_years_and_months() {
        // ~2 years, 6 months.
        let t = 912 * DAY;
        assert_eq!(relative_time(t, 0), "2 years, 6 months ago");
    }

    #[test]
    fn log_02_plain_years() {
        assert_eq!(relative_time(6 * 365 * DAY, 0), "6 years ago");
    }

    #[test]
    fn log_02_future() {
        assert_eq!(relative_time(0, 100), "in the future");
    }
}
