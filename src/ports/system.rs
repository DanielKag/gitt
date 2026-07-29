//! Real implementations of the system ports (clock, clipboard, browser, PR opener, env).
//!
//! Clipboard/Browser/PrOpener honor `GITT_TEST_SINK_DIR`: when set, they append their payload to a
//! file in that directory instead of performing the real OS action. This is the seam that lets e2e
//! tests assert on exactly what the real code path produced, without launching a browser or writing
//! to the real system clipboard.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Browser, Clipboard, Clock, Env, GitError, PrOpener, Summarizer, SummaryCache};
use crate::domain::DiffTool;
use crate::domain::PrStatus;
use crate::domain::config::{Config, diff_tool_choice, ollama_model_choice, parse_config};
use crate::domain::summary::{ollama_generate_url, ollama_model, resolve_cache_dir};
use crate::parse::parse_pr_list;

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
}

/// Read the user's `~/.gitt` (or `$GITT_CONFIG`) and parse it. A missing or unreadable file — by far
/// the common case — is an empty [`Config`], never an error, so a bad config can't stop `gitt` from
/// opening (CFG-08). Parsing itself is pure; this is only the file read.
pub fn load_config() -> Config {
    let path = config_path();
    let text = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    parse_config(&text)
}

/// Where the config file lives: `$GITT_CONFIG` if set (also the e2e seam), else `$HOME/.gitt`.
fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GITT_CONFIG").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|home| PathBuf::from(home).join(".gitt"))
}

/// Resolve which diff renderer to use for previews. Precedence: `--diff-tool` flag → `GITT_DIFF_TOOL`
/// env var → `~/.gitt` `diff_tool` → auto-detect the first tool installed on `PATH` (native
/// single-binary tools first). A chosen-but-missing tool, or nothing installed, resolves to
/// [`DiffTool::None`] (plain text) — never an error. `GITT_DIFF_TOOL=none` forces plain (used by e2e
/// for determinism, since CI has none of the tools installed). See CFG-06.
pub fn resolve_diff_tool(explicit: Option<&str>, config: &Config) -> DiffTool {
    let choice = diff_tool_choice(explicit, std::env::var("GITT_DIFF_TOOL").ok(), config);
    match choice {
        Some(name) => {
            let tool = DiffTool::parse(&name);
            // A configured tool that isn't installed degrades to plain rather than showing errors.
            match tool.binary() {
                Some(bin) if !which(bin) => DiffTool::None,
                _ => tool,
            }
        }
        None => DiffTool::AUTODETECT_ORDER
            .into_iter()
            .find(|t| t.binary().is_some_and(which))
            .unwrap_or(DiffTool::None),
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
    fn open_pr(&self, target: &str) -> Result<(), GitError> {
        if let Some(result) = write_sink("pr.txt", target) {
            return result;
        }
        // `target` is a PR number (log, from the subject's `(#N)`), a branch name (branch screen),
        // or a commit hash fallback. Capture gh's stderr so a failure reports *why* (e.g. "no pull
        // requests found") on the status line, instead of a bare "gh exited with 1".
        let output = Command::new("gh")
            .args(["pr", "view", target, "--web"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| GitError::Io(format!("gh: {e}")))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(GitError::Io(if stderr.is_empty() {
                "gh could not open the PR".to_string()
            } else {
                stderr
            }))
        }
    }

    fn close_pr(&self, branch: &str) -> Result<(), GitError> {
        if let Some(result) = write_sink("pr_close.txt", branch) {
            return result;
        }
        let output = Command::new("gh")
            .args(["pr", "close", branch])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| GitError::Io(format!("gh: {e}")))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(GitError::Io(if stderr.is_empty() {
                "gh could not close the PR".to_string()
            } else {
                stderr
            }))
        }
    }

    fn statuses(&self) -> Result<HashMap<String, PrStatus>, GitError> {
        // Test seam: a canned `gh` JSON payload keeps the PR column deterministic without a network.
        if let Ok(fake) = std::env::var("GITT_FAKE_PR_JSON") {
            return Ok(parse_pr_list(&fake));
        }
        // One `gh` call covers every branch. It is scoped to the current user's PRs (`--author @me`):
        // in a busy monorepo an unscoped newest-N list is filled with unrelated org/bot PRs and never
        // reaches the branches you actually have checked out (which are your own work). Failure
        // (missing gh / non-GitHub repo) leaves the column blank rather than erroring the screen.
        let json = capture(
            "gh",
            &[
                "pr",
                "list",
                "--author",
                "@me",
                "--state",
                "all",
                "--limit",
                "300",
                "--json",
                "headRefName,state,isDraft",
            ],
        )?;
        Ok(parse_pr_list(&json))
    }
}

/// Generates commit summaries via Ollama's HTTP API (`POST /api/generate`, non-streaming), shelling
/// out to `curl` (consistent with the tool's other subprocess ports). The HTTP API returns clean
/// text — unlike `ollama run`, whose piped output is polluted with terminal cursor/erase control
/// codes from its live word-wrapping.
///
/// Honors two test seams: `GITT_FAKE_SUMMARY` returns a canned summary without touching ollama, and
/// (when `GITT_TEST_SINK_DIR` is set) the prompt it *would* have sent is recorded so e2e can assert
/// the context was built correctly. `GITT_FAKE_SUMMARY_ERROR` forces a deterministic failure.
pub struct RealSummarizer {
    /// The Ollama model to generate with, already resolved (env → `~/.gitt` → default) at startup by
    /// [`RealSummarizer::new`], so the hot path doesn't re-read the environment per summary.
    model: String,
}

impl RealSummarizer {
    /// Resolve the model once: `GITT_OLLAMA_MODEL` → `~/.gitt` `ollama_model` → built-in default
    /// (CFG-07).
    pub fn new(config: &Config) -> Self {
        let configured = ollama_model_choice(std::env::var("GITT_OLLAMA_MODEL").ok(), config);
        RealSummarizer {
            model: ollama_model(configured),
        }
    }
}

impl Summarizer for RealSummarizer {
    fn summarize(&self, prompt: &str, on_token: &mut dyn FnMut(&str)) -> Result<(), GitError> {
        // Test seam: record which model this run resolved to, so e2e can assert that a `~/.gitt`
        // `ollama_model` really reached the summarizer (CFG-09) even on the faked paths below.
        let _ = write_sink("ollama_model.txt", &self.model);
        if let Ok(err) = std::env::var("GITT_FAKE_SUMMARY_ERROR") {
            let _ = write_sink("summary_prompt.txt", prompt);
            return Err(GitError::Io(err));
        }
        if let Ok(fake) = std::env::var("GITT_FAKE_SUMMARY") {
            let _ = write_sink("summary_prompt.txt", prompt);
            on_token(&fake);
            return Ok(());
        }
        let url = ollama_generate_url(std::env::var("OLLAMA_HOST").ok().as_deref());
        ollama_stream(&url, &self.model, prompt, on_token)
    }
}

/// Stream a completion from Ollama's `/api/generate` (NDJSON, one JSON object per line), invoking
/// `on_token` for each `response` chunk. Uses `curl` (like the tool's other subprocess ports); the
/// HTTP API returns clean text, unlike `ollama run` whose piped output carries terminal control codes.
fn ollama_stream(
    url: &str,
    model: &str,
    prompt: &str,
    on_token: &mut dyn FnMut(&str),
) -> Result<(), GitError> {
    // serde_json handles escaping the prompt (newlines, quotes, unicode) safely. `num_predict` caps
    // the output length (a summary is short) so a runaway response can't decode forever; a low
    // `temperature` keeps summaries steady.
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
        "options": { "num_predict": 200, "temperature": 0.2 },
    })
    .to_string();

    let mut child = Command::new("curl")
        .args([
            "-sS", // silent, but still report transport errors on stderr
            "-N",  // no output buffering, so tokens arrive as they stream
            "-X",
            "POST",
            url,
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-", // read the JSON body from stdin (avoids arg-length limits on big diffs)
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Io(format!("curl: {e}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| GitError::Io("curl: no stdin".into()))?
        .write_all(body.as_bytes())
        .map_err(|e| GitError::Io(e.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitError::Io("curl: no stdout".into()))?;

    let mut api_error: Option<String> = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| GitError::Io(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        // Each line is an independent JSON object; skip any we can't parse rather than aborting.
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
            api_error = Some(err.to_string());
            break;
        }
        if let Some(tok) = obj.get("response").and_then(|v| v.as_str())
            && !tok.is_empty()
        {
            on_token(tok);
        }
        if obj.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
            break;
        }
    }

    let status = child.wait().map_err(|e| GitError::Io(e.to_string()))?;
    if let Some(err) = api_error {
        return Err(GitError::Io(format!("ollama: {err}")));
    }
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut se) = child.stderr.take() {
            let _ = se.read_to_string(&mut stderr);
        }
        return Err(GitError::Io(format!(
            "ollama request failed (is `ollama serve` running at {url}?): {}",
            stderr.trim()
        )));
    }
    Ok(())
}

/// Filesystem-backed summary cache: one file per commit SHA under the resolved cache directory.
pub struct RealSummaryCache;

impl SummaryCache for RealSummaryCache {
    fn get(&self, key: &str) -> Option<String> {
        let contents = std::fs::read_to_string(summary_cache_dir()?.join(key)).ok()?;
        let trimmed = contents.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn put(&self, key: &str, summary: &str) -> Result<(), GitError> {
        let dir = summary_cache_dir()
            .ok_or_else(|| GitError::Io("no cache directory (HOME unset)".into()))?;
        std::fs::create_dir_all(&dir).map_err(|e| GitError::Io(e.to_string()))?;
        std::fs::write(dir.join(key), summary).map_err(|e| GitError::Io(e.to_string()))
    }
}

/// The summary cache directory, resolved from the environment (pure logic in `domain::summary`).
fn summary_cache_dir() -> Option<PathBuf> {
    resolve_cache_dir(
        std::env::var("GITT_CACHE_DIR").ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
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

/// Run `cmd <args>` and return its stdout on success (used to capture `gh`'s JSON output).
fn capture(cmd: &str, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| GitError::Io(format!("{cmd}: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(GitError::Io(format!(
            "{cmd} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// True if `bin` is found on `PATH`.
fn which(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|p| p.join(bin).is_file())
}
