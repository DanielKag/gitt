//! `gitt` — an AI-first git TUI client.
//!
//! Architecture: **Functional Core, Imperative Shell** (see `CLAUDE.md`).
//! `domain`, `parse`, `fuzzy`, `state`, `ui` are pure (no I/O). All side effects live behind the
//! traits in `ports` and are driven by `runtime`.

pub mod domain;
pub mod fuzzy;
pub mod parse;
pub mod ports;
pub mod runtime;
pub mod state;
pub mod ui;
