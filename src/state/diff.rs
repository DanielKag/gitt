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
    /// When true, the diff pane takes most of the height (90%, list shrinks to 10%) so a large diff
    /// is easier to read while the current file stays visible. Toggled with `f`.
    pub expanded: bool,
    pub preview: DiffPreview,
    /// First visible line of the diff pane content (vertical scroll offset within the diff).
    pub preview_scroll: u16,
    pub menu: Option<DiffMenu>,
    /// Transient status-line message (shown on the header bar, not the keymap legend).
    pub status: Option<String>,
    pub status_is_error: bool,
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
            expanded: false,
            preview: DiffPreview::Idle,
            preview_scroll: 0,
            menu: None,
            status: None,
            status_is_error: false,
            main_branch,
            size: (80, 24),
            should_quit: false,
        }
    }

    /// Set an informational status-line message (rendered dim on the header bar).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
        self.status_is_error = false;
    }

    /// Set an error status-line message (rendered in dominant red on the header bar).
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
        self.status_is_error = true;
    }

    /// The diff pane's share of the body height (percent). The pane sits *below* the file list:
    /// closed → 0, open → 50, expanded (`f`) → 90 (list shrinks to 10% but stays visible).
    fn diff_pct(&self) -> u16 {
        if !self.preview_open {
            0
        } else if self.expanded {
            90
        } else {
            50
        }
    }

    /// Rows of the body (between header and status line).
    fn body_rows(&self) -> u16 {
        self.size.1.saturating_sub(DIFF_CHROME_ROWS)
    }

    /// Number of file rows visible: the body minus the diff pane below it.
    pub fn viewport_rows(&self) -> usize {
        let body = self.body_rows();
        (body.saturating_sub(body * self.diff_pct() / 100)).max(1) as usize
    }

    /// Inner width (columns) of the diff pane. Because the pane spans the full terminal width (it is
    /// stacked below the list, not beside it), this is the whole width minus the pane border — so a
    /// wide terminal gives the diff tool room for a side-by-side layout.
    pub fn preview_width(&self) -> u16 {
        self.size.0.saturating_sub(2).max(1)
    }

    /// Inner height (rows) of the diff pane content, minus its border — how many diff lines are
    /// visible, used to clamp scrolling.
    pub fn preview_height(&self) -> u16 {
        let body = self.body_rows();
        (body * self.diff_pct() / 100).saturating_sub(2).max(1)
    }

    /// Number of lines in the currently-loaded diff text (for scroll clamping); 0 if not ready.
    pub fn preview_lines(&self) -> usize {
        match &self.preview {
            DiffPreview::Ready { text, .. } => text.lines().count(),
            _ => 0,
        }
    }

    /// The largest valid scroll offset so the last diff line can reach the top of the pane.
    pub fn max_preview_scroll(&self) -> u16 {
        (self.preview_lines() as u16).saturating_sub(self.preview_height())
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
