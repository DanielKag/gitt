//! Shared e2e helpers. Living under `tests/common/` (not a top-level `tests/*.rs`) means cargo does
//! not compile it as its own test binary.

pub mod fixture;
pub mod tui_tester;

/// The unix timestamp the tests pin "now" to (via `GITT_NOW`), so relative dates are deterministic.
pub const NOW: i64 = 1_700_000_000;

pub const DAY: i64 = 24 * 60 * 60;
