//! Core data for the `gitt branch` screen: a single local branch. Plain values — no I/O, no
//! rendering. Mirrors [`Commit`](crate::domain::Commit): each branch carries the searchable
//! `haystack` fed to the fuzzy matcher and a `relative` time computed at load from a `Clock`.

/// A single local branch as shown in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short branch name (`refs/heads/<name>` → `<name>`).
    pub name: String,
    /// True for the currently checked-out branch (`HEAD`).
    pub is_current: bool,
    /// Full 40-char SHA of the branch tip — the summary cache key is derived from this.
    pub tip: String,
    /// Upstream tracking branch (`%(upstream:short)`), if the branch has one.
    pub upstream: Option<String>,
    /// Tip commit's committer timestamp (unix seconds).
    pub timestamp: i64,
    /// Tip commit subject.
    pub subject: String,
    /// Human relative time of the tip commit ("3 days ago"), computed at load from a `Clock`.
    pub relative: String,
    /// Lowercase-free search string fed to the fuzzy matcher (name + upstream + subject).
    pub haystack: String,
}

/// The pull-request status of a branch, as reported by `gh pr list`. Overlaid on the list lazily (a
/// single background `gh` call) so it never blocks the instant first paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrStatus {
    /// An open PR (not a draft).
    Open,
    /// An open PR marked as a draft.
    Draft,
    /// A merged PR.
    Merged,
    /// A closed (unmerged) PR.
    Closed,
}

impl PrStatus {
    /// The short label shown in the PR column.
    pub fn label(self) -> &'static str {
        match self {
            PrStatus::Open => "open",
            PrStatus::Draft => "draft",
            PrStatus::Merged => "merged",
            PrStatus::Closed => "closed",
        }
    }

    /// Ranking used to pick a single status when a branch has more than one PR: an open/draft PR wins
    /// over a merged one, which wins over a closed one (show the most actionable state).
    pub fn rank(self) -> u8 {
        match self {
            PrStatus::Open | PrStatus::Draft => 3,
            PrStatus::Merged => 2,
            PrStatus::Closed => 1,
        }
    }
}

/// The on-disk summary cache key for a branch, derived from its tip SHA. Prefixed so a branch summary
/// can never collide with the commit summary that shares the same tip SHA (see BR-11).
pub fn summary_key(tip: &str) -> String {
    format!("branch-{tip}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_key_is_prefixed() {
        assert_eq!(summary_key("abc123"), "branch-abc123");
        // Distinct from the bare SHA a commit summary would use.
        assert_ne!(summary_key("abc123"), "abc123");
    }
}
