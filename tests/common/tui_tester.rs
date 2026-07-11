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
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
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
        cmd.env("GITT_NOW", NOW.to_string());
        cmd.env("GITT_NO_DELTA", "1");
        cmd.env("GITT_TEST_SINK_DIR", sink.path());
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }

        let child = pair.slave.spawn_command(cmd).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();

        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        {
            let parser = parser.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => parser.lock().unwrap().process(&buf[..n]),
                    }
                }
            });
        }

        Tui {
            _pair: pair,
            writer,
            parser,
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
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
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
