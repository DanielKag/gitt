//! The `gitt status` screen state and its pure helpers (selection, scrolling). No I/O.
//!
//! Mirrors the shape of the log [`AppState`](crate::state::AppState) — same selection/scroll model,
//! same overlay-driven action menu — so both screens behave the same for the user.

use crate::domain::StatusEntry;

/// Rows of chrome around the file list: header (1) + status/help (1).
pub const STATUS_CHROME_ROWS: u16 = 2;

/// Load state of the working-tree status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StatusLoad {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<StatusEntry>),
    Failed(String),
}

/// Input focus / interaction mode for the status screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMode {
    /// Browsing the file list.
    List,
    /// The per-file action menu is open.
    Menu,
    /// The discard confirmation is open.
    Confirm,
}

/// State of the file diff-preview pane, keyed by path (each path is unique in the flat list).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FilePreview {
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

/// An action the user can run on the selected file (mirrors the log action menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Stage,
    Unstage,
    Discard,
    CopyPath,
}

impl FileAction {
    pub fn label(self) -> &'static str {
        match self {
            FileAction::Stage => "Stage",
            FileAction::Unstage => "Unstage",
            FileAction::Discard => "Discard changes",
            FileAction::CopyPath => "Copy path",
        }
    }
}

/// The per-file action menu overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMenu {
    pub items: Vec<FileAction>,
    pub cursor: usize,
    pub path: String,
    pub untracked: bool,
}

impl FileMenu {
    pub fn selected(&self) -> FileAction {
        self.items[self.cursor]
    }
}

/// The pending discard confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDiscard {
    pub path: String,
    pub untracked: bool,
}

/// The whole `gitt status` screen state.
#[derive(Debug, Clone)]
pub struct StatusState {
    pub load: StatusLoad,
    /// Index into the entry list of the selected row.
    pub cursor: usize,
    /// Index of the first visible row.
    pub top: usize,
    pub mode: StatusMode,
    pub preview_open: bool,
    pub preview: FilePreview,
    pub menu: Option<FileMenu>,
    pub confirm: Option<ConfirmDiscard>,
    /// Transient status-line message.
    pub status: Option<String>,
    pub branch: String,
    /// Terminal size (cols, rows).
    pub size: (u16, u16),
    pub should_quit: bool,
}

impl StatusState {
    pub fn new(branch: String) -> Self {
        StatusState {
            load: StatusLoad::Idle,
            cursor: 0,
            top: 0,
            mode: StatusMode::List,
            preview_open: false,
            preview: FilePreview::Idle,
            menu: None,
            confirm: None,
            status: None,
            branch,
            size: (80, 24),
            should_quit: false,
        }
    }

    /// Number of file rows visible given the current terminal height.
    pub fn viewport_rows(&self) -> usize {
        (self.size.1.saturating_sub(STATUS_CHROME_ROWS)).max(1) as usize
    }

    /// The loaded entries (empty slice if not loaded).
    pub fn entries(&self) -> &[StatusEntry] {
        match &self.load {
            StatusLoad::Loaded(entries) => entries,
            _ => &[],
        }
    }

    /// The currently selected entry, if any.
    pub fn selected(&self) -> Option<&StatusEntry> {
        self.entries().get(self.cursor)
    }

    /// Path of the selected entry, if any.
    pub fn selected_path(&self) -> Option<String> {
        self.selected().map(|e| e.path.clone())
    }

    /// Keep the cursor within the entry list.
    pub fn clamp_cursor(&mut self) {
        let len = self.entries().len();
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
        let max_top = self.entries().len().saturating_sub(rows);
        if self.top > max_top {
            self.top = max_top;
        }
    }
}
