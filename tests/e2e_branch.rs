//! End-to-end tests for `gitt branch`: spawn the real binary against a real repo over a PTY, drive
//! keystrokes, and assert on what it renders and the real git side effects it produces. Each test
//! name carries the spec criterion id(s) it covers (see specs/branch.md).

mod common;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

fn spawn(repo: &std::path::Path) -> Tui {
    Tui::spawn_cmd(repo, "branch")
}

// BR-01 / BR-15: lists the local branches (current one marked) and quits cleanly on `q`.
#[test]
fn br_01_15_lists_branches_and_quits() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());

    tui.wait_for("wip-parser");
    tui.wait_for("bugfix");
    // The current branch (main) is marked with `*` (a blank AI-summary column sits between them).
    tui.wait_for("*   main");

    tui.send_str("q");
    tui.wait_exit();
}

// BR-03: '/' enters search and typing narrows the list to the matching branch.
#[test]
fn br_03_search_narrows_list() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.wait_until_gone("bugfix");

    tui.enter(); // leave search (keeps filter)
    tui.send_str("q");
    tui.wait_exit();
}

// BR-05 / BR-06: Enter opens the menu; "Checkout" moves HEAD onto the selected branch and, on
// success, gitt quits immediately (like a native `git checkout`), leaving a one-line exit report.
#[test]
fn br_05_06_checkout_moves_head() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    // Select wip-parser deterministically via search.
    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search

    tui.enter(); // open menu
    tui.wait_for("Checkout");
    tui.enter(); // Checkout is the first item

    // A successful checkout exits gitt on its own — no `q` needed — leaving the exit report behind.
    tui.wait_for("Checked out wip-parser");
    tui.wait_exit();

    assert_eq!(repo.current_branch(), "wip-parser");
    assert_eq!(repo.head(), repo.sha("refactor parser"));
}

// BR-06: a failed checkout keeps gitt open and reports only git's own message (no invoked command /
// exit code). We provoke it with an untracked file that `wip-parser` would overwrite.
#[test]
fn br_06_checkout_failure_reports_concise_error() {
    let repo = TempRepo::with_branches();
    // `wip-parser` tracks `parser.txt`; an untracked copy on `main` makes the checkout refuse.
    std::fs::write(repo.path().join("parser.txt"), "conflict\n").unwrap();

    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");
    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search
    tui.enter(); // open menu
    tui.wait_for("Checkout");
    tui.enter(); // Checkout

    // The failure surfaces on the status line; gitt stays open (HEAD unmoved).
    tui.wait_for("Checkout failed");
    let screen = tui.screen();
    assert!(
        !screen.contains("checkout --quiet") && !screen.contains("failed ("),
        "the invoked command and exit code are dropped — only git's message shows:\n{screen}"
    );

    tui.send_str("q");
    tui.wait_exit();
    assert_eq!(
        repo.current_branch(),
        "main",
        "a failed checkout leaves HEAD put"
    );
}

// BR-08: "Copy name" places the selected branch name on the clipboard.
#[test]
fn br_08_copy_name() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search

    tui.enter(); // open menu
    tui.wait_for("Copy name");
    tui.send_str("jj"); // Checkout → Open PR → Copy name
    tui.enter();
    tui.wait_for("Copied to clipboard");

    assert_eq!(tui.sink("clipboard.txt"), "wip-parser");

    tui.send_str("q");
    tui.wait_exit();
}

// BR-07: "Open PR" hands the branch name to the PR opener (captured in the sink).
#[test]
fn br_07_open_pr_uses_branch_name() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search

    tui.enter(); // open menu
    tui.wait_for("Open Pull Request");
    tui.send_str("j"); // Checkout → Open PR
    tui.enter();
    tui.wait_for("Opened PR");

    assert_eq!(tui.sink("pr.txt"), "wip-parser");

    tui.send_str("q");
    tui.wait_exit();
}

// BR-09: "d" opens the confirmation; "y" deletes the branch for real.
#[test]
fn br_09_delete_removes_branch() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("bugfix");
    assert!(repo.branch_exists("bugfix"));

    tui.send_str("/");
    tui.send_str("bugfix");
    tui.wait_for("bugfix");
    tui.enter(); // leave search, bugfix selected

    tui.send_str("d");
    tui.wait_for("Delete branch bugfix?");
    tui.send_str("y");
    tui.wait_for("Deleted branch");

    tui.send_str("q");
    tui.wait_exit();

    assert!(!repo.branch_exists("bugfix"), "the branch was deleted");
}

// BR-09: the current branch cannot be deleted — the status says so and no confirmation opens.
#[test]
fn br_09_cannot_delete_current() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("*   main");

    // main is the current branch and is now pinned first, so it's selected on load — delete it.
    tui.send_str("d");
    tui.wait_for("cannot delete the current branch");

    tui.send_str("q");
    tui.wait_exit();
    assert!(repo.branch_exists("main"), "main is untouched");
}

// BR-10: "n" creates a new branch off HEAD and switches to it.
#[test]
fn br_10_create_branch() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    tui.send_str("n");
    tui.wait_for("New branch");
    tui.send_str("feature-x");
    tui.enter();
    tui.wait_for("Created branch");
    // The new branch shows up in the reloaded list.
    tui.wait_for("feature-x");

    tui.send_str("q");
    tui.wait_exit();

    assert!(repo.branch_exists("feature-x"));
    assert_eq!(repo.current_branch(), "feature-x", "create switches to it");
}

// BR-11/12/13: `s` generates the branch summary via the fake summarizer; the panel shows it and the
// prompt carries the base branch, the branch's commit subjects, and its diff-vs-base.
#[test]
fn br_11_13_generate_summary_and_build_prompt() {
    let repo = TempRepo::with_branches();
    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "branch",
        &[("GITT_FAKE_SUMMARY", "This branch reworks the parser.")],
    );
    tui.wait_for("wip-parser");

    // Pin the selection to wip-parser, then summarize.
    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search

    tui.send_str("@");
    // "This branch " preamble is stripped and re-capitalized.
    tui.wait_for("Reworks the parser.");

    let prompt = tui.sink("summary_prompt.txt");
    assert!(prompt.contains("main"), "base branch in prompt:\n{prompt}");
    assert!(
        prompt.contains("refactor parser"),
        "commit subject in prompt:\n{prompt}"
    );
    assert!(
        prompt.contains("parser.txt"),
        "diff-vs-base context in prompt:\n{prompt}"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// BR-11: a generated branch summary is cached (keyed by branch tip) and reused by a later run — the
// second run uses a different fake yet shows the first run's cached text on selection.
#[test]
fn br_11_summary_cached_across_runs() {
    let repo = TempRepo::with_branches();
    let cache = tempfile::tempdir().unwrap();
    let cache_dir = cache.path().to_str().unwrap();

    {
        let mut tui = Tui::spawn_cmd_env(
            repo.path(),
            "branch",
            &[
                ("GITT_FAKE_SUMMARY", "First branch summary."),
                ("GITT_CACHE_DIR", cache_dir),
            ],
        );
        tui.wait_for("wip-parser");
        tui.send_str("/");
        tui.send_str("wip");
        tui.wait_for("wip-parser");
        tui.enter();
        tui.send_str("@");
        tui.wait_for("First branch summary.");
        tui.send_str("q");
        tui.wait_exit();
    }

    {
        let mut tui = Tui::spawn_cmd_env(
            repo.path(),
            "branch",
            &[
                ("GITT_FAKE_SUMMARY", "SECOND must not appear."),
                ("GITT_CACHE_DIR", cache_dir),
            ],
        );
        tui.send_str("/");
        tui.send_str("wip");
        tui.wait_for("wip-parser");
        tui.enter();
        // No `s`: the cached summary loads automatically on selection.
        tui.wait_for("First branch summary.");
        tui.send_str("q");
        tui.wait_exit();
    }
}

// BR-17: the PR-status column fills in from the background `gh` fetch (faked here for determinism),
// without blocking the branch list (which paints first).
#[test]
fn br_17_pr_status_column() {
    let repo = TempRepo::with_branches();
    let fake = r#"[
        {"headRefName":"wip-parser","state":"OPEN","isDraft":false},
        {"headRefName":"bugfix","state":"MERGED","isDraft":false}
    ]"#;
    let mut tui = Tui::spawn_cmd_env(repo.path(), "branch", &[("GITT_FAKE_PR_JSON", fake)]);

    // The list paints immediately…
    tui.wait_for("wip-parser");
    // …then the PR column fills in from the background fetch.
    tui.wait_for("open");
    tui.wait_for("merged");

    tui.send_str("q");
    tui.wait_exit();
}

// The inline window leaves a git-native footprint on exit: the branch-list chrome is erased and a
// one-line report of the last action persists (rather than a lingering 20-row blank block).
#[test]
fn clean_footprint_on_exit_with_report() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    // Do a (non-terminal) action so there's something to report, then quit. Checkout would exit on
    // its own, so use "Copy name", which leaves the screen open with a status to report.
    tui.send_str("/");
    tui.send_str("wip");
    tui.wait_for("wip-parser");
    tui.enter(); // leave search
    tui.enter(); // open menu
    tui.wait_for("Copy name");
    tui.send_str("jj"); // Checkout → Open PR → Copy name
    tui.enter();
    tui.wait_for("Copied to clipboard");

    tui.send_str("q");
    tui.wait_exit();

    // After exit the UI chrome is gone and the action is reported on a persistent line.
    let screen = tui.screen();
    assert!(
        screen.contains("Copied to clipboard"),
        "exit reports what happened:\n{screen}"
    );
    assert!(
        !screen.contains("ai summary") && !screen.contains("/search"),
        "the branch-list chrome is erased on exit:\n{screen}"
    );
}

// Esc from the base list quits the TUI (the universal exit path), like `q`.
#[test]
fn esc_quits_from_list() {
    let repo = TempRepo::with_branches();
    let mut tui = spawn(repo.path());
    tui.wait_for("wip-parser");

    tui.esc();
    tui.wait_exit();
}

// BR-15: running outside a git repo exits non-zero with a clear message (no TUI).
#[test]
fn br_15_not_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("gitt").unwrap();
    cmd.arg("branch")
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("HOME", dir.path());
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("not a git repository"));
}
