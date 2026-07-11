//! Shared UI building blocks used by every `gitt` screen, so they look and behave the same.
//!
//! If a screen needs a centered overlay, an action menu, or a bordered preview pane, it uses these —
//! it does not roll its own (see CLAUDE.md, "one tool, one feel").

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use super::theme;

/// A centered rectangle of at most `width`×`height` within `area`.
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Render a centered, bordered action menu over `body`: one item per line, the item at `cursor`
/// highlighted. Shared by the log commit menu and the status file menu.
pub fn overlay_menu(frame: &mut Frame, body: Rect, title: &str, labels: &[&str], cursor: usize) {
    let width = labels
        .iter()
        .map(|l| l.len())
        .max()
        .unwrap_or(10)
        .max(title.len())
        + 4;
    let height = labels.len() + 2;
    let area = centered_rect(body, width as u16, height as u16);

    let items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mut item = ListItem::new(format!(" {label}"));
            if i == cursor {
                item = item.style(theme::menu_selected());
            }
            item
        })
        .collect();

    frame.render_widget(Clear, area);
    frame.render_widget(
        List::new(items).block(Block::bordered().title(title.to_string())),
        area,
    );
}

/// Render a bordered diff-preview pane with the given already-resolved text.
pub fn preview_pane(frame: &mut Frame, area: Rect, title: &str, text: &str) {
    let block = Block::bordered().title(title.to_string());
    frame.render_widget(Paragraph::new(text.to_string()).block(block), area);
}

/// Truncate a string to at most `max` chars, appending `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
