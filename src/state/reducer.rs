//! The pure state transition function. `update` is the single entry point.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::event::Event;
use super::model::{ActionMenu, AppState, Load, MenuAction, Mode, PreviewState};
use crate::domain::{View, url};

/// Fold an event into the state, returning the side effects the shell must perform.
pub fn update(state: &mut AppState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            vec![]
        }
        Event::LogLoaded { view, commits } => {
            state.logs.insert(view, Load::Loaded(commits));
            if view == state.view {
                state.recompute_matches();
                if state.preview_open {
                    return request_diff(state);
                }
            }
            vec![]
        }
        Event::LogFailed { view, error } => {
            let failed_current = view == state.view;
            state.logs.insert(view, Load::Failed(error.clone()));
            if failed_current {
                state.matches.clear();
                state.status = Some(format!("log failed: {error}"));
            }
            vec![]
        }
        Event::DiffLoaded { hash, text } => {
            if state.preview_open && state.selected_hash().as_deref() == Some(hash.as_str()) {
                state.preview = PreviewState::Ready { hash, text };
            }
            vec![]
        }
        Event::DiffFailed { hash, error } => {
            if state.preview_open && state.selected_hash().as_deref() == Some(hash.as_str()) {
                state.preview = PreviewState::Failed { hash, error };
            }
            vec![]
        }
        Event::FetchFinished(result) => match result {
            Ok(()) => {
                state.status = Some("fetched".to_string());
                state.logs.insert(state.view, Load::Loading);
                vec![Effect::LoadLog(state.view)]
            }
            Err(e) => {
                state.status = Some(format!("fetch failed: {e}"));
                vec![]
            }
        },
        Event::ActionFinished { label, result } => {
            state.status = Some(match result {
                Ok(()) => label,
                Err(e) => format!("{label} failed: {e}"),
            });
            vec![]
        }
        // Status- and diff-screen events never reach the log reducer at runtime (separate screens);
        // ignore them.
        Event::StatusLoaded(_)
        | Event::StatusFailed(_)
        | Event::FileDiffLoaded { .. }
        | Event::FileDiffFailed { .. }
        | Event::StatusMutated { .. }
        | Event::DiffFilesLoaded { .. }
        | Event::DiffFilesFailed { .. }
        | Event::DiffTextLoaded { .. }
        | Event::DiffTextFailed { .. } => vec![],
    }
}

fn on_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    // Ctrl-C always quits, from any mode.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return vec![Effect::Quit];
    }
    match state.mode {
        Mode::List => on_key_list(state, key),
        Mode::Search => on_key_search(state, key),
        Mode::Menu => on_key_menu(state, key),
    }
}

fn on_key_list(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let half = (state.viewport_rows() / 2).max(1) as isize;
    let page = state.viewport_rows().max(1) as isize;

    match key.code {
        KeyCode::Char('q') => quit(state),
        KeyCode::Char('j') | KeyCode::Down => move_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up => move_by(state, -1),
        KeyCode::Char('g') => set_cursor(state, 0),
        KeyCode::Char('G') => set_cursor(state, state.matches.len().saturating_sub(1)),
        KeyCode::Char('d') if ctrl => move_by(state, half),
        KeyCode::Char('u') if ctrl => move_by(state, -half),
        KeyCode::Char('f') if ctrl => move_by(state, page),
        KeyCode::Char('b') if ctrl => move_by(state, -page),
        KeyCode::Right => switch_view(state, View::OriginMain),
        KeyCode::Left => switch_view(state, View::LocalHead),
        KeyCode::Char('/') => {
            state.mode = Mode::Search;
            vec![]
        }
        KeyCode::Tab => toggle_preview(state),
        KeyCode::Char('R') => {
            state.status = Some("fetching…".to_string());
            vec![Effect::Fetch]
        }
        KeyCode::Enter => open_menu(state),
        _ => vec![],
    }
}

fn on_key_search(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.mode = Mode::List;
            vec![]
        }
        KeyCode::Down => move_by(state, 1),
        KeyCode::Up => move_by(state, -1),
        KeyCode::Backspace => {
            state.filter.pop();
            after_filter_change(state)
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.filter.push(c);
            after_filter_change(state)
        }
        _ => vec![],
    }
}

fn on_key_menu(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.menu = None;
            state.mode = Mode::List;
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

fn quit(state: &mut AppState) -> Vec<Effect> {
    state.should_quit = true;
    vec![Effect::Quit]
}

/// Move the selection by `delta` rows (clamped), reloading the preview if the selection changed.
fn move_by(state: &mut AppState, delta: isize) -> Vec<Effect> {
    if state.matches.is_empty() {
        return vec![];
    }
    let len = state.matches.len() as isize;
    let target = (state.cursor as isize + delta).clamp(0, len - 1);
    set_cursor(state, target as usize)
}

fn set_cursor(state: &mut AppState, idx: usize) -> Vec<Effect> {
    if state.matches.is_empty() {
        return vec![];
    }
    let before = state.selected_hash();
    state.cursor = idx.min(state.matches.len() - 1);
    state.clamp_scroll();
    if state.preview_open && state.selected_hash() != before {
        request_diff(state)
    } else {
        vec![]
    }
}

fn switch_view(state: &mut AppState, view: View) -> Vec<Effect> {
    if state.view == view {
        return vec![];
    }
    state.view = view;
    state.cursor = 0;
    state.top = 0;
    state.recompute_matches();

    let mut effects = Vec::new();
    if matches!(state.logs.get(&view), None | Some(Load::Idle)) {
        state.logs.insert(view, Load::Loading);
        effects.push(Effect::LoadLog(view));
    }
    if state.preview_open {
        effects.extend(request_diff(state));
    }
    effects
}

fn toggle_preview(state: &mut AppState) -> Vec<Effect> {
    state.preview_open = !state.preview_open;
    if state.preview_open {
        request_diff(state)
    } else {
        state.preview = PreviewState::Idle;
        vec![]
    }
}

/// Ask the shell to load the diff for the current selection into the preview.
fn request_diff(state: &mut AppState) -> Vec<Effect> {
    match state.selected_hash() {
        Some(hash) => {
            state.preview = PreviewState::Loading(hash.clone());
            vec![Effect::LoadDiff(hash)]
        }
        None => {
            state.preview = PreviewState::Idle;
            vec![]
        }
    }
}

fn after_filter_change(state: &mut AppState) -> Vec<Effect> {
    state.recompute_matches();
    if state.preview_open {
        request_diff(state)
    } else {
        vec![]
    }
}

fn open_menu(state: &mut AppState) -> Vec<Effect> {
    if let Some(commit) = state.selected() {
        state.menu = Some(ActionMenu {
            items: MenuAction::all(),
            cursor: 0,
            hash: commit.hash.clone(),
            short: commit.short.clone(),
            subject: commit.subject.clone(),
        });
        state.mode = Mode::Menu;
    }
    vec![]
}

fn menu_move(state: &mut AppState, delta: isize) {
    if let Some(menu) = &mut state.menu {
        let len = menu.items.len() as isize;
        menu.cursor = (menu.cursor as isize + delta).clamp(0, len - 1) as usize;
    }
}

fn execute_menu(state: &mut AppState) -> Vec<Effect> {
    let Some(menu) = state.menu.take() else {
        state.mode = Mode::List;
        return vec![];
    };
    state.mode = Mode::List;
    let action = menu.selected();
    let hash = menu.hash;
    let short = menu.short;

    match action {
        MenuAction::OpenGithub => match state.remote_url.as_deref() {
            Some(remote) => match url::commit_url(remote, &hash) {
                Some(u) => {
                    state.status = Some("Opening GitHub…".to_string());
                    vec![Effect::OpenBrowser(u)]
                }
                None => {
                    state.status = Some("remote is not a GitHub URL".to_string());
                    vec![]
                }
            },
            None => {
                state.status = Some("no remote configured".to_string());
                vec![]
            }
        },
        MenuAction::OpenPr => {
            state.status = Some("Opening PR…".to_string());
            vec![Effect::OpenPr(hash)]
        }
        MenuAction::CopySha => {
            state.status = Some("Copied SHA to clipboard".to_string());
            vec![Effect::CopyToClipboard(hash)]
        }
        MenuAction::Checkout => {
            state.status = Some(format!("Checking out {short}…"));
            vec![Effect::Checkout(hash)]
        }
        MenuAction::CopyRevert => {
            state.status = Some("Copied revert command".to_string());
            vec![Effect::CopyToClipboard(format!("git revert {hash}"))]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Commit;

    fn commit(short: &str, subject: &str) -> Commit {
        Commit {
            hash: format!("{}{}", short, "0".repeat(40 - short.len())),
            short: short.to_string(),
            timestamp: 0,
            author: "Tester".to_string(),
            subject: subject.to_string(),
            relative: "now".to_string(),
            refs: Vec::new(),
            haystack: format!("{short} Tester {subject}"),
        }
    }

    fn app() -> AppState {
        let mut s = AppState::new(
            "feature".to_string(),
            "main".to_string(),
            Some("git@github.com:org/repo.git".to_string()),
        );
        s.size = (80, 24);
        let commits = vec![
            commit("aaaaaaa", "add fuzzy search"),
            commit("bbbbbbb", "fix flaky test"),
            commit("ccccccc", "refactor parser"),
            commit("ddddddd", "docs update"),
        ];
        s.logs.insert(View::LocalHead, Load::Loaded(commits));
        s.recompute_matches();
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

    fn drive(state: &mut AppState, events: Vec<Event>) -> Vec<Effect> {
        let mut all = Vec::new();
        for e in events {
            all.extend(update(state, e));
        }
        all
    }

    // LOG-06: vim motions move within bounds.
    #[test]
    fn log_06_jk_moves_and_clamps() {
        let mut s = app();
        assert_eq!(s.cursor, 0);
        drive(&mut s, vec![ch('j'), ch('j')]);
        assert_eq!(s.cursor, 2);
        drive(&mut s, vec![ch('k')]);
        assert_eq!(s.cursor, 1);
        // Up past the top clamps at 0.
        drive(&mut s, vec![ch('k'), ch('k'), ch('k')]);
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn log_06_g_and_shift_g() {
        let mut s = app();
        drive(&mut s, vec![ch('G')]);
        assert_eq!(s.cursor, 3);
        drive(&mut s, vec![ch('g')]);
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn log_06_ctrl_d_u_half_page() {
        let mut s = app();
        s.size = (80, 8); // viewport_rows = 5, half = 2
        s.recompute_matches();
        drive(&mut s, vec![ctrl('d')]);
        assert_eq!(s.cursor, 2);
        drive(&mut s, vec![ctrl('u')]);
        assert_eq!(s.cursor, 0);
    }

    // LOG-04: '/' enters search, typing filters, Esc keeps the filter.
    #[test]
    fn log_04_search_mode_and_filter() {
        let mut s = app();
        drive(&mut s, vec![ch('/')]);
        assert_eq!(s.mode, Mode::Search);
        drive(&mut s, vec![ch('f'), ch('i'), ch('x')]);
        assert_eq!(s.filter, "fix");
        drive(&mut s, vec![key(KeyCode::Esc)]);
        assert_eq!(s.mode, Mode::List);
        assert_eq!(s.filter, "fix", "Esc keeps the filter applied");
    }

    // LOG-05: filter narrows the visible matches.
    #[test]
    fn log_05_filter_narrows_matches() {
        let mut s = app();
        drive(&mut s, vec![ch('/'), ch('p'), ch('a'), ch('r')]);
        // Only "refactor parser" matches "par" strongly; assert it's selected/shown.
        assert!(s.matches.len() < 4);
        let idx = s.matches[0].commit_idx;
        assert_eq!(s.commits()[idx].subject, "refactor parser");
    }

    #[test]
    fn log_04_backspace_edits_filter() {
        let mut s = app();
        drive(&mut s, vec![ch('/'), ch('f'), ch('i'), ch('x')]);
        drive(&mut s, vec![key(KeyCode::Backspace)]);
        assert_eq!(s.filter, "fi");
    }

    // LOG-07: arrows toggle view and load origin lazily.
    #[test]
    fn log_07_right_switches_to_origin_and_loads() {
        let mut s = app();
        let effects = update(&mut s, key(KeyCode::Right));
        assert_eq!(s.view, View::OriginMain);
        assert_eq!(effects, vec![Effect::LoadLog(View::OriginMain)]);
        assert_eq!(s.logs.get(&View::OriginMain), Some(&Load::Loading));
    }

    #[test]
    fn log_07_origin_cached_after_load_no_reload() {
        let mut s = app();
        update(&mut s, key(KeyCode::Right));
        update(
            &mut s,
            Event::LogLoaded {
                view: View::OriginMain,
                commits: vec![commit("eeeeeee", "origin tip")],
            },
        );
        // Go back to head, then to origin again: no new LoadLog (cached).
        update(&mut s, key(KeyCode::Left));
        let effects = update(&mut s, key(KeyCode::Right));
        assert_eq!(effects, vec![]);
        assert_eq!(s.commits()[0].subject, "origin tip");
    }

    // LOG-08: Tab toggles preview and requests the diff for the selection.
    #[test]
    fn log_08_tab_toggles_preview() {
        let mut s = app();
        let effects = update(&mut s, key(KeyCode::Tab));
        assert!(s.preview_open);
        let sel = s.selected_hash().unwrap();
        assert_eq!(effects, vec![Effect::LoadDiff(sel.clone())]);
        assert_eq!(s.preview, PreviewState::Loading(sel));
        // Toggle off.
        let effects = update(&mut s, key(KeyCode::Tab));
        assert!(!s.preview_open);
        assert_eq!(effects, vec![]);
        assert_eq!(s.preview, PreviewState::Idle);
    }

    #[test]
    fn log_08_diff_loaded_sets_ready_when_current() {
        let mut s = app();
        update(&mut s, key(KeyCode::Tab));
        let hash = s.selected_hash().unwrap();
        update(
            &mut s,
            Event::DiffLoaded {
                hash: hash.clone(),
                text: "diff text".to_string(),
            },
        );
        assert_eq!(
            s.preview,
            PreviewState::Ready {
                hash,
                text: "diff text".to_string()
            }
        );
    }

    #[test]
    fn log_08_stale_diff_ignored() {
        let mut s = app();
        update(&mut s, key(KeyCode::Tab));
        update(
            &mut s,
            Event::DiffLoaded {
                hash: "deadbeef".to_string(),
                text: "stale".to_string(),
            },
        );
        // Still Loading the real selection, not the stale hash.
        assert!(matches!(s.preview, PreviewState::Loading(_)));
    }

    // LOG-10: R triggers fetch, and a successful fetch reloads the current view.
    #[test]
    fn log_10_r_fetches_then_reloads() {
        let mut s = app();
        let effects = update(&mut s, ch('R'));
        assert_eq!(effects, vec![Effect::Fetch]);
        let effects = update(&mut s, Event::FetchFinished(Ok(())));
        assert_eq!(effects, vec![Effect::LoadLog(View::LocalHead)]);
    }

    // LOG-11: Enter opens the action menu; Esc closes it.
    #[test]
    fn log_11_enter_opens_menu_esc_closes() {
        let mut s = app();
        update(&mut s, key(KeyCode::Enter));
        assert_eq!(s.mode, Mode::Menu);
        assert!(s.menu.is_some());
        assert_eq!(s.menu.as_ref().unwrap().items, MenuAction::all());
        update(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, Mode::List);
        assert!(s.menu.is_none());
    }

    // LOG-12: Copy SHA copies the full 40-char hash.
    #[test]
    fn log_12_copy_sha_full_hash() {
        let mut s = app();
        let full = s.selected_hash().unwrap();
        assert_eq!(full.len(), 40);
        // Open menu, "Copy SHA" is item index 2.
        update(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j')]); // move to CopySha
        assert_eq!(s.menu.as_ref().unwrap().selected(), MenuAction::CopySha);
        let effects = update(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::CopyToClipboard(full)]);
        assert_eq!(s.mode, Mode::List);
    }

    // LOG-13: Checkout emits a checkout effect for the selected hash.
    #[test]
    fn log_13_checkout_effect() {
        let mut s = app();
        let full = s.selected_hash().unwrap();
        update(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j'), ch('j')]); // Checkout is index 3
        assert_eq!(s.menu.as_ref().unwrap().selected(), MenuAction::Checkout);
        let effects = update(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::Checkout(full)]);
    }

    // LOG-14: Open in GitHub builds a commit URL from the remote.
    #[test]
    fn log_14_open_github_builds_url() {
        let mut s = app();
        let full = s.selected_hash().unwrap();
        update(&mut s, key(KeyCode::Enter)); // OpenGithub is index 0
        assert_eq!(s.menu.as_ref().unwrap().selected(), MenuAction::OpenGithub);
        let effects = update(&mut s, key(KeyCode::Enter));
        assert_eq!(
            effects,
            vec![Effect::OpenBrowser(format!(
                "https://github.com/org/repo/commit/{full}"
            ))]
        );
    }

    // LOG-15: Open PR emits an OpenPr effect.
    #[test]
    fn log_15_open_pr_effect() {
        let mut s = app();
        let full = s.selected_hash().unwrap();
        update(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j')]); // OpenPr is index 1
        assert_eq!(s.menu.as_ref().unwrap().selected(), MenuAction::OpenPr);
        let effects = update(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::OpenPr(full)]);
    }

    // LOG-16: Copy revert command copies "git revert <hash>".
    #[test]
    fn log_16_copy_revert_command() {
        let mut s = app();
        let full = s.selected_hash().unwrap();
        update(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j'), ch('j'), ch('j')]); // CopyRevert is index 4
        assert_eq!(s.menu.as_ref().unwrap().selected(), MenuAction::CopyRevert);
        let effects = update(&mut s, key(KeyCode::Enter));
        assert_eq!(
            effects,
            vec![Effect::CopyToClipboard(format!("git revert {full}"))]
        );
    }

    // LOG-18: q and Ctrl-c quit.
    #[test]
    fn log_18_quit_keys() {
        let mut s = app();
        let effects = update(&mut s, ch('q'));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        let mut s2 = app();
        let effects = update(&mut s2, ctrl('c'));
        assert!(s2.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    #[test]
    fn open_github_without_remote_reports_status() {
        let mut s = AppState::new("main".into(), "main".into(), None);
        s.logs
            .insert(View::LocalHead, Load::Loaded(vec![commit("aaaaaaa", "x")]));
        s.recompute_matches();
        update(&mut s, key(KeyCode::Enter));
        let effects = update(&mut s, key(KeyCode::Enter)); // OpenGithub, no remote
        assert_eq!(effects, vec![]);
        assert_eq!(s.status.as_deref(), Some("no remote configured"));
    }
}
