//! Pure rendering for `gitt status`: `draw_status(frame, state)` reads only `&StatusState` and
//! writes only into the frame. It reuses `theme` and the shared `components` so it is visually
//! consistent with `gitt log`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use super::components::{self, dim_area, overlay_menu, preview_pane, truncate};
use super::theme;
use crate::domain::{StatusEntry, text};
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
        StatusMode::Menu | StatusMode::Confirm | StatusMode::Commit => dim_area(frame, area),
        StatusMode::List => {}
    }
    match state.mode {
        StatusMode::Menu => render_menu(frame, chunks[1], state),
        StatusMode::Confirm => render_confirm(frame, chunks[1], state),
        StatusMode::Commit => render_commit(frame, chunks[1], state),
        StatusMode::List => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &StatusState) {
    let entries = state.entries();
    let staged = entries.iter().filter(|e| e.is_staged()).count();
    let title = format!(" {} (status) ", state.branch);
    let summary = format!("  {} changed · {} staged", entries.len(), staged);
    let mut spans = vec![
        Span::styled(title, theme::active_view()),
        Span::styled(summary, theme::dim()),
    ];
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

fn render_body(frame: &mut Frame, area: Rect, state: &StatusState) {
    // The diff pane sits BELOW the file list (vertical split) so it spans the full width — room for a
    // side-by-side layout. `f` grows it from 50% to 90% of the height (list stays visible at 10%).
    let (list_area, preview_area) = if state.preview_open {
        let diff_pct = if state.expanded { 90 } else { 50 };
        let rows = Layout::new(
            Direction::Vertical,
            [
                Constraint::Percentage(100 - diff_pct),
                Constraint::Percentage(diff_pct),
            ],
        )
        .split(area);
        (rows[0], Some(rows[1]))
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
    preview_pane(frame, area, "diff", &text, state.preview_scroll);
}

fn render_status(frame: &mut Frame, area: Rect, _state: &StatusState) {
    let text = "j/k · space stage · S/U all · c commit · a amend · d/D discard · Tab diff · q quit";
    frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
}

fn render_menu(frame: &mut Frame, body: Rect, state: &StatusState) {
    let Some(menu) = &state.menu else { return };
    let title = format!(" {} ", truncate(&menu.path, 30));
    let labels: Vec<&str> = menu.items.iter().map(|a| a.label()).collect();
    overlay_menu(frame, body, &title, &labels, menu.cursor);
}

fn render_confirm(frame: &mut Frame, body: Rect, state: &StatusState) {
    use crate::state::ConfirmDiscard;
    let Some(confirm) = &state.confirm else {
        return;
    };
    let question = match confirm {
        ConfirmDiscard::File { path, untracked } => {
            let verb = if *untracked {
                "Delete"
            } else {
                "Discard changes to"
            };
            format!("{verb} {}?", truncate(path, 40))
        }
        ConfirmDiscard::All => "Discard ALL changes?".to_string(),
    };
    let hint = "y discard · n cancel";

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

/// The commit-message editor overlay (`c` / `a`). A centered, bordered box — same chrome as the
/// discard confirmation and the branch create-input — showing the message (or a placeholder /
/// streaming spinner), an optional inline hint, and the keymap.
fn render_commit(frame: &mut Frame, body: Rect, state: &StatusState) {
    let Some(editor) = &state.commit else {
        return;
    };

    let inner_w = 58usize.min(body.width.saturating_sub(2).max(10) as usize);
    let empty = editor.message.is_empty();

    let msg_lines: Vec<Line> = if editor.busy && empty {
        let label = if editor.amend {
            "loading…"
        } else {
            "suggesting…"
        };
        vec![Line::styled(format!(" {label}"), theme::dim())]
    } else if empty {
        vec![Line::styled(" █", theme::dim())]
    } else {
        let cursor = if editor.busy { "▌" } else { "█" };
        let mut wrapped = text::wrap_words(&editor.message, inner_w);
        if wrapped.is_empty() {
            wrapped.push(String::new());
        }
        let last = wrapped.len() - 1;
        wrapped[last].push_str(cursor);
        wrapped
            .into_iter()
            .map(|l| Line::styled(format!(" {l}"), theme::subject()))
            .collect()
    };

    let mut lines = msg_lines;
    if let Some(hint) = &editor.hint {
        lines.push(Line::styled(format!(" {hint}"), theme::error()));
    }
    let hint = if empty && !editor.busy {
        " Enter send · @ suggest · Esc cancel"
    } else {
        " Enter send · Esc cancel"
    };
    lines.push(Line::styled(hint, theme::dim()));

    let title = if editor.amend {
        " Amend commit "
    } else {
        " Commit "
    };
    let height = lines.len() as u16 + 2;
    let width = (inner_w as u16 + 2).max(title.len() as u16 + 4);
    let area = components::centered_rect(body, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
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
    use crate::state::{CommitEditor, FileAction, FileMenu};

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
        s.confirm = Some(crate::state::ConfirmDiscard::File {
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

    // CMT-01: an empty commit editor shows the placeholder + suggest hint.
    #[test]
    fn cmt_01_commit_editor_empty_snapshot() {
        let mut s = app();
        s.mode = StatusMode::Commit;
        s.commit = Some(CommitEditor::new(false));
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // CMT-03: a commit editor with a typed message shows it with a cursor.
    #[test]
    fn cmt_03_commit_editor_typed_snapshot() {
        let mut s = app();
        s.mode = StatusMode::Commit;
        let mut editor = CommitEditor::new(false);
        editor.message = "Add the commit editor".into();
        s.commit = Some(editor);
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // CMT-05: the amend editor is titled differently and prefilled.
    #[test]
    fn cmt_05_amend_editor_snapshot() {
        let mut s = app();
        s.mode = StatusMode::Commit;
        let mut editor = CommitEditor::new(true);
        editor.message = "base".into();
        s.commit = Some(editor);
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
    }

    // CMT-06: a suggestion in flight shows the spinner; a failure shows the inline hint.
    #[test]
    fn cmt_06_commit_editor_suggesting_snapshot() {
        let mut s = app();
        s.mode = StatusMode::Commit;
        let mut editor = CommitEditor::new(false);
        editor.busy = true;
        s.commit = Some(editor);
        insta::assert_snapshot!(render_to_string(&s, 80, 12));
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
