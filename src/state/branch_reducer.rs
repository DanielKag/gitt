//! The pure state transition for `gitt branch`. `update_branch` is the single entry point.
//!
//! Checkout/create/delete emit an [`Effect`] and, when it finishes, the shell reports back a
//! `BranchMutated` event that reloads the list from git — so the view (including the current-branch
//! marker) always reflects real repo state rather than an optimistic guess. The AI-summary plumbing
//! reuses the same events as `gitt log`, keyed by the branch's summary cache key.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::branch::{BranchAction, BranchLoad, BranchMenu, BranchMode, BranchState, ConfirmDelete};
use super::effect::Effect;
use super::event::Event;
use super::model::SummaryState;
use crate::domain::summary::strip_preamble;

/// Fold an event into the branch-screen state, returning the side effects the shell must perform.
pub fn update_branch(state: &mut BranchState, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(key) => on_key(state, key),
        Event::Resize(w, h) => {
            state.size = (w, h);
            state.clamp_scroll();
            vec![]
        }
        Event::BranchesLoaded(branches) => on_branches_loaded(state, branches),
        Event::BranchesFailed(error) => {
            state.status = Some(format!("branches failed: {error}"));
            state.load = BranchLoad::Failed(error);
            state.matches.clear();
            vec![]
        }
        // A checkout/create/delete finished: report it, then reload so the list can't drift.
        Event::BranchMutated { label, result } => {
            state.status = Some(match result {
                Ok(()) => label,
                Err(e) => format!("{label} failed: {e}"),
            });
            vec![Effect::LoadBranches]
        }
        // A copy/PR action finished; report its outcome (no reload — it didn't change the list).
        Event::ActionFinished { label, result } => {
            state.status = Some(match result {
                Ok(()) => label,
                Err(e) => format!("{label} failed: {e}"),
            });
            vec![]
        }
        // --- AI summary (same events as gitt log, keyed by the branch summary cache key) ----------
        Event::SummaryLoaded { hash, text } => {
            state
                .summaries
                .entry(hash)
                .or_insert(SummaryState::Ready(strip_preamble(&text)));
            vec![]
        }
        Event::SummaryMissing { hash } => {
            state.summaries.entry(hash).or_insert(SummaryState::Missing);
            vec![]
        }
        Event::SummaryChunk { hash, delta } => {
            if let Some(SummaryState::Generating(buf)) = state.summaries.get_mut(&hash) {
                buf.push_str(&delta);
            }
            vec![]
        }
        Event::SummaryReady { hash, text } => {
            state
                .summaries
                .insert(hash, SummaryState::Ready(strip_preamble(&text)));
            vec![]
        }
        Event::SummaryFailed { hash, error } => {
            state.summaries.insert(hash, SummaryState::Failed(error));
            vec![]
        }
        // The background PR fetch landed: overlay the statuses onto the column.
        Event::PrStatusesLoaded(map) => {
            state.pr_statuses = Some(map);
            vec![]
        }
        // gh missing / non-GitHub repo: leave the column blank (no false "none", no status noise).
        Event::PrStatusesFailed(_) => vec![],
        // Events for the other screens never reach this reducer at runtime; ignore.
        _ => vec![],
    }
}

/// Fold a freshly loaded branch list into the state, keeping the previously-selected branch under the
/// cursor when it still exists, then requesting the (new) selection's cached summary.
fn on_branches_loaded(
    state: &mut BranchState,
    branches: Vec<crate::domain::Branch>,
) -> Vec<Effect> {
    let before = state.selected_name();
    state.load = BranchLoad::Loaded(branches);
    state.recompute_matches();
    if let Some(name) = before
        && let Some(idx) = match_index_of(state, &name)
    {
        state.cursor = idx;
        state.clamp_scroll();
    }
    on_selection_changed(state)
}

/// The index into `matches` of the branch with `name`, if it's currently visible.
fn match_index_of(state: &BranchState, name: &str) -> Option<usize> {
    let branches = state.branches();
    state
        .matches
        .iter()
        .position(|m| branches.get(m.commit_idx).is_some_and(|b| b.name == name))
}

fn on_key(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
    // Ctrl-C always quits, from any mode.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return vec![Effect::Quit];
    }
    match state.mode {
        BranchMode::List => on_key_list(state, key),
        BranchMode::Search => on_key_search(state, key),
        BranchMode::Menu => on_key_menu(state, key),
        BranchMode::Confirm => on_key_confirm(state, key),
        BranchMode::Create => on_key_create(state, key),
    }
}

fn on_key_list(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
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
        KeyCode::Char('/') => {
            state.mode = BranchMode::Search;
            vec![]
        }
        KeyCode::Char('s') => summarize_selected(state),
        KeyCode::Char('S') => {
            state.summary_expanded = !state.summary_expanded;
            state.clamp_scroll(); // the list viewport just changed size
            vec![]
        }
        KeyCode::Char('n') => {
            state.mode = BranchMode::Create;
            state.create_input.clear();
            vec![]
        }
        KeyCode::Char('d') => open_confirm(state),
        KeyCode::Char('R') => reload(state),
        KeyCode::Enter => open_menu(state),
        _ => vec![],
    }
}

fn on_key_search(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.mode = BranchMode::List;
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

fn on_key_menu(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.menu = None;
            state.mode = BranchMode::List;
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

fn on_key_confirm(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => confirm_delete(state),
        KeyCode::Esc | KeyCode::Char('n') => {
            state.confirm = None;
            state.mode = BranchMode::List;
            vec![]
        }
        _ => vec![],
    }
}

fn on_key_create(state: &mut BranchState, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            state.create_input.clear();
            state.mode = BranchMode::List;
            vec![]
        }
        KeyCode::Enter => create_branch(state),
        KeyCode::Backspace => {
            state.create_input.pop();
            vec![]
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.create_input.push(c);
            vec![]
        }
        _ => vec![],
    }
}

// --- helpers -------------------------------------------------------------------------------------

fn quit(state: &mut BranchState) -> Vec<Effect> {
    state.should_quit = true;
    vec![Effect::Quit]
}

fn reload(state: &mut BranchState) -> Vec<Effect> {
    state.status = Some("reloading…".to_string());
    // Refetch PR statuses too, so `R` picks up PRs opened/merged since the screen opened.
    vec![Effect::LoadBranches, Effect::LoadPrStatuses]
}

fn move_by(state: &mut BranchState, delta: isize) -> Vec<Effect> {
    if state.matches.is_empty() {
        return vec![];
    }
    let len = state.matches.len() as isize;
    let target = (state.cursor as isize + delta).clamp(0, len - 1);
    set_cursor(state, target as usize)
}

fn set_cursor(state: &mut BranchState, idx: usize) -> Vec<Effect> {
    if state.matches.is_empty() {
        return vec![];
    }
    let before = state.selected_name();
    state.cursor = idx.min(state.matches.len() - 1);
    state.clamp_scroll();
    if state.selected_name() != before {
        on_selection_changed(state)
    } else {
        vec![]
    }
}

fn after_filter_change(state: &mut BranchState) -> Vec<Effect> {
    let before = state.selected_name();
    state.recompute_matches();
    if state.selected_name() != before {
        on_selection_changed(state)
    } else {
        vec![]
    }
}

/// Effects to run when the selection may have changed: load the branch's cached summary (once).
fn on_selection_changed(state: &BranchState) -> Vec<Effect> {
    match state.selected_summary_key() {
        Some(key) if !state.summaries.contains_key(&key) => vec![Effect::LoadSummary { hash: key }],
        _ => vec![],
    }
}

/// `s`: (re)generate the selected branch's summary via the model, unless one is already generating.
fn summarize_selected(state: &mut BranchState) -> Vec<Effect> {
    let (Some(key), Some(branch)) = (state.selected_summary_key(), state.selected_name()) else {
        return vec![];
    };
    if matches!(state.summaries.get(&key), Some(SummaryState::Generating(_))) {
        return vec![]; // don't fire a duplicate generation.
    }
    state
        .summaries
        .insert(key.clone(), SummaryState::Generating(String::new()));
    vec![Effect::GenerateBranchSummary {
        key,
        branch,
        base: state.main_branch.clone(),
    }]
}

fn open_menu(state: &mut BranchState) -> Vec<Effect> {
    if let Some(branch) = state.selected() {
        state.menu = Some(BranchMenu {
            items: BranchAction::all(),
            cursor: 0,
            name: branch.name.clone(),
            is_current: branch.is_current,
        });
        state.mode = BranchMode::Menu;
    }
    vec![]
}

fn menu_move(state: &mut BranchState, delta: isize) {
    if let Some(menu) = &mut state.menu {
        let len = menu.items.len() as isize;
        menu.cursor = (menu.cursor as isize + delta).clamp(0, len - 1) as usize;
    }
}

fn execute_menu(state: &mut BranchState) -> Vec<Effect> {
    let Some(menu) = state.menu.take() else {
        state.mode = BranchMode::List;
        return vec![];
    };
    let action = menu.selected();
    let name = menu.name;

    match action {
        BranchAction::Checkout => {
            state.mode = BranchMode::List;
            state.status = Some(format!("Checking out {name}…"));
            vec![Effect::CheckoutBranch(name)]
        }
        BranchAction::OpenPr => {
            state.mode = BranchMode::List;
            state.status = Some("Opening PR…".to_string());
            vec![Effect::OpenPr(name)]
        }
        BranchAction::CopyName => {
            state.mode = BranchMode::List;
            state.status = Some("Copied branch name".to_string());
            vec![Effect::CopyToClipboard(name)]
        }
        BranchAction::Delete => {
            // Route through the mandatory confirmation overlay (and refuse the current branch).
            if menu.is_current {
                state.mode = BranchMode::List;
                state.status = Some("cannot delete the current branch".to_string());
                vec![]
            } else {
                state.confirm = Some(ConfirmDelete { name });
                state.mode = BranchMode::Confirm;
                vec![]
            }
        }
    }
}

/// `d` / menu → open the delete confirmation, unless the selection is the current branch.
fn open_confirm(state: &mut BranchState) -> Vec<Effect> {
    match state.selected() {
        Some(branch) if branch.is_current => {
            state.status = Some("cannot delete the current branch".to_string());
            vec![]
        }
        Some(branch) => {
            state.confirm = Some(ConfirmDelete {
                name: branch.name.clone(),
            });
            state.mode = BranchMode::Confirm;
            vec![]
        }
        None => vec![],
    }
}

fn confirm_delete(state: &mut BranchState) -> Vec<Effect> {
    state.mode = BranchMode::List;
    match state.confirm.take() {
        Some(c) => {
            state.status = Some(format!("Deleting {}…", c.name));
            vec![Effect::DeleteBranch(c.name)]
        }
        None => vec![],
    }
}

fn create_branch(state: &mut BranchState) -> Vec<Effect> {
    let name = state.create_input.trim().to_string();
    if name.is_empty() {
        return vec![]; // empty name is a no-op; stay in Create mode.
    }
    state.create_input.clear();
    state.mode = BranchMode::List;
    state.status = Some(format!("Creating {name}…"));
    vec![Effect::CreateBranch(name)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Branch, branch::summary_key};

    fn branch(name: &str, current: bool) -> Branch {
        Branch {
            name: name.to_string(),
            is_current: current,
            tip: format!("{name:0<40}"),
            upstream: None,
            timestamp: 0,
            subject: format!("{name} subject"),
            relative: "now".to_string(),
            haystack: format!("{name} {name} subject"),
        }
    }

    fn app() -> BranchState {
        let mut s = BranchState::new("feature".to_string(), "main".to_string());
        s.size = (80, 24);
        s.load = BranchLoad::Loaded(vec![
            branch("feature", true),
            branch("main", false),
            branch("wip-parser", false),
            branch("bugfix", false),
        ]);
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
    fn drive(state: &mut BranchState, events: Vec<Event>) -> Vec<Effect> {
        let mut all = Vec::new();
        for e in events {
            all.extend(update_branch(state, e));
        }
        all
    }

    // BR-04: vim motions move within bounds.
    #[test]
    fn br_04_jk_moves_and_clamps() {
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

    // BR-03: '/' enters search, typing filters, Esc keeps the filter.
    #[test]
    fn br_03_search_filters_by_name() {
        let mut s = app();
        drive(&mut s, vec![ch('/')]);
        assert_eq!(s.mode, BranchMode::Search);
        drive(&mut s, vec![ch('w'), ch('i'), ch('p')]);
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.selected_name().as_deref(), Some("wip-parser"));
        drive(&mut s, vec![key(KeyCode::Esc)]);
        assert_eq!(s.mode, BranchMode::List);
        assert_eq!(s.filter, "wip");
    }

    // BR-05: Enter opens the action menu with the four actions; Esc closes it.
    #[test]
    fn br_05_enter_opens_menu() {
        let mut s = app();
        update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(s.mode, BranchMode::Menu);
        assert_eq!(s.menu.as_ref().unwrap().items, BranchAction::all());
        update_branch(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, BranchMode::List);
        assert!(s.menu.is_none());
    }

    // BR-06: Checkout emits a CheckoutBranch effect for the selected branch.
    #[test]
    fn br_06_checkout_effect() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // wip-parser
        update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(s.menu.as_ref().unwrap().selected(), BranchAction::Checkout);
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::CheckoutBranch("wip-parser".into())]);
    }

    // BR-06: a finished mutation reloads the branch list.
    #[test]
    fn br_06_mutation_reloads_list() {
        let mut s = app();
        let effects = update_branch(
            &mut s,
            Event::BranchMutated {
                label: "Checked out".into(),
                result: Ok(()),
            },
        );
        assert_eq!(effects, vec![Effect::LoadBranches]);
        assert_eq!(s.status.as_deref(), Some("Checked out"));
    }

    // BR-07: Open PR emits an OpenPr effect carrying the branch name.
    #[test]
    fn br_07_open_pr_effect() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // wip-parser
        update_branch(&mut s, key(KeyCode::Enter));
        update_branch(&mut s, ch('j')); // Open PR
        assert_eq!(s.menu.as_ref().unwrap().selected(), BranchAction::OpenPr);
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::OpenPr("wip-parser".into())]);
    }

    // BR-08: Copy name copies the selected branch name.
    #[test]
    fn br_08_copy_name() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // wip-parser
        update_branch(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j')]); // Copy name
        assert_eq!(s.menu.as_ref().unwrap().selected(), BranchAction::CopyName);
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::CopyToClipboard("wip-parser".into())]);
    }

    // BR-09: Delete routes through the confirmation overlay; y confirms with a DeleteBranch effect.
    #[test]
    fn br_09_delete_requires_confirmation() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // wip-parser (not current)
        update_branch(&mut s, ch('d'));
        assert_eq!(s.mode, BranchMode::Confirm);
        assert_eq!(s.confirm.as_ref().unwrap().name, "wip-parser");
        let effects = update_branch(&mut s, ch('y'));
        assert_eq!(effects, vec![Effect::DeleteBranch("wip-parser".into())]);
        assert_eq!(s.mode, BranchMode::List);
        assert!(s.confirm.is_none());
    }

    // BR-09: Esc cancels the deletion without acting.
    #[test]
    fn br_09_delete_cancel() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]);
        update_branch(&mut s, ch('d'));
        let effects = update_branch(&mut s, key(KeyCode::Esc));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, BranchMode::List);
        assert!(s.confirm.is_none());
    }

    // BR-09: the current branch cannot be deleted — `d` on it reports a status, opens no overlay.
    #[test]
    fn br_09_cannot_delete_current() {
        let mut s = app(); // cursor 0 = "feature" is current
        let effects = update_branch(&mut s, ch('d'));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, BranchMode::List);
        assert!(s.confirm.is_none());
        assert_eq!(
            s.status.as_deref(),
            Some("cannot delete the current branch")
        );
    }

    // BR-09: the menu's Delete on the current branch is likewise refused.
    #[test]
    fn br_09_menu_delete_current_refused() {
        let mut s = app(); // cursor 0 = "feature" is current
        update_branch(&mut s, key(KeyCode::Enter));
        drive(&mut s, vec![ch('j'), ch('j'), ch('j')]); // Delete branch
        assert_eq!(s.menu.as_ref().unwrap().selected(), BranchAction::Delete);
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, BranchMode::List);
        assert_eq!(
            s.status.as_deref(),
            Some("cannot delete the current branch")
        );
    }

    // BR-10: `n` opens the create input; typing + Enter creates the branch; empty is a no-op; Esc cancels.
    #[test]
    fn br_10_create_branch_flow() {
        let mut s = app();
        update_branch(&mut s, ch('n'));
        assert_eq!(s.mode, BranchMode::Create);
        // Empty Enter is a no-op (stays in Create).
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![]);
        assert_eq!(s.mode, BranchMode::Create);
        // Type a name and confirm.
        drive(&mut s, vec![ch('f'), ch('o'), ch('o')]);
        assert_eq!(s.create_input, "foo");
        let effects = update_branch(&mut s, key(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::CreateBranch("foo".into())]);
        assert_eq!(s.mode, BranchMode::List);
        assert_eq!(s.create_input, "");
    }

    #[test]
    fn br_10_create_esc_cancels() {
        let mut s = app();
        drive(&mut s, vec![ch('n'), ch('x')]);
        assert_eq!(s.create_input, "x");
        update_branch(&mut s, key(KeyCode::Esc));
        assert_eq!(s.mode, BranchMode::List);
        assert_eq!(s.create_input, "");
    }

    // BR-11: loading branches requests the selected branch's cached summary (keyed by tip); a miss
    // records Missing and is not re-requested.
    #[test]
    fn br_11_load_requests_summary_and_records_miss() {
        let mut s = BranchState::new("main".into(), "main".into());
        s.size = (80, 24);
        let b = branch("feature", false);
        let effects = update_branch(&mut s, Event::BranchesLoaded(vec![b.clone()]));
        // The lookup key is derived from the branch tip (prefixed so it can't collide with a commit).
        let expected_key = summary_key(&b.tip);
        assert_eq!(
            s.selected_summary_key().as_deref(),
            Some(expected_key.as_str())
        );
        assert_eq!(
            effects,
            vec![Effect::LoadSummary {
                hash: expected_key.clone()
            }]
        );

        update_branch(
            &mut s,
            Event::SummaryMissing {
                hash: expected_key.clone(),
            },
        );
        assert_eq!(s.summaries.get(&expected_key), Some(&SummaryState::Missing));
        assert_eq!(
            on_selection_changed(&s),
            vec![],
            "known summary not re-requested"
        );
    }

    // BR-12: `s` starts generation for the selected branch (Generating + GenerateBranchSummary).
    #[test]
    fn br_12_s_starts_generation() {
        let mut s = app();
        let key = s.selected_summary_key().unwrap();
        let name = s.selected_name().unwrap();
        let effects = update_branch(&mut s, ch('s'));
        assert_eq!(
            effects,
            vec![Effect::GenerateBranchSummary {
                key: key.clone(),
                branch: name,
                base: "main".into(),
            }]
        );
        assert_eq!(
            s.summaries.get(&key),
            Some(&SummaryState::Generating(String::new()))
        );
        // A second `s` while generating is ignored.
        assert_eq!(update_branch(&mut s, ch('s')), vec![]);
    }

    // BR-12: streamed chunks accumulate; a Ready overwrites; `S` toggles the expanded footer.
    #[test]
    fn br_12_summary_stream_and_expand() {
        let mut s = app();
        let key = s.selected_summary_key().unwrap();
        update_branch(&mut s, ch('s'));
        for delta in ["Adds ", "the ", "widget."] {
            update_branch(
                &mut s,
                Event::SummaryChunk {
                    hash: key.clone(),
                    delta: delta.into(),
                },
            );
        }
        assert_eq!(
            s.summaries.get(&key),
            Some(&SummaryState::Generating("Adds the widget.".into()))
        );
        update_branch(
            &mut s,
            Event::SummaryReady {
                hash: key.clone(),
                text: "Adds the widget.".into(),
            },
        );
        assert_eq!(
            s.summaries.get(&key),
            Some(&SummaryState::Ready("Adds the widget.".into()))
        );

        assert!(!s.summary_expanded);
        update_branch(&mut s, ch('S'));
        assert!(s.summary_expanded);
        assert_eq!(s.mode, BranchMode::List, "still navigable");
    }

    // BR-12: summary progress/failure never touches the status line (keeps the keymap legend).
    #[test]
    fn br_12_summary_never_touches_status_line() {
        let mut s = app();
        update_branch(&mut s, ch('s'));
        assert_eq!(s.status, None);
        let key = s.selected_summary_key().unwrap();
        update_branch(
            &mut s,
            Event::SummaryFailed {
                hash: key,
                error: "boom".into(),
            },
        );
        assert_eq!(s.status, None);
    }

    // BR-17: a background PR fetch fills the status map; a failure leaves it unset (column blank).
    #[test]
    fn br_17_pr_statuses_loaded_and_failed() {
        use crate::domain::PrStatus;
        use std::collections::HashMap;

        let mut s = app();
        assert!(s.pr_statuses.is_none(), "unknown until the fetch lands");

        let mut map = HashMap::new();
        map.insert("wip-parser".to_string(), PrStatus::Open);
        update_branch(&mut s, Event::PrStatusesLoaded(map));
        assert_eq!(s.pr_status("wip-parser"), Some(PrStatus::Open));
        assert_eq!(s.pr_status("main"), None, "a branch with no PR is None");

        // A failure must not clobber a previously-loaded map, and never touches the status line.
        update_branch(&mut s, Event::PrStatusesFailed("no gh".into()));
        assert_eq!(s.pr_status("wip-parser"), Some(PrStatus::Open));
        assert_eq!(s.status, None);
    }

    // BR-15/17: R reloads the branch list AND refetches PR statuses; q and Ctrl-c quit.
    #[test]
    fn br_15_reload_and_quit() {
        let mut s = app();
        assert_eq!(
            update_branch(&mut s, ch('R')),
            vec![Effect::LoadBranches, Effect::LoadPrStatuses]
        );

        let mut s = app();
        let effects = update_branch(&mut s, ch('q'));
        assert!(s.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);

        let mut s2 = app();
        let effects = update_branch(&mut s2, ctrl('c'));
        assert!(s2.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    // BR-16: an empty list makes motions and actions safe no-ops.
    #[test]
    fn br_16_empty_list_noops() {
        let mut s = BranchState::new("main".into(), "main".into());
        s.load = BranchLoad::Loaded(vec![]);
        s.recompute_matches();
        assert_eq!(update_branch(&mut s, ch('j')), vec![]);
        assert_eq!(update_branch(&mut s, key(KeyCode::Enter)), vec![]);
        assert_eq!(update_branch(&mut s, ch('d')), vec![]);
        assert_eq!(update_branch(&mut s, ch('s')), vec![]);
        assert_eq!(s.mode, BranchMode::List);
    }

    // Reload keeps the previously-selected branch under the cursor when it still exists.
    #[test]
    fn reload_preserves_selection() {
        let mut s = app();
        drive(&mut s, vec![ch('j'), ch('j')]); // wip-parser
        assert_eq!(s.selected_name().as_deref(), Some("wip-parser"));
        update_branch(
            &mut s,
            Event::BranchesLoaded(vec![
                branch("feature", true),
                branch("wip-parser", false),
                branch("bugfix", false),
            ]),
        );
        assert_eq!(
            s.selected_name().as_deref(),
            Some("wip-parser"),
            "selection follows the branch across a reload"
        );
    }
}
