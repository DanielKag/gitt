//! Core data for the `gitt diff` viewer: the diff **scope** the user is looking at and one changed
//! file within it. Plain values — no I/O, no rendering.

/// Which diff `gitt diff` is showing. The four scopes mirror how a developer reasons about their
/// changes, from the least to the most committed, ending at the GitHub-PR "Files changed" view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffScope {
    /// Worktree ↔ index — changes not yet staged (`git diff`). The default.
    Unstaged,
    /// Index ↔ HEAD — changes staged for the next commit (`git diff --staged`).
    Staged,
    /// Worktree ↔ HEAD — everything uncommitted, staged and unstaged (`git diff HEAD`).
    Working,
    /// merge-base(`<main>`, HEAD)…HEAD — the GitHub-PR file-changes diff (`git diff <main>...HEAD`).
    Branch,
}

impl DiffScope {
    /// The scopes in tab / cycle order.
    pub const ALL: [DiffScope; 4] = [
        DiffScope::Unstaged,
        DiffScope::Staged,
        DiffScope::Working,
        DiffScope::Branch,
    ];

    /// Position of this scope in [`ALL`](DiffScope::ALL).
    pub fn index(self) -> usize {
        DiffScope::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The next scope, wrapping past the end (used by `→`).
    pub fn next(self) -> DiffScope {
        DiffScope::ALL[(self.index() + 1) % DiffScope::ALL.len()]
    }

    /// The previous scope, wrapping past the start (used by `←`).
    pub fn prev(self) -> DiffScope {
        let n = DiffScope::ALL.len();
        DiffScope::ALL[(self.index() + n - 1) % n]
    }

    /// A stable short name for this scope (the `Branch` label is composed with the branch name in the
    /// UI, so this returns `"Branch"` for it).
    pub fn short(self) -> &'static str {
        match self {
            DiffScope::Unstaged => "Unstaged",
            DiffScope::Staged => "Staged",
            DiffScope::Working => "Working",
            DiffScope::Branch => "Branch",
        }
    }
}

/// A single changed file in a diff, from `git diff --name-status`: its change status letter and path
/// (plus the original path for a rename/copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// Change status letter as git reports it: `M` modified, `A` added, `D` deleted, `R` renamed,
    /// `C` copied, `T` type-changed.
    pub status: char,
    /// The (new) path, relative to the repo root.
    pub path: String,
    /// For a rename/copy, the original path; otherwise `None`.
    pub orig_path: Option<String>,
}

impl DiffFile {
    /// True when this entry is a rename or copy (it carries an original path).
    pub fn is_rename(&self) -> bool {
        matches!(self.status, 'R' | 'C')
    }

    /// The one-letter badge shown in the list.
    pub fn badge(&self) -> char {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_cycle_and_wrap() {
        assert_eq!(DiffScope::Unstaged.next(), DiffScope::Staged);
        assert_eq!(DiffScope::Staged.next(), DiffScope::Working);
        assert_eq!(DiffScope::Working.next(), DiffScope::Branch);
        // Wraps at the end.
        assert_eq!(DiffScope::Branch.next(), DiffScope::Unstaged);
        // Prev wraps at the start.
        assert_eq!(DiffScope::Unstaged.prev(), DiffScope::Branch);
        assert_eq!(DiffScope::Staged.prev(), DiffScope::Unstaged);
    }

    #[test]
    fn rename_classification() {
        let r = DiffFile {
            status: 'R',
            path: "new.rs".into(),
            orig_path: Some("old.rs".into()),
        };
        assert!(r.is_rename());
        assert_eq!(r.badge(), 'R');

        let m = DiffFile {
            status: 'M',
            path: "f.rs".into(),
            orig_path: None,
        };
        assert!(!m.is_rename());
    }
}
