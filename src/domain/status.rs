//! Core working-tree status data. Plain values — no I/O, no rendering.

/// Which diff to show for a file in the preview pane, derived from its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// An untracked file: show its contents (there is no tracked version to diff against).
    Untracked,
    /// A file with worktree changes: show `git diff -- <path>`.
    Worktree,
    /// A fully-staged file (no worktree changes): show `git diff --staged -- <path>`.
    Staged,
}

/// A single entry from `git status --porcelain=v1`: one changed path with its index (staged) and
/// worktree (unstaged) status codes, exactly as git reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Index (staged) status char: one of `M A D R C ` `, or `?` for untracked.
    pub index: char,
    /// Worktree (unstaged) status char: one of `M D ` `, or `?` for untracked.
    pub worktree: char,
    /// The path, relative to the repo root.
    pub path: String,
    /// For a rename/copy, the original path (`git`'s `orig -> new`); otherwise `None`.
    pub orig_path: Option<String>,
}

impl StatusEntry {
    /// True when git reports the file as untracked (`??`).
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    /// The two-letter `XY` badge shown in the list (e.g. `MM`, `A `, `??`, ` D`).
    pub fn badge(&self) -> String {
        format!("{}{}", self.index, self.worktree)
    }

    /// True when the file has worktree/untracked changes that could be staged.
    pub fn has_worktree_changes(&self) -> bool {
        self.is_untracked() || self.worktree != ' '
    }

    /// True when the file has changes recorded in the index.
    pub fn is_staged(&self) -> bool {
        !self.is_untracked() && self.index != ' '
    }

    /// Which diff the preview pane should show for this file.
    pub fn diff_kind(&self) -> DiffKind {
        if self.is_untracked() {
            DiffKind::Untracked
        } else if self.worktree != ' ' {
            DiffKind::Worktree
        } else {
            DiffKind::Staged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: char, worktree: char) -> StatusEntry {
        StatusEntry {
            index,
            worktree,
            path: "f".into(),
            orig_path: None,
        }
    }

    #[test]
    fn untracked_classification() {
        let u = entry('?', '?');
        assert!(u.is_untracked());
        assert!(u.has_worktree_changes());
        assert!(!u.is_staged());
        assert_eq!(u.diff_kind(), DiffKind::Untracked);
        assert_eq!(u.badge(), "??");
    }

    #[test]
    fn worktree_modified_classification() {
        let m = entry(' ', 'M');
        assert!(!m.is_untracked());
        assert!(m.has_worktree_changes());
        assert!(!m.is_staged());
        assert_eq!(m.diff_kind(), DiffKind::Worktree);
        assert_eq!(m.badge(), " M");
    }

    #[test]
    fn fully_staged_classification() {
        let a = entry('A', ' ');
        assert!(a.is_staged());
        assert!(!a.has_worktree_changes());
        assert_eq!(a.diff_kind(), DiffKind::Staged);
        assert_eq!(a.badge(), "A ");
    }

    #[test]
    fn staged_and_modified_prefers_worktree_diff() {
        let mm = entry('M', 'M');
        assert!(mm.is_staged());
        assert!(mm.has_worktree_changes());
        assert_eq!(mm.diff_kind(), DiffKind::Worktree);
        assert_eq!(mm.badge(), "MM");
    }
}
