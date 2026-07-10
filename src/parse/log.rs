//! Parse the pinned `git log --pretty=format` output into [`Commit`]s.
//!
//! Format contract (see `CLAUDE.md`):
//! `%H<US>%h<US>%ct<US>%an<US>%s<US>%D<RS>`
//! where `<US>` = 0x1f (field separator) and `<RS>` = 0x1e (record separator).

use crate::domain::{Commit, time::relative_time};
use crate::parse::decorations::parse_refs;

/// Field separator (ASCII Unit Separator).
pub const FIELD_SEP: char = '\u{1f}';
/// Record separator (ASCII Record Separator).
pub const RECORD_SEP: char = '\u{1e}';

/// The `--pretty=format` string to pass to `git log` so output matches [`parse_log`].
pub const PRETTY_FORMAT: &str = "%H\u{1f}%h\u{1f}%ct\u{1f}%an\u{1f}%s\u{1f}%D\u{1e}";

/// Parse raw `git log` output into commits, computing each relative time against `now`.
///
/// Malformed records (wrong field count, unparseable timestamp) are skipped rather than aborting the
/// whole log — a single odd line should never blank the UI.
pub fn parse_log(raw: &str, now: i64) -> Vec<Commit> {
    raw.split(RECORD_SEP)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| parse_record(record, now))
        .collect()
}

fn parse_record(record: &str, now: i64) -> Option<Commit> {
    let mut fields = record.split(FIELD_SEP);
    let hash = fields.next()?.trim().to_string();
    let short = fields.next()?.trim().to_string();
    let timestamp: i64 = fields.next()?.trim().parse().ok()?;
    let author = fields.next()?.to_string();
    let subject = fields.next()?.to_string();
    let decorations = fields.next().unwrap_or("");

    if hash.is_empty() {
        return None;
    }

    let refs = parse_refs(decorations);
    let relative = relative_time(now, timestamp);
    let haystack = build_haystack(&short, &author, &subject, &refs);

    Some(Commit {
        hash,
        short,
        timestamp,
        author,
        subject,
        relative,
        refs,
        haystack,
    })
}

/// The searchable text for a commit: everything a user might reasonably type to find it.
fn build_haystack(short: &str, author: &str, subject: &str, refs: &[crate::domain::Ref]) -> String {
    let mut s = String::with_capacity(short.len() + author.len() + subject.len() + 16);
    s.push_str(short);
    s.push(' ');
    s.push_str(author);
    s.push(' ');
    s.push_str(subject);
    for r in refs {
        s.push(' ');
        s.push_str(r.label());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Ref;

    /// Build one raw record from fields (test helper mirroring the git format).
    fn record(hash: &str, short: &str, ct: i64, an: &str, subject: &str, decos: &str) -> String {
        format!("{hash}\u{1f}{short}\u{1f}{ct}\u{1f}{an}\u{1f}{subject}\u{1f}{decos}\u{1e}")
    }

    // LOG-01 / LOG-03: parse a well-formed log into commits with typed refs.
    #[test]
    fn log_01_parses_fields() {
        let now = 1_000_000;
        let raw = record(
            "a".repeat(40).as_str(),
            "aaaaaaa",
            now - 3 * 86400,
            "Ada Lovelace",
            "Add the analytical engine (#42)",
            "HEAD -> main, origin/main",
        );
        let commits = parse_log(&raw, now);
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert_eq!(c.short, "aaaaaaa");
        assert_eq!(c.author, "Ada Lovelace");
        assert_eq!(c.subject, "Add the analytical engine (#42)");
        assert_eq!(c.relative, "3 days ago");
        assert_eq!(
            c.refs,
            vec![
                Ref::Head,
                Ref::Local("main".into()),
                Ref::Remote("origin/main".into())
            ]
        );
        assert!(c.haystack.contains("Ada Lovelace"));
        assert!(c.haystack.contains("analytical"));
    }

    #[test]
    fn log_01_parses_multiple_records() {
        let now = 500;
        let raw = format!(
            "{}{}",
            record(&"1".repeat(40), "1111111", 500, "A", "first", ""),
            record(&"2".repeat(40), "2222222", 400, "B", "second", ""),
        );
        let commits = parse_log(&raw, now);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "first");
        assert_eq!(commits[1].subject, "second");
    }

    #[test]
    fn log_01_skips_malformed_records() {
        let now = 100;
        // Second "record" has too few fields and a bad timestamp; it must be skipped, not panic.
        let raw = format!(
            "{}{}",
            record(&"1".repeat(40), "1111111", 100, "A", "ok", ""),
            "garbage\u{1f}line\u{1e}",
        );
        let commits = parse_log(&raw, now);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "ok");
    }

    #[test]
    fn log_01_empty_input_is_empty() {
        assert_eq!(parse_log("", 0).len(), 0);
        assert_eq!(parse_log("   \n  ", 0).len(), 0);
    }
}
