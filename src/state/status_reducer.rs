//! The pure state transition for `gitt status`. `update_status` is the single entry point.
//!
//! Every stage/unstage/discard emits an [`Effect`] and, when it finishes, the shell reports back a
//! `StatusMutated` event that reloads the list from git — so the view always reflects real repo
//! state rather than an optimistic guess (STAT-10).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::event::Event;
use super::status::{
    CommitEditor, ConfirmDiscard, FileAction, FileMenu, FilePreview, PendingCommit, StatusLoad,
    StatusMode, StatusState,
};

/// Fold an event into the status-screen state, returning the side effects the shell must perform.
pub fn update_status(state: &mut StatusState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            // Re-render the pane for the new width so the diff tool re-picks split vs unified.
            if state.preview_open && state.selected().is_some() {
                return request_diff(state);
            }
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
        // The amend prefill landed: fill the (still-empty) editor with HEAD's message. A failure
        // (e.g. no commit to amend) shows an inline hint and leaves the field editable.
        Event::HeadMessageLoaded(result) => {
            if let Some(editor) = &mut state.commit
                && editor.amend
            {
                editor.busy = false;
                match result {
                    Ok(msg) if editor.message.is_empty() => editor.message = msg,
                    Ok(_) => {}
                    Err(e) => editor.hint = Some(format!("cannot amend: {e}")),
                }
            }
            vec![]
        }
        Event::CommitSuggestionChunk { delta } => {
            if let Some(editor) = &mut state.commit
                && editor.busy
            {
                editor.message.push_str(&delta);
            }
            vec![]
        }
        // Only settle a suggestion into an editor that's actually awaiting one (`busy`), so a late
        // result from a cancelled-then-reopened editor can't clobber a fresh draft.
        Event::CommitSuggestionReady { text } => {
            if let Some(editor) = &mut state.commit
                && editor.busy
            {
                editor.busy = false;
                editor.message = text;
                editor.hint = None;
            }
            vec![]
        }
        Event::CommitSuggestionFailed { error } => {
            if let Some(editor) = &mut state.commit
                && editor.busy
            {
                editor.busy = false;
                editor.hint = Some(format!("suggestion failed: {error}"));
            }
            vec![]
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
        StatusMode::Commit => on_key_commit(state, key),
    }
}

fn on_key_list(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let half = (state.viewport_rows() / 2).max(1) as isize;
    let page = state.viewport_rows().max(1) as isize;

    match key.code {
        // Esc from the base list quits; Menu/Confirm handle their own Esc first, so repeated Esc is
        // always the way out (consistent across every gitt screen).
        KeyCode::Char('q') | KeyCode::Esc => quit(state),
        // Shift+j/k and Shift+↓/↑ scroll the diff pane (plain keys move the file selection).
        KeyCode::Char('J') => scroll_preview(state, 1),
        KeyCode::Char('K') => scroll_preview(state, -1),
        KeyCode::Down if shift => scroll_preview(state, 1),
        KeyCode::Up if shift => scroll_preview(state, -1),
        KeyCode::Char('j') | KeyCode::Down => move_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up => move_by(state, -1),
        KeyCode::Char('g') => set_cursor(state, 0),
        KeyCode::Char('G') => set_cursor(state, state.entries().len().saturating_sub(1)),
        KeyCode::Char('d') if ctrl => move_by(state, half),
        KeyCode::Char('u') if ctrl => move_by(state, -half),
        KeyCode::Char('f') if ctrl => move_by(state, page),
        KeyCode::Char('b') if ctrl => move_by(state, -page),
        KeyCode::Tab => toggle_preview(state),
        KeyCode::Char('f') => toggle_expanded(state),
        KeyCode::Char('R') => reload(state),
        KeyCode::Char(' ') => toggle_stage(state),
        KeyCode::Char('s') => stage_selected(state),
        KeyCode::Char('u') => unstage_selected(state),
        KeyCode::Char('c') => open_commit(state),
        KeyCode::Char('a') => open_amend(state),
        KeyCode::Char('S') => stage_all(state),
        KeyCode::Char('U') => unstage_all(state),
        KeyCode::Char('D') => open_confirm_all(state),
        KeyCode::Char('d') => open_confirm(state),
        KeyCode::Enter => open_menu(state),
        _ => vec![],
    }
}

/// The commit-message editor keymap. Input is paused while `busy` (a suggestion streaming or the amend
/// prefill loading) so it can't interleave with typing; only Esc/Ctrl-c get through.
///
/// AI suggestion is bound to `S` (Shift+S) when the message buffer is empty — once the user starts
/// typing, `S` becomes a regular character. This avoids hijacking typed text while keeping the
/// suggest shortcut easy to reach.
fn on_key_commit(state: &mut StatusState, key: KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    // S (Shift+S) triggers AI suggestion only when the buffer is empty.
    if key.code == KeyCode::Char('S')
        && !ctrl
        && state.commit.as_ref().is_some_and(|e| e.message.is_empty())
    {
        return suggest_commit_message(state);
    }
    match key.code {
        KeyCode::Esc => {
            state.commit = None;
            state.mode = StatusMode::List;
            vec![]
        }
        _ if editor_busy(state) => vec![], // input paused mid-stream/prefill
        KeyCode::Enter => commit_from_editor(state),
        KeyCode::Backspace => {
            if let Some(editor) = &mut state.commit {
                editor.message.pop();
            }
            vec![]
        }
        KeyCode::Char(c) if !ctrl => {
            if let Some(editor) = &mut state.commit {
                editor.message.push(c);
                editor.hint = None;
            }
            vec![]
        }
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
        state.expanded = false;
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
            state.preview_scroll = 0; // a fresh diff starts at the top
            let width = state.preview_width();
            vec![Effect::LoadFileDiff { path, kind, width }]
        }
        None => {
            state.preview = FilePreview::Idle;
            vec![]
        }
    }
}

/// Toggle the expanded diff pane (`f`): the diff grows to 90% of the height (the file list shrinks
/// to 10% but stays visible). A no-op when the pane is closed. The pane always spans the full width,
/// so the diff-tool layout is unchanged (no reload).
fn toggle_expanded(state: &mut StatusState) -> Vec<Effect> {
    if !state.preview_open {
        return vec![];
    }
    state.expanded = !state.expanded;
    state.preview_scroll = state.preview_scroll.min(state.max_preview_scroll());
    vec![]
}

/// Scroll the diff pane by `delta` lines (Shift+j/k, Shift+↓/↑), clamped to the content.
fn scroll_preview(state: &mut StatusState, delta: i32) -> Vec<Effect> {
    if !matches!(state.preview, FilePreview::Ready { .. }) {
        return vec![];
    }
    let max = state.max_preview_scroll() as i32;
    state.preview_scroll = (state.preview_scroll as i32 + delta).clamp(0, max) as u16;
    vec![]
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

fn editor_busy(state: &StatusState) -> bool {
    state.commit.as_ref().is_some_and(|e| e.busy)
}

/// The paths of the currently-staged files (context for the AI suggestion, and the "is anything
/// staged?" guard for a non-amend commit).
fn staged_paths(state: &StatusState) -> Vec<String> {
    state
        .entries()
        .iter()
        .filter(|e| e.is_staged())
        .map(|e| e.path.clone())
        .collect()
}

/// `c`: open the commit-message editor for a fresh commit.
fn open_commit(state: &mut StatusState) -> Vec<Effect> {
    state.commit = Some(CommitEditor::new(false));
    state.mode = StatusMode::Commit;
    vec![]
}

/// `a`: open the editor in amend mode, prefilled asynchronously with HEAD's message.
fn open_amend(state: &mut StatusState) -> Vec<Effect> {
    let mut editor = CommitEditor::new(true);
    editor.busy = true; // input paused until the prefill lands
    state.commit = Some(editor);
    state.mode = StatusMode::Commit;
    vec![Effect::LoadHeadMessage]
}

/// `Enter`: confirm the commit. An empty message is a no-op (stays open); a non-amend commit with
/// nothing staged is refused with an inline hint. Otherwise we DON'T run `git` here — we record a
/// [`PendingCommit`] and quit, so the shell runs it in the restored terminal with inherited stdio
/// (pre-commit hooks stream live; a failure stays on screen and is easy to re-run). This makes commit
/// a terminal handoff, like a successful `gitt branch` checkout (BR-06).
fn commit_from_editor(state: &mut StatusState) -> Vec<Effect> {
    let Some(editor) = &state.commit else {
        return vec![];
    };
    let message = editor.message.trim().to_string();
    let amend = editor.amend;
    if message.is_empty() {
        return vec![]; // nothing to commit yet; keep the editor open (like branch-create).
    }
    if !amend && staged_paths(state).is_empty() {
        if let Some(editor) = &mut state.commit {
            editor.hint = Some("nothing staged to commit".to_string());
        }
        return vec![];
    }
    state.commit = None;
    state.pending_commit = Some(PendingCommit { message, amend });
    state.should_quit = true;
    vec![Effect::Quit]
}

/// `s` (blank editor) / `Ctrl-s`: draft an AI commit message from the staged diff. Clears the buffer
/// and streams the suggestion in; a duplicate request while one is in flight is ignored.
fn suggest_commit_message(state: &mut StatusState) -> Vec<Effect> {
    if editor_busy(state) {
        return vec![];
    }
    let files = staged_paths(state);
    let branch = state.branch.clone();
    let Some(editor) = &mut state.commit else {
        return vec![];
    };
    editor.message.clear();
    editor.hint = None;
    editor.busy = true;
    vec![Effect::SuggestCommitMessage { branch, files }]
}

fn open_confirm(state: &mut StatusState) -> Vec<Effect> {
    if let Some(entry) = state.selected() {
        state.confirm = Some(ConfirmDiscard::File {
            path: entry.path.clone(),
            untracked: entry.is_untracked(),
        });
        state.mode = StatusMode::Confirm;
    }
    vec![]
}

fn open_confirm_all(state: &mut StatusState) -> Vec<Effect> {
    if state.entries().is_empty() {
        return vec![];
    }
    state.confirm = Some(ConfirmDiscard::All);
    state.mode = StatusMode::Confirm;
    vec![]
}

fn confirm_discard(state: &mut StatusState) -> Vec<Effect> {
    state.mode = StatusMode::List;
    match state.confirm.take() {
        Some(ConfirmDiscard::File { path, untracked }) => {
            state.status = Some(format!("Discarding {path}…"));
            vec![Effect::Discard { path, untracked }]
        }
        Some(ConfirmDiscard::All) => {
            state.status = Some("Discarding all…".to_string());
            vec![Effect::DiscardAll]
        }
        None => vec![],
    }
}

fn stage_all(state: &mut StatusState) -> Vec<Effect> {
    if state.entries().is_empty() {
        return vec![];
    }
    state.status = Some("Staging all…".to_string());
    vec![Effect::StageAll]
}

fn unstage_all(state: &mut StatusState) -> Vec<Effect> {
    if state.entries().is_empty() {
        return vec![];
    }
    state.status = Some("Unstaging all…".to_string());
    vec![Effect::UnstageAll]
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
            state.confirm = Some(ConfirmDiscard::File {
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
                width: 78,
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
                width: 78,
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
        assert!(matches!(
            s.confirm.as_ref().unwrap(),
            ConfirmDiscard::File { path, untracked: true } if path == "untracked.txt"
        ));

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

    // Esc from the base list quits; from the Menu/Confirm it dismisses that first (then Esc quits).
    #[test]
    fn esc_is_the_universal_exit() {
        let mut s = app();
        let effects = update_status(&mut s, key(KeyCode::Esc));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        // Confirm → List (not quit) on Esc.
        let mut s = app();
        update_status(&mut s, ch('d')); // open discard confirm
        assert_eq!(s.mode, StatusMode::Confirm);
        let effects = update_status(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, StatusMode::List, "first Esc cancels the confirm");
        assert!(!s.should_quit);
        assert_eq!(effects, vec![]);
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

    // --- gitt commit (CMT-*) ---------------------------------------------------------------------

    fn clean() -> StatusState {
        // A dirty tree with nothing staged (all worktree-side), for the "nothing staged" guard.
        let mut s = StatusState::new("feature".into());
        s.size = (80, 24);
        s.load = StatusLoad::Loaded(vec![entry(' ', 'M', "a.txt"), entry('?', '?', "b.txt")]);
        s
    }

    // CMT-01: `c` opens the commit editor (fresh commit).
    #[test]
    fn cmt_01_c_opens_commit_editor() {
        let mut s = app();
        let effects = update_status(&mut s, ch('c'));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, StatusMode::Commit);
        let editor = s.commit.as_ref().unwrap();
        assert!(!editor.amend);
        assert!(editor.message.is_empty());
        assert!(!editor.busy);
    }

    // CMT-02: typing edits the message, Backspace deletes, empty Enter is a no-op.
    #[test]
    fn cmt_02_typing_and_empty_enter() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        // Empty Enter: no-op, stays open.
        assert_eq!(update_status(&mut s, key(KeyCode::Enter)), vec![]);
        assert_eq!(s.mode, StatusMode::Commit);
        // Type "hi", backspace once → "h".
        drive(&mut s, vec![ch('h'), ch('i'), key(KeyCode::Backspace)]);
        assert_eq!(s.commit.as_ref().unwrap().message, "h");
    }

    // CMT-03 / CMT-09: Enter with a message + staged changes records the pending commit and quits, so
    // the shell runs `git commit` in the restored terminal (commit is a terminal handoff, not
    // in-place). No `git` effect fires from the reducer.
    #[test]
    fn cmt_03_09_enter_defers_commit_and_quits() {
        let mut s = app(); // staged_new.txt is staged (A )
        update_status(&mut s, ch('c'));
        drive(&mut s, vec![ch('f'), ch('i'), ch('x')]);
        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::Quit]);
        assert!(s.should_quit);
        assert!(s.commit.is_none());
        assert_eq!(
            s.pending_commit,
            Some(PendingCommit {
                message: "fix".into(),
                amend: false
            })
        );
    }

    // CMT-04: committing with nothing staged (non-amend) is refused inline, no effect.
    #[test]
    fn cmt_04_nothing_staged_is_refused() {
        let mut s = clean();
        update_status(&mut s, ch('c'));
        drive(&mut s, vec![ch('x')]);
        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![], "no commit runs");
        assert_eq!(s.mode, StatusMode::Commit, "editor stays open");
        assert_eq!(
            s.commit.as_ref().unwrap().hint.as_deref(),
            Some("nothing staged to commit")
        );
    }

    // CMT-05: `a` opens amend, loads HEAD's message to prefill, then Enter amends.
    #[test]
    fn cmt_05_amend_prefills_and_commits() {
        let mut s = app();
        let effects = update_status(&mut s, ch('a'));
        assert_eq!(effects, vec![Effect::LoadHeadMessage]);
        assert_eq!(s.mode, StatusMode::Commit);
        let editor = s.commit.as_ref().unwrap();
        assert!(editor.amend);
        assert!(editor.busy, "input paused until the prefill lands");

        // Prefill lands.
        update_status(
            &mut s,
            Event::HeadMessageLoaded(Ok("previous subject".into())),
        );
        let editor = s.commit.as_ref().unwrap();
        assert!(!editor.busy);
        assert_eq!(editor.message, "previous subject");

        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::Quit]);
        assert!(s.should_quit);
        assert_eq!(
            s.pending_commit,
            Some(PendingCommit {
                message: "previous subject".into(),
                amend: true
            })
        );
    }

    // CMT-04/05: amend is exempt from the "nothing staged" guard (a reword-only amend is valid).
    #[test]
    fn cmt_05_amend_allowed_with_nothing_staged() {
        let mut s = clean();
        update_status(&mut s, ch('a'));
        update_status(&mut s, Event::HeadMessageLoaded(Ok("subject".into())));
        let effects = update_status(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::Quit]);
        assert_eq!(
            s.pending_commit,
            Some(PendingCommit {
                message: "subject".into(),
                amend: true
            })
        );
    }

    // CMT-05: a failed amend prefill (no commit yet) shows an inline hint, leaves the field editable.
    #[test]
    fn cmt_05_amend_prefill_failure_is_inline() {
        let mut s = app();
        update_status(&mut s, ch('a'));
        update_status(
            &mut s,
            Event::HeadMessageLoaded(Err("does not have any commits yet".into())),
        );
        let editor = s.commit.as_ref().unwrap();
        assert!(!editor.busy);
        assert!(editor.message.is_empty());
        assert!(editor.hint.as_deref().unwrap().contains("cannot amend"));
    }

    // CMT-06/07: `S` (empty buffer) drafts a suggestion; chunks stream in; Ready settles it.
    #[test]
    fn cmt_06_shift_s_suggests_and_streams() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        let effects = update_status(&mut s, ch('S'));
        // Branch + every staged path (staged_new.txt `A `, both.txt `MM`) become the AI context.
        assert_eq!(
            effects,
            vec![Effect::SuggestCommitMessage {
                branch: "main".into(),
                files: vec!["staged_new.txt".into(), "both.txt".into()],
            }]
        );
        assert!(s.commit.as_ref().unwrap().busy);

        // Streamed tokens accumulate into the buffer.
        for delta in ["Add ", "staged ", "file"] {
            update_status(
                &mut s,
                Event::CommitSuggestionChunk {
                    delta: delta.into(),
                },
            );
        }
        assert_eq!(s.commit.as_ref().unwrap().message, "Add staged file");
        // Ready settles the draft and clears busy.
        update_status(
            &mut s,
            Event::CommitSuggestionReady {
                text: "Add staged file".into(),
            },
        );
        let editor = s.commit.as_ref().unwrap();
        assert!(!editor.busy);
        assert_eq!(editor.message, "Add staged file");
    }

    // CMT-06: while busy, typing is paused; a failure shows a hint and re-enables the field.
    #[test]
    fn cmt_06_busy_pauses_input_and_failure_is_inline() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        update_status(&mut s, ch('S')); // busy (empty buffer → suggest)
        // A keypress mid-stream is ignored.
        update_status(&mut s, ch('x'));
        assert_eq!(s.commit.as_ref().unwrap().message, "");
        // Enter is ignored while busy (no commit fired).
        assert_eq!(update_status(&mut s, key(KeyCode::Enter)), vec![]);
        assert_eq!(s.mode, StatusMode::Commit);

        update_status(
            &mut s,
            Event::CommitSuggestionFailed {
                error: "ollama down".into(),
            },
        );
        let editor = s.commit.as_ref().unwrap();
        assert!(!editor.busy);
        assert!(editor.hint.as_deref().unwrap().contains("ollama down"));
        // The field is editable again.
        update_status(&mut s, ch('x'));
        assert_eq!(s.commit.as_ref().unwrap().message, "x");
    }

    // CMT-06: lowercase `s` always types a literal. `S` triggers suggest only when the buffer is
    // empty; once text is present, `S` types the letter.
    #[test]
    fn cmt_06_s_keybinding_behavior() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        // Blank editor: lowercase `s` types, does not suggest.
        let effects = update_status(&mut s, ch('s'));
        assert_eq!(effects, vec![], "bare s never triggers a suggestion");
        drive(&mut s, vec![ch('h'), ch('i'), ch('p')]);
        assert_eq!(s.commit.as_ref().unwrap().message, "ship");
        assert!(!s.commit.as_ref().unwrap().busy);

        // `S` on a non-empty buffer types the character instead of suggesting.
        let effects = update_status(&mut s, ch('S'));
        assert_eq!(effects, vec![], "S with text types, doesn't suggest");
        assert_eq!(s.commit.as_ref().unwrap().message, "shipS");
    }

    // CMT-08: Esc cancels the editor (→ List), making no commit; a second Esc then quits.
    #[test]
    fn cmt_08_esc_cancels_then_quits() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        drive(&mut s, vec![ch('h'), ch('i')]);
        let effects = update_status(&mut s, key(KeyCode::Esc));
        assert_eq!(effects, vec![], "cancel makes no commit");
        assert_eq!(s.mode, StatusMode::List);
        assert!(s.commit.is_none());
        // From the base list, Esc now quits (universal exit).
        let effects = update_status(&mut s, key(KeyCode::Esc));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    // Ctrl-c quits even from inside the commit editor.
    #[test]
    fn cmt_ctrl_c_quits_from_editor() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        let effects = update_status(&mut s, ctrl('c'));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    // A suggestion arriving after the editor was cancelled — and after a fresh editor was reopened —
    // must not clobber the new draft (it only settles into the editor that's `busy` awaiting it).
    #[test]
    fn cmt_stale_suggestion_after_cancel_is_ignored() {
        let mut s = app();
        update_status(&mut s, ch('c'));
        update_status(&mut s, ch('S')); // request a suggestion (empty → busy)
        update_status(&mut s, key(KeyCode::Esc)); // cancel: editor closed, generation orphaned
        assert!(s.commit.is_none());

        // Reopen a fresh editor and type a manual draft.
        update_status(&mut s, ch('c'));
        drive(&mut s, vec![ch('h'), ch('i')]);

        // The orphaned generation finishes late — the fresh (non-busy) draft is untouched.
        update_status(
            &mut s,
            Event::CommitSuggestionReady {
                text: "stale".into(),
            },
        );
        assert_eq!(s.commit.as_ref().unwrap().message, "hi");
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
        assert_eq!(update_status(&mut s, ch('S')), vec![]);
        assert_eq!(update_status(&mut s, ch('U')), vec![]);
        assert_eq!(update_status(&mut s, ch('D')), vec![]);
        assert_eq!(s.mode, StatusMode::List);
    }

    // S stages all files.
    #[test]
    fn stage_all_emits_effect() {
        let mut s = app();
        let effects = update_status(&mut s, ch('S'));
        assert_eq!(effects, vec![Effect::StageAll]);
        assert_eq!(s.status.as_deref(), Some("Staging all…"));
    }

    // U unstages all files.
    #[test]
    fn unstage_all_emits_effect() {
        let mut s = app();
        let effects = update_status(&mut s, ch('U'));
        assert_eq!(effects, vec![Effect::UnstageAll]);
        assert_eq!(s.status.as_deref(), Some("Unstaging all…"));
    }

    // D opens a discard-all confirmation; y confirms, Esc cancels.
    #[test]
    fn discard_all_requires_confirmation() {
        let mut s = app();
        update_status(&mut s, ch('D'));
        assert_eq!(s.mode, StatusMode::Confirm);
        assert!(matches!(s.confirm, Some(ConfirmDiscard::All)));

        let effects = update_status(&mut s, ch('y'));
        assert_eq!(effects, vec![Effect::DiscardAll]);
        assert_eq!(s.mode, StatusMode::List);
        assert_eq!(s.status.as_deref(), Some("Discarding all…"));
    }

    #[test]
    fn discard_all_cancel() {
        let mut s = app();
        update_status(&mut s, ch('D'));
        assert_eq!(s.mode, StatusMode::Confirm);
        update_status(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, StatusMode::List);
        assert!(s.confirm.is_none());
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
                width: 78,
            }]
        );
    }

    // Diff-pane parity with `gitt diff`: `f` expands the pane height (no reload, width unchanged),
    // and Shift+j/k / Shift+↓ scroll a ready diff, clamped.
    #[test]
    fn preview_expand_and_scroll() {
        let mut s = app();
        s.size = (80, 24);
        update_status(&mut s, key(KeyCode::Tab)); // open preview
        assert!(s.preview_open);

        let width = s.preview_width();
        let effects = update_status(&mut s, ch('f'));
        assert!(s.expanded, "f expands the pane");
        assert_eq!(effects, vec![], "no reload — full-width in both states");
        assert_eq!(s.preview_width(), width);
        update_status(&mut s, ch('f'));
        assert!(!s.expanded);

        // Scroll a diff taller than the pane.
        let body: String = (0..300).map(|i| format!("l{i}\n")).collect();
        s.preview = FilePreview::Ready {
            path: "tracked.txt".into(),
            text: body,
        };
        update_status(&mut s, ch('J'));
        assert_eq!(s.preview_scroll, 1);
        update_status(&mut s, ch('K'));
        update_status(&mut s, ch('K'));
        assert_eq!(s.preview_scroll, 0, "clamped at the top");
        let shift_down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        for _ in 0..1000 {
            update_status(&mut s, shift_down.clone());
        }
        assert_eq!(s.preview_scroll, s.max_preview_scroll());
        assert!(s.preview_scroll > 0);
    }
}
