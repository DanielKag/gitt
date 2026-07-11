//! Pure parsers for git plumbing output. No I/O — feed captured strings in, get typed data out.

pub mod decorations;
pub mod log;
pub mod remote;
pub mod status;

pub use log::{FIELD_SEP, RECORD_SEP, parse_log};
pub use status::parse_status;
