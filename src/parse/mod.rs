//! Pure parsers for git plumbing output. No I/O — feed captured strings in, get typed data out.

pub mod decorations;
pub mod log;
pub mod remote;

pub use log::{FIELD_SEP, RECORD_SEP, parse_log};
