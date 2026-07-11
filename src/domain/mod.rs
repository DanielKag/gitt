//! Pure domain types and functions. No I/O.

pub mod commit;
pub mod diff;
pub mod main_branch;
pub mod status;
pub mod summary;
pub mod time;
pub mod url;

pub use commit::{Commit, Ref, View};
pub use diff::{DiffFile, DiffScope};
pub use status::{DiffKind, StatusEntry};
