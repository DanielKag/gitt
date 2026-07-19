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

// LOG-21: the log loads progressively in small pages (GITT_LOG_PAGE=2 here), yet search still
// reaches the OLDEST commit — which only lands after several background pages stream in behind the
// instant first paint.
#[test]
fn log_21_progressive_load_search_reaches_full_history() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn_cmd_env(repo.path(), "log", &[("GITT_LOG_PAGE", "2")]);

    // First page paints immediately: the newest commit is visible.
    tui.wait_for("local only change");

    // "init project" is the oldest commit — reachable only if later pages loaded.
    tui.send_str("/");
    tui.send_str("init");
    tui.wait_for("init project");
    // The first-page commit is filtered out, confirming this is a real search hit.
    tui.wait_until_gone("local only change");

    tui.enter();
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
    tui.wait_for("diff"); // the preview block title (a pane below the list)
    // Expand the diff to 90% height so the git-show output has room in the 24-row test terminal.
    tui.send_str("f");
    // git show output for the top commit includes the touched file.
    tui.wait_for("file.txt");

    tui.send_str("f"); // restore the split
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

// SUM-01/03: the AI-summary panel is visible and shows the "press s" hint for the selected commit
// when nothing is cached yet.
#[test]
fn sum_01_03_summary_panel_shows_hint() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");
    tui.wait_for("ai summary");
    tui.wait_for("press @ for an AI summary");

    tui.send_str("q");
    tui.wait_exit();
}

// SUM-04/05/06: pressing `s` generates a summary (via the fake summarizer), the panel shows it (with
// the redundant "This commit" preamble stripped), and the prompt carried the subject and diff.
#[test]
fn sum_04_06_generate_shows_summary_and_builds_prompt() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[("GITT_FAKE_SUMMARY", "This commit adds a local-only change.")],
    );
    tui.wait_for("local only change");
    tui.wait_for("press @ for an AI summary");

    tui.send_str("@");
    // The panel shows the summary with the "This commit " preamble stripped and re-capitalized.
    tui.wait_for("Adds a local-only change.");

    // The context fed to the model: the subject and the commit's diff (touches file.txt).
    let prompt = tui.sink("summary_prompt.txt");
    assert!(
        prompt.contains("local only change"),
        "prompt subject:\n{prompt}"
    );
    assert!(
        prompt.contains("file.txt"),
        "prompt diff context:\n{prompt}"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// SUM-06: a generated summary is cached on disk (keyed by commit SHA) and reused by a later run
// without calling the model again. The second run uses a *different* fake, yet the panel shows the
// first run's cached text on selection — proving the cache hit.
#[test]
fn sum_06_summary_is_cached_across_runs() {
    let repo = TempRepo::with_graph();
    let cache = tempfile::tempdir().unwrap();
    let cache_dir = cache.path().to_str().unwrap();

    {
        let mut tui = Tui::spawn_cmd_env(
            repo.path(),
            "log",
            &[
                ("GITT_FAKE_SUMMARY", "First generated summary."),
                ("GITT_CACHE_DIR", cache_dir),
            ],
        );
        tui.wait_for("local only change");
        tui.send_str("@");
        tui.wait_for("First generated summary.");
        tui.send_str("q");
        tui.wait_exit();
    }

    {
        let mut tui = Tui::spawn_cmd_env(
            repo.path(),
            "log",
            &[
                ("GITT_FAKE_SUMMARY", "SECOND summary must not appear."),
                ("GITT_CACHE_DIR", cache_dir),
            ],
        );
        // No `s` pressed: the cached summary loads automatically on selection.
        tui.wait_for("local only change");
        tui.wait_for("First generated summary.");
        tui.send_str("q");
        tui.wait_exit();
    }
}

// SUM-08: a generation failure surfaces on the panel without crashing.
#[test]
fn sum_08_generation_failure_shows_on_panel() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[("GITT_FAKE_SUMMARY_ERROR", "model unavailable")],
    );
    tui.wait_for("local only change");

    tui.send_str("@");
    tui.wait_for("summary failed");

    tui.send_str("q");
    tui.wait_exit();
}

// LOG-05: search is exact substring-per-term, NOT a fuzzy subsequence. A subsequence of a commit's
// text that isn't a contiguous substring must not match; two real substrings are AND-ed together.
#[test]
fn log_05_exact_substring_search() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("refactor parser");

    tui.send_str("/");
    // "factorpar" is a subsequence of "refactor parser" (f-a-c-t-o-r-p-a-r) but not a substring —
    // exact search rejects it (fuzzy would have matched).
    tui.send_str("factorpar");
    tui.wait_until_gone("refactor parser");

    // Backspace it away, then type the two terms with a space: both are real substrings, so
    // "refactor parser" matches while "add parser" (no "factor") is excluded by the AND.
    for _ in 0.."factorpar".len() {
        tui.send(&[0x7f]); // Backspace
    }
    tui.send_str("factor par");
    tui.wait_for("refactor parser");
    tui.wait_until_gone("add parser");

    tui.enter();
    tui.send_str("q");
    tui.wait_exit();
}

// LOG-26: Tab toggles the diff preview from inside search mode, without leaving the search.
#[test]
fn log_26_tab_previews_in_search_mode() {
    let repo = TempRepo::with_graph();
    let mut tui = Tui::spawn(repo.path());
    tui.wait_for("local only change");

    tui.send_str("/"); // enter search mode
    tui.send_str("local"); // narrow to the top commit
    tui.wait_for("local only change");

    tui.tab(); // toggle the preview while still typing a search
    tui.wait_for("diff"); // preview pane title (below the list)
    // In search mode `f` can't expand (it types into the filter), so the diff pane stays at 50%;
    // assert on the git-show header line, which is at the very top of the pane and always visible.
    tui.wait_for("commit"); // git show output for the selected commit

    // Still in search mode: further keystrokes keep editing the filter (0 matches now).
    tui.send_str("zzz");
    tui.wait_for("(0 matches)");

    tui.enter();
    tui.send_str("q");
    tui.wait_exit();
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
