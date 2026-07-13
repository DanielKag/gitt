//! Shared UI building blocks used by every `gitt` screen, so they look and behave the same.
//!
//! If a screen needs a centered overlay, an action menu, or a bordered preview pane, it uses these —
//! it does not roll its own (see CLAUDE.md, "one tool, one feel").

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Wrap};

use super::theme;

/// The one-character AI-summary marker shown beside an entry whose summary is cached (log & branch).
pub const AI_BADGE: &str = "✦";

/// The one-column AI-summary marker span for a list row: the [`AI_BADGE`] glyph when `summarized`,
/// otherwise a blank of the same width so rows stay aligned. Shared by the log and branch lists.
pub fn ai_badge_span(summarized: bool) -> Span<'static> {
    if summarized {
        Span::styled(AI_BADGE, theme::ai_badge())
    } else {
        Span::raw(" ")
    }
}

/// A centered rectangle of at most `width`×`height` within `area`.
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Dim every cell in `area` (used to darken the base screen behind a modal overlay so the overlay
/// stands out). Applied before the overlay's own `Clear`, which resets the modal's own cells back to
/// full brightness. Shared by every screen so modals feel the same everywhere.
pub fn dim_area(frame: &mut Frame, area: Rect) {
    let buf = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].modifier.insert(Modifier::DIM);
        }
    }
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

/// Render a bordered pane whose (already-styled) text wraps to the pane width. Used for the log
/// screen's AI-summary panel and its expanded modal.
pub fn wrapped_pane(frame: &mut Frame, area: Rect, title: &str, content: Text<'static>) {
    let block = Block::bordered().title(title.to_string());
    frame.render_widget(
        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// Split `text` into spans, styling the query's matched substrings with the search-match highlight
/// (LOG-25) and everything else with `base`. With no query (or no match) the whole field is one
/// `base` span, so an unfiltered list renders exactly as before. Shared by the log and branch lists.
pub fn highlight(text: &str, query: &str, base: ratatui::style::Style) -> Vec<Span<'static>> {
    let ranges = crate::fuzzy::match_ranges(text, query);
    if ranges.is_empty() {
        return vec![Span::styled(text.to_string(), base)];
    }
    let chars: Vec<char> = text.chars().collect();
    let hl = base.patch(theme::search_match());
    let mut spans = Vec::new();
    let mut pos = 0;
    for (s, e) in ranges {
        if pos < s {
            spans.push(Span::styled(chars[pos..s].iter().collect::<String>(), base));
        }
        spans.push(Span::styled(chars[s..e].iter().collect::<String>(), hl));
        pos = e;
    }
    if pos < chars.len() {
        spans.push(Span::styled(chars[pos..].iter().collect::<String>(), base));
    }
    spans
}

/// Cut `line` so it ends with a `…` marker within `width` columns (used when a teaser overflows).
pub fn with_ellipsis(line: &str, width: usize) -> String {
    let keep = width.saturating_sub(1);
    let mut out: String = line.chars().take(keep).collect();
    out.push('…');
    out
}

/// Split text into `(segment, is_code)` runs on markdown-style backtick pairs, stripping the
/// backticks. Text with no backticks — or an odd (unbalanced) number — is returned as a single
/// non-code run left verbatim, so a stray backtick in a partial/streaming summary never mis-styles.
pub fn split_code_spans(text: &str) -> Vec<(String, bool)> {
    let ticks = text.bytes().filter(|&b| b == b'`').count();
    if ticks == 0 || ticks % 2 != 0 {
        return vec![(text.to_string(), false)];
    }
    text.split('`')
        .enumerate()
        .filter(|(_, part)| !part.is_empty())
        .map(|(i, part)| (part.to_string(), i % 2 == 1))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{split_code_spans, with_ellipsis};

    #[test]
    fn with_ellipsis_fits_in_width() {
        assert_eq!(with_ellipsis("hello world", 6), "hello…");
        assert!(with_ellipsis("hello world", 6).chars().count() <= 6);
    }

    #[test]
    fn no_backticks_is_one_plain_run() {
        assert_eq!(
            split_code_spans("just prose here"),
            vec![("just prose here".to_string(), false)]
        );
    }

    #[test]
    fn backtick_pairs_become_code_runs_without_the_backticks() {
        assert_eq!(
            split_code_spans("bumps `package.json` to `1.2.0` now"),
            vec![
                ("bumps ".to_string(), false),
                ("package.json".to_string(), true),
                (" to ".to_string(), false),
                ("1.2.0".to_string(), true),
                (" now".to_string(), false),
            ]
        );
    }

    #[test]
    fn unbalanced_backtick_left_verbatim() {
        // A half-streamed summary with one open backtick must not style the tail.
        assert_eq!(
            split_code_spans("updates the `yarn.lock"),
            vec![("updates the `yarn.lock".to_string(), false)]
        );
    }

    #[test]
    fn leading_code_span() {
        assert_eq!(
            split_code_spans("`main.rs` changed"),
            vec![
                ("main.rs".to_string(), true),
                (" changed".to_string(), false),
            ]
        );
    }
}
