//! Pure rendering for `gitt branch`: `draw_branch(frame, state)` reads only `&BranchState` and writes
//! only into the frame. The layout mirrors `gitt log` exactly (header · search · list · AI-summary
//! footer · status) and reuses `theme`, the shared `components` (list highlight, overlay menu), and
//! the shared summary footer — so the screen is visually consistent with the rest of the tool.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use super::components::{self, ai_badge_span, dim_area, highlight, overlay_menu, truncate};
use super::{summary, theme};
use crate::domain::branch::summary_key;
use crate::domain::{Branch, PrStatus};
use crate::state::{BranchLoad, BranchMode, BranchState};

// The subject/commit column is intentionally dropped so the branch name gets room to breathe.
const NAME_WIDTH: usize = 40;
const PR_WIDTH: usize = 8;
const DATE_WIDTH: usize = 12;

/// Render the whole branch UI for the current state.
pub fn draw_branch(frame: &mut Frame, state: &BranchState) {
    let area = frame.area();
    // No header/title row: the search bar (which already shows the match count) is the top row, so the
    // redundant branch-name title and branch count are omitted (BR-01).
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // search bar
            Constraint::Min(1),    // body (list + summary footer)
            Constraint::Length(1), // status
        ],
    )
    .split(area);

    render_search(frame, chunks[0], state);
    render_body(frame, chunks[1], state);
    render_status(frame, chunks[2], state);

    match state.mode {
        BranchMode::Menu | BranchMode::Confirm | BranchMode::Create => dim_area(frame, area),
        BranchMode::List | BranchMode::Search => {}
    }
    match state.mode {
        BranchMode::Menu => render_menu(frame, chunks[1], state),
        BranchMode::Confirm => render_confirm(frame, chunks[1], state),
        BranchMode::Create => render_create(frame, chunks[1], state),
        BranchMode::List | BranchMode::Search => {}
    }
}

fn render_search(frame: &mut Frame, area: Rect, state: &BranchState) {
    let mut spans = vec![Span::styled("Search: ", theme::dim())];
    spans.push(Span::raw(state.filter.clone()));
    if state.mode == BranchMode::Search {
        spans.push(Span::raw("█"));
    }
    let count = format!("  ({} matches)", state.matches.len());
    spans.push(Span::styled(count, theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &BranchState) {
    // Reserve the bottom rows for the AI summary footer (taller when expanded); rest is the list.
    let rows = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(1),
            Constraint::Length(state.summary_panel_rows()),
        ],
    )
    .split(area);

    render_list(frame, rows[0], state);
    summary::render_footer(
        frame,
        rows[1],
        state.selected_summary(),
        state.summary_expanded,
    );
}

fn render_list(frame: &mut Frame, area: Rect, state: &BranchState) {
    let branches = state.branches();

    if state.matches.is_empty() {
        let msg = match &state.load {
            BranchLoad::Loaded(_) if state.filter.is_empty() => "No branches".to_string(),
            BranchLoad::Loaded(_) => format!("No matches for “{}”", state.filter),
            BranchLoad::Failed(e) => format!("branches failed: {e}"),
            _ => "Loading branches…".to_string(),
        };
        frame.render_widget(Paragraph::new(Line::styled(msg, theme::dim())), area);
        return;
    }

    let pr_loaded = state.pr_statuses.is_some();
    let rows = area.height as usize;
    let end = (state.top + rows).min(state.matches.len());
    let items: Vec<ListItem> = state.matches[state.top..end]
        .iter()
        .enumerate()
        // `.get` (not index) so a stale match entry can never panic the TUI mid-reload.
        .filter_map(|(offset, m)| {
            let idx = state.top + offset;
            let branch = branches.get(m.commit_idx)?;
            let pr = state.pr_status(&branch.name);
            let summarized = matches!(
                state.summaries.get(&summary_key(&branch.tip)),
                Some(crate::state::SummaryState::Ready(_))
            );
            let mut item = ListItem::new(branch_line(
                branch,
                &state.filter,
                pr,
                pr_loaded,
                summarized,
            ));
            if idx == state.cursor {
                item = item.style(theme::selected());
            }
            Some(item)
        })
        .collect();

    frame.render_widget(List::new(items), area);
}

/// Build one branch's display line: `<marker><ai> name  pr  date`. The current branch is marked with
/// `*` and its name styled distinctly; the `<ai>` column carries the one-char AI-summary marker when
/// this branch's summary is cached; search matches are highlighted in the name; the PR column shows
/// the branch's pull-request state (`loading…` until the background `gh` fetch lands). The commit
/// subject is intentionally omitted so the name has room.
fn branch_line(
    b: &Branch,
    query: &str,
    pr: Option<PrStatus>,
    pr_loaded: bool,
    summarized: bool,
) -> Line<'static> {
    let marker = if b.is_current { "* " } else { "  " };
    let name_base = if b.is_current {
        theme::current_branch()
    } else {
        theme::subject()
    };
    let name = format!("{:<w$}", truncate(&b.name, NAME_WIDTH), w = NAME_WIDTH);

    let mut spans = vec![
        Span::styled(marker.to_string(), theme::current_branch()),
        ai_badge_span(summarized),
        Span::raw(" "),
    ];
    spans.extend(highlight(&name, query, name_base));
    spans.push(Span::raw(" "));
    spans.push(pr_span(pr, pr_loaded));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!("{:<w$}", truncate(&b.relative, DATE_WIDTH), w = DATE_WIDTH),
        theme::date(),
    ));
    Line::from(spans)
}

/// The PR-column span (fixed [`PR_WIDTH`]): a dim `loading…` until the background fetch lands, a dim
/// `—` for a branch with no PR, or the coloured PR state otherwise.
fn pr_span(pr: Option<PrStatus>, pr_loaded: bool) -> Span<'static> {
    let (text, style) = match (pr_loaded, pr) {
        (false, _) => ("loading…".to_string(), theme::dim()),
        (true, None) => ("—".to_string(), theme::dim()),
        (true, Some(status)) => (status.label().to_string(), theme::pr_status(status)),
    };
    Span::styled(format!("{text:<PR_WIDTH$}"), style)
}

fn render_status(frame: &mut Frame, area: Rect, state: &BranchState) {
    let (text, style) = match &state.status {
        // An error (e.g. a failed checkout) is shown in dominant red for visibility.
        Some(msg) if state.status_is_error => (msg.clone(), theme::error()),
        Some(msg) => (msg.clone(), theme::dim()),
        None => (
            "j/k · /search · s summary · n new · d delete · Enter · R reload · q quit".to_string(),
            theme::dim(),
        ),
    };
    frame.render_widget(Paragraph::new(Line::styled(text, style)), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &BranchState) {
    let Some(menu) = &state.menu else { return };
    let title = format!(" {} ", truncate(&menu.name, 30));
    let labels: Vec<&str> = menu.items.iter().map(|a| a.label()).collect();
    overlay_menu(frame, body, &title, &labels, menu.cursor);
}

fn render_confirm(frame: &mut Frame, body: Rect, state: &BranchState) {
    let Some(confirm) = &state.confirm else {
        return;
    };
    let question = format!("Delete branch {}?", truncate(&confirm.name, 40));
    let hint = "y  delete    n  cancel";

    let width = question.len().max(hint.len()) + 4;
    let height = 4; // 2 text lines + top/bottom border
    let area = components::centered_rect(body, width as u16, height as u16);

    let lines = vec![
        Line::styled(format!(" {question}"), theme::subject()),
        Line::styled(format!(" {hint}"), theme::dim()),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Delete branch? ")),
        area,
    );
}

fn render_create(frame: &mut Frame, body: Rect, state: &BranchState) {
    let hint = "Enter  create    Esc  cancel";
    let input = format!(" {}█", state.create_input);
    let width = input.len().max(hint.len()).max(20) + 4;
    let height = 4;
    let area = components::centered_rect(body, width as u16, height as u16);

    let lines = vec![
        Line::styled(input, theme::subject()),
        Line::styled(format!(" {hint}"), theme::dim()),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" New branch ")),
        area,
    );
}

/// Render `state` into a fresh `TestBackend` and return the screen as trimmed text lines.
#[cfg(test)]
pub fn render_to_string(state: &BranchState, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw_branch(f, state)).unwrap();
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
    use crate::state::{BranchAction, BranchMenu, ConfirmDeleteBranch, SummaryState};

    fn branch(name: &str, current: bool, rel: &str, subject: &str) -> Branch {
        Branch {
            name: name.to_string(),
            is_current: current,
            tip: format!("{name:0<40}"),
            upstream: None,
            timestamp: 0,
            subject: subject.to_string(),
            relative: rel.to_string(),
            haystack: format!("{name} {subject}"),
        }
    }

    fn app() -> BranchState {
        let mut s = BranchState::new("feature".into(), "main".into());
        s.size = (80, 12);
        s.load = BranchLoad::Loaded(vec![
            branch("feature", true, "3 days ago", "add fuzzy search"),
            branch("main", false, "2 weeks ago", "base"),
            branch("wip-parser", false, "1 hour ago", "refactor parser"),
        ]);
        s.recompute_matches();
        s
    }

    // BR-01: list renders marker / name / date (no commit-subject column); the current branch is
    // marked and the name column is spaced out.
    #[test]
    fn br_01_list_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-17: before the PR statuses load, the column reads "loading…" rather than sitting blank.
    #[test]
    fn br_17_pr_column_loading() {
        let s = app(); // pr_statuses is None until the background fetch lands
        let out = render_to_string(&s, 80, 12);
        assert!(
            out.contains("loading…"),
            "PR column shows a loading hint:\n{out}"
        );
    }

    // A branch whose summary is cached carries the one-char AI marker; others don't.
    #[test]
    fn ai_badge_marks_summarized_branch() {
        let mut s = app();
        let key = s.selected_summary_key().unwrap();
        s.summaries
            .insert(key, SummaryState::Ready("Reworks the parser.".into()));
        let out = render_to_string(&s, 80, 12);
        assert_eq!(
            out.matches('✦').count(),
            1,
            "exactly the summarized branch is marked:\n{out}"
        );
    }

    // BR-17: once PR statuses load, the column shows each branch's state (coloured); a branch with no
    // PR shows a dim `—`.
    #[test]
    fn br_17_pr_column_snapshot() {
        use crate::domain::PrStatus;
        use std::collections::HashMap;

        let mut s = app();
        let mut map = HashMap::new();
        map.insert("wip-parser".to_string(), PrStatus::Open);
        map.insert("main".to_string(), PrStatus::Merged);
        s.pr_statuses = Some(map);
        let out = render_to_string(&s, 80, 12);
        assert!(out.contains("open"), "open PR shown:\n{out}");
        assert!(out.contains("merged"), "merged PR shown");
        assert!(out.contains('—'), "a branch with no PR shows a dash");
        insta::assert_snapshot!(out);
    }

    // BR-03: search bar shows the filter and the narrowed list.
    #[test]
    fn br_03_filtered_snapshot() {
        let mut s = app();
        s.mode = BranchMode::Search;
        s.filter = "parser".into();
        s.recompute_matches();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-05: the per-branch action menu overlay.
    #[test]
    fn br_05_menu_snapshot() {
        let mut s = app();
        s.mode = BranchMode::Menu;
        s.menu = Some(BranchMenu {
            items: BranchAction::all(),
            cursor: 0,
            name: "wip-parser".into(),
            is_current: false,
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-09: the delete confirmation overlay.
    #[test]
    fn br_09_confirm_snapshot() {
        let mut s = app();
        s.mode = BranchMode::Confirm;
        s.confirm = Some(ConfirmDeleteBranch {
            name: "wip-parser".into(),
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-10: the new-branch input overlay.
    #[test]
    fn br_10_create_snapshot() {
        let mut s = app();
        s.mode = BranchMode::Create;
        s.create_input = "feature-x".into();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-11/12: a ready branch summary is shown in the footer.
    #[test]
    fn br_11_summary_ready_snapshot() {
        let mut s = app();
        let key = s.selected_summary_key().unwrap();
        s.summaries.insert(
            key,
            SummaryState::Ready("Adds an in-process fuzzy finder over the branch list.".into()),
        );
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // BR-11: with no summary, the footer shows the "press s" hint (shared with gitt log).
    #[test]
    fn br_11_summary_hint_snapshot() {
        let s = app();
        let out = render_to_string(&s, 80, 12);
        assert!(out.contains("ai summary"));
        assert!(out.contains("press s for an AI summary"));
    }

    #[test]
    fn loading_snapshot() {
        let mut s = BranchState::new("main".into(), "main".into());
        s.size = (80, 8);
        s.load = BranchLoad::Loading;
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    // An open overlay dims the base screen behind it so the modal stands out; the plain list doesn't.
    #[test]
    fn menu_dims_the_background() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let dimmed_at = |mode: BranchMode| {
            let mut s = app();
            s.mode = mode;
            if mode == BranchMode::Menu {
                s.menu = Some(BranchMenu {
                    items: BranchAction::all(),
                    cursor: 0,
                    name: "wip-parser".into(),
                    is_current: false,
                });
            }
            let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
            term.draw(|f| draw_branch(f, &s)).unwrap();
            // The current-branch marker (list row 0, col 0) is styled distinctly (not dim) in the
            // plain list and sits outside the centered overlay, so it cleanly reflects the dim layer.
            term.backend().buffer()[(0, 1)]
                .modifier
                .contains(Modifier::DIM)
        };

        assert!(dimmed_at(BranchMode::Menu), "menu dims the background");
        assert!(!dimmed_at(BranchMode::List), "the plain list is not dimmed");
    }

    // BR-06: a failed checkout is reported on the status line in dominant red (not the dim legend).
    #[test]
    fn error_status_is_red() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let mut s = app();
        s.set_error("Checkout failed: fatal: 'master' is already checked out");
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw_branch(f, &s)).unwrap();
        let buf = term.backend().buffer();
        // The status line is the last row; its first cell carries the error colour.
        let cell = &buf[(0, 11)];
        assert_eq!(cell.fg, Color::Red, "error status is red for visibility");
    }

    // Selected row is reversed, consistent with the log/status/diff lists.
    #[test]
    fn selected_row_is_reversed() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let s = app();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw_branch(f, &s)).unwrap();
        let buf = term.backend().buffer();
        // Row index 1 is the first list row (only the search bar sits above it now); cursor = 0.
        let cell = &buf[(0, 1)];
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "selected row should be reversed"
        );
    }
}
