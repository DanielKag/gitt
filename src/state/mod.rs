//! The pure application state machine. No I/O.
//!
//! `reducer::update(&mut AppState, Event) -> Vec<Effect>` is the single entry point: it folds an
//! input event into the state and returns the side effects the shell must perform. Everything a
//! keystroke does lives here and is unit-testable without a terminal or a real git.

pub mod effect;
pub mod event;
pub mod model;
pub mod reducer;

pub use effect::Effect;
pub use event::Event;
pub use model::{ActionMenu, AppState, Load, MenuAction, Mode, PreviewState};
pub use reducer::update;
