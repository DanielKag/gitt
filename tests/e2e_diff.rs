//! End-to-end tests for `gitt diff`: spawn the real binary against a real repo over a PTY, drive
//! keystrokes, and assert on what it renders and the real git side effects it produces. Each test
//! name carries the spec criterion id(s) it covers (see specs/diff.md).

mod common;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

fn spawn(repo: &std::path::Path) -> Tui {
    Tui::spawn_cmd(repo, "diff")
}

// DIFF-01 / DIFF-06 / DIFF-12: the default Unstaged scope lists the modified file, the diff pane
// shows its diff, and `q` quits cleanly.
#[test]
fn diff_01_06_12_default_unstaged_and_quit() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());

    // Unstaged = `git diff`: only the modified tracked file (staged/untracked files are excluded).
    tui.wait_for("tracked.txt");
    // The diff pane (open by default) shows that file's diff.
    tui.wait_for("changed line");

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-06: Tab toggles the diff pane off (its content disappears) and back on.
#[test]
fn diff_06_tab_toggles_pane() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("changed line");

    tui.tab();
    tui.wait_until_gone("changed line");

    tui.tab();
    tui.wait_for("changed line");

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-04 / DIFF-05: →/← cycle the scopes, and each maps to the right git diff:
//   Unstaged = `git diff`      → tracked.txt only
//   Staged   = `git diff --staged` → newstaged.txt only
//   Working  = `git diff HEAD` → both
#[test]
fn diff_04_05_scopes_map_to_correct_diffs() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());

    // Unstaged: the modified file is present, the staged-new file is not.
    tui.wait_for("tracked.txt");
    assert!(!tui.screen().contains("newstaged.txt"));

    // → Staged: the staged-new file appears, the unstaged modification is gone.
    tui.right();
    tui.wait_for("newstaged.txt");
    tui.wait_until_gone("tracked.txt");

    // → Working (everything uncommitted vs HEAD): both files are present.
    tui.right();
    tui.wait_for("tracked.txt");
    tui.wait_for("newstaged.txt");

    // ← back to Staged (cached): only the staged-new file again.
    tui.left();
    tui.wait_for("newstaged.txt");
    tui.wait_until_gone("tracked.txt");

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-05: the vs-main scope shows the GitHub-PR "Files changed" diff of a feature branch.
#[test]
fn diff_05_vs_main_shows_branch_diff() {
    let repo = TempRepo::with_feature_branch();
    let mut tui = spawn(repo.path());

    // The working tree is clean, so the default Unstaged scope is empty.
    tui.wait_for("no unstaged changes");
    // The header offers the vs-main tab.
    tui.wait_for("vs main");

    // → → → to the Branch scope: it shows the feature commit's file and its content.
    tui.right();
    tui.right();
    tui.right();
    tui.wait_for("feature.txt");
    tui.wait_for("feature work");

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-11: a scope with no changes renders a clear empty state, not a panic.
#[test]
fn diff_11_empty_scope_state() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("tracked.txt");

    // with_dirty is on `main` == HEAD, so `git diff main...HEAD` is empty.
    tui.right(); // Staged
    tui.right(); // Working
    tui.right(); // vs main
    tui.wait_for("no changes vs main");

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-08 / DIFF-09: Enter opens the action menu; Copy diff copies the file's diff, Copy path the
// path — both landing in the captured clipboard sink.
#[test]
fn diff_08_09_menu_copy_actions() {
    let repo = TempRepo::with_dirty();
    let mut tui = spawn(repo.path());
    tui.wait_for("changed line");

    // Copy diff (second menu item). The completion label appears only after the worker has loaded
    // the diff and written the clipboard sink, so the sink read below is race-free.
    tui.enter(); // open menu
    tui.wait_for("Copy diff");
    tui.send_str("j"); // move to Copy diff
    tui.enter();
    tui.wait_for("Copied diff");
    assert!(
        tui.sink("clipboard.txt").contains("changed line"),
        "Copy diff should place the file's diff on the clipboard, got: {:?}",
        tui.sink("clipboard.txt")
    );

    // Copy path (first menu item).
    tui.enter();
    tui.wait_for("Copy path");
    tui.enter();
    tui.wait_for("Copied to clipboard");
    assert!(
        tui.sink("clipboard.txt").contains("tracked.txt"),
        "Copy path should place the path on the clipboard, got: {:?}",
        tui.sink("clipboard.txt")
    );

    tui.send_str("q");
    tui.wait_exit();
}

// DIFF-13: running outside a git repo exits non-zero with a clear message (no TUI).
#[test]
fn diff_13_not_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("gitt").unwrap();
    cmd.arg("diff")
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", dir.path());
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("not a git repository"));
}
