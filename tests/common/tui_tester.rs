//! Drive the real compiled `gitt` binary in a PTY and assert on what it renders.
//!
//! Synchronization is by `wait_for` (poll the vt100 grid until text appears) — never fixed sleeps —
//! so the tests are deterministic. Side effects are captured via `GITT_TEST_SINK_DIR`.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, PtyPair, PtySize, native_pty_system};
use tempfile::TempDir;

use super::NOW;

const ROWS: u16 = 24;
const COLS: u16 = 80;
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Tui {
    _pair: PtyPair,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    /// Every byte gitt wrote to the PTY, in order — lets teardown-hygiene tests assert on the exact
    /// control sequences (e.g. an SGR reset after leaving the alternate screen), which the parsed
    /// grid can't show.
    raw: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn Child + Send + Sync>,
    _home: TempDir,
    sink: TempDir,
}

impl Tui {
    /// Spawn `gitt log` against `repo`, with an isolated HOME and a side-effect sink.
    pub fn spawn(repo: &Path) -> Tui {
        Self::spawn_cmd(repo, "log")
    }

    /// Spawn `gitt <subcommand>` against `repo`, with an isolated HOME and a side-effect sink.
    pub fn spawn_cmd(repo: &Path, subcommand: &str) -> Tui {
        Self::spawn_cmd_env(repo, subcommand, &[])
    }

    /// Spawn `gitt <subcommand>` with additional environment variables layered over the deterministic
    /// defaults (used for feature seams like `GITT_FAKE_SUMMARY` / `GITT_CACHE_DIR`).
    pub fn spawn_cmd_env(repo: &Path, subcommand: &str, extra: &[(&str, &str)]) -> Tui {
        let home = tempfile::tempdir().unwrap();
        let sink = tempfile::tempdir().unwrap();

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_gitt"));
        cmd.arg(subcommand);
        cmd.cwd(repo);
        // Deterministic environment (see CLAUDE.md "E2E determinism rules").
        cmd.env("TERM", "xterm-256color");
        cmd.env("NO_COLOR", "1");
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
        cmd.env("GIT_CONFIG_SYSTEM", "/dev/null");
        cmd.env("HOME", home.path());
        cmd.env("XDG_CONFIG_HOME", home.path());
        // Isolate the summary cache too, so tests never read/write the developer's real ~/.cache.
        cmd.env("XDG_CACHE_HOME", home.path());
        cmd.env("GITT_NOW", NOW.to_string());
        // Force plain diffs: the harness passes the real PATH, so without this a dev machine with
        // difftastic/delta/git-split-diffs installed would auto-detect it and colorize the pane,
        // making the rendered-grid assertions non-deterministic.
        cmd.env("GITT_DIFF_TOOL", "none");
        cmd.env("GITT_TEST_SINK_DIR", sink.path());
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }
        for (k, v) in extra {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().unwrap()));

        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let raw = Arc::new(Mutex::new(Vec::<u8>::new()));
        {
            let parser = parser.clone();
            let writer = writer.clone();
            let raw = raw.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let chunk = &buf[..n];
                            raw.lock().unwrap().extend_from_slice(chunk);
                            parser.lock().unwrap().process(chunk);
                            // Answer a Device Status Report (cursor position) query, `ESC[6n`, with
                            // the current cursor position — a real terminal always does. Inline
                            // viewports (e.g. `gitt branch`) block on this handshake at startup.
                            if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                                let (row, col) = parser.lock().unwrap().screen().cursor_position();
                                let resp = format!("\x1b[{};{}R", row + 1, col + 1);
                                let mut w = writer.lock().unwrap();
                                let _ = w.write_all(resp.as_bytes());
                                let _ = w.flush();
                            }
                        }
                    }
                }
            });
        }

        Tui {
            _pair: pair,
            writer,
            parser,
            raw,
            child,
            _home: home,
            sink,
        }
    }

    /// The current rendered screen as text.
    pub fn screen(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    /// Send raw bytes as keystrokes.
    pub fn send(&mut self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    }

    pub fn send_str(&mut self, s: &str) {
        self.send(s.as_bytes());
    }

    pub fn enter(&mut self) {
        self.send(b"\r");
    }
    pub fn tab(&mut self) {
        self.send(b"\t");
    }
    pub fn esc(&mut self) {
        self.send(b"\x1b");
    }
    pub fn right(&mut self) {
        self.send(b"\x1b[C");
    }
    pub fn left(&mut self) {
        self.send(b"\x1b[D");
    }

    /// Block until `needle` appears on screen, or panic (dumping the screen) after the timeout.
    pub fn wait_for(&self, needle: &str) {
        let start = Instant::now();
        loop {
            if self.screen().contains(needle) {
                return;
            }
            if start.elapsed() > TIMEOUT {
                panic!(
                    "timed out waiting for {needle:?}\n--- screen ---\n{}\n--------------",
                    self.screen()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Block until `needle` is NO LONGER on screen (e.g. after switching views).
    pub fn wait_until_gone(&self, needle: &str) {
        let start = Instant::now();
        loop {
            if !self.screen().contains(needle) {
                return;
            }
            if start.elapsed() > TIMEOUT {
                panic!(
                    "timed out waiting for {needle:?} to disappear\n--- screen ---\n{}\n--------------",
                    self.screen()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Read a captured side-effect sink file (e.g. "clipboard.txt"), trimmed.
    pub fn sink(&self, file: &str) -> String {
        std::fs::read_to_string(self.sink.path().join(file))
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// Assert gitt left the terminal pen clean on teardown. For fullscreen screens, checks for
    /// `\x1b[0m` (SGR reset) after the last `\x1b[?1049l` (leave alternate screen). For inline
    /// screens (no alternate screen), checks that a reset appears in the final bytes of output.
    pub fn assert_pen_reset_on_teardown(&self) {
        const LEAVE_ALT: &[u8] = b"\x1b[?1049l";
        const RESET: &[u8] = b"\x1b[0m";
        let start = Instant::now();
        loop {
            let raw = self.raw.lock().unwrap().clone();
            if let Some(pos) = raw.windows(LEAVE_ALT.len()).rposition(|w| w == LEAVE_ALT) {
                if raw[pos..].windows(RESET.len()).any(|w| w == RESET) {
                    return;
                }
            } else {
                // Inline viewport (no alternate screen): just check for a reset in the tail.
                let tail_start = raw.len().saturating_sub(128);
                if raw[tail_start..].windows(RESET.len()).any(|w| w == RESET) {
                    return;
                }
            }
            if start.elapsed() > TIMEOUT {
                let tail_start = raw.len().saturating_sub(64);
                panic!(
                    "no SGR reset (\\x1b[0m) on teardown — the terminal pen leaks into the shell \
                     prompt.\n--- raw teardown tail ---\n{:?}",
                    String::from_utf8_lossy(&raw[tail_start..])
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Wait for the process to exit; panic on timeout.
    pub fn wait_exit(&mut self) {
        let start = Instant::now();
        loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            if start.elapsed() > TIMEOUT {
                panic!("process did not exit");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
