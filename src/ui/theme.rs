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
/// Inline `code` spans in AI summaries: light text on a subtle grey block, like markdown code.
pub fn code() -> Style {
    Style::new().fg(Color::Indexed(252)).bg(Color::Indexed(237))
}
pub fn menu_selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
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
