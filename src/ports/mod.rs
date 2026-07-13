//! Ports: the trait seams between the pure core and the outside world, plus their real
//! implementations. This is the ONLY place the app performs I/O (spawning `git`/`gh`, clipboard,
//! browser, clock, environment). Tests inject fakes; the shell wires the real impls in `runtime`.

pub mod git_cli;
pub mod system;

use std::sync::Arc;

use std::collections::HashMap;

use crate::domain::{Branch, Commit, DiffFile, DiffKind, DiffScope, PrStatus, StatusEntry, View};

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

impl GitError {
    /// A short, user-facing message for a status line: for a failed command, just git's own stderr
    /// (dropping the invoked command and exit code, which are noise to a user); otherwise the full
    /// message. E.g. ``git checkout` failed (128): fatal: 'x' is already checked out`` → `fatal: 'x'
    /// is already checked out`.
    pub fn concise(&self) -> String {
        match self {
            GitError::Exit { stderr, .. } if !stderr.trim().is_empty() => stderr.trim().to_string(),
            other => other.to_string(),
        }
    }
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

    // --- gitt branch -------------------------------------------------------------------------------
    /// The local branches, parsed from `git for-each-ref refs/heads` (most-recently-committed first).
    fn branches(&self) -> Result<Vec<Branch>, GitError>;
    /// Create a new branch off `HEAD` and switch to it (`git switch -c <name>`).
    fn create_branch(&self, name: &str) -> Result<(), GitError>;
    /// Delete a local branch (`git branch -D <name>`).
    fn delete_branch(&self, name: &str) -> Result<(), GitError>;
    /// The whole diff of a branch against the base (`git diff <base>...<name>`), whitespace-ignored —
    /// the input the branch AI summary reasons over.
    fn branch_diff(&self, name: &str) -> Result<String, GitError>;
    /// The subjects of the commits a branch has ahead of the base (`git log <base>..<name> --pretty=%s`).
    fn branch_commit_subjects(&self, name: &str) -> Result<Vec<String>, GitError>;
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
    /// The PR status of the current user's branches, keyed by head-branch name (`gh pr list
    /// --author @me`). One call, run off the UI thread; a missing `gh`/non-GitHub repo surfaces as an
    /// error (the column then simply stays blank). Scoped to the user's own PRs so it stays correct in
    /// a busy monorepo, where an unscoped newest-N list never reaches the branches you have locally.
    fn statuses(&self) -> Result<HashMap<String, PrStatus>, GitError>;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concise_error_keeps_only_git_stderr() {
        let e = GitError::Exit {
            cmd: "git checkout --quiet master".into(),
            code: 128,
            stderr: "fatal: 'master' is already checked out".into(),
        };
        // The command and exit code are dropped; only git's own message remains.
        assert_eq!(e.concise(), "fatal: 'master' is already checked out");
    }

    #[test]
    fn concise_error_falls_back_to_full_message() {
        // With no stderr, keep the (still useful) full message rather than an empty status.
        let e = GitError::Exit {
            cmd: "git checkout x".into(),
            code: 1,
            stderr: "   ".into(),
        };
        assert_eq!(e.concise(), "`git checkout x` failed (1):    ");
        assert_eq!(GitError::NotARepo.concise(), "not a git repository");
    }
}
