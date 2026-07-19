//! End-to-end tests for committing from `gitt status`. On Enter-to-commit, gitt tears down the TUI
//! and runs `git commit` in the restored terminal (echoing the command first) so pre-commit hooks
//! stream live and a failure is re-runnable — so these drive the real binary over a PTY, then assert
//! the real commit landed. Each test name carries the spec criterion id(s) (see specs/commit.md).

mod common;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

fn spawn(repo: &std::path::Path) -> Tui {
    Tui::spawn_cmd(repo, "status")
}

// CMT-01 / CMT-03 / CMT-09: `c` opens the editor; on Enter gitt echoes `git commit …`, exits, and the
// commit runs in the shell — landing a real commit with the typed subject.
#[test]
fn cmt_01_03_09_c_commits_via_shell_handoff() {
    let repo = TempRepo::with_dirty(); // newstaged.txt is staged (A )
    assert_eq!(repo.commit_count(), 1);
    let mut tui = spawn(repo.path());
    tui.wait_for("newstaged.txt");

    tui.send_str("c");
    tui.wait_for("Commit");
    tui.wait_for("@ suggest"); // hint bar shows suggest shortcut when empty
    tui.send_str("add new file");
    tui.wait_for("add new file");
    tui.enter();

    // gitt hands off to the shell: the command is echoed, then run, then gitt exits.
    tui.wait_for("git commit -m 'add new file'");
    tui.wait_exit();

    assert_eq!(repo.commit_count(), 2, "a new commit landed");
    assert_eq!(repo.head_subject(), "add new file");
    assert!(
        repo.is_tracked_at_head("newstaged.txt"),
        "the staged file is now committed"
    );
}

// CMT-05: `a` opens the amend editor prefilled with HEAD's message; Enter hands off to
// `git commit --amend`, rewriting the last commit in place (count unchanged).
#[test]
fn cmt_05_a_amends_via_shell_handoff() {
    let repo = TempRepo::with_dirty(); // base commit; newstaged.txt staged
    assert_eq!(repo.commit_count(), 1);
    assert!(!repo.is_tracked_at_head("newstaged.txt"));
    let mut tui = spawn(repo.path());
    tui.wait_for("newstaged.txt");

    tui.send_str("a");
    tui.wait_for("Amend commit");
    tui.wait_for("base"); // HEAD's message, prefilled asynchronously
    tui.enter();

    tui.wait_for("git commit --amend -m 'base'");
    tui.wait_exit();

    assert_eq!(
        repo.commit_count(),
        1,
        "amend rewrites in place, no new commit"
    );
    assert_eq!(repo.head_subject(), "base", "the message is preserved");
    assert!(
        repo.is_tracked_at_head("newstaged.txt"),
        "the staged change was folded into HEAD"
    );
}

// CMT-06 / CMT-07: in a blank editor `@` streams an AI suggestion (from the staged diff + context)
// into the field, which then commits via the shell handoff. The recorded prompt proves the staged
// diff drove it.
#[test]
fn cmt_06_07_s_suggests_message_from_staged_diff() {
    let repo = TempRepo::with_dirty();
    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "status",
        &[("GITT_FAKE_SUMMARY", "Add the staged content file")],
    );
    tui.wait_for("newstaged.txt");

    tui.send_str("c");
    tui.wait_for("Commit");
    tui.send_str("@"); // @ (empty buffer) triggers AI suggestion
    tui.wait_for("Add the staged content file");

    // The prompt the model saw carried the staged diff + the staged file path (CMT-07).
    let prompt = tui.sink("summary_prompt.txt");
    assert!(
        prompt.contains("Staged diff:"),
        "prompt has the staged diff"
    );
    assert!(
        prompt.contains("newstaged.txt"),
        "prompt lists the staged file"
    );
    assert!(
        prompt.contains("staged content"),
        "prompt carries the staged diff body"
    );

    // The suggestion commits like any typed message (via the shell handoff).
    tui.enter();
    tui.wait_for("git commit -m 'Add the staged content file'");
    tui.wait_exit();
    assert_eq!(repo.head_subject(), "Add the staged content file");
}
