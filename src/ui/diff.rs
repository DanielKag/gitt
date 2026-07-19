//! Pure rendering for `gitt diff`: `draw_diff(frame, state)` reads only `&DiffState` and writes only
//! into the frame. It reuses `theme` and the shared `components` (list, overlay menu, preview pane,
//! help bar) so it is visually consistent with `gitt log` and `gitt status`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use super::components::{dim_area, overlay_menu, preview_pane, truncate};
use super::theme;
use crate::domain::{DiffFile, DiffScope};
use crate::state::{DiffLoad, DiffMode, DiffPreview, DiffState};

/// Render the whole diff UI for the current state.
pub fn draw_diff(frame: &mut Frame, state: &DiffState) {
    let area = frame.area();
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // header (scope tabs)
            Constraint::Min(1),    // body (list [+ diff pane])
            Constraint::Length(1), // status / help
        ],
    )
    .split(area);

    render_header(frame, chunks[0], state);
    render_body(frame, chunks[1], state);
    render_status(frame, chunks[2], state);

    if state.mode == DiffMode::Menu {
        dim_area(frame, area);
        render_menu(frame, chunks[1], state);
    }
}

/// The label for a scope's tab (the `Branch` scope shows the resolved base branch).
fn scope_tab_label(scope: DiffScope, main_branch: &str) -> String {
    match scope {
        DiffScope::Branch => format!("vs {main_branch}"),
        _ => scope.short().to_string(),
    }
}

/// The noun for a scope's empty state ("no <noun>").
fn scope_empty_noun(scope: DiffScope, main_branch: &str) -> String {
    match scope {
        DiffScope::Unstaged => "unstaged changes".to_string(),
        DiffScope::Staged => "staged changes".to_string(),
        DiffScope::Working => "working-tree changes".to_string(),
        DiffScope::Branch => format!("changes vs {main_branch}"),
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &DiffState) {
    let mut spans = Vec::new();
    for scope in DiffScope::ALL {
        let label = scope_tab_label(scope, &state.main_branch);
        let style = if scope == state.scope {
            theme::active_view()
        } else {
            theme::inactive_view()
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
    }
    if let Some(msg) = &state.status {
        let style = if state.status_is_error {
            theme::error()
        } else {
            theme::dim()
        };
        spans.push(Span::styled(format!("  {msg}"), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &DiffState) {
    if !state.preview_open {
        render_list(frame, area, state);
        return;
    }
    // The diff pane sits BELOW the file list (a vertical split), so it spans the full terminal width
    // — room for a side-by-side layout. `f` grows it from 50% to 90% of the height (list → 10%, but
    // still visible).
    let diff_pct = if state.expanded { 90 } else { 50 };
    let rows = Layout::new(
        Direction::Vertical,
        [
            Constraint::Percentage(100 - diff_pct),
            Constraint::Percentage(diff_pct),
        ],
    )
    .split(area);
    render_list(frame, rows[0], state);
    render_preview(frame, rows[1], state);
}

fn render_list(frame: &mut Frame, area: Rect, state: &DiffState) {
    let files = state.files();

    if files.is_empty() {
        let msg = match state.loads.get(&state.scope) {
            Some(DiffLoad::Loaded(_)) => {
                format!("no {}", scope_empty_noun(state.scope, &state.main_branch))
            }
            Some(DiffLoad::Failed(e)) => format!("diff failed: {e}"),
            _ => "Loading…".to_string(),
        };
        frame.render_widget(Paragraph::new(Line::styled(msg, theme::dim())), area);
        return;
    }

    let rows = area.height as usize;
    let end = (state.top + rows).min(files.len());
    let items: Vec<ListItem> = files[state.top..end]
        .iter()
        .enumerate()
        .map(|(offset, file)| {
            let idx = state.top + offset;
            let mut item = ListItem::new(file_line(file, area.width));
            if idx == state.cursor {
                item = item.style(theme::selected());
            }
            item
        })
        .collect();

    frame.render_widget(List::new(items), area);
}

/// The color for a change-status letter (git's diffstat convention).
fn status_style(status: char) -> ratatui::style::Style {
    match status {
        'A' => theme::added(),
        'D' => theme::deleted(),
        'R' | 'C' => theme::renamed(),
        _ => theme::modified(), // M, T, and anything else.
    }
}

/// Build one file's display line: `<badge> path` (with a rename arrow when applicable).
fn file_line(file: &DiffFile, width: u16) -> Line<'static> {
    let path = match &file.orig_path {
        Some(orig) => format!("{orig} → {}", file.path),
        None => file.path.clone(),
    };
    // Keep the row within the pane width (badge is 2 cols: letter + space).
    let path = truncate(&path, (width as usize).saturating_sub(2).max(1));

    Line::from(vec![
        Span::styled(file.badge().to_string(), status_style(file.status)),
        Span::raw(" "),
        Span::styled(path, theme::subject()),
    ])
}

fn render_preview(frame: &mut Frame, area: Rect, state: &DiffState) {
    let text = match &state.preview {
        DiffPreview::Idle => "…".to_string(),
        DiffPreview::Loading(_) => "Loading diff…".to_string(),
        DiffPreview::Ready { text, .. } => text.clone(),
        DiffPreview::Failed { error, .. } => format!("diff failed: {error}"),
    };
    preview_pane(frame, area, "diff", &text, state.preview_scroll);
}

fn render_status(frame: &mut Frame, area: Rect, _state: &DiffState) {
    let text = "j/k move · ←/→ scope · Tab diff · f wide · Enter actions · R reload · q quit";
    frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &DiffState) {
    let Some(menu) = &state.menu else { return };
    let title = format!(" {} ", truncate(&menu.path, 30));
    let labels: Vec<&str> = menu.items.iter().map(|a| a.label()).collect();
    overlay_menu(frame, body, &title, &labels, menu.cursor);
}

/// Render `state` into a fresh `TestBackend` and return the screen as trimmed text lines.
#[cfg(test)]
pub fn render_to_string(state: &DiffState, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw_diff(f, state)).unwrap();
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
    use crate::domain::DiffFile;
    use crate::state::{DiffAction, DiffMenu};

    fn file(status: char, path: &str) -> DiffFile {
        DiffFile {
            status,
            path: path.to_string(),
            orig_path: None,
        }
    }

    fn app() -> DiffState {
        let mut s = DiffState::new("main".into());
        s.size = (80, 12);
        s.preview_open = false;
        s.loads.insert(
            DiffScope::Unstaged,
            DiffLoad::Loaded(vec![
                file('M', "src/reducer.rs"),
                file('A', "src/diff.rs"),
                file('D', "src/old.rs"),
                DiffFile {
                    status: 'R',
                    path: "src/new_name.rs".into(),
                    orig_path: Some("src/old_name.rs".into()),
                },
            ]),
        );
        s
    }

    // DIFF-01: flat list with change badges + paths (and the scope tabs in the header).
    #[test]
    fn diff_01_list_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // DIFF-04: the active scope tab is highlighted; the Branch tab shows `vs main`.
    #[test]
    fn diff_04_scope_tabs_snapshot() {
        let mut s = app();
        s.scope = DiffScope::Branch;
        s.loads
            .insert(DiffScope::Branch, DiffLoad::Loaded(vec![file('M', "x.rs")]));
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // DIFF-06: the diff pane open beside the list with a ready diff.
    #[test]
    fn diff_06_preview_snapshot() {
        let mut s = app();
        s.preview_open = true;
        s.cursor = 0;
        s.preview = DiffPreview::Ready {
            path: "src/reducer.rs".into(),
            text: "@@ -1 +1 @@\n-old line\n+new line".into(),
        };
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // DIFF-08: per-file action menu overlay.
    #[test]
    fn diff_08_menu_snapshot() {
        let mut s = app();
        s.mode = DiffMode::Menu;
        s.menu = Some(DiffMenu {
            items: vec![DiffAction::CopyPath, DiffAction::CopyDiff],
            cursor: 0,
            path: "src/reducer.rs".into(),
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // DIFF-11: an empty scope shows a scope-specific empty state.
    #[test]
    fn diff_11_empty_scope_snapshot() {
        let mut s = DiffState::new("main".into());
        s.size = (80, 8);
        s.preview_open = false;
        s.loads
            .insert(DiffScope::Unstaged, DiffLoad::Loaded(vec![]));
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    #[test]
    fn loading_snapshot() {
        let mut s = DiffState::new("main".into());
        s.size = (80, 8);
        s.preview_open = false;
        s.loads.insert(DiffScope::Unstaged, DiffLoad::Loading);
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    // DIFF-15: a preview carrying ANSI color (from the configured diff tool) renders as styled
    // spans in the pane — a green added-line escape lands as a green cell.
    #[test]
    fn diff_15_preview_ansi_is_colorized() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let mut s = app();
        s.preview_open = true;
        s.cursor = 0;
        s.preview = DiffPreview::Ready {
            path: "src/reducer.rs".into(),
            // ESC[32m … ESC[0m = green foreground, as delta/difft would emit for an added line.
            text: "\x1b[32m+added line\x1b[0m".into(),
        };
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw_diff(f, &s)).unwrap();
        let buf = term.backend().buffer();
        let has_green = (0..buf.area.width)
            .any(|x| (0..buf.area.height).any(|y| buf[(x, y)].fg == Color::Green));
        assert!(
            has_green,
            "an ANSI green diff line should render green in the pane"
        );
    }

    // Selected row is reversed, consistent with the log/status lists.
    #[test]
    fn selected_row_is_reversed() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let s = app();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw_diff(f, &s)).unwrap();
        let buf = term.backend().buffer();
        // Row index 1 is the first list row (header above); cursor = 0.
        let cell = &buf[(0, 1)];
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "selected row should be reversed"
        );
    }
}
