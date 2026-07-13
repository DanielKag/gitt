//! Pure domain types and functions. No I/O.

pub mod branch;
pub mod commit;
pub mod diff;
pub mod diff_tool;
pub mod main_branch;
pub mod status;
pub mod summary;
pub mod text;
pub mod time;
pub mod url;

pub use branch::{Branch, PrStatus};
pub use commit::{Commit, Ref, View};
pub use diff::{DiffFile, DiffScope};
pub use diff_tool::DiffTool;
pub use status::{DiffKind, StatusEntry};
