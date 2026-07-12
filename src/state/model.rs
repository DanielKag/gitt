//! The application state and its pure helpers (selection, matching, scrolling).

use std::collections::HashMap;

use crate::domain::{Commit, View, text};
use crate::fuzzy::{self, MatchEntry};

/// Rows of "chrome" around the commit list: header (1) + search bar (1) + status (1).
pub const CHROME_ROWS: u16 = 3;

/// How many commits each background page fetches. The first page paints instantly; the rest stream
/// in behind it (see LOG-21..24). Chosen so first paint stays instant even on a large history while
/// keeping the number of `git log` pages small for a 20k–200k-commit repo.
pub const LOG_PAGE: usize = 5000;

/// Rows reserved for the AI summary panel when collapsed: border (2) + 2 text lines. Also the floor
/// when expanded, so short summaries look identical collapsed or expanded.
pub const SUMMARY_ROWS: u16 = 4;

/// Input focus / interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the list with vim motions.
    List,
    /// Typing into the search filter.
    Search,
    /// The action menu is open over the selected commit.
    Menu,
}

/// Load state of a single view's log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    /// Not requested yet.
    #[default]
    Idle,
    /// First page in flight; no commits yet.
    Loading,
    /// Some commits are loaded (newest first) but more pages are still streaming in.
    Streaming(Vec<Commit>),
    /// All commits loaded (newest first); the history is complete (or the cap was hit).
    Loaded(Vec<Commit>),
    /// Load failed with a message.
    Failed(String),
}

/// An action the user can run on the selected commit (mirrors glogm's secondary menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    OpenGithub,
    OpenPr,
    CopySha,
    Checkout,
    CopyRevert,
}

impl MenuAction {
    /// The menu items, in display order.
    pub fn all() -> Vec<MenuAction> {
        vec![
            MenuAction::OpenGithub,
            MenuAction::OpenPr,
            MenuAction::CopySha,
            MenuAction::Checkout,
            MenuAction::CopyRevert,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuAction::OpenGithub => "Open in GitHub",
            MenuAction::OpenPr => "Open Pull Request",
            MenuAction::CopySha => "Copy SHA",
            MenuAction::Checkout => "Checkout",
            MenuAction::CopyRevert => "Copy revert command",
        }
    }
}

/// The action menu overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMenu {
    pub items: Vec<MenuAction>,
    pub cursor: usize,
    /// The commit the menu was opened on.
    pub hash: String,
    pub short: String,
    pub subject: String,
}

impl ActionMenu {
    pub fn selected(&self) -> MenuAction {
        self.items[self.cursor]
    }
}

/// State of the diff-preview pane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreviewState {
    #[default]
    Idle,
    Loading(String),
    Ready {
        hash: String,
        text: String,
    },
    Failed {
        hash: String,
        error: String,
    },
}

/// The AI-summary state of a single commit, keyed by its full SHA in [`AppState::summaries`].
/// Absence from the map means "not looked up yet".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryState {
    /// Looked up the cache; nothing there. Press `s` to generate.
    Missing,
    /// A generation is in flight (Ollama call running off the UI thread), holding the tokens streamed
    /// so far (empty until the first token arrives).
    Generating(String),
    /// A summary is available (from cache or freshly generated).
    Ready(String),
    /// Generation failed; the message is shown and `s` can retry.
    Failed(String),
}

/// The whole application state.
#[derive(Debug, Clone)]
pub struct AppState {
    pub view: View,
    pub logs: HashMap<View, Load>,
    /// Current load generation per view. Bumped each time a view's load (re)starts so batches from a
    /// superseded load (e.g. the user pressed `R` mid-stream) are recognised as stale and dropped.
    pub log_epoch: HashMap<View, u64>,
    /// Commits per background page. Defaults to [`LOG_PAGE`]; overridable (tests, `GITT_LOG_PAGE`).
    pub log_page: usize,
    /// Hard cap on total commits loaded per view (`0` = unlimited). From `--max-count`.
    pub max_count: usize,
    pub filter: String,
    /// Ranked matches for the active view + filter.
    pub matches: Vec<MatchEntry>,
    /// Index into `matches` of the selected row.
    pub cursor: usize,
    /// Index into `matches` of the first visible row.
    pub top: usize,
    pub mode: Mode,
    pub preview_open: bool,
    pub preview: PreviewState,
    pub menu: Option<ActionMenu>,
    /// AI summaries keyed by commit full SHA (shared across views).
    pub summaries: HashMap<String, SummaryState>,
    /// When true, the summary footer grows to show the selected commit's full summary (toggled by
    /// `S`); the list stays navigable above it.
    pub summary_expanded: bool,
    /// Transient status-line message.
    pub status: Option<String>,
    pub current_branch: String,
    pub main_branch: String,
    pub remote_url: Option<String>,
    /// Terminal size (cols, rows).
    pub size: (u16, u16),
    pub should_quit: bool,
}

impl AppState {
    pub fn new(current_branch: String, main_branch: String, remote_url: Option<String>) -> Self {
        AppState {
            view: View::LocalHead,
            logs: HashMap::new(),
            log_epoch: HashMap::new(),
            log_page: LOG_PAGE,
            max_count: 0,
            filter: String::new(),
            matches: Vec::new(),
            cursor: 0,
            top: 0,
            mode: Mode::List,
            preview_open: false,
            preview: PreviewState::Idle,
            menu: None,
            summaries: HashMap::new(),
            summary_expanded: false,
            status: None,
            current_branch,
            main_branch,
            remote_url,
            size: (80, 24),
            should_quit: false,
        }
    }

    /// Height (rows) of the summary footer, including its border. Collapsed it is [`SUMMARY_ROWS`];
    /// expanded it grows to fit the selected commit's summary (word-wrapped to the current width),
    /// floored at [`SUMMARY_ROWS`] and capped so the list keeps at least a few rows. Computed the
    /// same way the UI lays it out, so list scroll math and rendering agree.
    pub fn summary_panel_rows(&self) -> u16 {
        if !self.summary_expanded {
            return SUMMARY_ROWS;
        }
        let width = self.size.0.saturating_sub(2).max(1) as usize;
        let content = text::wrap_words(&self.summary_display_text(), width)
            .len()
            .max(1) as u16;
        let cap = self
            .size
            .1
            .saturating_sub(CHROME_ROWS + 3)
            .max(SUMMARY_ROWS);
        (content + 2).clamp(SUMMARY_ROWS, cap)
    }

    /// The plain (backticks stripped) text shown for the selected commit's summary — used to size the
    /// expanded footer. Empty when there's nothing substantial to show (so the footer stays at floor).
    fn summary_display_text(&self) -> String {
        match self.selected_summary() {
            Some(SummaryState::Ready(t)) => t.replace('`', ""),
            Some(SummaryState::Generating(b)) if !b.trim().is_empty() => b.replace('`', ""),
            Some(SummaryState::Failed(e)) => format!("summary failed: {e}"),
            _ => String::new(),
        }
    }

    /// Number of commit rows visible in the list given the current terminal height, accounting for
    /// the (possibly expanded) summary footer.
    pub fn viewport_rows(&self) -> usize {
        (self
            .size
            .1
            .saturating_sub(CHROME_ROWS + self.summary_panel_rows()))
        .max(1) as usize
    }

    /// The AI-summary state for the selected commit, if any is recorded.
    pub fn selected_summary(&self) -> Option<&SummaryState> {
        self.summaries.get(&self.selected()?.hash)
    }

    /// Loaded commits for the active view (empty slice if not loaded). Includes commits loaded so far
    /// while a background load is still streaming.
    pub fn commits(&self) -> &[Commit] {
        match self.logs.get(&self.view) {
            Some(Load::Loaded(commits)) | Some(Load::Streaming(commits)) => commits,
            _ => &[],
        }
    }

    /// While the active view is still streaming pages in the background, the count of commits loaded
    /// so far (for the status-line progress indicator); `None` once the load is complete or idle.
    pub fn log_loading_count(&self) -> Option<usize> {
        match self.logs.get(&self.view) {
            Some(Load::Streaming(commits)) => Some(commits.len()),
            _ => None,
        }
    }

    /// The currently selected commit, if any.
    pub fn selected(&self) -> Option<&Commit> {
        let entry = self.matches.get(self.cursor)?;
        self.commits().get(entry.commit_idx)
    }

    /// Full hash of the selected commit.
    pub fn selected_hash(&self) -> Option<String> {
        self.selected().map(|c| c.hash.clone())
    }

    /// Recompute matches for the active view + filter, then re-clamp cursor and scroll.
    pub fn recompute_matches(&mut self) {
        self.matches = match self.logs.get(&self.view) {
            Some(Load::Loaded(commits)) | Some(Load::Streaming(commits)) => {
                fuzzy::filter(commits, &self.filter)
            }
            _ => Vec::new(),
        };
        self.clamp_cursor();
        self.clamp_scroll();
    }

    /// Keep the cursor within the match list.
    pub fn clamp_cursor(&mut self) {
        if self.matches.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.matches.len() {
            self.cursor = self.matches.len() - 1;
        }
    }

    /// Keep the selected row visible: adjust `top` so `cursor` is within the viewport.
    pub fn clamp_scroll(&mut self) {
        let rows = self.viewport_rows();
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
        // Don't leave blank space at the bottom when the list is longer than the viewport.
        let max_top = self.matches.len().saturating_sub(rows);
        if self.top > max_top {
            self.top = max_top;
        }
    }
}
