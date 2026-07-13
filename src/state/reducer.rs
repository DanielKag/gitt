//! The pure state transition function. `update` is the single entry point.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::Effect;
use super::event::Event;
use super::model::{ActionMenu, AppState, Load, MenuAction, Mode, PreviewState, SummaryState};
use crate::domain::summary::strip_preamble;
use crate::domain::{Commit, View, url};
use crate::fuzzy::MatchEntry;

/// Fold an event into the state, returning the side effects the shell must perform.
pub fn update(state: &mut AppState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            vec![]
        }
        Event::LogBatch {
            view,
            skip,
            epoch,
            commits,
        } => on_log_batch(state, view, skip, epoch, commits),
        Event::LogPageFailed { view, epoch, error } => {
            on_log_page_failed(state, view, epoch, error)
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
                vec![start_log_load(state, state.view)]
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
        // Status-, diff-, and branch-screen events never reach the log reducer at runtime (separate
        // screens); ignore them.
        Event::StatusLoaded(_)
        | Event::StatusFailed(_)
        | Event::FileDiffLoaded { .. }
        | Event::FileDiffFailed { .. }
        | Event::StatusMutated { .. }
        | Event::DiffFilesLoaded { .. }
        | Event::DiffFilesFailed { .. }
        | Event::DiffTextLoaded { .. }
        | Event::DiffTextFailed { .. }
        | Event::BranchesLoaded(_)
        | Event::BranchesFailed(_)
        | Event::BranchMutated { .. }
        | Event::PrStatusesLoaded(_)
        | Event::PrStatusesFailed(_) => vec![],
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
        KeyCode::Char('s') => summarize_selected(state),
        KeyCode::Char('S') => {
            state.summary_expanded = !state.summary_expanded;
            state.clamp_scroll(); // the list viewport just changed size
            vec![]
        }
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
        // LOG-26: peek at the diff without leaving the search you're typing.
        KeyCode::Tab => toggle_preview(state),
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
    if state.selected_hash() != before {
        on_selection_changed(state)
    } else {
        vec![]
    }
}

/// (Re)start a view's progressive load: bump its epoch (so any in-flight older load's batches become
/// stale and are ignored), mark it `Loading`, and return the effect that fetches the first page.
pub(crate) fn start_log_load(state: &mut AppState, view: View) -> Effect {
    let epoch = state.log_epoch.entry(view).or_insert(0);
    *epoch += 1;
    let epoch = *epoch;
    state.logs.insert(view, Load::Loading);
    if view == state.view {
        // Reloading in place (e.g. after `R`) drops the current view's commits back to `Loading`.
        // Recompute so `matches` can't keep indexing into the now-empty commit slice — otherwise the
        // redraw before the first batch arrives would index out of bounds and panic the TUI.
        state.recompute_matches();
    }
    Effect::LoadLogPage {
        view,
        skip: 0,
        limit: page_limit(state, 0),
        epoch,
    }
}

/// The page size to request when `loaded` commits are already in for this view, honouring the cap:
/// unlimited (`max_count == 0`) always asks for a full page; otherwise never asks for more than the
/// cap leaves, so a small `--max-count` doesn't pull (and parse) a full 5000-commit page to throw away.
fn page_limit(state: &AppState, loaded: usize) -> usize {
    match state.max_count {
        0 => state.log_page,
        cap => state.log_page.min(cap.saturating_sub(loaded)),
    }
}

/// Fold one loaded page into a view. Appends to what's already there (or replaces on the first page),
/// finalises to `Loaded` when the history is exhausted or the cap is hit, and otherwise requests the
/// next page. Stale batches (from a superseded load) are dropped. Appending never disturbs the
/// selected commit.
fn on_log_batch(
    state: &mut AppState,
    view: View,
    skip: usize,
    epoch: u64,
    commits: Vec<Commit>,
) -> Vec<Effect> {
    // Drop batches from a load that's already been superseded (e.g. a reload started meanwhile).
    if state.log_epoch.get(&view).copied().unwrap_or(0) != epoch {
        return vec![];
    }

    let is_current = view == state.view;
    // Remember the selection so appended (older) commits can't shift it out from under the cursor.
    let before = if is_current {
        state.selected_hash()
    } else {
        None
    };

    let batch_len = commits.len();
    // The first page (skip == 0) replaces any prior contents; later pages append.
    let mut all = if skip == 0 {
        Vec::new()
    } else {
        match state.logs.remove(&view) {
            Some(Load::Streaming(v)) | Some(Load::Loaded(v)) => v,
            _ => Vec::new(),
        }
    };
    let loaded_before = all.len();
    all.extend(commits);

    // A short page (fewer than we asked for) means git ran out of history; hitting the cap also ends
    // the load. Comparing against what we actually requested (not the raw page size) keeps this right
    // when the cap shrinks the final request.
    let mut done = batch_len < page_limit(state, loaded_before);
    if state.max_count != 0 && all.len() >= state.max_count {
        all.truncate(state.max_count);
        done = true;
    }
    let total = all.len();
    state.logs.insert(
        view,
        if done {
            Load::Loaded(all)
        } else {
            Load::Streaming(all)
        },
    );

    let mut effects = Vec::new();
    if is_current {
        if state.filter.trim().is_empty() {
            // Fast path: with no query the matches are just every commit in order, so extend with the
            // newly appended tail instead of re-fuzzy-filtering the whole (growing) list each page —
            // that keeps a streaming load O(n) overall rather than O(n²).
            let start = state.matches.len();
            state
                .matches
                .extend((start..total).map(|commit_idx| MatchEntry { commit_idx }));
        } else {
            state.recompute_matches();
        }
        // Keep the previously-selected commit under the cursor across the append.
        if let Some(ref hash) = before
            && let Some(idx) = match_index_of(state, hash)
        {
            state.cursor = idx;
        }
        state.clamp_cursor();
        state.clamp_scroll();
        // The very first batch introduces a selection where there was none: load its preview/summary.
        if state.selected_hash() != before {
            effects.extend(on_selection_changed(state));
        }
    }
    if !done {
        effects.push(Effect::LoadLogPage {
            view,
            skip: total,
            limit: page_limit(state, total),
            epoch,
        });
    }
    effects
}

/// The index into `matches` of the commit with `hash`, if it's currently visible.
fn match_index_of(state: &AppState, hash: &str) -> Option<usize> {
    let commits = state.commits();
    state
        .matches
        .iter()
        .position(|m| commits.get(m.commit_idx).is_some_and(|c| c.hash == hash))
}

/// Handle a failed page. Pages that already landed are kept (the load simply stops with a status
/// message); a failure of the very first page puts the view into the failed state as before.
fn on_log_page_failed(state: &mut AppState, view: View, epoch: u64, error: String) -> Vec<Effect> {
    if state.log_epoch.get(&view).copied().unwrap_or(0) != epoch {
        return vec![];
    }
    let is_current = view == state.view;
    match state.logs.remove(&view) {
        // Later page failed but earlier ones are here: keep them, stop paging, note it.
        Some(Load::Streaming(commits)) if !commits.is_empty() => {
            state.logs.insert(view, Load::Loaded(commits));
            if is_current {
                state.status = Some(format!("log load stopped: {error}"));
            }
        }
        // First page failed (or nothing loaded): surface the failure.
        _ => {
            state.logs.insert(view, Load::Failed(error.clone()));
            if is_current {
                state.matches.clear();
                state.status = Some(format!("log failed: {error}"));
            }
        }
    }
    vec![]
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
        effects.push(start_log_load(state, view));
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

    // --- progressive-load helpers (LOG-21..24) ---------------------------------------------------

    /// A fresh app with no commits loaded yet (unlike `app()`, which pre-seeds a `Loaded` view).
    fn empty_app() -> AppState {
        let mut s = AppState::new("feature".into(), "main".into(), None);
        s.size = (80, 24);
        s
    }

    /// `count` distinct commits starting at index `start` (unique short hashes and subjects).
    fn commits_n(start: usize, count: usize) -> Vec<Commit> {
        (start..start + count)
            .map(|i| commit(&format!("{i:07x}"), &format!("commit {i}")))
            .collect()
    }

    fn batch(skip: usize, epoch: u64, commits: Vec<Commit>) -> Event {
        Event::LogBatch {
            view: View::LocalHead,
            skip,
            epoch,
            commits,
        }
    }

    fn has_next_page(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadLogPage { .. }))
    }

    // LOG-21/24: the log streams in page by page — a full page keeps loading; a short page completes.
    #[test]
    fn log_21_pages_stream_then_complete() {
        let mut s = empty_app();
        s.log_page = 3;
        let eff = start_log_load(&mut s, View::LocalHead);
        assert_eq!(
            eff,
            Effect::LoadLogPage {
                view: View::LocalHead,
                skip: 0,
                limit: 3,
                epoch: 1,
            }
        );

        // First full page → still streaming, and the next page is requested.
        let effects = update(&mut s, batch(0, 1, commits_n(0, 3)));
        assert!(matches!(
            s.logs.get(&View::LocalHead),
            Some(Load::Streaming(_))
        ));
        assert_eq!(s.commits().len(), 3);
        assert!(effects.contains(&Effect::LoadLogPage {
            view: View::LocalHead,
            skip: 3,
            limit: 3,
            epoch: 1,
        }));

        // Second, short page → complete (Loaded), no further page requested.
        let effects = update(&mut s, batch(3, 1, commits_n(3, 2)));
        assert!(matches!(
            s.logs.get(&View::LocalHead),
            Some(Load::Loaded(_))
        ));
        assert_eq!(s.commits().len(), 5);
        assert!(!has_next_page(&effects));
    }

    // LOG-22: the loading count is reported while streaming and clears once complete.
    #[test]
    fn log_22_streaming_reports_progress_then_clears() {
        let mut s = empty_app();
        s.log_page = 3;
        start_log_load(&mut s, View::LocalHead);
        update(&mut s, batch(0, 1, commits_n(0, 3)));
        assert_eq!(s.log_loading_count(), Some(3));
        update(&mut s, batch(3, 1, commits_n(3, 1))); // short page → done
        assert_eq!(s.log_loading_count(), None);
    }

    // LOG-23: appending a page keeps the same commit selected even when it wasn't at the top.
    #[test]
    fn log_23_append_preserves_selection_while_browsing() {
        let mut s = empty_app();
        s.log_page = 3;
        start_log_load(&mut s, View::LocalHead);
        update(&mut s, batch(0, 1, commits_n(0, 3)));
        drive(&mut s, vec![ch('j')]); // select the 2nd row
        assert_eq!(s.cursor, 1);
        let sel = s.selected_hash().unwrap();

        update(&mut s, batch(3, 1, commits_n(3, 3))); // append older commits
        assert_eq!(s.commits().len(), 6);
        assert_eq!(
            s.selected_hash().unwrap(),
            sel,
            "selection follows the commit, not the index"
        );
    }

    // LOG-23: with an active filter, appending a later page keeps the same commit selected and
    // re-filters over everything loaded so far (the cursor stays put as the match list grows).
    #[test]
    fn log_23_append_preserves_selection() {
        let mut s = empty_app();
        s.log_page = 2;
        start_log_load(&mut s, View::LocalHead);
        // First page: two commits, both contain the substring "fix".
        update(
            &mut s,
            batch(
                0,
                1,
                vec![commit("aaaaaaa", "fix one"), commit("bbbbbbb", "fix two")],
            ),
        );
        s.filter = "fix".into();
        s.recompute_matches();
        // Select the second match.
        s.cursor = 1;
        let sel = s.selected_hash().unwrap();
        assert_eq!(sel, commit("bbbbbbb", "fix two").hash);

        // Append a later page: another "fix" plus a non-matching commit.
        update(
            &mut s,
            batch(
                2,
                1,
                vec![commit("ccccccc", "fix three"), commit("ddddddd", "nope")],
            ),
        );
        assert_eq!(
            s.matches.len(),
            3,
            "the three 'fix' commits match; 'nope' does not"
        );
        assert_eq!(
            s.selected_hash().unwrap(),
            sel,
            "selection preserved across the append"
        );
    }

    // LOG-10 regression: a fetch-reload swaps the loaded view back to `Loading`, which must also clear
    // `matches` — otherwise the redraw before the first batch arrives indexes an empty commit slice.
    #[test]
    fn log_10_reload_clears_stale_matches() {
        let mut s = app();
        assert!(!s.matches.is_empty());
        update(&mut s, ch('R'));
        update(&mut s, Event::FetchFinished(Ok(())));
        assert!(matches!(s.logs.get(&View::LocalHead), Some(Load::Loading)));
        assert!(
            s.matches.is_empty(),
            "reload must clear matches so rendering can't index an empty commit slice"
        );
    }

    // LOG-24: a small `--max-count` bounds the very first page too, so we don't fetch a full page to
    // immediately throw most of it away.
    #[test]
    fn log_24_first_page_bounded_by_max_count() {
        let mut s = empty_app();
        s.log_page = 5000;
        s.max_count = 2;
        let eff = start_log_load(&mut s, View::LocalHead);
        assert_eq!(
            eff,
            Effect::LoadLogPage {
                view: View::LocalHead,
                skip: 0,
                limit: 2, // min(log_page, max_count), not 5000
                epoch: 1,
            }
        );
    }

    // LOG-23: the empty-filter fast path produces exactly the same matches a full recompute would.
    #[test]
    fn log_23_incremental_matches_equal_full_recompute() {
        let mut s = empty_app();
        s.log_page = 3;
        start_log_load(&mut s, View::LocalHead);
        update(&mut s, batch(0, 1, commits_n(0, 3)));
        update(&mut s, batch(3, 1, commits_n(3, 3)));
        let incremental = s.matches.clone();
        s.recompute_matches(); // authoritative full pass
        assert_eq!(
            incremental, s.matches,
            "incremental append must match a full recompute"
        );
    }

    // LOG-24: `max_count` caps the total loaded and ends the stream once reached.
    #[test]
    fn log_24_max_count_caps_total_and_stops() {
        let mut s = empty_app();
        s.log_page = 2;
        s.max_count = 3;
        start_log_load(&mut s, View::LocalHead);

        let effects = update(&mut s, batch(0, 1, commits_n(0, 2)));
        assert_eq!(s.commits().len(), 2, "under the cap → keep going");
        assert!(has_next_page(&effects));

        let effects = update(&mut s, batch(2, 1, commits_n(2, 2)));
        assert_eq!(s.commits().len(), 3, "truncated to the cap");
        assert!(matches!(
            s.logs.get(&View::LocalHead),
            Some(Load::Loaded(_))
        ));
        assert!(!has_next_page(&effects), "cap reached → stop paging");
    }

    // LOG-21: a batch from a superseded load (older epoch) is ignored, not merged in.
    #[test]
    fn log_21_stale_batch_ignored() {
        let mut s = empty_app();
        s.log_page = 2;
        start_log_load(&mut s, View::LocalHead); // epoch 1
        update(&mut s, batch(0, 1, commits_n(0, 2))); // streaming
        start_log_load(&mut s, View::LocalHead); // epoch 2 → resets to Loading
        assert!(matches!(s.logs.get(&View::LocalHead), Some(Load::Loading)));

        let effects = update(&mut s, batch(2, 1, commits_n(2, 2))); // stale (epoch 1)
        assert!(matches!(s.logs.get(&View::LocalHead), Some(Load::Loading)));
        assert!(effects.is_empty());
    }

    // LOG-21 edge case: a page failing mid-stream keeps what already loaded and stops, with a status.
    #[test]
    fn log_21_mid_stream_failure_keeps_partial() {
        let mut s = empty_app();
        s.log_page = 2;
        start_log_load(&mut s, View::LocalHead);
        update(&mut s, batch(0, 1, commits_n(0, 2)));
        let effects = update(
            &mut s,
            Event::LogPageFailed {
                view: View::LocalHead,
                epoch: 1,
                error: "boom".into(),
            },
        );
        assert_eq!(s.commits().len(), 2, "partial commits are kept");
        assert!(matches!(
            s.logs.get(&View::LocalHead),
            Some(Load::Loaded(_))
        ));
        assert!(s.status.as_deref().unwrap().contains("stopped"));
        assert!(effects.is_empty());
    }

    // LOG-21 edge case: the very first page failing leaves the view in the failed state.
    #[test]
    fn log_21_first_page_failure_marks_failed() {
        let mut s = empty_app();
        start_log_load(&mut s, View::LocalHead);
        update(
            &mut s,
            Event::LogPageFailed {
                view: View::LocalHead,
                epoch: 1,
                error: "boom".into(),
            },
        );
        assert!(matches!(
            s.logs.get(&View::LocalHead),
            Some(Load::Failed(_))
        ));
        assert!(s.status.as_deref().unwrap().contains("failed"));
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

    // LOG-05: search is exact substring-per-term, not a fuzzy subsequence.
    #[test]
    fn log_05_search_is_exact_substring() {
        let mut s = app();
        // "refctor" is a subsequence of "refactor parser" but not a substring -> no matches.
        drive(&mut s, vec![ch('/')]);
        for c in "refctor".chars() {
            drive(&mut s, vec![ch(c)]);
        }
        assert_eq!(s.matches.len(), 0, "subsequence must not match");
        // The real substring does match.
        drive(&mut s, vec![key(KeyCode::Backspace)]); // "refcto"
        s.filter = "refactor".into();
        s.recompute_matches();
        assert_eq!(s.matches.len(), 1);
        assert_eq!(
            s.commits()[s.matches[0].commit_idx].subject,
            "refactor parser"
        );
    }

    // LOG-26: Tab toggles the diff preview while in search mode, staying in search mode.
    #[test]
    fn log_26_tab_toggles_preview_in_search_mode() {
        let mut s = app();
        drive(&mut s, vec![ch('/')]);
        assert_eq!(s.mode, Mode::Search);
        assert!(!s.preview_open);

        let effects = drive(&mut s, vec![key(KeyCode::Tab)]);
        assert!(s.preview_open, "Tab opens the preview in search mode");
        assert_eq!(s.mode, Mode::Search, "still typing a search");
        assert!(
            effects.iter().any(|e| matches!(e, Effect::LoadDiff(_))),
            "opening the preview requests the selected commit's diff"
        );

        drive(&mut s, vec![key(KeyCode::Tab)]);
        assert!(!s.preview_open, "Tab again closes it");
        assert_eq!(s.mode, Mode::Search);
    }

    // LOG-07: arrows toggle view and load origin lazily.
    #[test]
    fn log_07_right_switches_to_origin_and_loads() {
        let mut s = app();
        let effects = update(&mut s, key(KeyCode::Right));
        assert_eq!(s.view, View::OriginMain);
        assert_eq!(
            effects,
            vec![Effect::LoadLogPage {
                view: View::OriginMain,
                skip: 0,
                limit: crate::state::model::LOG_PAGE,
                epoch: 1,
            }]
        );
        assert_eq!(s.logs.get(&View::OriginMain), Some(&Load::Loading));
    }

    #[test]
    fn log_07_origin_cached_after_load_no_reload() {
        let mut s = app();
        update(&mut s, key(KeyCode::Right));
        update(
            &mut s,
            Event::LogBatch {
                view: View::OriginMain,
                skip: 0,
                epoch: 1,
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
        assert_eq!(
            effects,
            vec![Effect::LoadLogPage {
                view: View::LocalHead,
                skip: 0,
                limit: crate::state::model::LOG_PAGE,
                epoch: 1,
            }]
        );
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
        start_log_load(&mut s, View::LocalHead); // epoch 1, view Loading
        let effects = update(
            &mut s,
            Event::LogBatch {
                view: View::LocalHead,
                skip: 0,
                epoch: 1,
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

    // `S` toggles the expanded summary footer in place — the screen stays in List mode so navigation
    // still works — and `Esc` does NOT collapse it.
    #[test]
    fn sum_s_toggles_expanded_footer() {
        let mut s = app();
        assert!(!s.summary_expanded);
        update(&mut s, ch('S'));
        assert!(s.summary_expanded);
        assert_eq!(s.mode, Mode::List, "still in List mode → navigation works");

        // Esc must not collapse it.
        update(&mut s, key(KeyCode::Esc));
        assert!(s.summary_expanded, "Esc does not minimize");

        // S again collapses.
        update(&mut s, ch('S'));
        assert!(!s.summary_expanded);
    }

    // Expanding grows the footer for a long summary, shrinking the list viewport; the reducer's
    // scroll math stays consistent (cursor remains visible).
    #[test]
    fn sum_expanded_footer_shrinks_list_viewport() {
        let mut s = app();
        let hash = s.selected_hash().unwrap();
        let long = (0..90)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        s.summaries.insert(hash, SummaryState::Ready(long));
        let collapsed = s.viewport_rows();
        update(&mut s, ch('S'));
        assert!(
            s.viewport_rows() < collapsed,
            "expanded footer leaves fewer list rows"
        );
    }

    // `s` still generates while the footer is expanded (generation isn't gated on collapse).
    #[test]
    fn sum_generate_works_while_expanded() {
        let mut s = app();
        update(&mut s, ch('S')); // expand
        let effects = update(&mut s, ch('s'));
        assert!(matches!(
            effects.as_slice(),
            [Effect::GenerateSummary { .. }]
        ));
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
