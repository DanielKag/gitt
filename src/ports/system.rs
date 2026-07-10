//! Real implementations of the system ports (clock, clipboard, browser, PR opener, env).
//!
//! Clipboard/Browser/PrOpener honor `GITT_TEST_SINK_DIR`: when set, they append their payload to a
//! file in that directory instead of performing the real OS action. This is the seam that lets e2e
//! tests assert on exactly what the real code path produced, without launching a browser or writing
//! to the real system clipboard.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Browser, Clipboard, Clock, Env, GitError, PrOpener};

/// System clock, overridable by `GITT_NOW=<unix>` for deterministic tests.
pub struct RealClock;

impl Clock for RealClock {
    fn now_unix(&self) -> i64 {
        if let Some(v) = std::env::var("GITT_NOW").ok().and_then(|s| s.parse().ok()) {
            return v;
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Process environment.
pub struct RealEnv;

impl Env for RealEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn has_delta(&self) -> bool {
        if std::env::var_os("GITT_NO_DELTA").is_some() {
            return false;
        }
        which("delta")
    }
}

/// Directory to redirect side-effects into during tests, if set.
fn sink_dir() -> Option<PathBuf> {
    std::env::var_os("GITT_TEST_SINK_DIR").map(PathBuf::from)
}

/// Append `line` to `<sink>/<file>` (creating it). Returns true if a sink was configured.
fn write_sink(file: &str, line: &str) -> Option<Result<(), GitError>> {
    let dir = sink_dir()?;
    let result = (|| {
        std::fs::create_dir_all(&dir).map_err(|e| GitError::Io(e.to_string()))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(file))
            .map_err(|e| GitError::Io(e.to_string()))?;
        writeln!(f, "{line}").map_err(|e| GitError::Io(e.to_string()))
    })();
    Some(result)
}

pub struct RealClipboard;

impl Clipboard for RealClipboard {
    fn copy(&self, text: &str) -> Result<(), GitError> {
        if let Some(result) = write_sink("clipboard.txt", text) {
            return result;
        }
        pbcopy(text)
    }
}

pub struct RealBrowser;

impl Browser for RealBrowser {
    fn open(&self, url: &str) -> Result<(), GitError> {
        if let Some(result) = write_sink("browser.txt", url) {
            return result;
        }
        open_os(url)
    }
}

pub struct RealPr;

impl PrOpener for RealPr {
    fn open_pr(&self, hash: &str) -> Result<(), GitError> {
        if let Some(result) = write_sink("pr.txt", hash) {
            return result;
        }
        // Best-effort: ask gh to open the PR associated with this commit in the browser.
        run("gh", &["pr", "view", hash, "--web"])
    }
}

// --- OS helpers ----------------------------------------------------------------------------------

fn pbcopy(text: &str) -> Result<(), GitError> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Io(format!("pbcopy: {e}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| GitError::Io("pbcopy: no stdin".into()))?
        .write_all(text.as_bytes())
        .map_err(|e| GitError::Io(e.to_string()))?;
    let status = child.wait().map_err(|e| GitError::Io(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(GitError::Io("pbcopy failed".into()))
    }
}

fn open_os(url: &str) -> Result<(), GitError> {
    run("open", &[url])
}

fn run(cmd: &str, args: &[&str]) -> Result<(), GitError> {
    let status = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| GitError::Io(format!("{cmd}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(GitError::Io(format!("{cmd} exited with {status}")))
    }
}

/// True if `bin` is found on `PATH`.
fn which(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|p| p.join(bin).is_file())
}
