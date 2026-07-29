//! End-to-end tests for the `~/.gitt` config file: spawn the real binary with a real config file on
//! disk and assert its settings actually reach the running tool. See specs/config.md.

mod common;

use std::path::PathBuf;

use common::fixture::TempRepo;
use common::tui_tester::Tui;

/// Write `body` to a `.gitt` file in a fresh tempdir and return (dir, path). The `TempDir` must be
/// held by the caller for as long as the spawned binary runs.
fn config_file(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitt");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

// CFG-08 / CFG-09: a model set only in the config file reaches the real summarizer.
#[test]
fn cfg_09_config_ollama_model_reaches_the_summarizer() {
    let repo = TempRepo::with_graph();
    let (_dir, config) = config_file("# my gitt config\nollama_model = e2e-config-model\n");

    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[
            ("GITT_CONFIG", config.to_str().unwrap()),
            ("GITT_FAKE_SUMMARY", "A summary from the configured model."),
        ],
    );
    tui.wait_for("local only change");
    tui.send_str("@");
    tui.wait_for("A summary from the configured model.");

    assert_eq!(
        tui.sink("ollama_model.txt"),
        "e2e-config-model",
        "the ~/.gitt ollama_model must reach the real summarizer"
    );

    tui.send_str("q");
    tui.wait_exit();
}

// CFG-07: an env var still beats the config file, end to end.
#[test]
fn cfg_07_env_model_beats_the_config_file() {
    let repo = TempRepo::with_graph();
    let (_dir, config) = config_file("ollama_model = from-the-file\n");

    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[
            ("GITT_CONFIG", config.to_str().unwrap()),
            ("GITT_OLLAMA_MODEL", "from-the-env"),
            ("GITT_FAKE_SUMMARY", "Summarized."),
        ],
    );
    tui.wait_for("local only change");
    tui.send_str("@");
    tui.wait_for("Summarized.");

    assert_eq!(tui.sink("ollama_model.txt"), "from-the-env");

    tui.send_str("q");
    tui.wait_exit();
}

// CFG-02 / CFG-09: a config full of junk — including a `diff_tool` naming a tool that doesn't exist —
// never stops `gitt` from opening, and diffs still render (as plain text).
#[test]
fn cfg_09_garbage_config_still_opens_and_previews() {
    let repo = TempRepo::with_graph();
    let (_dir, config) = config_file(
        "!!! not a setting\n\
         no equals sign here\n\
         unknown_key = whatever\n\
         diff_tool = totally-not-a-differ\n",
    );

    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[
            ("GITT_CONFIG", config.to_str().unwrap()),
            // Blank counts as unset (CFG-04), so the file's `diff_tool` is what resolves here — the
            // harness otherwise pins this to "none".
            ("GITT_DIFF_TOOL", ""),
        ],
    );
    tui.wait_for("local only change");

    tui.tab();
    tui.wait_for("diff");
    tui.send_str("f");
    tui.wait_for("file.txt"); // plain `git show` output, not an error

    tui.send_str("q");
    tui.wait_exit();
}

// CFG-08: a `GITT_CONFIG` path that doesn't exist is treated as "no config" — not an error.
#[test]
fn cfg_08_missing_config_path_is_not_an_error() {
    let repo = TempRepo::with_graph();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope").join(".gitt");

    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[("GITT_CONFIG", missing.to_str().unwrap())],
    );
    tui.wait_for("local only change");
    tui.send_str("q");
    tui.wait_exit();
}

// CFG-08: a directory where the config file should be is unreadable-as-a-file, and still not an error.
#[test]
fn cfg_08_directory_at_config_path_is_not_an_error() {
    let repo = TempRepo::with_graph();
    let dir = tempfile::tempdir().unwrap();

    let mut tui = Tui::spawn_cmd_env(
        repo.path(),
        "log",
        &[("GITT_CONFIG", dir.path().to_str().unwrap())],
    );
    tui.wait_for("local only change");
    tui.send_str("q");
    tui.wait_exit();
}
