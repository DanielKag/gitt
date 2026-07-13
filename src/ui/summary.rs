//! The shared AI-summary footer, used identically by `gitt log` (per commit) and `gitt branch` (per
//! branch). Given the selected item's [`SummaryState`] and whether the footer is expanded, it renders
//! the bordered "ai summary" panel: the hint, the "summarizing…" placeholder, streaming/ready prose
//! (with markdown-style `code` spans, backticks stripped) that is cut with a `…` teaser when it
//! overflows, or the failed state. Factored out so both screens look and behave the same.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};

use super::components::{split_code_spans, wrapped_pane};
use super::theme;
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

/// Build the footer content for a summary: word-wrap it into styled lines (markdown `code` spans kept
/// styled, backticks stripped). If it fits in `rows` lines, show them all; if not, show the first
/// `rows` with a trailing `…` on the last. Returns `(content, overflowed)`. Both the fits and overflow
/// paths style code spans identically — so a teaser looks the same as the expanded view, just cut.
fn teaser(text: &str, suffix: &str, width: usize, rows: usize) -> (Text<'static>, bool) {
    let mut runs = split_code_spans(text);
    if !suffix.is_empty() {
        runs.push((suffix.to_string(), false));
    }
    let lines = wrap_styled(&runs, width);

    if rows == 0 || lines.len() <= rows {
        let content: Vec<Line> = lines.into_iter().map(Line::from).collect();
        (Text::from(content), false)
    } else {
        let mut shown: Vec<Line> = lines[..rows].iter().cloned().map(Line::from).collect();
        shown[rows - 1] = Line::from(ellipsize_spans(&lines[rows - 1], width));
        (Text::from(shown), true)
    }
}

/// Word-wrap styled `(segment, is_code)` runs into lines of spans, using the same greedy algorithm as
/// [`crate::domain::text::wrap_words`] (so the line count agrees with the footer height math) while
/// preserving each character's `code`/prose styling across wrap points.
fn wrap_styled(runs: &[(String, bool)], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);

    // Split into paragraphs on '\n' (like `wrap_words`), keeping each char's code flag.
    let mut paragraphs: Vec<Vec<(char, bool)>> = vec![Vec::new()];
    for (seg, is_code) in runs {
        for ch in seg.chars() {
            if ch == '\n' {
                paragraphs.push(Vec::new());
            } else {
                paragraphs.last_mut().unwrap().push((ch, *is_code));
            }
        }
    }

    let mut out: Vec<Vec<(char, bool)>> = Vec::new();
    for para in &paragraphs {
        // Split into whitespace-delimited words (each char carries its style).
        let mut words: Vec<Vec<(char, bool)>> = Vec::new();
        let mut cur: Vec<(char, bool)> = Vec::new();
        for &(ch, code) in para {
            if ch.is_whitespace() {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push((ch, code));
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }

        // Greedy line fill, hard-breaking any word too long to ever fit.
        let mut line: Vec<(char, bool)> = Vec::new();
        for mut word in words {
            while word.len() > width {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                out.push(word.iter().take(width).copied().collect());
                word = word.into_iter().skip(width).collect();
            }
            let wlen = word.len();
            if line.is_empty() {
                line = word;
            } else if line.len() + 1 + wlen <= width {
                line.push((' ', false));
                line.extend(word);
            } else {
                out.push(std::mem::take(&mut line));
                line = word;
            }
        }
        out.push(line);
    }

    out.iter().map(|l| chars_to_spans(l)).collect()
}

/// Collapse a wrapped line's `(char, is_code)` cells into styled spans, merging adjacent cells that
/// share a style so `code` runs render as one contiguous block.
fn chars_to_spans(line: &[(char, bool)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<bool> = None;
    for &(ch, code) in line {
        if cur != Some(code) {
            if let Some(c) = cur {
                spans.push(span_for(std::mem::take(&mut buf), c));
            }
            cur = Some(code);
        }
        buf.push(ch);
    }
    if let Some(c) = cur {
        spans.push(span_for(buf, c));
    }
    spans
}

fn span_for(text: String, is_code: bool) -> Span<'static> {
    let style = if is_code {
        theme::code()
    } else {
        theme::subject()
    };
    Span::styled(text, style)
}

/// Truncate a styled line to `width` columns ending in a `…`, preserving the spans' styles up to the
/// cut. Used on the last visible line of an overflowing teaser.
fn ellipsize_spans(spans: &[Span<'static>], width: usize) -> Vec<Span<'static>> {
    let keep = width.saturating_sub(1);
    let mut out = Vec::new();
    let mut used = 0usize;
    for sp in spans {
        if used >= keep {
            break;
        }
        let count = sp.content.chars().count();
        if used + count <= keep {
            out.push(sp.clone());
            used += count;
        } else {
            let take = keep - used;
            out.push(Span::styled(
                sp.content.chars().take(take).collect::<String>(),
                sp.style,
            ));
            break;
        }
    }
    out.push(Span::styled("…".to_string(), theme::subject()));
    out
}
