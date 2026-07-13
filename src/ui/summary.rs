//! The shared AI-summary footer, used identically by `gitt log` (per commit) and `gitt branch` (per
//! branch). Given the selected item's [`SummaryState`] and whether the footer is expanded, it renders
//! the bordered "ai summary" panel: the hint, the "summarizing…" placeholder, streaming/ready prose
//! (with markdown-style `code` spans, backticks stripped) that is cut with a `…` teaser when it
//! overflows, or the failed state. Factored out so both screens look and behave the same.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};

use super::components::{split_code_spans, with_ellipsis, wrapped_pane};
use super::theme;
use crate::domain::text::wrap_words;
use crate::state::SummaryState;

/// Render the AI-summary footer for `summary` into `area`.
pub fn render_footer(
    frame: &mut Frame,
    area: Rect,
    summary: Option<&SummaryState>,
    expanded: bool,
) {
    let width = area.width.saturating_sub(2) as usize; // inside the border
    let rows = area.height.saturating_sub(2) as usize; // visible content lines

    let (content, overflow) = match summary {
        Some(SummaryState::Ready(text)) => teaser(text, "", width, rows),
        Some(SummaryState::Generating(buf)) if buf.trim().is_empty() => (
            Text::from(Line::styled("summarizing with ollama…", theme::dim())),
            false,
        ),
        Some(SummaryState::Generating(buf)) => teaser(buf, "▌", width, rows),
        Some(SummaryState::Failed(error)) => (
            Text::from(Line::styled(
                format!("summary failed: {error}"),
                theme::dim(),
            )),
            false,
        ),
        // Missing, or not looked up yet.
        _ => (
            Text::from(Line::styled("press s for an AI summary", theme::dim())),
            false,
        ),
    };

    let title = if expanded {
        "ai summary · S: minimize"
    } else if overflow {
        "ai summary · S: expand"
    } else {
        "ai summary"
    };
    wrapped_pane(frame, area, title, content);
}

/// Build the footer content for a summary: if it fits in `rows` lines, the full styled line; if not,
/// the first `rows` wrapped lines (plain) with a trailing `…`. Returns `(content, overflowed)`.
fn teaser(text: &str, suffix: &str, width: usize, rows: usize) -> (Text<'static>, bool) {
    let plain = format!("{}{}", text.replace('`', ""), suffix);
    let lines = wrap_words(&plain, width);
    if rows == 0 || lines.len() <= rows {
        (Text::from(summary_line(text, suffix)), false)
    } else {
        let mut shown: Vec<Line> = lines[..rows].iter().map(|l| Line::raw(l.clone())).collect();
        let last = with_ellipsis(&lines[rows - 1], width);
        shown[rows - 1] = Line::styled(last, theme::subject());
        (Text::from(shown), true)
    }
}

/// Build a summary line: normal prose in the subject style, `code` spans in the code style, with an
/// optional trailing marker (e.g. a streaming cursor).
fn summary_line(text: &str, suffix: &str) -> Line<'static> {
    let mut spans: Vec<Span> = split_code_spans(text)
        .into_iter()
        .map(|(seg, is_code)| {
            let style = if is_code {
                theme::code()
            } else {
                theme::subject()
            };
            Span::styled(seg, style)
        })
        .collect();
    if !suffix.is_empty() {
        spans.push(Span::styled(suffix.to_string(), theme::subject()));
    }
    Line::from(spans)
}
