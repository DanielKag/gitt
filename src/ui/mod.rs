//! Pure rendering: `draw(frame, state)` reads only `&AppState` and writes only into the frame.
//! No I/O, no port calls — so it is exercised directly with ratatui's `TestBackend`.

pub mod branch;
pub mod components;
pub mod diff;
pub mod status;
pub mod summary;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::domain::{Commit, Ref, View};
use crate::state::{AppState, Mode, PreviewState, SummaryState};

pub use branch::draw_branch;
pub use diff::draw_diff;
pub use status::draw_status;

use components::{ai_badge_span, dim_area, highlight, overlay_menu, preview_pane, truncate};

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
    summary::render_footer(
        frame,
        rows[1],
        state.selected_summary(),
        state.summary_expanded,
    );
}

fn render_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let commits = state.commits();

    if state.matches.is_empty() {
        let streaming = matches!(
            state.logs.get(&state.view),
            Some(crate::state::Load::Streaming(_))
        );
        let msg = if matches!(
            state.logs.get(&state.view),
            Some(crate::state::Load::Loading) | None
        ) {
            "Loading commits…".to_string()
        } else if state.filter.is_empty() {
            "No commits".to_string()
        } else if streaming {
            // The match may still be in a page that hasn't streamed in yet — don't claim it's absent.
            format!("No matches for “{}” yet — still loading…", state.filter)
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
        // `.get` (not index) so a match entry that briefly outlives its commit slice — e.g. between a
        // reload swapping to `Loading` and the first batch landing — can never panic the whole TUI.
        .filter_map(|(offset, m)| {
            let idx = state.top + offset;
            let commit = commits.get(m.commit_idx)?;
            let summarized = matches!(
                state.summaries.get(&commit.hash),
                Some(SummaryState::Ready(_))
            );
            let mut item = ListItem::new(commit_line(commit, &state.filter, summarized));
            if idx == state.cursor {
                item = item.style(theme::selected());
            }
            Some(item)
        })
        .collect();

    frame.render_widget(List::new(items), area);
}

/// Build one commit's display line: `<ai> hash  date  author  subject (refs)`. The leading column is
/// the one-char AI-summary marker (present only when this commit's summary is cached). When a search
/// `query` is active, the substrings it matched are highlighted in the searchable fields (LOG-25);
/// the date is not searchable, so it's never highlighted.
fn commit_line(c: &Commit, query: &str, summarized: bool) -> Line<'static> {
    let author = format!(
        "{:<w$}",
        truncate(&c.author, AUTHOR_WIDTH),
        w = AUTHOR_WIDTH
    );

    let mut spans = vec![ai_badge_span(summarized), Span::raw(" ")];
    spans.extend(highlight(&format!("{:<8}", c.short), query, theme::hash()));
    spans.push(Span::styled(
        format!("{:<w$}", truncate(&c.relative, DATE_WIDTH), w = DATE_WIDTH),
        theme::date(),
    ));
    spans.push(Span::raw(" "));
    spans.extend(highlight(&author, query, theme::author()));
    spans.push(Span::raw(" "));
    spans.extend(highlight(&c.subject, query, theme::subject()));
    if !c.refs.is_empty() {
        spans.extend(highlight(
            &format!(" ({})", refs_label(&c.refs)),
            query,
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
    // A transient action message wins; otherwise, while the history is still streaming in, show the
    // load progress (it clears itself once the load completes); otherwise the key hints.
    let text = if let Some(status) = &state.status {
        status.clone()
    } else if let Some(n) = state.log_loading_count() {
        format!("⟳ loading commits… {n} so far")
    } else {
        "j/k · /search · Tab preview · s summary · ←/→ view · Enter · R fetch · q quit".to_string()
    };
    frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &AppState) {
    let Some(menu) = &state.menu else { return };

    dim_area(frame, frame.area());
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

    // LOG-22: while streaming, the status line shows the load progress; once loaded it doesn't.
    #[test]
    fn log_22_status_line_shows_loading_progress() {
        let mut s = app();
        // Re-cast the current view as still streaming.
        let commits = match s.logs.remove(&View::LocalHead) {
            Some(Load::Loaded(c)) => c,
            _ => unreachable!(),
        };
        s.logs.insert(View::LocalHead, Load::Streaming(commits));
        s.recompute_matches();
        let streaming = render_to_string(&s, 80, 12);
        assert!(
            streaming.contains("loading"),
            "streaming status line should show progress:\n{streaming}"
        );

        // Once loaded, the indicator is gone.
        let commits = match s.logs.remove(&View::LocalHead) {
            Some(Load::Streaming(c)) => c,
            _ => unreachable!(),
        };
        s.logs.insert(View::LocalHead, Load::Loaded(commits));
        let loaded = render_to_string(&s, 80, 12);
        assert!(
            !loaded.contains("loading"),
            "loaded status line should not show progress"
        );
    }

    // LOG-21: while streaming, a filter that hasn't matched yet must not claim "No matches" (the
    // commit may still be in an unloaded page); once loaded, the definitive message is fine.
    #[test]
    fn log_21_streaming_no_match_says_still_loading() {
        let mut s = app();
        let commits = match s.logs.remove(&View::LocalHead) {
            Some(Load::Loaded(c)) => c,
            _ => unreachable!(),
        };
        s.logs.insert(View::LocalHead, Load::Streaming(commits));
        s.filter = "zzzznope".into();
        s.recompute_matches();
        let streaming = render_to_string(&s, 80, 12);
        assert!(
            streaming.contains("still loading"),
            "streaming no-match should be tentative:\n{streaming}"
        );

        // Same filter, but the load is complete → the message is definitive.
        let commits = match s.logs.remove(&View::LocalHead) {
            Some(Load::Streaming(c)) => c,
            _ => unreachable!(),
        };
        s.logs.insert(View::LocalHead, Load::Loaded(commits));
        let loaded = render_to_string(&s, 80, 12);
        assert!(
            !loaded.contains("still loading"),
            "loaded no-match should be definitive"
        );
        assert!(loaded.contains("No matches"));
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

    // The teaser (collapsed/overflow) path styles `code` spans just like the expanded view — a long
    // summary whose `code` span sits on the first visible line keeps the code background.
    #[test]
    fn sum_teaser_styles_code_spans() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries.insert(
            hash,
            SummaryState::Ready(
                "Bumps `package.json` and then rewrites a great deal of surrounding prose so that \
                 the summary overflows the collapsed footer and is shown as a cut teaser rather \
                 than in full."
                    .into(),
            ),
        );
        // Height forces the collapsed footer into overflow (the teaser path).
        let out = render_to_string(&s, 80, 12);
        assert!(
            out.contains('…'),
            "the summary overflows into a teaser:\n{out}"
        );

        let (y, line) = out
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("package.json"))
            .map(|(y, l)| (y as u16, l.to_string()))
            .expect("the code span is on a visible teaser line");
        let x = line.find("package.json").unwrap() as u16;

        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw(f, &s)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(
            buf[(x, y)].bg,
            theme::code().bg.unwrap(),
            "the code span carries the code background in the teaser"
        );
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

    // LOG-25: matched substrings in the list are highlighted with the search-match style; the
    // surrounding characters keep their normal (non-highlight) style.
    #[test]
    fn log_25_matches_are_highlighted() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut s = app();
        s.mode = Mode::Search;
        s.filter = "parser".into();
        s.recompute_matches();

        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw(f, &s)).unwrap();
        let buf = term.backend().buffer();

        // Find the "refactor parser" row and the column where "parser" starts.
        let screen = render_to_string(&s, 80, 12);
        let (y, line) = screen
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("refactor parser"))
            .map(|(y, l)| (y as u16, l.to_string()))
            .expect("the matching commit is shown");
        let x = line.find("parser").expect("subject text is present") as u16;

        let hl = theme::search_match();
        // Every char of the matched "parser" carries the highlight bg/modifier.
        for dx in 0.."parser".len() as u16 {
            let cell = &buf[(x + dx, y)];
            assert_eq!(cell.bg, hl.bg.unwrap(), "matched char has highlight bg");
        }
        // The character just before the match (a space) does not.
        assert_ne!(
            buf[(x - 1, y)].bg,
            hl.bg.unwrap(),
            "non-matched char keeps its normal background"
        );
    }

    // A commit whose summary is cached carries the one-char AI marker; a commit that is only
    // generating/failed/absent does not.
    #[test]
    fn ai_badge_marks_summarized_commit() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        s.summaries
            .insert(hash, SummaryState::Ready("Adds a fuzzy finder.".into()));
        let out = render_to_string(&s, 80, 12);
        assert_eq!(
            out.matches('✦').count(),
            1,
            "exactly the summarized commit is marked:\n{out}"
        );
    }

    // An open action menu dims the base screen behind it.
    #[test]
    fn menu_dims_the_background() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let mut s = app();
        s.mode = Mode::Menu;
        s.menu = Some(crate::state::ActionMenu {
            items: MenuAction::all(),
            cursor: 0,
            hash: "aaaaaaa".into(),
            short: "aaaaaaa".into(),
            subject: "add fuzzy search".into(),
        });
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw(f, &s)).unwrap();
        assert!(
            term.backend().buffer()[(0, 0)]
                .modifier
                .contains(Modifier::DIM),
            "the background behind the menu is dimmed"
        );
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
