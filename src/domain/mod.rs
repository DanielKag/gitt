//! Pure domain types and functions. No I/O.

pub mod commit;
pub mod main_branch;
pub mod time;
pub mod url;

pub use commit::{Commit, Ref, View};
