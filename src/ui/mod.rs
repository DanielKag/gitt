//! Pure rendering: `draw(frame, state)` reads only `&AppState` and writes only into the frame.
//! No I/O, no port calls — so it is exercised directly with ratatui's `TestBackend`.

pub mod components;
pub mod diff;
pub mod status;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::domain::text::wrap_words;
use crate::domain::{Commit, Ref, View};
use crate::state::{AppState, Mode, PreviewState, SummaryState};

pub use diff::draw_diff;
pub use status::draw_status;

use components::{
    overlay_menu, preview_pane, split_code_spans, truncate, with_ellipsis, wrapped_pane,
};

const DATE_WIDTH: usize = 13;
const AUTHOR_WIDTH: usize = 16;

/// Render the whole log UI for the current state.
pub fn draw(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // header (view tabs)
            Constraint::Length(1), // search bar
            Constraint::Min(1),    // body (list [+ preview])
            Constraint::Length(1), // status
        ],
    )
    .split(area);

    render_header(frame, chunks[0], state);
    render_search(frame, chunks[1], state);
    render_body(frame, chunks[2], state);
    render_status(frame, chunks[3], state);

    if state.mode == Mode::Menu {
        render_menu(frame, chunks[2], state);
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let head = format!(" {} (HEAD) ", state.current_branch);
    let base = format!(" origin/{} ", state.main_branch);
    let (head_style, base_style) = match state.view {
        View::LocalHead => (theme::active_view(), theme::inactive_view()),
        View::OriginMain => (theme::inactive_view(), theme::active_view()),
    };
    let line = Line::from(vec![
        Span::styled(head, head_style),
        Span::raw("  "),
        Span::styled(base, base_style),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_search(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut spans = vec![Span::styled("Search: ", theme::dim())];
    spans.push(Span::raw(state.filter.clone()));
    if state.mode == Mode::Search {
        spans.push(Span::raw("█"));
    }
    let count = format!("  ({} matches)", state.matches.len());
    spans.push(Span::styled(count, theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &AppState) {
    // Reserve the bottom rows for the AI summary footer (taller when expanded); rest is list [+ preview].
    let rows = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(1),
            Constraint::Length(state.summary_panel_rows()),
        ],
    )
    .split(area);
    let main = rows[0];

    let (list_area, preview_area) = if state.preview_open {
        let cols = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(main);
        (cols[0], Some(cols[1]))
    } else {
        (main, None)
    };

    render_list(frame, list_area, state);
    if let Some(preview_area) = preview_area {
        render_preview(frame, preview_area, state);
    }
    render_summary(frame, rows[1], state);
}

/// The AI-summary footer for the selected commit. Prose renders markdown-style `code` spans
/// (backticks stripped, styled distinctly). If the summary still overflows the footer it is cut with
/// a `…`; the title reflects whether `S` will expand or minimize.
fn render_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let width = area.width.saturating_sub(2) as usize; // inside the border
    let rows = area.height.saturating_sub(2) as usize; // visible content lines

    let (content, overflow) = match state.selected_summary() {
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

    let title = if state.summary_expanded {
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

fn render_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let commits = state.commits();

    if state.matches.is_empty() {
        let msg = if matches!(
            state.logs.get(&state.view),
            Some(crate::state::Load::Loading) | None
        ) {
            "Loading commits…".to_string()
        } else if state.filter.is_empty() {
            "No commits".to_string()
        } else {
            format!("No matches for “{}”", state.filter)
        };
        frame.render_widget(Paragraph::new(Line::styled(msg, theme::dim())), area);
        return;
    }

    let rows = area.height as usize;
    let end = (state.top + rows).min(state.matches.len());
    let items: Vec<ListItem> = state.matches[state.top..end]
        .iter()
        .enumerate()
        .map(|(offset, m)| {
            let idx = state.top + offset;
            let commit = &commits[m.commit_idx];
            let mut item = ListItem::new(commit_line(commit));
            if idx == state.cursor {
                item = item.style(theme::selected());
            }
            item
        })
        .collect();

    frame.render_widget(List::new(items), area);
}

/// Build one commit's display line: `hash  date  author  subject (refs)`.
fn commit_line(c: &Commit) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{:<8}", c.short), theme::hash()),
        Span::styled(
            format!("{:<w$}", truncate(&c.relative, DATE_WIDTH), w = DATE_WIDTH),
            theme::date(),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "{:<w$}",
                truncate(&c.author, AUTHOR_WIDTH),
                w = AUTHOR_WIDTH
            ),
            theme::author(),
        ),
        Span::raw(" "),
        Span::styled(c.subject.clone(), theme::subject()),
    ];
    if !c.refs.is_empty() {
        spans.push(Span::styled(
            format!(" ({})", refs_label(&c.refs)),
            theme::refs(),
        ));
    }
    Line::from(spans)
}

fn refs_label(refs: &[Ref]) -> String {
    refs.iter()
        .map(|r| r.label())
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let text = match &state.preview {
        PreviewState::Idle => "…".to_string(),
        PreviewState::Loading(_) => "Loading diff…".to_string(),
        PreviewState::Ready { text, .. } => text.clone(),
        PreviewState::Failed { error, .. } => format!("diff failed: {error}"),
    };
    preview_pane(frame, area, "diff", &text);
}

fn render_status(frame: &mut Frame, area: Rect, state: &AppState) {
    let text = state.status.clone().unwrap_or_else(|| {
        "j/k · /search · Tab preview · s summary · ←/→ view · Enter · R fetch · q quit".to_string()
    });
    frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &AppState) {
    let Some(menu) = &state.menu else { return };

    let title = format!(" {} {} ", menu.short, truncate(&menu.subject, 30));
    let labels: Vec<&str> = menu.items.iter().map(|a| a.label()).collect();
    overlay_menu(frame, body, &title, &labels, menu.cursor);
}

/// Render `state` into a fresh `TestBackend` and return the screen as trimmed text lines.
/// Test-only helper shared by snapshot tests.
#[cfg(test)]
pub fn render_to_string(state: &AppState, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw(f, state)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Commit, Ref};
    use crate::state::{Load, MenuAction, PreviewState, SummaryState};

    fn commit(short: &str, rel: &str, author: &str, subject: &str, refs: Vec<Ref>) -> Commit {
        Commit {
            hash: format!("{}{}", short, "0".repeat(40 - short.len())),
            short: short.to_string(),
            timestamp: 0,
            author: author.to_string(),
            subject: subject.to_string(),
            relative: rel.to_string(),
            refs,
            haystack: format!("{short} {author} {subject}"),
        }
    }

    fn app() -> AppState {
        let mut s = AppState::new(
            "feature".into(),
            "main".into(),
            Some("git@github.com:o/r".into()),
        );
        s.size = (80, 12);
        s.logs.insert(
            View::LocalHead,
            Load::Loaded(vec![
                commit(
                    "aaaaaaa",
                    "3 days ago",
                    "Ada Lovelace",
                    "add fuzzy search",
                    vec![Ref::Head, Ref::Local("feature".into())],
                ),
                commit("bbbbbbb", "5 days ago", "Bo", "fix flaky test", vec![]),
                commit(
                    "ccccccc",
                    "2 weeks ago",
                    "Cy",
                    "refactor parser",
                    vec![Ref::Tag("v1.0".into())],
                ),
            ]),
        );
        s.recompute_matches();
        s
    }

    // LOG-01: list renders hash / date / author / subject / refs.
    #[test]
    fn log_01_list_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // LOG-04/05: search bar shows filter + narrowed list.
    #[test]
    fn log_05_filtered_snapshot() {
        let mut s = app();
        s.mode = Mode::Search;
        s.filter = "parser".into();
        s.recompute_matches();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // LOG-07: origin view active in header.
    #[test]
    fn log_07_origin_view_header_snapshot() {
        let mut s = app();
        s.view = View::OriginMain;
        s.logs.insert(
            View::OriginMain,
            Load::Loaded(vec![commit(
                "eeeeeee",
                "1 hour ago",
                "Cy",
                "origin tip",
                vec![Ref::Remote("origin/main".into())],
            )]),
        );
        s.recompute_matches();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // LOG-08: preview pane open with ready diff.
    #[test]
    fn log_08_preview_snapshot() {
        let mut s = app();
        s.preview_open = true;
        s.preview = PreviewState::Ready {
            hash: s.selected_hash().unwrap(),
            text: "commit aaaaaaa\n+added line\n-removed line".into(),
        };
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // LOG-11: action menu overlay.
    #[test]
    fn log_11_menu_snapshot() {
        let mut s = app();
        s.mode = Mode::Menu;
        s.menu = Some(crate::state::ActionMenu {
            items: MenuAction::all(),
            cursor: 2,
            hash: "aaaaaaa".into(),
            short: "aaaaaaa".into(),
            subject: "add fuzzy search".into(),
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // SUM-01: with no summary, the panel shows the "press s" hint.
    #[test]
    fn sum_01_summary_hint_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // SUM-06: a ready summary is shown (wrapped) in the panel.
    #[test]
    fn sum_06_summary_ready_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Ready(
                "Adds an in-process fuzzy finder so the commit list filters as you type.".into(),
            ),
        );
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // SUM-04/08: generating (placeholder + streaming) and failed states render distinctly.
    #[test]
    fn sum_04_summary_generating_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries
            .insert(hash, SummaryState::Generating(String::new()));
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // Request: `code` spans in a summary render with the backticks stripped (styled distinctly).
    #[test]
    fn sum_06_summary_code_spans_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Ready(
                "Bumps `sled-playwright` from `1.295.0` to `1.307.0` in `package.json`.".into(),
            ),
        );
        let out = render_to_string(&s, 80, 12);
        assert!(
            !out.contains('`'),
            "backticks must be stripped from the rendered panel"
        );
        assert!(out.contains("sled-playwright") && out.contains("package.json"));
        insta::assert_snapshot!(out);
    }

    // A long summary is cut in the panel with a `…` and the title hints `S: expand`.
    #[test]
    fn sum_summary_overflow_teaser_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Ready(
                "Adds an in-process fuzzy finder over the commit list, a diff preview pane toggled \
                 with Tab, an action menu on Enter, and AI summaries generated locally via Ollama \
                 and cached on disk keyed by commit SHA."
                    .into(),
            ),
        );
        let out = render_to_string(&s, 80, 12);
        assert!(out.contains('…'), "overflowing teaser shows an ellipsis");
        assert!(out.contains("S: expand"), "title hints how to expand");
        insta::assert_snapshot!(out);
    }

    // Expanding (S) grows the footer in place to show the full summary; the list stays above it.
    #[test]
    fn sum_summary_expanded_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Ready(
                "Adds an in-process fuzzy finder over the commit list, a diff preview pane toggled \
                 with Tab, an action menu on Enter, and AI summaries generated locally via Ollama \
                 and cached on disk keyed by commit SHA."
                    .into(),
            ),
        );
        s.summary_expanded = true;
        let out = render_to_string(&s, 80, 20);
        assert!(
            !out.contains('…'),
            "expanded footer shows the whole summary"
        );
        assert!(
            out.contains("S: minimize"),
            "title reflects it can be minimized"
        );
        assert!(
            out.contains("commit SHA."),
            "the tail of the summary is visible"
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn sum_04_summary_streaming_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Generating("Adds an in-process fuzzy".to_string()),
        );
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    #[test]
    fn sum_08_summary_failed_snapshot() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries
            .insert(hash, SummaryState::Failed("ollama not found".into()));
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // Empty / loading states.
    #[test]
    fn empty_state_snapshot() {
        let mut s = AppState::new("main".into(), "main".into(), None);
        s.size = (80, 8);
        s.logs.insert(View::LocalHead, Load::Loaded(vec![]));
        s.recompute_matches();
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    #[test]
    fn loading_state_snapshot() {
        let mut s = AppState::new("main".into(), "main".into(), None);
        s.size = (80, 8);
        s.logs.insert(View::LocalHead, Load::Loading);
        s.recompute_matches();
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    // Narrow width still renders without panic.
    #[test]
    fn narrow_width_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 40, 10));
    }

    // A few style assertions (color-locking) rather than only text.
    #[test]
    fn log_01_selected_row_is_reversed() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let s = app();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw(f, &s)).unwrap();
        let buf = term.backend().buffer();
        // Row index 2 in the buffer is the first list row (header+search above); cursor=0.
        let cell = &buf[(0, 2)];
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "selected row should be reversed"
        );
    }
}
