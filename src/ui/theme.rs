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
pub fn menu_selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
