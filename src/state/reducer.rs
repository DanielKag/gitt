//! The pure state transition function. `update` is the single entry point.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::event::Event;
use super::model::{ActionMenu, AppState, Load, MenuAction, Mode, PreviewState, SummaryState};
use crate::domain::summary::strip_preamble;
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
                return on_selection_changed(state);
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
        // Cache hit. `or_insert`: never clobber a `Generating`/`Ready`/`Failed` state — if the user
        // pressed `s` while this cache read was in flight, that generation's result must win.
        Event::SummaryLoaded { hash, text } => {
            state
                .summaries
                .entry(hash)
                .or_insert(SummaryState::Ready(strip_preamble(&text)));
            vec![]
        }
        // Cache miss; same `or_insert` guard against a racing generation.
        Event::SummaryMissing { hash } => {
            state.summaries.entry(hash).or_insert(SummaryState::Missing);
            vec![]
        }
        // A streamed token: append to the partial summary, but only while still generating (ignore
        // late chunks that arrive after the state moved on).
        Event::SummaryChunk { hash, delta } => {
            if let Some(SummaryState::Generating(buf)) = state.summaries.get_mut(&hash) {
                buf.push_str(&delta);
            }
            vec![]
        }
        // A freshly generated summary is authoritative → overwrite (the panel shows it; the status
        // line is left alone so the keymap legend stays visible).
        Event::SummaryReady { hash, text } => {
            state
                .summaries
                .insert(hash, SummaryState::Ready(strip_preamble(&text)));
            vec![]
        }
        // Failures surface in the panel (per selected commit), not the status line.
        Event::SummaryFailed { hash, error } => {
            state.summaries.insert(hash, SummaryState::Failed(error));
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
        Mode::Summary => on_key_summary(state, key),
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
        KeyCode::Char('s') => summarize_selected(state),
        KeyCode::Char('S') => open_summary_modal(state),
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

/// The expanded-summary modal is view-only: any of Esc/q/S/Enter dismisses it; everything else is
/// ignored (the summary keeps streaming underneath via `SummaryChunk` events regardless).
fn on_key_summary(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if matches!(
        key.code,
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('S') | KeyCode::Enter
    ) {
        state.mode = Mode::List;
    }
    vec![]
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
    if state.selected_hash() != before {
        on_selection_changed(state)
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
    // If the new view is already loaded, refresh preview + summary for its selection now; otherwise
    // that happens when its `LogLoaded` arrives.
    effects.extend(on_selection_changed(state));
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
    let before = state.selected_hash();
    state.recompute_matches();
    if state.selected_hash() != before {
        on_selection_changed(state)
    } else {
        vec![]
    }
}

/// Effects to run when the selected commit may have changed: load its cached summary (if not already
/// tracked) and, when the preview is open, reload its diff.
fn on_selection_changed(state: &mut AppState) -> Vec<Effect> {
    let mut effects = request_summary(state);
    if state.preview_open {
        effects.extend(request_diff(state));
    }
    effects
}

/// Ask the shell to load the selected commit's summary from cache — but only the first time we see
/// this commit, so rapid navigation doesn't re-request commits we already know the state of.
fn request_summary(state: &AppState) -> Vec<Effect> {
    match state.selected() {
        Some(commit) if !state.summaries.contains_key(&commit.hash) => {
            vec![Effect::LoadSummary {
                hash: commit.hash.clone(),
            }]
        }
        _ => vec![],
    }
}

/// `s`: (re)generate the selected commit's summary via the model, unless one is already generating.
/// Progress and result show in the panel; the status line (keymap legend) is left untouched.
fn summarize_selected(state: &mut AppState) -> Vec<Effect> {
    let Some(commit) = state.selected() else {
        return vec![];
    };
    let hash = commit.hash.clone();
    let subject = commit.subject.clone();
    if matches!(
        state.summaries.get(&hash),
        Some(SummaryState::Generating(_))
    ) {
        return vec![]; // SUM-09: don't fire a duplicate generation.
    }
    state
        .summaries
        .insert(hash.clone(), SummaryState::Generating(String::new()));
    vec![Effect::GenerateSummary { hash, subject }]
}

/// `S`: open the expanded-summary modal for the selected commit (no-op if nothing is selected).
fn open_summary_modal(state: &mut AppState) -> Vec<Effect> {
    if state.selected().is_some() {
        state.mode = Mode::Summary;
    }
    vec![]
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
        s.size = (80, 11); // viewport_rows = 11 - CHROME(3) - SUMMARY(4) = 4, half = 2
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
        // Pre-seed the summaries for both views' top commits so re-selecting a cached view emits no
        // summary lookup either — restoring the strong guarantee that a cached view produces NO
        // effects at all (not just no LoadLog).
        for short in ["aaaaaaa", "eeeeeee"] {
            let hash = format!("{short}{}", "0".repeat(40 - short.len()));
            s.summaries
                .insert(hash, SummaryState::Ready("cached".into()));
        }
        // Go back to head, then to origin again: fully cached → no effects whatsoever.
        update(&mut s, key(KeyCode::Left));
        let effects = update(&mut s, key(KeyCode::Right));
        assert_eq!(
            effects,
            vec![],
            "a fully-cached view re-switch must emit no effects"
        );
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

    // SUM-02/03: the initial log load requests a cache lookup for the selected commit; a miss records
    // `Missing` (so the panel shows the hint) and is not re-requested.
    #[test]
    fn sum_02_03_initial_load_requests_summary_and_records_miss() {
        let mut s = AppState::new("feature".into(), "main".into(), None);
        s.size = (80, 24);
        let effects = update(
            &mut s,
            Event::LogLoaded {
                view: View::LocalHead,
                commits: vec![commit("aaaaaaa", "add fuzzy search")],
            },
        );
        let hash = s.selected_hash().unwrap();
        assert_eq!(effects, vec![Effect::LoadSummary { hash: hash.clone() }]);

        update(&mut s, Event::SummaryMissing { hash: hash.clone() });
        assert_eq!(s.summaries.get(&hash), Some(&SummaryState::Missing));

        // Moving away and back must not re-request a commit whose state we already know.
        drive(&mut s, vec![ch('j')]); // only one commit → no move, but exercise the path
        let effects = request_summary(&s);
        assert_eq!(effects, vec![], "known commit is not re-requested");
    }

    // SUM-02: a cache hit (SummaryLoaded) shows the summary immediately (Ready), no generation.
    #[test]
    fn sum_02_cache_hit_sets_ready() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(
            &mut s,
            Event::SummaryLoaded {
                hash: hash.clone(),
                text: "Adds fuzzy search to the log.".into(),
            },
        );
        assert_eq!(
            s.selected_summary(),
            Some(&SummaryState::Ready("Adds fuzzy search to the log.".into()))
        );
    }

    // A slow cache read landing after a user-triggered generation must not clobber the fresh result.
    #[test]
    fn sum_cache_read_does_not_clobber_generation() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s')); // Generating
        update(
            &mut s,
            Event::SummaryReady {
                hash: hash.clone(),
                text: "fresh".into(),
            },
        );
        // A late cache hit for the same commit arrives after generation already completed.
        update(
            &mut s,
            Event::SummaryLoaded {
                hash: hash.clone(),
                text: "stale-cached".into(),
            },
        );
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Ready("fresh".into())),
            "a late cache read must not overwrite the fresh generated summary"
        );
    }

    // Summary progress/failure lives in the panel, not the status line — so the keymap legend stays
    // visible throughout generation (no `status` writes on `s`, chunk, ready, or failure).
    #[test]
    fn sum_generation_never_touches_status_line() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s'));
        assert_eq!(s.status, None, "`s` must not overwrite the legend");
        update(
            &mut s,
            Event::SummaryChunk {
                hash: hash.clone(),
                delta: "x".into(),
            },
        );
        update(
            &mut s,
            Event::SummaryReady {
                hash: hash.clone(),
                text: "done".into(),
            },
        );
        assert_eq!(s.status, None, "completion must not touch the status line");

        update(&mut s, ch('s'));
        update(
            &mut s,
            Event::SummaryFailed {
                hash,
                error: "boom".into(),
            },
        );
        assert_eq!(s.status, None, "failure shows in the panel, not the legend");
    }

    // SUM-04: streamed chunks accumulate into the commit's partial summary while generating.
    #[test]
    fn sum_04_chunks_stream_into_partial() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s'));
        for delta in ["Adds ", "fuzzy ", "search."] {
            update(
                &mut s,
                Event::SummaryChunk {
                    hash: hash.clone(),
                    delta: delta.into(),
                },
            );
        }
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Generating("Adds fuzzy search.".into()))
        );
        // A chunk for a commit that isn't generating is ignored (no panic, no state).
        let other = format!("zzzzzzz{}", "0".repeat(33));
        update(
            &mut s,
            Event::SummaryChunk {
                hash: other.clone(),
                delta: "stray".into(),
            },
        );
        assert_eq!(s.summaries.get(&other), None);
    }

    // SUM-04: `s` starts generation for the selected commit (Generating + GenerateSummary effect).
    #[test]
    fn sum_04_s_starts_generation() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        let subject = s.selected().unwrap().subject.clone();
        let effects = update(&mut s, ch('s'));
        assert_eq!(
            effects,
            vec![Effect::GenerateSummary {
                hash: hash.clone(),
                subject
            }]
        );
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Generating(String::new()))
        );
    }

    // SUM-06: a finished generation stores the summary as Ready.
    #[test]
    fn sum_06_generated_summary_becomes_ready() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s'));
        update(
            &mut s,
            Event::SummaryReady {
                hash: hash.clone(),
                text: "Refactors the parser.".into(),
            },
        );
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Ready("Refactors the parser.".into()))
        );
    }

    // SUM-08: a failed generation records Failed (shown in the panel); `s` retries.
    #[test]
    fn sum_08_failure_records_failed_and_retries() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s'));
        update(
            &mut s,
            Event::SummaryFailed {
                hash: hash.clone(),
                error: "ollama not found".into(),
            },
        );
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Failed("ollama not found".into()))
        );

        // Retry: `s` re-enters Generating and emits a fresh effect.
        let effects = update(&mut s, ch('s'));
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Generating(String::new()))
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::GenerateSummary { .. }]
        ));
    }

    // SUM-09: pressing `s` while already generating is ignored (no duplicate effect).
    #[test]
    fn sum_09_no_duplicate_generation() {
        let mut s = app();
        update(&mut s, ch('s'));
        let effects = update(&mut s, ch('s'));
        assert_eq!(effects, vec![], "already generating → ignored");
    }

    // SUM-03: a late cache-miss must not clobber an in-flight generation.
    #[test]
    fn sum_03_miss_does_not_clobber_generating() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        update(&mut s, ch('s')); // Generating
        update(&mut s, Event::SummaryMissing { hash: hash.clone() });
        assert_eq!(
            s.summaries.get(&hash),
            Some(&SummaryState::Generating(String::new()))
        );
    }

    // `S` opens the expanded-summary modal; Esc (or S/q/Enter) closes it back to List.
    #[test]
    fn sum_modal_open_and_close() {
        let mut s = app();
        update(&mut s, ch('S'));
        assert_eq!(s.mode, Mode::Summary);
        update(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, Mode::List);

        update(&mut s, ch('S'));
        assert_eq!(s.mode, Mode::Summary);
        update(&mut s, ch('S')); // toggles closed
        assert_eq!(s.mode, Mode::List);
    }

    #[test]
    fn sum_modal_no_selection_is_noop() {
        let mut s = AppState::new("main".into(), "main".into(), None);
        s.logs.insert(View::LocalHead, Load::Loaded(vec![]));
        s.recompute_matches();
        update(&mut s, ch('S'));
        assert_eq!(s.mode, Mode::List, "nothing selected → modal stays closed");
    }

    // SUM-04: `s` with no commits selected is a safe no-op.
    #[test]
    fn sum_04_no_selection_is_noop() {
        let mut s = AppState::new("main".into(), "main".into(), None);
        s.logs.insert(View::LocalHead, Load::Loaded(vec![]));
        s.recompute_matches();
        assert_eq!(update(&mut s, ch('s')), vec![]);
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
