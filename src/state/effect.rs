//! Effects: the reducer's only output besides mutating state. The shell (`runtime`) executes these
//! and feeds results back as [`crate::state::Event`]s. The reducer itself performs no I/O.

use crate::domain::{DiffKind, DiffScope, View};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Load one page of a view's log (`skip`..`skip+limit`, newest first). `epoch` tags the load
    /// generation so the reducer can drop batches from a superseded load. The reducer emits the next
    /// page's effect as each batch arrives, so the history streams in behind the first paint.
    LoadLogPage {
        view: View,
        skip: usize,
        limit: usize,
        epoch: u64,
    },
    /// Load the diff (`git show`) for a commit hash, for the preview pane, rendered into `width`
    /// columns (so the configured diff tool can pick split vs unified).
    LoadDiff { hash: String, width: u16 },
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
    /// Look up a commit's AI summary in the on-disk cache (cheap; no model call).
    LoadSummary { hash: String },
    /// Bulk-look-up many summaries in the on-disk cache on one background thread, so the AI marker
    /// can appear for every already-summarized entry on first paint (not just the selected one). The
    /// keys are commit SHAs (log) or branch summary keys (branch); cache hits come back in one
    /// [`crate::state::Event::SummariesPrefetched`]. No model calls, no UI blocking.
    PrefetchSummaries(Vec<String>),
    /// Generate a commit's AI summary: fetch its diff, prompt the model, cache the result.
    GenerateSummary { hash: String, subject: String },

    // --- gitt status ---------------------------------------------------------------------------
    /// Load (or reload) the working-tree status.
    LoadStatus,
    /// Load the diff for a file into the preview pane, rendered into `width` columns.
    LoadFileDiff {
        path: String,
        kind: DiffKind,
        width: u16,
    },
    /// Stage a file (`git add -- <path>`).
    Stage(String),
    /// Unstage a file (`git restore --staged -- <path>`).
    Unstage(String),
    /// Discard a file's changes (`git restore` for tracked, delete for untracked).
    Discard { path: String, untracked: bool },

    // --- gitt branch ---------------------------------------------------------------------------
    /// Load (or reload) the local branch list.
    LoadBranches,
    /// Fetch the per-branch PR status (one background `gh pr list` call) to fill the PR column.
    LoadPrStatuses,
    /// Check out a branch by name (attaches HEAD to it), then reload the branch list.
    CheckoutBranch(String),
    /// Create a new branch off HEAD and switch to it, then reload the branch list.
    CreateBranch(String),
    /// Delete a local branch, then reload the branch list.
    DeleteBranch(String),
    /// Generate a branch's AI summary from its diff-vs-base and commit subjects, cache it under `key`.
    GenerateBranchSummary {
        key: String,
        branch: String,
        base: String,
    },

    // --- gitt diff -----------------------------------------------------------------------------
    /// Load (or reload) the changed-file list for a diff scope.
    LoadDiffFiles(DiffScope),
    /// Load a file's diff text (for a scope) into the diff pane, rendered into `width` columns.
    LoadDiffText {
        scope: DiffScope,
        path: String,
        width: u16,
    },
    /// Load a file's diff text (for a scope) and copy it to the clipboard.
    CopyScopeDiff { scope: DiffScope, path: String },

    /// Quit the application.
    Quit,
}
