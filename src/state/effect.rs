//! Effects: the reducer's only output besides mutating state. The shell (`runtime`) executes these
//! and feeds results back as [`crate::state::Event`]s. The reducer itself performs no I/O.

use crate::domain::{DiffKind, View};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Load (or reload) the log for a view.
    LoadLog(View),
    /// Load the diff (`git show`) for a commit hash, for the preview pane.
    LoadDiff(String),
    /// `git fetch`, then reload the current view.
    Fetch,
    /// `git checkout <hash>`.
    Checkout(String),
    /// Copy text to the system clipboard.
    CopyToClipboard(String),
    /// Open a URL in the browser.
    OpenBrowser(String),
    /// Open the pull request for a commit hash (via `gh`).
    OpenPr(String),

    // --- gitt status ---------------------------------------------------------------------------
    /// Load (or reload) the working-tree status.
    LoadStatus,
    /// Load the diff for a file into the preview pane.
    LoadFileDiff { path: String, kind: DiffKind },
    /// Stage a file (`git add -- <path>`).
    Stage(String),
    /// Unstage a file (`git restore --staged -- <path>`).
    Unstage(String),
    /// Discard a file's changes (`git restore` for tracked, delete for untracked).
    Discard { path: String, untracked: bool },

    /// Quit the application.
    Quit,
}
