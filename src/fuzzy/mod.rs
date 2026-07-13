//! In-process **exact** filtering over commits.
//!
//! Matching is `fzf`-style *exact* (not fuzzy subsequence): the query is split on whitespace into
//! terms, and a commit matches only when **every** term appears as a literal substring of its
//! searchable text (smart-case). So `dan kag` matches `daniel kagan` but `daniel nagn` (no `kag`)
//! never does, and `dankag` — a single term with no space — matches neither.
//!
//! This is intentionally **synchronous and pure**: `filter` takes the commits and a query and
//! returns the matches (in original reverse-chronological order). For a bounded log matching a few
//! thousand short strings is sub-millisecond, so no worker threads are needed — and staying
//! synchronous keeps the reducer fully deterministic and unit-testable.

use crate::domain::Commit;

/// One commit that matched the current query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchEntry {
    /// Index into the source `commits` slice.
    pub commit_idx: usize,
}

/// A parsed query term: its characters plus whether it must match case-sensitively (smart-case: any
/// uppercase char in the term makes it case-sensitive).
struct Term {
    needle: Vec<char>,
    case_sensitive: bool,
}

fn parse_terms(query: &str) -> Vec<Term> {
    query
        .split_whitespace()
        .map(|t| Term {
            needle: t.chars().collect(),
            case_sensitive: t.chars().any(|c| c.is_uppercase()),
        })
        .collect()
}

/// Smart-case char comparison: exact when the term is case-sensitive, otherwise case-folded.
fn char_eq(hay: char, needle: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        hay == needle
    } else {
        hay.to_lowercase().eq(needle.to_lowercase())
    }
}

/// The first char index in `hay` where `needle` occurs as a contiguous substring, if any.
fn find(hay: &[char], needle: &[char], case_sensitive: bool) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| {
        needle
            .iter()
            .zip(&hay[i..i + needle.len()])
            .all(|(&n, &h)| char_eq(h, n, case_sensitive))
    })
}

/// Filter `commits` by `query`.
///
/// An empty query returns every commit in original (reverse-chronological) order. Otherwise a commit
/// is kept only when every whitespace-separated term is a literal substring of its haystack
/// (smart-case); matches keep their original order.
pub fn filter(commits: &[Commit], query: &str) -> Vec<MatchEntry> {
    filter_items(commits, query, |c| c.haystack.as_str())
}

/// Filter arbitrary `items` by `query`, with the same exact substring-per-term (smart-case)
/// semantics as [`filter`]; `haystack` extracts each item's searchable text. Used by both the log
/// (over `Commit`) and the branch screen (over `Branch`) so both filter identically.
pub fn filter_items<T>(items: &[T], query: &str, haystack: impl Fn(&T) -> &str) -> Vec<MatchEntry> {
    let terms = parse_terms(query);
    items
        .iter()
        .enumerate()
        .filter_map(|(commit_idx, item)| {
            if terms.is_empty() {
                return Some(MatchEntry { commit_idx });
            }
            let hay: Vec<char> = haystack(item).chars().collect();
            terms
                .iter()
                .all(|t| find(&hay, &t.needle, t.case_sensitive).is_some())
                .then_some(MatchEntry { commit_idx })
        })
        .collect()
}

/// Char-index ranges of `text` covered by any query term — for highlighting matches in the list.
///
/// Every occurrence of every term is reported (smart-case, matching [`filter`]); overlapping or
/// touching ranges are merged. Ranges are `(start, end)` half-open char indices into `text`. An empty
/// query yields no ranges. Because terms never contain whitespace and the haystack joins fields with
/// spaces, a term can only match within a single field — so highlighting a field with these ranges is
/// consistent with what `filter` matched.
pub fn match_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    let hay: Vec<char> = text.chars().collect();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for term in parse_terms(query) {
        let len = term.needle.len();
        if len == 0 || len > hay.len() {
            continue;
        }
        let mut i = 0;
        while i + len <= hay.len() {
            let hit = term
                .needle
                .iter()
                .zip(&hay[i..i + len])
                .all(|(&n, &h)| char_eq(h, n, term.case_sensitive));
            if hit {
                ranges.push((i, i + len));
                i += len; // step past this occurrence (non-overlapping per term)
            } else {
                i += 1;
            }
        }
    }
    merge(ranges)
}

/// Sort and coalesce overlapping/touching ranges.
fn merge(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
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

    // LOG-05: only commits containing the term as a literal substring are returned.
    #[test]
    fn log_05_filters_non_matching() {
        let c = corpus();
        let m = filter(&c, "parser");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].commit_idx, 2);
    }

    // LOG-05: matching is exact substring, NOT a fuzzy subsequence — a query whose chars appear only
    // scattered (not contiguous) does not match.
    #[test]
    fn log_05_exact_not_subsequence() {
        let c = vec![
            commit("1111111", "Daniel Kagan", "add feature"),
            commit("2222222", "Daniel Nagn", "fix bug"),
        ];
        // "dankag" is a subsequence of "daniel kagan" but not a substring -> no match.
        assert_eq!(filter(&c, "dankag").len(), 0);
        // Typed as two terms, both are substrings of "daniel kagan" but "kag" isn't in "daniel nagn".
        let m = filter(&c, "dan kag");
        assert_eq!(m.len(), 1);
        assert_eq!(c[m[0].commit_idx].author, "Daniel Kagan");
    }

    // LOG-05: whitespace-separated terms are AND-ed; each must be a substring somewhere.
    #[test]
    fn log_05_terms_are_anded() {
        let c = corpus();
        // "add" is in commit 0's subject, "fuzzy" too -> matches; "add test" matches nothing.
        assert_eq!(filter(&c, "add fuzzy").len(), 1);
        assert_eq!(filter(&c, "add test").len(), 0);
    }

    // LOG-05: matches keep original (reverse-chronological) order, not a re-ranked score order.
    #[test]
    fn log_05_keeps_original_order() {
        let c = vec![
            commit("1111111", "z", "parser rewrite"), // scattered position of "parser"
            commit("2222222", "z", "the parser"),     // "parser" later in the string
        ];
        let m = filter(&c, "parser");
        assert_eq!(
            m.iter().map(|e| e.commit_idx).collect::<Vec<_>>(),
            vec![0, 1]
        );
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

    // LOG-25: match_ranges reports the char ranges of each term occurrence for highlighting.
    #[test]
    fn log_25_match_ranges_marks_each_occurrence() {
        // "par" occurs once in "refactor parser" at chars 9..12 (after "refactor" + space).
        assert_eq!(match_ranges("refactor parser", "par"), vec![(9, 12)]);
        // Multiple terms are each highlighted.
        let mut r = match_ranges("add fuzzy search", "add search");
        r.sort_unstable();
        assert_eq!(r, vec![(0, 3), (10, 16)]);
    }

    // LOG-25: overlapping/touching ranges are merged; empty query yields nothing.
    #[test]
    fn log_25_match_ranges_merges_and_empties() {
        // "ab" and "bc" overlap on the shared "b" -> merged into one 0..3 range.
        assert_eq!(match_ranges("abc", "ab bc"), vec![(0, 3)]);
        assert!(match_ranges("anything", "").is_empty());
        assert!(match_ranges("anything", "   ").is_empty());
    }

    // LOG-25: match_ranges is smart-case, consistent with filter.
    #[test]
    fn log_25_match_ranges_smart_case() {
        assert_eq!(match_ranges("Add Feature", "feature"), vec![(4, 11)]);
        assert!(match_ranges("add feature", "Feature").is_empty());
    }
}
