//! Events: everything that can drive the reducer. Terminal input plus async results routed back
//! from the shell's worker threads.

use crate::domain::{Commit, View};
use crossterm::event::KeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A key press from the terminal.
    Key(KeyEvent),
    /// Terminal resized to (cols, rows).
    Resize(u16, u16),
    /// A view's log finished loading.
    LogLoaded { view: View, commits: Vec<Commit> },
    /// A view's log failed to load.
    LogFailed { view: View, error: String },
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
}
