//! Colors and styles for the log UI, kept in one place.

use ratatui::style::{Color, Modifier, Style};

pub fn hash() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::DIM)
}
pub fn date() -> Style {
    Style::new().fg(Color::Green)
}
pub fn author() -> Style {
    Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
}
pub fn subject() -> Style {
    Style::new()
}
pub fn refs() -> Style {
    Style::new().fg(Color::Yellow)
}
pub fn selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
pub fn active_view() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
pub fn inactive_view() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}
pub fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}
/// A dominant error message on the status line (e.g. a failed checkout): red + bold so it's hard to
/// miss and still legible under `NO_COLOR`.
pub fn error() -> Style {
    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
}
/// Inline `code` spans in AI summaries: light text on a subtle grey block, like markdown code.
pub fn code() -> Style {
    Style::new().fg(Color::Indexed(252)).bg(Color::Indexed(237))
}
pub fn menu_selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
/// One-character marker shown beside a log commit / branch that already has an AI summary cached.
/// Magenta + bold so it reads as a distinct "AI" signal and still shows under `NO_COLOR`.
pub fn ai_badge() -> Style {
    Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
}
/// The currently checked-out branch in `gitt branch` (green + bold, like git's own `* branch`).
pub fn current_branch() -> Style {
    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
}
/// Colour for a branch's PR-status badge (GitHub's own palette: open green, merged magenta/purple,
/// closed red, draft grey).
pub fn pr_status(status: crate::domain::PrStatus) -> Style {
    use crate::domain::PrStatus;
    match status {
        PrStatus::Open => Style::new().fg(Color::Green),
        PrStatus::Merged => Style::new().fg(Color::Magenta),
        PrStatus::Closed => Style::new().fg(Color::Red),
        PrStatus::Draft => Style::new().fg(Color::Gray),
    }
}
/// The characters of a list row that matched the active search query (LOG-25). Black-on-yellow like a
/// classic find highlight; bold so it still reads on a selected (reversed) row and under `NO_COLOR`.
pub fn search_match() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

// --- gitt status badges (git's own convention: green = staged, red = unstaged/untracked) ---------
pub fn staged() -> Style {
    Style::new().fg(Color::Green)
}
pub fn unstaged() -> Style {
    Style::new().fg(Color::Red)
}
pub fn untracked() -> Style {
    Style::new().fg(Color::Red).add_modifier(Modifier::DIM)
}

// --- gitt diff change badges (git's diffstat convention: added green, deleted red, modified yellow)
pub fn added() -> Style {
    Style::new().fg(Color::Green)
}
pub fn deleted() -> Style {
    Style::new().fg(Color::Red)
}
pub fn modified() -> Style {
    Style::new().fg(Color::Yellow)
}
pub fn renamed() -> Style {
    Style::new().fg(Color::Cyan)
}
