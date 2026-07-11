//! The pure state transition for `gitt status`. `update_status` is the single entry point.
//!
//! Every stage/unstage/discard emits an [`Effect`] and, when it finishes, the shell reports back a
//! `StatusMutated` event that reloads the list from git — so the view always reflects real repo
//! state rather than an optimistic guess (STAT-10).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::event::Event;
use super::status::{
    ConfirmDiscard, FileAction, FileMenu, FilePreview, StatusLoad, StatusMode, StatusState,
};

/// Fold an event into the status-screen state, returning the side effects the shell must perform.
pub fn update_status(state: &mut StatusState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            vec![]
        }
        Event::StatusLoaded(entries) => {
            state.load = StatusLoad::Loaded(entries);
            state.clamp_cursor();
            state.clamp_scroll();
            if state.preview_open {
                request_diff(state)
            } else {
                vec![]
            }
        }
        Event::StatusFailed(error) => {
            state.status = Some(format!("status failed: {error}"));
            state.load = StatusLoad::Failed(error);
            vec![]
        }
        Event::FileDiffLoaded { path, text } => {
            if state.preview_open && state.selected_path().as_deref() == Some(path.as_str()) {
                state.preview = FilePreview::Ready { path, text };
            }
            vec![]
        }
        Event::FileDiffFailed { path, error } => {
            if state.preview_open && state.selected_path().as_deref() == Some(path.as_str()) {
                state.preview = FilePreview::Failed { path, error };
            }
            vec![]
        }
        Event::StatusMutated { label, result } => {
            state.status = Some(match result {
                Ok(()) => label,
                Err(e) => format!("{label} failed: {e}"),
            });
            // Reload from git regardless of outcome so the view can't drift.
            vec![Effect::LoadStatus]
        }
        // Log-screen events never reach the status reducer at runtime (separate screen); ignore.
        _ => vec![],
    }
}

fn on_key(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    // Ctrl-C always quits, from any mode.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return vec![Effect::Quit];
    }
    match state.mode {
        StatusMode::List => on_key_list(state, key),
        StatusMode::Menu => on_key_menu(state, key),
        StatusMode::Confirm => on_key_confirm(state, key),
    }
}

fn on_key_list(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let half = (state.viewport_rows() / 2).max(1) as isize;
    let page = state.viewport_rows().max(1) as isize;

    match key.code {
        KeyCode::Char('q') => quit(state),
        KeyCode::Char('j') | KeyCode::Down => move_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up => move_by(state, -1),
        KeyCode::Char('g') => set_cursor(state, 0),
        KeyCode::Char('G') => set_cursor(state, state.entries().len().saturating_sub(1)),
        KeyCode::Char('d') if ctrl => move_by(state, half),
        KeyCode::Char('u') if ctrl => move_by(state, -half),
        KeyCode::Char('f') if ctrl => move_by(state, page),
        KeyCode::Char('b') if ctrl => move_by(state, -page),
        KeyCode::Tab => toggle_preview(state),
        KeyCode::Char('R') => reload(state),
        KeyCode::Char(' ') => toggle_stage(state),
        KeyCode::Char('s') => stage_selected(state),
        KeyCode::Char('u') => unstage_selected(state),
        KeyCode::Char('d') => open_confirm(state),
        KeyCode::Enter => open_menu(state),
        _ => vec![],
    }
}

fn on_key_menu(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.menu = None;
            state.mode = StatusMode::List;
            vec![]
        }
        KeyCode::Char('j') | KeyCode::Down => {
            menu_move(state, 1);
            vec![]
        }
        KeyCode::Char('k') | KeyCode::Up => {
            menu_move(state, -1);
            vec![]
        }
        KeyCode::Enter => execute_menu(state),
        _ => vec![],
    }
}

fn on_key_confirm(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => confirm_discard(state),
        KeyCode::Esc | KeyCode::Char('n') => {
            state.confirm = None;
            state.mode = StatusMode::List;
            vec![]
        }
        _ => vec![],
    }
}

// --- helpers -------------------------------------------------------------------------------------

fn quit(state: &mut StatusState) -> Vec<Effect> {
    state.should_quit = true;
    vec![Effect::Quit]
}

fn reload(state: &mut StatusState) -> Vec<Effect> {
    state.status = Some("reloading…".to_string());
    vec![Effect::LoadStatus]
}

fn move_by(state: &mut StatusState, delta: isize) -> Vec<Effect> {
    if state.entries().is_empty() {
        return vec![];
    }
    let len = state.entries().len() as isize;
    let target = (state.cursor as isize + delta).clamp(0, len - 1);
    set_cursor(state, target as usize)
}

fn set_cursor(state: &mut StatusState, idx: usize) -> Vec<Effect> {
    if state.entries().is_empty() {
        return vec![];
    }
    let before = state.selected_path();
    state.cursor = idx.min(state.entries().len() - 1);
    state.clamp_scroll();
    if state.preview_open && state.selected_path() != before {
        request_diff(state)
    } else {
        vec![]
    }
}

fn toggle_preview(state: &mut StatusState) -> Vec<Effect> {
    state.preview_open = !state.preview_open;
    if state.preview_open {
        request_diff(state)
    } else {
        state.preview = FilePreview::Idle;
        vec![]
    }
}

/// Ask the shell to load the diff for the current selection into the preview.
fn request_diff(state: &mut StatusState) -> Vec<Effect> {
    match state.selected() {
        Some(entry) => {
            let path = entry.path.clone();
            let kind = entry.diff_kind();
            state.preview = FilePreview::Loading(path.clone());
            vec![Effect::LoadFileDiff { path, kind }]
        }
        None => {
            state.preview = FilePreview::Idle;
            vec![]
        }
    }
}

/// `Space`: stage a file with worktree/untracked changes, otherwise unstage it.
fn toggle_stage(state: &mut StatusState) -> Vec<Effect> {
    match state.selected() {
        Some(entry) if entry.has_worktree_changes() => stage_selected(state),
        Some(_) => unstage_selected(state),
        None => vec![],
    }
}

fn stage_selected(state: &mut StatusState) -> Vec<Effect> {
    match state.selected_path() {
        Some(path) => {
            state.status = Some(format!("Staging {path}…"));
            vec![Effect::Stage(path)]
        }
        None => vec![],
    }
}

fn unstage_selected(state: &mut StatusState) -> Vec<Effect> {
    match state.selected_path() {
        Some(path) => {
            state.status = Some(format!("Unstaging {path}…"));
            vec![Effect::Unstage(path)]
        }
        None => vec![],
    }
}

fn open_confirm(state: &mut StatusState) -> Vec<Effect> {
    if let Some(entry) = state.selected() {
        state.confirm = Some(ConfirmDiscard {
            path: entry.path.clone(),
            untracked: entry.is_untracked(),
        });
        state.mode = StatusMode::Confirm;
    }
    vec![]
}

fn confirm_discard(state: &mut StatusState) -> Vec<Effect> {
    state.mode = StatusMode::List;
    match state.confirm.take() {
        Some(c) => {
            state.status = Some(format!("Discarding {}…", c.path));
            vec![Effect::Discard {
                path: c.path,
                untracked: c.untracked,
            }]
        }
        None => vec![],
    }
}

fn open_menu(state: &mut StatusState) -> Vec<Effect> {
    if let Some(entry) = state.selected() {
        // Offer the stage/unstage action that matches the file's current side.
        let toggle = if entry.has_worktree_changes() {
            FileAction::Stage
        } else {
            FileAction::Unstage
        };
        state.menu = Some(FileMenu {
            items: vec![toggle, FileAction::Discard, FileAction::CopyPath],
            cursor: 0,
            path: entry.path.clone(),
            untracked: entry.is_untracked(),
        });
        state.mode = StatusMode::Menu;
    }
    vec![]
}

fn menu_move(state: &mut StatusState, delta: isize) {
    if let Some(menu) = &mut state.menu {
        let len = menu.items.len() as isize;
        menu.cursor = (menu.cursor as isize + delta).clamp(0, len - 1) as usize;
    }
}

fn execute_menu(state: &mut StatusState) -> Vec<Effect> {
    let Some(menu) = state.menu.take() else {
        state.mode = StatusMode::List;
        return vec![];
    };
    let action = menu.selected();
    let path = menu.path;

    match action {
        FileAction::Stage => {
            state.mode = StatusMode::List;
            state.status = Some(format!("Staging {path}…"));
            vec![Effect::Stage(path)]
        }
        FileAction::Unstage => {
            state.mode = StatusMode::List;
            state.status = Some(format!("Unstaging {path}…"));
            vec![Effect::Unstage(path)]
        }
        FileAction::Discard => {
            // Route through the mandatory confirmation overlay.
            state.confirm = Some(ConfirmDiscard {
                path,
                untracked: menu.untracked,
            });
            state.mode = StatusMode::Confirm;
            vec![]
        }
        FileAction::CopyPath => {
            state.mode = StatusMode::List;
            state.status = Some("Copied path".to_string());
            vec![Effect::CopyToClipboard(path)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::StatusEntry;

    fn entry(index: char, worktree: char, path: &str) -> StatusEntry {
        StatusEntry {
            index,
            worktree,
            path: path.to_string(),
            orig_path: None,
        }
    }

    fn app() -> StatusState {
        let mut s = StatusState::new("main".into());
        s.size = (80, 24);
        s.load = StatusLoad::Loaded(vec![
            entry('A', ' ', "staged_new.txt"),
            entry(' ', 'M', "tracked.txt"),
            entry('M', 'M', "both.txt"),
            entry('?', '?', "untracked.txt"),
        ]);
        s
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }
    fn ch(c: char) -> Event {
        key(KeyCode::Char(c))
    }

    fn drive(state: &mut StatusState, events: Vec<Event>) -> Vec<Effect> {
        let mut all = Vec::new();
        for e in events {
            all.extend(update_status(state, e));
        }
        all
    }

    // STAT-03: vim motions move within bounds.
    #[test]
    fn stat_03_jk_moves_and_clamps() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]);
        assert_eq!(s.cursor, 2);
        drive(&mut s, vec![ch('k')]);
        assert_eq!(s.cursor, 1);
        drive(&mut s, vec![ch('k'), ch('k'), ch('k')]);
        assert_eq!(s.cursor, 0);
        drive(&mut s, vec![ch('G')]);
        assert_eq!(s.cursor, 3);
        drive(&mut s, vec![ch('g')]);
        assert_eq!(s.cursor, 0);
    }

    // STAT-04: Space stages a file with worktree changes and unstages a fully-staged one.
    #[test]
    fn stat_04_space_toggles_by_worktree_state() {
        let mut s = app();
        // cursor 0 = staged_new.txt (A ) -> fully staged -> Space unstages.
        let effects = update_status(&mut s, ch(' '));
        assert_eq!(effects, vec![Effect::Unstage("staged_new.txt".into())]);

        // cursor 1 = tracked.txt ( M) -> worktree change -> Space stages.
        update_status(&mut s, ch('j'));
        let effects = update_status(&mut s, ch(' '));
        assert_eq!(effects, vec![Effect::Stage("tracked.txt".into())]);
    }

    // STAT-05: s always stages, u always unstages, regardless of the badge.
    #[test]
    fn stat_05_s_stages_u_unstages() {
        let mut s = app();
        // On the fully-staged file, `s` still stages (a no-op in git, but section-independent).
        let effects = update_status(&mut s, ch('s'));
        assert_eq!(effects, vec![Effect::Stage("staged_new.txt".into())]);
        let effects = update_status(&mut s, ch('u'));
        assert_eq!(effects, vec![Effect::Unstage("staged_new.txt".into())]);
    }

    // STAT-06: Tab toggles the preview and requests the right diff kind.
    #[test]
    fn stat_06_tab_requests_diff_by_kind() {
        let mut s = app();
        // cursor 1 = tracked.txt ( M) -> Worktree diff.
        update_status(&mut s, ch('j'));
        let effects = update_status(&mut s, key(KeyCode::Tab));
        assert_eq!(
            effects,
            vec![Effect::LoadFileDiff {
                path: "tracked.txt".into(),
                kind: crate::domain::DiffKind::Worktree,
            }]
        );
        assert!(s.preview_open);

        // Toggle off.
        let effects = update_status(&mut s, key(KeyCode::Tab));
        assert!(!s.preview_open);
        assert_eq!(effects, vec![]);
        assert_eq!(s.preview, FilePreview::Idle);
    }

    #[test]
    fn stat_06_untracked_previews_contents() {
        let mut s = app();
        drive(&mut s, vec![ch('G')]); // untracked.txt
        let effects = update_status(&mut s, key(KeyCode::Tab));
        assert_eq!(
            effects,
            vec![Effect::LoadFileDiff {
                path: "untracked.txt".into(),
                kind: crate::domain::DiffKind::Untracked,
            }]
        );
    }

    // STAT-07 / STAT-08: d opens confirm; y/Enter discards; Esc cancels.
    #[test]
    fn stat_07_discard_requires_confirmation() {
        let mut s = app();
        drive(&mut s, vec![ch('G')]); // untracked.txt
        update_status(&mut s, ch('d'));
        assert_eq!(s.mode, StatusMode::Confirm);
        assert_eq!(s.confirm.as_ref().unwrap().path, "untracked.txt");
        assert!(s.confirm.as_ref().unwrap().untracked);

        let effects = update_status(&mut s, ch('y'));
        assert_eq!(
            effects,
            vec![Effect::Discard {
                path: "untracked.txt".into(),
                untracked: true,
            }]
        );
        assert_eq!(s.mode, StatusMode::List);
        assert!(s.confirm.is_none());
    }

    #[test]
    fn stat_08_confirm_cancel_does_nothing() {
        let mut s = app();
        update_status(&mut s, ch('d'));
        assert_eq!(s.mode, StatusMode::Confirm);
        let effects = update_status(&mut s, key(KeyCode::Esc));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, StatusMode::List);
        assert!(s.confirm.is_none());
    }

    #[test]
    fn stat_08_other_keys_do_not_discard() {
        let mut s = app();
        update_status(&mut s, ch('d'));
        let effects = update_status(&mut s, ch('x'));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, StatusMode::Confirm, "still awaiting a decision");
    }

    // STAT-10: a finished mutation reloads the status from git.
    #[test]
    fn stat_10_mutation_triggers_reload() {
        let mut s = app();
        let effects = update_status(
            &mut s,
            Event::StatusMutated {
                label: "Staged".into(),
                result: Ok(()),
            },
        );
        assert_eq!(effects, vec![Effect::LoadStatus]);
        assert_eq!(s.status.as_deref(), Some("Staged"));
    }

    #[test]
    fn stat_10_reload_clamps_cursor() {
        let mut s = app();
        drive(&mut s, vec![ch('G')]); // cursor 3
        // Reload with a shorter list; cursor must clamp into range.
        update_status(
            &mut s,
            Event::StatusLoaded(vec![entry(' ', 'M', "only.txt")]),
        );
        assert_eq!(s.cursor, 0);
        assert_eq!(s.selected_path().as_deref(), Some("only.txt"));
    }

    // STAT-09: Enter opens the per-file action menu; Esc closes it.
    #[test]
    fn stat_09_enter_opens_menu() {
        let mut s = app();
        update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(s.mode, StatusMode::Menu);
        let menu = s.menu.as_ref().unwrap();
        // cursor 0 is fully staged -> first item is Unstage.
        assert_eq!(menu.items[0], FileAction::Unstage);
        assert_eq!(menu.items[1], FileAction::Discard);
        assert_eq!(menu.items[2], FileAction::CopyPath);

        update_status(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, StatusMode::List);
        assert!(s.menu.is_none());
    }

    #[test]
    fn stat_09_menu_discard_routes_through_confirm() {
        let mut s = app();
        update_status(&mut s, key(KeyCode::Enter));
        update_status(&mut s, ch('j')); // move to Discard
        assert_eq!(s.menu.as_ref().unwrap().selected(), FileAction::Discard);
        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![], "discard waits for confirmation");
        assert_eq!(s.mode, StatusMode::Confirm);
    }

    #[test]
    fn stat_09_menu_copy_path() {
        let mut s = app();
        update_status(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j')]); // Copy path
        assert_eq!(s.menu.as_ref().unwrap().selected(), FileAction::CopyPath);
        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(
            effects,
            vec![Effect::CopyToClipboard("staged_new.txt".into())]
        );
    }

    // STAT-11: a staged+modified file (MM) stages its worktree portion via Space.
    #[test]
    fn stat_11_both_sided_file_stages_worktree() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // both.txt (MM)
        assert_eq!(s.selected().unwrap().badge(), "MM");
        let effects = update_status(&mut s, ch(' '));
        assert_eq!(effects, vec![Effect::Stage("both.txt".into())]);
    }

    // STAT-12: R reloads the status.
    #[test]
    fn stat_12_r_reloads() {
        let mut s = app();
        let effects = update_status(&mut s, ch('R'));
        assert_eq!(effects, vec![Effect::LoadStatus]);
    }

    // STAT-13: q and Ctrl-c quit.
    #[test]
    fn stat_13_quit_keys() {
        let mut s = app();
        let effects = update_status(&mut s, ch('q'));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        let mut s2 = app();
        let effects = update_status(&mut s2, ctrl('c'));
        assert!(s2.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    // Empty tree: motions and actions are safe no-ops.
    #[test]
    fn empty_tree_actions_are_noops() {
        let mut s = StatusState::new("main".into());
        s.load = StatusLoad::Loaded(vec![]);
        assert_eq!(update_status(&mut s, ch(' ')), vec![]);
        assert_eq!(update_status(&mut s, ch('j')), vec![]);
        assert_eq!(update_status(&mut s, key(KeyCode::Enter)), vec![]);
        assert_eq!(update_status(&mut s, ch('d')), vec![]);
        assert_eq!(s.mode, StatusMode::List);
    }

    #[test]
    fn selection_change_reloads_preview() {
        let mut s = app();
        update_status(&mut s, key(KeyCode::Tab)); // open preview on cursor 0
        let effects = update_status(&mut s, ch('j'));
        assert_eq!(
            effects,
            vec![Effect::LoadFileDiff {
                path: "tracked.txt".into(),
                kind: crate::domain::DiffKind::Worktree,
            }]
        );
    }
}
