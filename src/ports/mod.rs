//! Ports: the trait seams between the pure core and the outside world, plus their real
//! implementations. This is the ONLY place the app performs I/O (spawning `git`/`gh`, clipboard,
//! browser, clock, environment). Tests inject fakes; the shell wires the real impls in `runtime`.

pub mod git_cli;
pub mod system;

use std::sync::Arc;

use crate::domain::{Commit, DiffFile, DiffKind, DiffScope, StatusEntry, View};

/// Whether git output should include ANSI color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Always,
    Never,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("not a git repository")]
    NotARepo,
    #[error("`{cmd}` failed ({code}): {stderr}")]
    Exit {
        cmd: String,
        code: i32,
        stderr: String,
    },
    #[error("io: {0}")]
    Io(String),
}

/// Semantic access to a git repository. `log` returns already-parsed commits so the reducer never
/// sees raw text (parsing is a separate pure function tested against fixtures).
pub trait GitRepo: Send + Sync {
    /// One page of the log for a view: `skip` commits in, at most `limit` commits, newest first.
    /// Returns already-parsed `Commit`s. Paging lets the shell stream a large history in behind the
    /// first paint (see LOG-21..24).
    fn log_page(&self, view: View, skip: usize, limit: usize) -> Result<Vec<Commit>, GitError>;
    /// `git show` for a commit. `ignore_whitespace` adds `-w` so whitespace-only churn is dropped —
    /// used for the AI summary (less noise, fewer prompt tokens); the preview passes `false` to stay
    /// faithful to the real diff.
    fn show(
        &self,
        hash: &str,
        color: ColorMode,
        ignore_whitespace: bool,
    ) -> Result<String, GitError>;
    fn fetch(&self) -> Result<(), GitError>;
    fn checkout(&self, hash: &str) -> Result<(), GitError>;

    // --- gitt status -------------------------------------------------------------------------------
    /// Parsed `git status` for the working tree.
    fn status(&self) -> Result<Vec<StatusEntry>, GitError>;
    /// The diff text to show for a file, depending on which side of it changed.
    fn file_diff(&self, path: &str, kind: DiffKind) -> Result<String, GitError>;
    /// Stage a file (`git add`), covering modifications, additions, and deletions.
    fn stage(&self, path: &str) -> Result<(), GitError>;
    /// Unstage a file (`git restore --staged`).
    fn unstage(&self, path: &str) -> Result<(), GitError>;
    /// Discard a file's changes: restore a tracked file, or delete an untracked one.
    fn discard(&self, path: &str, untracked: bool) -> Result<(), GitError>;

    // --- gitt diff ---------------------------------------------------------------------------------
    /// The changed files for a diff scope (parsed `git diff --name-status -z <scope-args>`).
    fn diff_files(&self, scope: DiffScope) -> Result<Vec<DiffFile>, GitError>;
    /// The plain-text diff of one file for a scope (`git diff <scope-args> -- <path>`).
    fn diff_scope_file(&self, scope: DiffScope, path: &str) -> Result<String, GitError>;
}

/// A source of "now" (unix seconds) — injected so relative dates are deterministic in tests.
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}

pub trait Clipboard: Send + Sync {
    fn copy(&self, text: &str) -> Result<(), GitError>;
}

pub trait Browser: Send + Sync {
    fn open(&self, url: &str) -> Result<(), GitError>;
}

pub trait PrOpener: Send + Sync {
    fn open_pr(&self, hash: &str) -> Result<(), GitError>;
}

pub trait Env: Send + Sync {
    fn var(&self, key: &str) -> Option<String>;
    fn has_delta(&self) -> bool;
}

/// Streams a commit summary from a built prompt, invoking `on_token` for each chunk as it arrives
/// (real impl calls the local Ollama HTTP API). The accumulated tokens are the full summary.
pub trait Summarizer: Send + Sync {
    fn summarize(&self, prompt: &str, on_token: &mut dyn FnMut(&str)) -> Result<(), GitError>;
}

/// Content-addressed store for commit summaries, keyed by the commit's full SHA (real impl is a
/// directory of files under the user's cache home).
pub trait SummaryCache: Send + Sync {
    /// The cached summary for `key`, if present.
    fn get(&self, key: &str) -> Option<String>;
    /// Store `summary` under `key`. Best-effort: a write error is surfaced but not fatal.
    fn put(&self, key: &str, summary: &str) -> Result<(), GitError>;
}

/// The bundle of side-effecting ports the runtime dispatches effects to.
#[derive(Clone)]
pub struct Ports {
    pub git: Arc<dyn GitRepo>,
    pub clipboard: Arc<dyn Clipboard>,
    pub browser: Arc<dyn Browser>,
    pub pr: Arc<dyn PrOpener>,
    /// Generates commit summaries via the local model.
    pub summarizer: Arc<dyn Summarizer>,
    /// On-disk cache for generated summaries.
    pub summary_cache: Arc<dyn SummaryCache>,
}
