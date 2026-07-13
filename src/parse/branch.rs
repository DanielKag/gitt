//! Parse the pinned `git for-each-ref --format` output into [`Branch`]es.
//!
//! Format contract (analogous to the log's `--pretty=format`):
//! `%(HEAD)<US>%(refname:short)<US>%(objectname)<US>%(upstream:short)<US>%(committerdate:unix)<US>%(contents:subject)<RS>`
//! where `<US>` = 0x1f (field separator) and `<RS>` = 0x1e (record separator). The `%(HEAD)` field is
//! `*` for the current branch and a space otherwise.

use crate::domain::Branch;
use crate::domain::time::relative_time;
use crate::parse::log::{FIELD_SEP, RECORD_SEP};

/// The `--format` string to pass to `git for-each-ref` so its output matches [`parse_branches`].
/// Literal `US`/`RS` control bytes are embedded directly (for-each-ref passes them through verbatim).
pub const BRANCH_FORMAT: &str = "%(HEAD)\u{1f}%(refname:short)\u{1f}%(objectname)\u{1f}%(upstream:short)\u{1f}%(committerdate:unix)\u{1f}%(contents:subject)\u{1e}";

/// Parse raw `git for-each-ref` output into branches, computing each relative time against `now`.
///
/// Malformed records (wrong field count, unparseable timestamp, empty name) are skipped rather than
/// aborting the whole list — one odd line should never blank the UI.
pub fn parse_branches(raw: &str, now: i64) -> Vec<Branch> {
    raw.split(RECORD_SEP)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| parse_record(record, now))
        .collect()
}

fn parse_record(record: &str, now: i64) -> Option<Branch> {
    let mut fields = record.split(FIELD_SEP);
    // The HEAD marker is `*` for the current branch; trimming the record leaves it as `*` or "".
    let is_current = fields.next()?.trim() == "*";
    let name = fields.next()?.trim().to_string();
    let tip = fields.next()?.trim().to_string();
    let upstream = fields.next()?.trim();
    let timestamp: i64 = fields.next()?.trim().parse().ok()?;
    let subject = fields.next().unwrap_or("").to_string();

    if name.is_empty() {
        return None;
    }

    let upstream = (!upstream.is_empty()).then(|| upstream.to_string());
    let relative = relative_time(now, timestamp);
    let haystack = build_haystack(&name, upstream.as_deref(), &subject);

    Some(Branch {
        name,
        is_current,
        tip,
        upstream,
        timestamp,
        subject,
        relative,
        haystack,
    })
}

/// The searchable text for a branch: everything a user might reasonably type to find it.
fn build_haystack(name: &str, upstream: Option<&str>, subject: &str) -> String {
    let mut s = String::with_capacity(name.len() + subject.len() + 16);
    s.push_str(name);
    if let Some(up) = upstream {
        s.push(' ');
        s.push_str(up);
    }
    s.push(' ');
    s.push_str(subject);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one raw record from fields (test helper mirroring the for-each-ref format).
    fn record(head: &str, name: &str, tip: &str, up: &str, ct: i64, subject: &str) -> String {
        format!("{head}\u{1f}{name}\u{1f}{tip}\u{1f}{up}\u{1f}{ct}\u{1f}{subject}\u{1e}")
    }

    // BR-02: parse well-formed records into branches with the current flag, upstream, and tip data.
    #[test]
    fn br_02_parses_fields() {
        let now = 1_000_000;
        let raw = format!(
            "{}\n{}",
            record(
                "*",
                "feature",
                &"a".repeat(40),
                "origin/feature",
                now - 3 * 86400,
                "add the widget",
            ),
            record(
                " ",
                "main",
                &"b".repeat(40),
                "origin/main",
                now - 20 * 86400,
                "base"
            ),
        );
        let branches = parse_branches(&raw, now);
        assert_eq!(branches.len(), 2);

        let f = &branches[0];
        assert_eq!(f.name, "feature");
        assert!(f.is_current);
        assert_eq!(f.tip, "a".repeat(40));
        assert_eq!(f.upstream.as_deref(), Some("origin/feature"));
        assert_eq!(f.subject, "add the widget");
        assert_eq!(f.relative, "3 days ago");
        assert!(f.haystack.contains("feature") && f.haystack.contains("widget"));

        let m = &branches[1];
        assert_eq!(m.name, "main");
        assert!(!m.is_current, "the space-marked branch is not current");
    }

    // BR-02: a branch with no upstream parses with `upstream: None`.
    #[test]
    fn br_02_no_upstream_is_none() {
        let now = 500;
        let raw = record(" ", "wip", &"c".repeat(40), "", 400, "scratch");
        let branches = parse_branches(&raw, now);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].upstream, None);
    }

    #[test]
    fn br_02_skips_malformed_and_empty() {
        assert_eq!(parse_branches("", 0).len(), 0);
        assert_eq!(parse_branches("   \n ", 0).len(), 0);
        // Too few fields / bad timestamp → skipped, not a panic.
        let raw = format!(
            "{}{}",
            record(" ", "ok", &"d".repeat(40), "", 100, "fine"),
            "garbage\u{1f}line\u{1e}",
        );
        let branches = parse_branches(&raw, 100);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "ok");
    }
}
