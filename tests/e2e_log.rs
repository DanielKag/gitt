//! End-to-end tests: spawn the real `gitt` binary against a real throwaway git repo over a PTY and
//! assert on what it renders and the real side effects it produces. Each test name carries the
//! spec criterion id(s) it covers (see specs/log.md).

mod common;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

// LOG-01 / LOG-18: renders the commit log, then quits cleanly on `q`.
#[test]
fn log_01_18_renders_log_and_quits() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());

    tui.wait_for("local only change");
    // Older commits are present too.
    tui.wait_for("refactor parser");
    tui.wait_for("fix flaky test");

    tui.send_str("q");
    tui.wait_exit();
}

// LOG-04 / LOG-05: '/' enters search and typing narrows the visible commits.
#[test]
fn log_04_05_search_narrows_list() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");

    tui.send_str("/");
    tui.send_str("flaky");
    tui.wait_for("fix flaky test");
    // Non-matching commits are filtered out.
    tui.wait_until_gone("refactor parser");
    tui.wait_until_gone("local only change");

    tui.enter(); // leave search (keeps filter), back to List
    tui.send_str("q");
    tui.wait_exit();
}

// LOG-07: →/← toggle between the local HEAD view and the origin/main view.
#[test]
fn log_07_view_toggle_head_vs_origin() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());

    // Local HEAD has the extra commit.
    tui.wait_for("local only change");

    // Switch to origin/main — which does not have it.
    tui.right();
    tui.wait_until_gone("local only change");
    tui.wait_for("refactor parser");

    // Back to HEAD — it reappears.
    tui.left();
    tui.wait_for("local only change");

    tui.send_str("q");
    tui.wait_exit();
}

// LOG-08: Tab toggles the diff preview pane.
#[test]
fn log_08_preview_toggle() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");

    tui.tab();
    tui.wait_for("diff"); // the preview block title
    // git show output for the top commit includes the touched file.
    tui.wait_for("file.txt");

    tui.tab();
    tui.wait_until_gone("diff");

    tui.send_str("q");
    tui.wait_exit();
}

// LOG-11 / LOG-12: Enter opens the action menu; "Copy SHA" copies the full 40-char hash.
#[test]
fn log_11_12_menu_copy_sha() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");

    tui.enter(); // open menu (top commit = "local only change")
    tui.wait_for("Copy SHA");
    tui.send_str("jj"); // move to "Copy SHA" (index 2)
    tui.enter();
    // The optimistic status is replaced by the ActionFinished label once the copy completes.
    tui.wait_for("Copied to clipboard");

    let expected = repo.sha("local only change");
    assert_eq!(tui.sink("clipboard.txt"), expected);
    assert_eq!(expected.len(), 40);

    tui.send_str("q");
    tui.wait_exit();
}

// LOG-13: "Checkout" actually moves HEAD to the selected commit (real git, real binary).
#[test]
fn log_13_checkout_moves_head() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");

    // Select "fix flaky test" deterministically via search.
    tui.send_str("/");
    tui.send_str("flaky");
    tui.wait_for("fix flaky test");
    tui.enter(); // leave search (keeps filter)

    tui.enter(); // open menu
    tui.wait_for("Checkout");
    tui.send_str("jjj"); // move to "Checkout" (index 3)
    tui.enter();
    tui.wait_for("Checked out");

    tui.send_str("q");
    tui.wait_exit();

    assert_eq!(repo.head(), repo.sha("fix flaky test"));
}

// LOG-19: running outside a git repo exits non-zero with a clear message (no TUI).
#[test]
fn log_19_not_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("gitt").unwrap();
    cmd.arg("log")
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", dir.path());
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("not a git repository"));
}
