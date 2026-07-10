//! Ports: the trait seams between the pure core and the outside world, plus their real
//! implementations. This is the ONLY place the app performs I/O (spawning `git`/`gh`, clipboard,
//! browser, clock, environment). Tests inject fakes; the shell wires the real impls in `runtime`.

pub mod git_cli;
pub mod system;

use std::sync::Arc;

use crate::domain::{Commit, View};

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
    fn log(&self, view: View, limit: usize) -> Result<Vec<Commit>, GitError>;
    fn show(&self, hash: &str, color: ColorMode) -> Result<String, GitError>;
    fn fetch(&self) -> Result<(), GitError>;
    fn checkout(&self, hash: &str) -> Result<(), GitError>;
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

/// The bundle of side-effecting ports the runtime dispatches effects to.
#[derive(Clone)]
pub struct Ports {
    pub git: Arc<dyn GitRepo>,
    pub clipboard: Arc<dyn Clipboard>,
    pub browser: Arc<dyn Browser>,
    pub pr: Arc<dyn PrOpener>,
    /// Max commits to load per view.
    pub log_limit: usize,
}
