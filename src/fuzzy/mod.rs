//! In-process fuzzy filtering over commits, powered by nucleo's matcher.
//!
//! This is intentionally **synchronous and pure**: `filter` takes the commits and a query and
//! returns the ranked matches. For a bounded log (see the `--max-count` cap in the CLI) matching a
//! few thousand short strings is sub-millisecond, so we don't need nucleo's async worker threads —
//! and staying synchronous keeps the reducer fully deterministic and unit-testable.

use crate::domain::Commit;
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};

/// One commit that matched the current query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchEntry {
    /// Index into the source `commits` slice.
    pub commit_idx: usize,
    /// Match score (higher is better); `0` when the query is empty.
    pub score: u32,
    /// Sorted, de-duplicated char positions in the commit's haystack that matched — for highlight.
    pub positions: Vec<u32>,
}

/// Rank `commits` against `query`.
///
/// An empty query returns every commit in original (reverse-chronological) order with no
/// highlights. Otherwise only matching commits are returned, ordered by descending score with
/// original order as a stable tie-breaker (so equally-scored commits stay newest-first).
pub fn filter(commits: &[Commit], query: &str) -> Vec<MatchEntry> {
    if query.trim().is_empty() {
        return commits
            .iter()
            .enumerate()
            .map(|(commit_idx, _)| MatchEntry {
                commit_idx,
                score: 0,
                positions: Vec::new(),
            })
            .collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

    let mut char_buf = Vec::new();
    let mut positions = Vec::new();

    let mut matches: Vec<MatchEntry> = commits
        .iter()
        .enumerate()
        .filter_map(|(commit_idx, commit)| {
            positions.clear();
            let haystack = Utf32Str::new(&commit.haystack, &mut char_buf);
            let score = pattern.indices(haystack, &mut matcher, &mut positions)?;
            positions.sort_unstable();
            positions.dedup();
            Some(MatchEntry {
                commit_idx,
                score,
                positions: positions.clone(),
            })
        })
        .collect();

    // Descending score; stable sort keeps original order for ties.
    matches.sort_by_key(|e| std::cmp::Reverse(e.score));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Ref;

    fn commit(short: &str, author: &str, subject: &str) -> Commit {
        let refs: Vec<Ref> = Vec::new();
        Commit {
            hash: format!("{short}{}", "0".repeat(40 - short.len())),
            short: short.to_string(),
            timestamp: 0,
            author: author.to_string(),
            subject: subject.to_string(),
            relative: "now".into(),
            refs: refs.clone(),
            haystack: format!("{short} {author} {subject}"),
        }
    }

    fn corpus() -> Vec<Commit> {
        vec![
            commit("aaaaaaa", "Ada", "Add fuzzy search to the log"),
            commit("bbbbbbb", "Bo", "Fix flaky test"),
            commit("ccccccc", "Cy", "Refactor the parser"),
        ]
    }

    // LOG-05: empty query returns all, in original order.
    #[test]
    fn log_05_empty_query_returns_all_in_order() {
        let c = corpus();
        let m = filter(&c, "");
        assert_eq!(m.len(), 3);
        assert_eq!(
            m.iter().map(|e| e.commit_idx).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    // LOG-05: only matching commits are returned.
    #[test]
    fn log_05_filters_non_matching() {
        let c = corpus();
        let m = filter(&c, "parser");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].commit_idx, 2);
        assert!(!m[0].positions.is_empty());
    }

    // LOG-05: results ordered by score — a contiguous match outranks a scattered one.
    #[test]
    fn log_05_orders_by_score() {
        let c = vec![
            commit("1111111", "z", "fix"),          // contiguous "fix"
            commit("2222222", "z", "far ice xray"), // f..i..x scattered across words
        ];
        let m = filter(&c, "fix");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].commit_idx, 0, "contiguous match should rank first");
        assert!(m[0].score > m[1].score);
    }

    // LOG-05: smart-case — lowercase query is case-insensitive.
    #[test]
    fn log_05_smart_case_insensitive_when_lowercase() {
        let c = vec![commit("1111111", "Ada", "Add Feature")];
        assert_eq!(filter(&c, "feature").len(), 1);
    }

    // LOG-05: smart-case — an uppercase letter makes matching case-sensitive.
    #[test]
    fn log_05_smart_case_sensitive_when_uppercase() {
        let c = vec![commit("1111111", "Ada", "add feature")];
        // Query has uppercase F, subject has lowercase f -> no match.
        assert_eq!(filter(&c, "Feature").len(), 0);
    }
}
