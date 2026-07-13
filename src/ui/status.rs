//! Pure rendering for `gitt status`: `draw_status(frame, state)` reads only `&StatusState` and
//! writes only into the frame. It reuses `theme` and the shared `components` so it is visually
//! consistent with `gitt log`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use super::components::{self, dim_area, overlay_menu, preview_pane, truncate};
use super::theme;
use crate::domain::StatusEntry;
use crate::state::{FilePreview, StatusLoad, StatusMode, StatusState};

/// Render the whole status UI for the current state.
pub fn draw_status(frame: &mut Frame, state: &StatusState) {
    let area = frame.area();
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // header
            Constraint::Min(1),    // body (list [+ preview])
            Constraint::Length(1), // status / help
        ],
    )
    .split(area);

    render_header(frame, chunks[0], state);
    render_body(frame, chunks[1], state);
    render_status(frame, chunks[2], state);

    match state.mode {
        StatusMode::Menu | StatusMode::Confirm => dim_area(frame, area),
        StatusMode::List => {}
    }
    match state.mode {
        StatusMode::Menu => render_menu(frame, chunks[1], state),
        StatusMode::Confirm => render_confirm(frame, chunks[1], state),
        StatusMode::List => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &StatusState) {
    let entries = state.entries();
    let staged = entries.iter().filter(|e| e.is_staged()).count();
    let title = format!(" {} (status) ", state.branch);
    let summary = format!("  {} changed · {} staged", entries.len(), staged);
    let line = Line::from(vec![
        Span::styled(title, theme::active_view()),
        Span::styled(summary, theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &StatusState) {
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

fn render_list(frame: &mut Frame, area: Rect, state: &StatusState) {
    let entries = state.entries();

    if entries.is_empty() {
        let msg = match &state.load {
            StatusLoad::Loaded(_) => "nothing to commit, working tree clean".to_string(),
            StatusLoad::Failed(e) => format!("status failed: {e}"),
            _ => "Loading…".to_string(),
        };
        frame.render_widget(Paragraph::new(Line::styled(msg, theme::dim())), area);
        return;
    }

    let rows = area.height as usize;
    let end = (state.top + rows).min(entries.len());
    let items: Vec<ListItem> = entries[state.top..end]
        .iter()
        .enumerate()
        .map(|(offset, entry)| {
            let idx = state.top + offset;
            let mut item = ListItem::new(file_line(entry, area.width));
            if idx == state.cursor {
                item = item.style(theme::selected());
            }
            item
        })
        .collect();

    frame.render_widget(List::new(items), area);
}

/// Build one file's display line: `XY path` (with a rename arrow when applicable).
fn file_line(entry: &StatusEntry, width: u16) -> Line<'static> {
    let (x_style, y_style) = if entry.is_untracked() {
        (theme::untracked(), theme::untracked())
    } else {
        let x = if entry.index == ' ' {
            theme::dim()
        } else {
            theme::staged()
        };
        let y = if entry.worktree == ' ' {
            theme::dim()
        } else {
            theme::unstaged()
        };
        (x, y)
    };

    let path = match &entry.orig_path {
        Some(orig) => format!("{orig} → {}", entry.path),
        None => entry.path.clone(),
    };
    // Keep the row within the pane width (badge is 3 cols: X, Y, space).
    let path = truncate(&path, (width as usize).saturating_sub(3).max(1));

    Line::from(vec![
        Span::styled(entry.index.to_string(), x_style),
        Span::styled(entry.worktree.to_string(), y_style),
        Span::raw(" "),
        Span::styled(path, theme::subject()),
    ])
}

fn render_preview(frame: &mut Frame, area: Rect, state: &StatusState) {
    let text = match &state.preview {
        FilePreview::Idle => "…".to_string(),
        FilePreview::Loading(_) => "Loading diff…".to_string(),
        FilePreview::Ready { text, .. } => text.clone(),
        FilePreview::Failed { error, .. } => format!("diff failed: {error}"),
    };
    preview_pane(frame, area, "diff", &text);
}

fn render_status(frame: &mut Frame, area: Rect, state: &StatusState) {
    let text = state.status.clone().unwrap_or_else(|| {
        "j/k move · space stage/unstage · d discard · Tab diff · Enter actions · R reload · q quit"
            .to_string()
    });
    frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &StatusState) {
    let Some(menu) = &state.menu else { return };
    let title = format!(" {} ", truncate(&menu.path, 30));
    let labels: Vec<&str> = menu.items.iter().map(|a| a.label()).collect();
    overlay_menu(frame, body, &title, &labels, menu.cursor);
}

fn render_confirm(frame: &mut Frame, body: Rect, state: &StatusState) {
    let Some(confirm) = &state.confirm else {
        return;
    };
    let verb = if confirm.untracked {
        "Delete"
    } else {
        "Discard changes to"
    };
    let question = format!("{verb} {}?", truncate(&confirm.path, 40));
    let hint = "y  discard    n  cancel";

    let width = question.len().max(hint.len()) + 4;
    let height = 4; // 2 text lines + top/bottom border
    let area = components::centered_rect(body, width as u16, height as u16);

    let lines = vec![
        Line::styled(format!(" {question}"), theme::subject()),
        Line::styled(format!(" {hint}"), theme::dim()),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Discard? ")),
        area,
    );
}

/// Render `state` into a fresh `TestBackend` and return the screen as trimmed text lines.
#[cfg(test)]
pub fn render_to_string(state: &StatusState, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw_status(f, state)).unwrap();
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
    use crate::domain::StatusEntry;
    use crate::state::{ConfirmDiscard, FileAction, FileMenu};

    fn entry(index: char, worktree: char, path: &str) -> StatusEntry {
        StatusEntry {
            index,
            worktree,
            path: path.to_string(),
            orig_path: None,
        }
    }

    fn app() -> StatusState {
        let mut s = StatusState::new("feature".into());
        s.size = (80, 12);
        s.load = StatusLoad::Loaded(vec![
            entry('A', ' ', "src/status.rs"),
            entry(' ', 'M', "src/reducer.rs"),
            entry('M', 'M', "src/ui/mod.rs"),
            entry('?', '?', "notes.txt"),
        ]);
        s
    }

    // STAT-01: flat list with XY badges + paths.
    #[test]
    fn stat_01_list_snapshot() {
        let s = app();
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // STAT-06: preview pane open with a ready diff.
    #[test]
    fn stat_06_preview_snapshot() {
        let mut s = app();
        s.cursor = 1;
        s.preview_open = true;
        s.preview = FilePreview::Ready {
            path: "src/reducer.rs".into(),
            text: "@@ -1 +1 @@\n-old line\n+new line".into(),
        };
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // STAT-09: per-file action menu overlay.
    #[test]
    fn stat_09_menu_snapshot() {
        let mut s = app();
        s.cursor = 1;
        s.mode = StatusMode::Menu;
        s.menu = Some(FileMenu {
            items: vec![FileAction::Stage, FileAction::Discard, FileAction::CopyPath],
            cursor: 0,
            path: "src/reducer.rs".into(),
            untracked: false,
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // STAT-07: discard confirmation overlay.
    #[test]
    fn stat_07_confirm_snapshot() {
        let mut s = app();
        s.cursor = 3;
        s.mode = StatusMode::Confirm;
        s.confirm = Some(ConfirmDiscard {
            path: "notes.txt".into(),
            untracked: true,
        });
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // STAT-14: clean working tree empty state.
    #[test]
    fn stat_14_clean_snapshot() {
        let mut s = StatusState::new("main".into());
        s.size = (80, 8);
        s.load = StatusLoad::Loaded(vec![]);
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    #[test]
    fn loading_snapshot() {
        let mut s = StatusState::new("main".into());
        s.size = (80, 8);
        s.load = StatusLoad::Loading;
        insta::assert_snapshot!(render_to_string(&s, 80, 8));
    }

    // A style assertion: selected row is reversed, consistent with the log list.
    #[test]
    fn selected_row_is_reversed() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let s = app();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| draw_status(f, &s)).unwrap();
        let buf = term.backend().buffer();
        // Row index 1 is the first list row (header above); cursor = 0.
        let cell = &buf[(0, 1)];
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "selected row should be reversed"
        );
    }
}
