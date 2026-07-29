//! End-to-end tests for `gitt status`: spawn the real binary against a real dirty repo over a PTY,
//! drive keystrokes, and assert on what it renders and the real git side effects it produces. Each
//! test name carries the spec criterion id(s) it covers (see specs/status.md).

mod common;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

fn spawn(repo: &std::path::Path) -> Tui {
    Tui::spawn_cmd(repo, "status")
}

// STAT-01 / STAT-13: renders the changed files with badges, then quits cleanly on `q`.
#[test]
fn stat_01_13_renders_files_and_quits() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());

    tui.wait_for("newstaged.txt");
    tui.wait_for("tracked.txt");
    tui.wait_for("untracked.txt");
    // The header summary reflects the counts.
    tui.wait_for("3 changed");

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-16 (regression): quitting must leave the terminal exactly as native git would — no leaked
// pen. The last frame renders the list with a REVERSED selected row (and, on the commit path, a
// full-screen DIM overlay); without an SGR reset on teardown that style bleeds into the shell prompt
// as stray colored blocks / underlines until the window is closed.
#[test]
fn stat_16_quit_resets_terminal_pen() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("newstaged.txt");

    tui.send_str("q");
    tui.wait_exit();

    tui.assert_pen_reset_on_teardown();
}

// STAT-04: Space stages a modified file (badge flips), and again unstages it — real index changes.
#[test]
fn stat_04_space_stages_and_unstages() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("tracked.txt");
    // One file (newstaged.txt) starts staged. Sync on the *header count* rather than on the transient
    // "Staged"/"Unstaged" status message: the message appears as soon as git returns, but the list is
    // only correct once the follow-up reload lands. Pressing Space in that window would act on the
    // stale row and re-stage instead of unstaging.
    tui.wait_for("1 staged");

    // Cursor 0 = newstaged.txt; move to tracked.txt (cursor 1).
    tui.send_str("j");
    tui.send_str(" ");
    tui.wait_for("2 staged");
    assert!(repo.is_staged("tracked.txt"), "Space should stage the file");

    // Now fully staged → Space unstages it.
    tui.send_str(" ");
    tui.wait_for("1 staged");
    assert!(
        !repo.is_staged("tracked.txt"),
        "Space again should unstage the file"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-06: Tab previews the worktree diff of a modified file, and the contents of an untracked one.
#[test]
fn stat_06_preview_diff_and_untracked_contents() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("tracked.txt");

    // Preview the modified file's worktree diff.
    tui.send_str("j"); // tracked.txt
    tui.tab();
    tui.wait_for("changed line"); // the added line in `git diff`

    // Move to the untracked file: the preview shows its contents.
    tui.send_str("j"); // untracked.txt
    tui.wait_for("brand new");

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-07: `d` confirms, then discards — an untracked file is deleted from disk.
#[test]
fn stat_07_discard_deletes_untracked() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("untracked.txt");
    assert!(repo.exists("untracked.txt"));

    // Move to untracked.txt (cursor 2) and open the discard confirmation.
    tui.send_str("jj");
    tui.send_str("d");
    tui.wait_for("Delete untracked.txt?");

    // Confirm.
    tui.send_str("y");
    tui.wait_for("Discarded");
    tui.wait_until_gone("untracked.txt");

    assert!(
        !repo.exists("untracked.txt"),
        "discarding an untracked file should delete it"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-07 / STAT-08: opening the confirmation and cancelling leaves the file untouched.
#[test]
fn stat_08_discard_cancel_keeps_file() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("untracked.txt");

    tui.send_str("jj");
    tui.send_str("d");
    tui.wait_for("Delete untracked.txt?");
    tui.esc(); // cancel
    tui.wait_until_gone("Delete untracked.txt?");

    assert!(
        repo.exists("untracked.txt"),
        "cancelling discard must not touch the file"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-14: a clean working tree renders the empty state, not a panic.
#[test]
fn stat_14_clean_tree_empty_state() {
    // `with_graph` leaves a clean working tree (everything committed).
    let repo = TempRepo::with_graph();
    let mut tui = spawn(repo.path());

    tui.wait_for("working tree clean");

    tui.send_str("q");
    tui.wait_exit();
}

// STAT-15: running outside a git repo exits non-zero with a clear message (no TUI).
#[test]
fn stat_15_not_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("gitt").unwrap();
    cmd.arg("status")
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", dir.path());
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("not a git repository"));
}
