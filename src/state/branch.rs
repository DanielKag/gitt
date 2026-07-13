//! The `gitt branch` screen state and its pure helpers (selection, matching, scrolling). No I/O.
//!
//! Mirrors the shape of the log [`AppState`](crate::state::AppState): the same selection/scroll
//! model, the same fuzzy-filter search, the same overlay-driven action menu, and the same AI-summary
//! footer (shared layout math in [`super::model`]) — so both screens behave the same for the user.

use std::collections::HashMap;

use crate::domain::{Branch, PrStatus, branch::summary_key};
use crate::fuzzy::{self, MatchEntry};

use super::model::{CHROME_ROWS, SummaryState, summary_panel_rows};

/// Load state of the branch list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BranchLoad {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<Branch>),
    Failed(String),
}

/// Input focus / interaction mode for the branch screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchMode {
    /// Browsing the branch list with vim motions.
    List,
    /// Typing into the search filter.
    Search,
    /// The per-branch action menu is open.
    Menu,
    /// The delete-confirmation overlay is open.
    Confirm,
    /// Typing the name of a new branch to create.
    Create,
}

/// An action the user can run on the selected branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchAction {
    Checkout,
    OpenPr,
    CopyName,
    Delete,
}

impl BranchAction {
    /// The menu items, in display order.
    pub fn all() -> Vec<BranchAction> {
        vec![
            BranchAction::Checkout,
            BranchAction::OpenPr,
            BranchAction::CopyName,
            BranchAction::Delete,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            BranchAction::Checkout => "Checkout",
            BranchAction::OpenPr => "Open Pull Request",
            BranchAction::CopyName => "Copy name",
            BranchAction::Delete => "Delete branch",
        }
    }
}

/// The per-branch action menu overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchMenu {
    pub items: Vec<BranchAction>,
    pub cursor: usize,
    /// The branch the menu was opened on.
    pub name: String,
    /// Whether that branch is the currently checked-out one (delete is refused for it).
    pub is_current: bool,
}

impl BranchMenu {
    pub fn selected(&self) -> BranchAction {
        self.items[self.cursor]
    }
}

/// The pending delete confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDelete {
    pub name: String,
}

/// The whole `gitt branch` screen state.
#[derive(Debug, Clone)]
pub struct BranchState {
    pub load: BranchLoad,
    pub filter: String,
    /// Ranked matches for the current filter (indices into the loaded branch list).
    pub matches: Vec<MatchEntry>,
    /// Index into `matches` of the selected row.
    pub cursor: usize,
    /// Index into `matches` of the first visible row.
    pub top: usize,
    pub mode: BranchMode,
    pub menu: Option<BranchMenu>,
    pub confirm: Option<ConfirmDelete>,
    /// The name being typed in `Create` mode.
    pub create_input: String,
    /// AI summaries keyed by the branch's summary cache key (`branch-<tip>`).
    pub summaries: HashMap<String, SummaryState>,
    /// Per-branch PR status, keyed by branch name. `None` until the background `gh` fetch first
    /// succeeds (so the column is blank rather than falsely "none" while unknown/unavailable).
    pub pr_statuses: Option<HashMap<String, PrStatus>>,
    /// When true, the summary footer grows to show the selected branch's full summary (toggled by `S`).
    pub summary_expanded: bool,
    /// Transient status-line message.
    pub status: Option<String>,
    pub current_branch: String,
    pub main_branch: String,
    /// Terminal size (cols, rows).
    pub size: (u16, u16),
    pub should_quit: bool,
}

impl BranchState {
    pub fn new(current_branch: String, main_branch: String) -> Self {
        BranchState {
            load: BranchLoad::Idle,
            filter: String::new(),
            matches: Vec::new(),
            cursor: 0,
            top: 0,
            mode: BranchMode::List,
            menu: None,
            confirm: None,
            create_input: String::new(),
            summaries: HashMap::new(),
            pr_statuses: None,
            summary_expanded: false,
            status: None,
            current_branch,
            main_branch,
            size: (80, 24),
            should_quit: false,
        }
    }

    /// Height (rows) of the summary footer — identical to `gitt log`'s (shared layout math).
    pub fn summary_panel_rows(&self) -> u16 {
        summary_panel_rows(self.selected_summary(), self.summary_expanded, self.size)
    }

    /// Number of branch rows visible given the current terminal height, accounting for the footer.
    pub fn viewport_rows(&self) -> usize {
        (self
            .size
            .1
            .saturating_sub(CHROME_ROWS + self.summary_panel_rows()))
        .max(1) as usize
    }

    /// The loaded branches (empty slice if not loaded).
    pub fn branches(&self) -> &[Branch] {
        match &self.load {
            BranchLoad::Loaded(branches) => branches,
            _ => &[],
        }
    }

    /// The currently selected branch, if any.
    pub fn selected(&self) -> Option<&Branch> {
        let entry = self.matches.get(self.cursor)?;
        self.branches().get(entry.commit_idx)
    }

    /// Name of the selected branch, if any.
    pub fn selected_name(&self) -> Option<String> {
        self.selected().map(|b| b.name.clone())
    }

    /// The summary cache key for the selected branch, if any (derived from its tip SHA).
    pub fn selected_summary_key(&self) -> Option<String> {
        self.selected().map(|b| summary_key(&b.tip))
    }

    /// The AI-summary state for the selected branch, if any is recorded.
    pub fn selected_summary(&self) -> Option<&SummaryState> {
        self.summaries.get(&self.selected_summary_key()?)
    }

    /// The PR status recorded for branch `name`, if the statuses have loaded and it has a PR.
    pub fn pr_status(&self, name: &str) -> Option<PrStatus> {
        self.pr_statuses.as_ref()?.get(name).copied()
    }

    /// Recompute matches for the current filter, then re-clamp cursor and scroll.
    pub fn recompute_matches(&mut self) {
        self.matches = fuzzy::filter_items(self.branches(), &self.filter, |b| b.haystack.as_str());
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
        let max_top = self.matches.len().saturating_sub(rows);
        if self.top > max_top {
            self.top = max_top;
        }
    }
}
