//! Convert a byte/UTF-8 string carrying ANSI SGR escapes into styled ratatui [`Text`].
//!
//! This is the seam that lets `gitt` render a third-party diff tool's colored output (delta,
//! difftastic, git-split-diffs — see [`crate::domain::diff_tool`]) inside a `ratatui` pane: the tool
//! writes ANSI to a pipe, and this pure function turns those escapes into `Line`/`Span` styles. The
//! blocker recorded for the colorized preview (LOG-09) was precisely "no ANSI→spans parser"; this is
//! it. Pure and deterministic, so it is unit-tested against canned ANSI fixtures.

use ansi_to_tui::IntoText;
use ratatui::text::Text;

/// Parse `ansi` (which may contain SGR color/style escapes) into styled [`Text`]. On the rare parse
/// error, fall back to the raw text as a single unstyled block so a diff never fails to render.
pub fn ansi_to_text(ansi: &str) -> Text<'static> {
    match ansi.into_text() {
        Ok(text) => owned(text),
        Err(_) => Text::raw(ansi.to_string()),
    }
}

/// `into_text()` borrows from the input; clone into an owned `Text<'static>` so it can live in state
/// / be returned past the borrow.
fn owned(text: Text<'_>) -> Text<'static> {
    Text::from(
        text.lines
            .into_iter()
            .map(|line| {
                let spans = line
                    .spans
                    .into_iter()
                    .map(|s| ratatui::text::Span::styled(s.content.into_owned(), s.style))
                    .collect::<Vec<_>>();
                ratatui::text::Line::from(spans).style(line.style)
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn plain_text_has_no_styling() {
        let t = ansi_to_text("hello\nworld");
        assert_eq!(t.lines.len(), 2);
        assert_eq!(t.lines[0].spans[0].content, "hello");
    }

    #[test]
    fn sgr_colors_become_span_styles() {
        // ESC[32m = green foreground, ESC[0m = reset.
        let t = ansi_to_text("\x1b[32m+added\x1b[0m");
        let span = &t.lines[0].spans[0];
        assert_eq!(span.content, "+added");
        assert_eq!(span.style.fg, Some(Color::Green));
    }

    #[test]
    fn red_and_reset_split_into_spans() {
        let t = ansi_to_text("\x1b[31m-removed\x1b[0m tail");
        let line = &t.lines[0];
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
        // The text after reset is present (styling of later spans is parser-defined).
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "-removed tail");
    }

    #[test]
    fn malformed_escape_falls_back_to_raw() {
        // A lone ESC with no valid sequence should not panic; content is preserved.
        let t = ansi_to_text("before\x1bafter");
        let joined: String = t
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(joined.contains("before") && joined.contains("after"));
    }
}
