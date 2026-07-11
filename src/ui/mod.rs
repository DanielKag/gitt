//! Pure rendering: `draw(frame, state)` reads only `&AppState` and writes only into the frame.
//! No I/O, no port calls — so it is exercised directly with ratatui's `TestBackend`.

pub mod components;
pub mod diff;
pub mod status;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::domain::{Commit, Ref, View};
use crate::state::{AppState, Mode, PreviewState};

pub use diff::draw_diff;
pub use status::draw_status;

use components::{overlay_menu, preview_pane, truncate};

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
    let (list_area, preview_area) = if state.preview_open {
        let cols = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(area);
        (cols[0], Some(cols[1]))
    } else {
        (area, None)
    };

    render_list(frame, list_area, state);
    if let Some(preview_area) = preview_area {
        render_preview(frame, preview_area, state);
    }
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
        "j/k move · / search · Tab preview · ←/→ view · Enter actions · R fetch · q quit"
            .to_string()
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
    use crate::state::{Load, MenuAction, PreviewState};

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
