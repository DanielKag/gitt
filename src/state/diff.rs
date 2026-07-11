//! The `gitt diff` screen state and its pure helpers (scope, selection, scrolling). No I/O.
//!
//! Mirrors the shape of the log [`AppState`](crate::state::AppState) and the status
//! [`StatusState`](crate::state::StatusState): same selection/scroll model, same overlay-driven
//! action menu, and a per-scope load cache exactly like the log's per-view cache — so all three
//! screens behave the same for the user.

use std::collections::HashMap;

use crate::domain::{DiffFile, DiffScope};

/// Rows of chrome around the file list: header (scope tabs, 1) + status/help (1).
pub const DIFF_CHROME_ROWS: u16 = 2;

/// Load state of one scope's changed-file list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiffLoad {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<DiffFile>),
    Failed(String),
}

/// Input focus / interaction mode for the diff screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// Browsing the file list.
    List,
    /// The per-file action menu is open.
    Menu,
}

/// State of the diff pane, keyed by path (each path is unique within a scope's flat list).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiffPreview {
    #[default]
    Idle,
    Loading(String),
    Ready {
        path: String,
        text: String,
    },
    Failed {
        path: String,
        error: String,
    },
}

/// A read-only action the user can run on the selected file (mirrors the log/status action menus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAction {
    CopyPath,
    CopyDiff,
}

impl DiffAction {
    pub fn label(self) -> &'static str {
        match self {
            DiffAction::CopyPath => "Copy path",
            DiffAction::CopyDiff => "Copy diff",
        }
    }
}

/// The per-file action menu overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffMenu {
    pub items: Vec<DiffAction>,
    pub cursor: usize,
    pub path: String,
}

impl DiffMenu {
    pub fn selected(&self) -> DiffAction {
        self.items[self.cursor]
    }
}

/// The whole `gitt diff` screen state.
#[derive(Debug, Clone)]
pub struct DiffState {
    /// The scope currently shown.
    pub scope: DiffScope,
    /// Per-scope file-list cache (each scope loads once, like the log's per-view cache).
    pub loads: HashMap<DiffScope, DiffLoad>,
    /// Index into the active scope's file list of the selected row.
    pub cursor: usize,
    /// Index of the first visible row.
    pub top: usize,
    pub mode: DiffMode,
    pub preview_open: bool,
    pub preview: DiffPreview,
    pub menu: Option<DiffMenu>,
    /// Transient status-line message.
    pub status: Option<String>,
    /// Resolved main-branch name, used for the `vs <main>` tab and empty state.
    pub main_branch: String,
    /// Terminal size (cols, rows).
    pub size: (u16, u16),
    pub should_quit: bool,
}

impl DiffState {
    pub fn new(main_branch: String) -> Self {
        DiffState {
            scope: DiffScope::Unstaged,
            loads: HashMap::new(),
            cursor: 0,
            top: 0,
            mode: DiffMode::List,
            // The diff pane is the point of this screen, so it starts open.
            preview_open: true,
            preview: DiffPreview::Idle,
            menu: None,
            status: None,
            main_branch,
            size: (80, 24),
            should_quit: false,
        }
    }

    /// Number of file rows visible given the current terminal height.
    pub fn viewport_rows(&self) -> usize {
        (self.size.1.saturating_sub(DIFF_CHROME_ROWS)).max(1) as usize
    }

    /// The active scope's loaded files (empty slice if not loaded).
    pub fn files(&self) -> &[DiffFile] {
        match self.loads.get(&self.scope) {
            Some(DiffLoad::Loaded(files)) => files,
            _ => &[],
        }
    }

    /// The currently selected file, if any.
    pub fn selected(&self) -> Option<&DiffFile> {
        self.files().get(self.cursor)
    }

    /// Path of the selected file, if any.
    pub fn selected_path(&self) -> Option<String> {
        self.selected().map(|f| f.path.clone())
    }

    /// Keep the cursor within the active scope's file list.
    pub fn clamp_cursor(&mut self) {
        let len = self.files().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// Keep the selected row visible by adjusting `top`.
    pub fn clamp_scroll(&mut self) {
        let rows = self.viewport_rows();
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
        let max_top = self.files().len().saturating_sub(rows);
        if self.top > max_top {
            self.top = max_top;
        }
    }
}
