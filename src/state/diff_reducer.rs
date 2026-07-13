//! The pure state transition for `gitt diff`. `update_diff` is the single entry point.
//!
//! `gitt diff` is a read-only viewer: no effect here mutates the repository. Switching scope with
//! `←`/`→` loads that scope's file list once and caches it (like the log's per-view cache); moving
//! the selection or switching scope reloads the diff pane so it always reflects the active scope.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::diff::{DiffAction, DiffLoad, DiffMenu, DiffMode, DiffPreview, DiffState};
use super::effect::Effect;
use super::event::Event;
use crate::domain::DiffScope;

/// Fold an event into the diff-screen state, returning the side effects the shell must perform.
pub fn update_diff(state: &mut DiffState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            // Re-render the pane for the new width so the diff tool re-picks split vs unified as the
            // window grows or shrinks (this is what makes the view responsive to a live resize).
            if state.preview_open && state.selected_path().is_some() {
                return request_diff(state);
            }
            vec![]
        }
        Event::DiffFilesLoaded { scope, files } => {
            state.loads.insert(scope, DiffLoad::Loaded(files));
            if scope == state.scope {
                state.clamp_cursor();
                state.clamp_scroll();
                if state.preview_open {
                    return request_diff(state);
                }
            }
            vec![]
        }
        Event::DiffFilesFailed { scope, error } => {
            if scope == state.scope {
                state.status = Some(format!("diff failed: {error}"));
            }
            state.loads.insert(scope, DiffLoad::Failed(error));
            vec![]
        }
        Event::DiffTextLoaded { scope, path, text } => {
            if state.preview_open
                && scope == state.scope
                && state.selected_path().as_deref() == Some(path.as_str())
            {
                state.preview = DiffPreview::Ready { path, text };
            }
            vec![]
        }
        Event::DiffTextFailed { scope, path, error } => {
            if state.preview_open
                && scope == state.scope
                && state.selected_path().as_deref() == Some(path.as_str())
            {
                state.preview = DiffPreview::Failed { path, error };
            }
            vec![]
        }
        // A copy action finished; report its outcome on the status line (as the log screen does).
        Event::ActionFinished { label, result } => {
            state.status = Some(match result {
                Ok(()) => label,
                Err(e) => format!("{label} failed: {e}"),
            });
            vec![]
        }
        // Events for the other screens never reach this reducer at runtime; ignore.
        _ => vec![],
    }
}

fn on_key(state: &mut DiffState, key: KeyEvent) -> Vec<Effect> {
    // Ctrl-C always quits, from any mode.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return vec![Effect::Quit];
    }
    match state.mode {
        DiffMode::List => on_key_list(state, key),
        DiffMode::Menu => on_key_menu(state, key),
    }
}

fn on_key_list(state: &mut DiffState, key: KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let half = (state.viewport_rows() / 2).max(1) as isize;
    let page = state.viewport_rows().max(1) as isize;

    match key.code {
        // Esc from the base list quits; the Menu handles its own Esc first, so repeated Esc is always
        // the way out (consistent across every gitt screen).
        KeyCode::Char('q') | KeyCode::Esc => quit(state),
        // Shift+j/k and Shift+↓/↑ scroll the diff pane (the plain keys move the file selection).
        KeyCode::Char('J') => scroll_preview(state, 1),
        KeyCode::Char('K') => scroll_preview(state, -1),
        KeyCode::Down if shift => scroll_preview(state, 1),
        KeyCode::Up if shift => scroll_preview(state, -1),
        KeyCode::Char('j') | KeyCode::Down => move_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up => move_by(state, -1),
        KeyCode::Char('g') => set_cursor(state, 0),
        KeyCode::Char('G') => set_cursor(state, state.files().len().saturating_sub(1)),
        KeyCode::Char('d') if ctrl => move_by(state, half),
        KeyCode::Char('u') if ctrl => move_by(state, -half),
        KeyCode::Char('f') if ctrl => move_by(state, page),
        KeyCode::Char('b') if ctrl => move_by(state, -page),
        KeyCode::Right => switch_scope(state, state.scope.next()),
        KeyCode::Left => switch_scope(state, state.scope.prev()),
        KeyCode::Tab => toggle_preview(state),
        KeyCode::Char('f') => toggle_expanded(state),
        KeyCode::Char('R') => reload(state),
        KeyCode::Enter => open_menu(state),
        _ => vec![],
    }
}

fn on_key_menu(state: &mut DiffState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.menu = None;
            state.mode = DiffMode::List;
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

// --- helpers -------------------------------------------------------------------------------------

fn quit(state: &mut DiffState) -> Vec<Effect> {
    state.should_quit = true;
    vec![Effect::Quit]
}

fn reload(state: &mut DiffState) -> Vec<Effect> {
    state.status = Some("reloading…".to_string());
    vec![Effect::LoadDiffFiles(state.scope)]
}

/// Switch to another scope: reset the selection, load the scope's file list if not cached, and
/// refresh the diff pane for the new selection.
fn switch_scope(state: &mut DiffState, scope: DiffScope) -> Vec<Effect> {
    if scope == state.scope {
        return vec![];
    }
    state.scope = scope;
    state.cursor = 0;
    state.top = 0;
    state.status = None;

    match state.loads.get(&scope) {
        // Already loaded: just refresh the pane for the new selection.
        Some(DiffLoad::Loaded(_)) => {
            state.clamp_cursor();
            state.clamp_scroll();
            if state.preview_open {
                request_diff(state)
            } else {
                vec![]
            }
        }
        // Not cached (or failed/loading): (re)load its file list; the pane refreshes on arrival.
        _ => {
            state.loads.insert(scope, DiffLoad::Loading);
            state.preview = DiffPreview::Idle;
            vec![Effect::LoadDiffFiles(scope)]
        }
    }
}

fn move_by(state: &mut DiffState, delta: isize) -> Vec<Effect> {
    if state.files().is_empty() {
        return vec![];
    }
    let len = state.files().len() as isize;
    let target = (state.cursor as isize + delta).clamp(0, len - 1);
    set_cursor(state, target as usize)
}

fn set_cursor(state: &mut DiffState, idx: usize) -> Vec<Effect> {
    if state.files().is_empty() {
        return vec![];
    }
    let before = state.selected_path();
    state.cursor = idx.min(state.files().len() - 1);
    state.clamp_scroll();
    if state.preview_open && state.selected_path() != before {
        request_diff(state)
    } else {
        vec![]
    }
}

fn toggle_preview(state: &mut DiffState) -> Vec<Effect> {
    state.preview_open = !state.preview_open;
    if state.preview_open {
        request_diff(state)
    } else {
        // A hidden pane can't be expanded; reset so re-opening starts at the split layout.
        state.expanded = false;
        state.preview = DiffPreview::Idle;
        vec![]
    }
}

/// Toggle the expanded diff pane (`f`): the diff grows to 90% of the height (the file list shrinks
/// to 10% but stays visible) so a large diff is easier to read. A no-op when the pane is closed.
/// (The pane always spans the full width, so this doesn't change the diff-tool layout — no reload.)
fn toggle_expanded(state: &mut DiffState) -> Vec<Effect> {
    if !state.preview_open {
        return vec![];
    }
    state.expanded = !state.expanded;
    // Keep the scroll offset valid for the pane's new height.
    state.preview_scroll = state.preview_scroll.min(state.max_preview_scroll());
    vec![]
}

/// Scroll the diff pane by `delta` lines (Shift+j/k, Shift+↓/↑), clamped so it can't overscroll.
fn scroll_preview(state: &mut DiffState, delta: i32) -> Vec<Effect> {
    if !matches!(state.preview, DiffPreview::Ready { .. }) {
        return vec![];
    }
    let max = state.max_preview_scroll() as i32;
    state.preview_scroll = (state.preview_scroll as i32 + delta).clamp(0, max) as u16;
    vec![]
}

/// Ask the shell to load the diff for the current selection into the pane.
fn request_diff(state: &mut DiffState) -> Vec<Effect> {
    match state.selected_path() {
        Some(path) => {
            state.preview = DiffPreview::Loading(path.clone());
            state.preview_scroll = 0; // a fresh diff starts at the top
            vec![Effect::LoadDiffText {
                scope: state.scope,
                path,
                width: state.preview_width(),
            }]
        }
        None => {
            state.preview = DiffPreview::Idle;
            vec![]
        }
    }
}

fn open_menu(state: &mut DiffState) -> Vec<Effect> {
    if let Some(file) = state.selected() {
        state.menu = Some(DiffMenu {
            items: vec![DiffAction::CopyPath, DiffAction::CopyDiff],
            cursor: 0,
            path: file.path.clone(),
        });
        state.mode = DiffMode::Menu;
    }
    vec![]
}

fn menu_move(state: &mut DiffState, delta: isize) {
    if let Some(menu) = &mut state.menu {
        let len = menu.items.len() as isize;
        menu.cursor = (menu.cursor as isize + delta).clamp(0, len - 1) as usize;
    }
}

fn execute_menu(state: &mut DiffState) -> Vec<Effect> {
    let Some(menu) = state.menu.take() else {
        state.mode = DiffMode::List;
        return vec![];
    };
    state.mode = DiffMode::List;
    let action = menu.selected();
    let path = menu.path;

    // The status line is set on completion via `ActionFinished` (so it reflects the real outcome and
    // surfaces failures), mirroring the log screen's copy actions — not optimistically here.
    match action {
        DiffAction::CopyPath => vec![Effect::CopyToClipboard(path)],
        DiffAction::CopyDiff => vec![Effect::CopyScopeDiff {
            scope: state.scope,
            path,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DiffFile;

    fn file(status: char, path: &str) -> DiffFile {
        DiffFile {
            status,
            path: path.to_string(),
            orig_path: None,
        }
    }

    fn app() -> DiffState {
        let mut s = DiffState::new("main".into());
        s.size = (80, 24);
        s.loads.insert(
            DiffScope::Unstaged,
            DiffLoad::Loaded(vec![
                file('M', "a.rs"),
                file('M', "b.rs"),
                file('A', "c.rs"),
                file('D', "d.rs"),
            ]),
        );
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
    fn drive(state: &mut DiffState, events: Vec<Event>) -> Vec<Effect> {
        let mut all = Vec::new();
        for e in events {
            all.extend(update_diff(state, e));
        }
        all
    }

    // DIFF-03: vim motions move within bounds.
    #[test]
    fn diff_03_jk_moves_and_clamps() {
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

    // DIFF-04: →/← cycle the scope and load each once; a cached scope is not reloaded.
    #[test]
    fn diff_04_arrows_cycle_scopes_and_cache() {
        let mut s = app();
        // → to Staged: not cached yet → loads it.
        let effects = update_diff(&mut s, key(KeyCode::Right));
        assert_eq!(s.scope, DiffScope::Staged);
        assert_eq!(effects, vec![Effect::LoadDiffFiles(DiffScope::Staged)]);

        // Pretend it loaded.
        update_diff(
            &mut s,
            Event::DiffFilesLoaded {
                scope: DiffScope::Staged,
                files: vec![file('A', "s.rs")],
            },
        );

        // ← back to Unstaged: already cached → no reload effect, just refresh the pane.
        let effects = update_diff(&mut s, key(KeyCode::Left));
        assert_eq!(s.scope, DiffScope::Unstaged);
        assert!(
            !effects.contains(&Effect::LoadDiffFiles(DiffScope::Unstaged)),
            "a cached scope must not reload its file list"
        );

        // → to Staged again: cached now → no reload.
        let effects = update_diff(&mut s, key(KeyCode::Right));
        assert!(!effects.contains(&Effect::LoadDiffFiles(DiffScope::Staged)));
    }

    // DIFF-04: arrows wrap around the four scopes.
    #[test]
    fn diff_04_arrows_wrap() {
        let mut s = app();
        assert_eq!(s.scope, DiffScope::Unstaged);
        // ← from the first scope wraps to the last (Branch).
        update_diff(&mut s, key(KeyCode::Left));
        assert_eq!(s.scope, DiffScope::Branch);
        // → from the last wraps back to the first.
        update_diff(&mut s, key(KeyCode::Right));
        assert_eq!(s.scope, DiffScope::Unstaged);
    }

    // DIFF-05/07: switching to a cached scope refreshes the diff pane for the new selection,
    // scoped to the new scope.
    #[test]
    fn diff_07_scope_switch_reloads_pane() {
        let mut s = app();
        // Open pane on Unstaged/a.rs first.
        update_diff(&mut s, key(KeyCode::Tab)); // toggles: it was open by default...
        // ...so re-open it.
        update_diff(&mut s, key(KeyCode::Tab));
        assert!(s.preview_open);

        // Cache Staged with a file, then switch to it: pane reloads for Staged's first file.
        s.loads
            .insert(DiffScope::Staged, DiffLoad::Loaded(vec![file('A', "s.rs")]));
        let effects = update_diff(&mut s, key(KeyCode::Right));
        assert_eq!(
            effects,
            vec![Effect::LoadDiffText {
                scope: DiffScope::Staged,
                path: "s.rs".into(),
                width: 78,
            }]
        );
    }

    // DIFF-18: `f` expands the diff pane to 90% of the height (list shrinks but stays visible), and
    // `f` again restores the split. The pane spans the full width in both states, so the diff-tool
    // width is unchanged and no reload is needed.
    #[test]
    fn diff_18_f_toggles_expanded_height() {
        let mut s = app(); // 80x24, pane open, cursor on a.rs
        assert!(!s.expanded);
        let width = s.preview_width();
        let split_rows = s.preview_height();

        let effects = update_diff(&mut s, ch('f'));
        assert!(s.expanded, "f expands the diff pane");
        assert_eq!(effects, vec![], "no reload — width is unchanged");
        assert_eq!(
            s.preview_width(),
            width,
            "pane is full-width in both states"
        );
        assert!(
            s.preview_height() > split_rows,
            "expanded pane is taller ({} > {split_rows})",
            s.preview_height()
        );

        // Toggle back.
        update_diff(&mut s, ch('f'));
        assert!(!s.expanded);
        assert_eq!(s.preview_height(), split_rows);
    }

    // `f` is a no-op when the pane is closed.
    #[test]
    fn diff_18_f_noop_when_pane_closed() {
        let mut s = app();
        update_diff(&mut s, key(KeyCode::Tab)); // close pane
        assert!(!s.preview_open);
        let effects = update_diff(&mut s, ch('f'));
        assert!(!s.expanded);
        assert_eq!(effects, vec![]);
    }

    // DIFF-19: Shift+j/k (and Shift+↓/↑) scroll the diff pane, clamped to the content.
    #[test]
    fn diff_19_shift_jk_scrolls_diff() {
        let mut s = app();
        s.size = (80, 24);
        // A diff taller than the pane so there is room to scroll.
        let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
        s.preview = DiffPreview::Ready {
            path: "a.rs".into(),
            text: body,
        };
        assert_eq!(s.preview_scroll, 0);

        // Shift+j scrolls down; Shift+k back up (clamped at 0).
        update_diff(&mut s, ch('J'));
        assert_eq!(s.preview_scroll, 1);
        update_diff(&mut s, ch('J'));
        assert_eq!(s.preview_scroll, 2);
        update_diff(&mut s, ch('K'));
        assert_eq!(s.preview_scroll, 1);
        update_diff(&mut s, ch('K'));
        update_diff(&mut s, ch('K'));
        assert_eq!(s.preview_scroll, 0, "cannot scroll above the top");

        // Shift+↓ also scrolls; it cannot overscroll past the last line.
        let shift_down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        for _ in 0..500 {
            update_diff(&mut s, shift_down.clone());
        }
        assert_eq!(s.preview_scroll, s.max_preview_scroll());
        assert!(s.preview_scroll > 0);

        // Moving to another file resets the scroll to the top.
        s.loads.insert(
            DiffScope::Unstaged,
            DiffLoad::Loaded(vec![file('M', "a.rs"), file('M', "b.rs")]),
        );
        update_diff(&mut s, ch('j'));
        assert_eq!(s.preview_scroll, 0);
    }

    // DIFF-06: the pane is open by default; Tab toggles it and requests the selected file's diff.
    #[test]
    fn diff_06_pane_open_by_default_and_toggles() {
        let mut s = app();
        assert!(s.preview_open, "diff pane starts open");

        // Tab hides it.
        let effects = update_diff(&mut s, key(KeyCode::Tab));
        assert!(!s.preview_open);
        assert_eq!(effects, vec![]);
        assert_eq!(s.preview, DiffPreview::Idle);

        // Tab shows it again and asks for the selected file's diff in the active scope.
        let effects = update_diff(&mut s, key(KeyCode::Tab));
        assert!(s.preview_open);
        assert_eq!(
            effects,
            vec![Effect::LoadDiffText {
                scope: DiffScope::Unstaged,
                path: "a.rs".into(),
                width: 78,
            }]
        );
    }

    // DIFF-06/07: moving the selection reloads the pane for the newly selected file.
    #[test]
    fn diff_07_move_reloads_pane() {
        let mut s = app(); // pane open by default
        let effects = update_diff(&mut s, ch('j'));
        assert_eq!(
            effects,
            vec![Effect::LoadDiffText {
                scope: DiffScope::Unstaged,
                path: "b.rs".into(),
                width: 78,
            }]
        );
    }

    // DIFF-08/09: Enter opens the menu; Copy path / Copy diff emit the right effects.
    #[test]
    fn diff_08_09_menu_copy_actions() {
        let mut s = app();
        update_diff(&mut s, key(KeyCode::Enter));
        assert_eq!(s.mode, DiffMode::Menu);
        let menu = s.menu.as_ref().unwrap();
        assert_eq!(menu.items, vec![DiffAction::CopyPath, DiffAction::CopyDiff]);
        assert_eq!(menu.path, "a.rs");

        // Copy path.
        let effects = update_diff(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::CopyToClipboard("a.rs".into())]);
        assert_eq!(s.mode, DiffMode::List);

        // Reopen, move to Copy diff, run it.
        update_diff(&mut s, key(KeyCode::Enter));
        update_diff(&mut s, ch('j'));
        assert_eq!(s.menu.as_ref().unwrap().selected(), DiffAction::CopyDiff);
        let effects = update_diff(&mut s, key(KeyCode::Enter));
        assert_eq!(
            effects,
            vec![Effect::CopyScopeDiff {
                scope: DiffScope::Unstaged,
                path: "a.rs".into(),
            }]
        );
    }

    // DIFF-08: Esc closes the menu without acting.
    #[test]
    fn diff_08_menu_esc_closes() {
        let mut s = app();
        update_diff(&mut s, key(KeyCode::Enter));
        let effects = update_diff(&mut s, key(KeyCode::Esc));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, DiffMode::List);
        assert!(s.menu.is_none());
    }

    // DIFF-10: R reloads the active scope's file list.
    #[test]
    fn diff_10_r_reloads() {
        let mut s = app();
        let effects = update_diff(&mut s, ch('R'));
        assert_eq!(effects, vec![Effect::LoadDiffFiles(DiffScope::Unstaged)]);
    }

    // DIFF-11: an empty scope makes motions and actions safe no-ops.
    #[test]
    fn diff_11_empty_scope_actions_are_noops() {
        let mut s = DiffState::new("main".into());
        s.loads
            .insert(DiffScope::Unstaged, DiffLoad::Loaded(vec![]));
        assert_eq!(update_diff(&mut s, ch('j')), vec![]);
        assert_eq!(update_diff(&mut s, key(KeyCode::Enter)), vec![]);
        assert_eq!(s.mode, DiffMode::List);
        assert!(s.menu.is_none());
    }

    // DIFF-11: reloading with a shorter list clamps the cursor.
    #[test]
    fn diff_11_reload_clamps_cursor() {
        let mut s = app();
        drive(&mut s, vec![ch('G')]); // cursor 3
        update_diff(
            &mut s,
            Event::DiffFilesLoaded {
                scope: DiffScope::Unstaged,
                files: vec![file('M', "only.rs")],
            },
        );
        assert_eq!(s.cursor, 0);
        assert_eq!(s.selected_path().as_deref(), Some("only.rs"));
    }

    // Esc from the base list quits; from the Menu it closes the menu first (then a later Esc quits).
    #[test]
    fn esc_is_the_universal_exit() {
        let mut s = app();
        let effects = update_diff(&mut s, key(KeyCode::Esc));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        let mut s = app();
        update_diff(&mut s, key(KeyCode::Enter)); // open menu
        assert_eq!(s.mode, DiffMode::Menu);
        let effects = update_diff(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, DiffMode::List, "first Esc closes the menu");
        assert!(!s.should_quit);
        assert_eq!(effects, vec![]);
    }

    // DIFF-12: q and Ctrl-c quit.
    #[test]
    fn diff_12_quit_keys() {
        let mut s = app();
        let effects = update_diff(&mut s, ch('q'));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        let mut s2 = app();
        let effects = update_diff(&mut s2, ctrl('c'));
        assert!(s2.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    // A stale diff-text result for the wrong scope must not land in the pane.
    #[test]
    fn stale_diff_text_for_other_scope_ignored() {
        let mut s = app(); // scope = Unstaged, cursor on a.rs, pane open
        update_diff(
            &mut s,
            Event::DiffTextLoaded {
                scope: DiffScope::Staged, // not the active scope
                path: "a.rs".into(),
                text: "stale".into(),
            },
        );
        assert_ne!(
            s.preview,
            DiffPreview::Ready {
                path: "a.rs".into(),
                text: "stale".into()
            }
        );
    }
}
