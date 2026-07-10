//! Effects: the reducer's only output besides mutating state. The shell (`runtime`) executes these
//! and feeds results back as [`crate::state::Event`]s. The reducer itself performs no I/O.

use crate::domain::View;

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
    /// Quit the application.
    Quit,
}
