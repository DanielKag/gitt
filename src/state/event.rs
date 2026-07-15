//! Events: everything that can drive the reducer. Terminal input plus async results routed back
//! from the shell's worker threads.

use std::collections::HashMap;

use crate::domain::{Branch, Commit, DiffFile, DiffScope, PrStatus, StatusEntry, View};
use crossterm::event::KeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A key press from the terminal.
    Key(KeyEvent),
    /// Terminal resized to (cols, rows).
    Resize(u16, u16),
    /// One page of a view's log finished loading (`skip` is the offset it was fetched at; `epoch` the
    /// load generation). The reducer appends it and requests the next page until the history is
    /// exhausted or the cap is hit.
    LogBatch {
        view: View,
        skip: usize,
        epoch: u64,
        commits: Vec<Commit>,
    },
    /// A view's log page failed to load. If earlier pages already landed they are kept; otherwise the
    /// view enters the failed state.
    LogPageFailed {
        view: View,
        epoch: u64,
        error: String,
    },
    /// A commit's diff finished loading.
    DiffLoaded { hash: String, text: String },
    /// A commit's diff failed to load.
    DiffFailed { hash: String, error: String },
    /// `git fetch` finished (ok or error message).
    FetchFinished(Result<(), String>),
    /// A one-shot action (copy/checkout/browser/pr) finished.
    ActionFinished {
        label: String,
        result: Result<(), String>,
    },

    // --- gitt log AI summaries -----------------------------------------------------------------
    /// A cache lookup found an existing summary (fills state only if not already tracked, so it
    /// can't clobber a generation the user kicked off while the lookup was in flight).
    SummaryLoaded { hash: String, text: String },
    /// A cache lookup found no summary for this commit; the user can generate one.
    SummaryMissing { hash: String },
    /// A background bulk cache prefetch finished, carrying the (key, summary) pairs that were found
    /// on disk. Used to light up the AI marker for every already-summarized entry at once, without
    /// clobbering any state the user has since changed.
    SummariesPrefetched(Vec<(String, String)>),
    /// A streamed token of an in-flight generation; appended to the commit's partial summary.
    SummaryChunk { hash: String, delta: String },
    /// A freshly generated summary (authoritative: overwrites any prior state for the commit).
    SummaryReady { hash: String, text: String },
    /// Generating a commit's summary failed.
    SummaryFailed { hash: String, error: String },

    // --- gitt branch ---------------------------------------------------------------------------
    /// The local branch list finished loading.
    BranchesLoaded(Vec<Branch>),
    /// The local branch list failed to load.
    BranchesFailed(String),
    /// A branch mutation (create/delete) finished; the branch view reloads afterward.
    BranchMutated {
        label: String,
        result: Result<(), String>,
    },
    /// A checkout finished. On success `gitt branch` quits immediately (like a native `git checkout`);
    /// on failure the screen stays open and reports the error (BR-06).
    BranchCheckedOut {
        branch: String,
        result: Result<(), String>,
    },
    /// The per-branch PR statuses finished loading (fills the PR column).
    PrStatusesLoaded(HashMap<String, PrStatus>),
    /// The per-branch PR statuses failed to load (e.g. no `gh` / not a GitHub repo); column stays blank.
    PrStatusesFailed(String),

    // --- gitt status ---------------------------------------------------------------------------
    /// The working-tree status finished loading.
    StatusLoaded(Vec<StatusEntry>),
    /// The working-tree status failed to load.
    StatusFailed(String),
    /// A file's diff finished loading (keyed by path).
    FileDiffLoaded { path: String, text: String },
    /// A file's diff failed to load.
    FileDiffFailed { path: String, error: String },
    /// A stage/unstage/discard/commit mutation finished; the status view reloads afterward.
    StatusMutated {
        label: String,
        result: Result<(), String>,
    },
    /// HEAD's message finished loading (the amend prefill), or an error (e.g. no commit yet).
    HeadMessageLoaded(Result<String, String>),
    /// A streamed token of an in-flight AI commit-message suggestion; appended to the editor buffer.
    CommitSuggestionChunk { delta: String },
    /// The AI commit-message suggestion finished (the full drafted subject line).
    CommitSuggestionReady { text: String },
    /// The AI commit-message suggestion failed.
    CommitSuggestionFailed { error: String },

    // --- gitt diff -----------------------------------------------------------------------------
    /// A scope's changed-file list finished loading.
    DiffFilesLoaded {
        scope: DiffScope,
        files: Vec<DiffFile>,
    },
    /// A scope's changed-file list failed to load.
    DiffFilesFailed { scope: DiffScope, error: String },
    /// A file's diff text (for a scope) finished loading, for the diff pane.
    DiffTextLoaded {
        scope: DiffScope,
        path: String,
        text: String,
    },
    /// A file's diff text failed to load.
    DiffTextFailed {
        scope: DiffScope,
        path: String,
        error: String,
    },
}
