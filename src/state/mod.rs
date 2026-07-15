//! The pure application state machine. No I/O.
//!
//! `reducer::update(&mut AppState, Event) -> Vec<Effect>` is the single entry point: it folds an
//! input event into the state and returns the side effects the shell must perform. Everything a
//! keystroke does lives here and is unit-testable without a terminal or a real git.

pub mod branch;
pub mod branch_reducer;
pub mod diff;
pub mod diff_reducer;
pub mod effect;
pub mod event;
pub mod model;
pub mod reducer;
pub mod status;
pub mod status_reducer;

pub use branch::{
    BranchAction, BranchLoad, BranchMenu, BranchMode, BranchState,
    ConfirmDelete as ConfirmDeleteBranch,
};
pub use branch_reducer::update_branch;
pub use diff::{DiffAction, DiffLoad, DiffMenu, DiffMode, DiffPreview, DiffState};
pub use diff_reducer::update_diff;
pub use effect::Effect;
pub use event::Event;
pub use model::{ActionMenu, AppState, Load, MenuAction, Mode, PreviewState, SummaryState};
pub use reducer::update;
pub use status::{
    CommitEditor, ConfirmDiscard, FileAction, FileMenu, FilePreview, PendingCommit, StatusLoad,
    StatusMode, StatusState,
};
pub use status_reducer::update_status;
