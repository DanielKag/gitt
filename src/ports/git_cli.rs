//! `RealGit`: the `GitRepo` port backed by shelling out to the `git` binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{ColorMode, GitError, GitRepo};
use crate::domain::{
    Commit, DiffFile, DiffKind, DiffScope, StatusEntry, View, main_branch::resolve_main_branch,
};
use crate::parse::diff::parse_diff_name_status;
use crate::parse::log::{PRETTY_FORMAT, parse_log};
use crate::parse::remote::parse_remote_show;
use crate::parse::status::{STATUS_ARGS, parse_status};

/// A git repository at `dir`, with the main branch and "now" resolved once at construction.
pub struct RealGit {
    dir: PathBuf,
    main_branch: String,
    now: i64,
}

impl RealGit {
    pub fn new(dir: PathBuf, main_branch: String, now: i64) -> Self {
        RealGit {
            dir,
            main_branch,
            now,
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        run_git(&self.dir, args)
    }

    /// The base revision the `Branch` scope diffs against: `origin/<main>` when that ref exists
    /// (matching how GitHub compares a PR to the remote base), otherwise the local `<main>`.
    fn branch_base(&self) -> String {
        let origin = format!("origin/{}", self.main_branch);
        if self.ref_exists(&origin) {
            origin
        } else {
            self.main_branch.clone()
        }
    }

    /// True if `rev` resolves to an object (used to pick the `Branch` base ref).
    fn ref_exists(&self, rev: &str) -> bool {
        self.run(&["rev-parse", "--verify", "--quiet", rev])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// The revision argument(s) that select a scope's diff, e.g. `["--staged"]`, `["HEAD"]`, or the
    /// three-dot `["<base>...HEAD"]`. The unstaged scope needs no revision argument.
    fn scope_revs(&self, scope: DiffScope) -> Vec<String> {
        match scope {
            DiffScope::Unstaged => vec![],
            DiffScope::Staged => vec!["--staged".to_string()],
            DiffScope::Working => vec!["HEAD".to_string()],
            DiffScope::Branch => vec![format!("{}...HEAD", self.branch_base())],
        }
    }
}

impl GitRepo for RealGit {
    fn log(&self, view: View, limit: usize) -> Result<Vec<Commit>, GitError> {
        let target = match view {
            View::LocalHead => "HEAD".to_string(),
            View::OriginMain => format!("origin/{}", self.main_branch),
        };
        let limit_arg = format!("-n{limit}");
        let format_arg = format!("--pretty=format:{PRETTY_FORMAT}");
        let raw = self.run(&[
            "log",
            &target,
            &limit_arg,
            "--no-color",
            "--decorate=short",
            &format_arg,
        ])?;
        Ok(parse_log(&raw, self.now))
    }

    fn show(&self, hash: &str, color: ColorMode) -> Result<String, GitError> {
        let color_arg = match color {
            ColorMode::Always => "--color=always",
            ColorMode::Never => "--no-color",
        };
        self.run(&["show", color_arg, "--stat", "--patch", hash])
    }

    fn fetch(&self) -> Result<(), GitError> {
        self.run(&["fetch", "--quiet"]).map(|_| ())
    }

    fn checkout(&self, hash: &str) -> Result<(), GitError> {
        self.run(&["checkout", "--quiet", hash]).map(|_| ())
    }

    fn status(&self) -> Result<Vec<StatusEntry>, GitError> {
        let raw = self.run(STATUS_ARGS)?;
        Ok(parse_status(&raw))
    }

    fn file_diff(&self, path: &str, kind: DiffKind) -> Result<String, GitError> {
        match kind {
            // Untracked files have no tracked version to diff against; show their contents.
            DiffKind::Untracked => std::fs::read_to_string(self.dir.join(path))
                .map_err(|e| GitError::Io(format!("{path}: {e}"))),
            DiffKind::Worktree => self.run(&["diff", "--no-color", "--", path]),
            DiffKind::Staged => self.run(&["diff", "--no-color", "--staged", "--", path]),
        }
    }

    fn stage(&self, path: &str) -> Result<(), GitError> {
        self.run(&["add", "--", path]).map(|_| ())
    }

    fn unstage(&self, path: &str) -> Result<(), GitError> {
        self.run(&["restore", "--staged", "--", path]).map(|_| ())
    }

    fn discard(&self, path: &str, untracked: bool) -> Result<(), GitError> {
        if untracked {
            std::fs::remove_file(self.dir.join(path))
                .map_err(|e| GitError::Io(format!("{path}: {e}")))
        } else {
            self.run(&["restore", "--", path]).map(|_| ())
        }
    }

    fn diff_files(&self, scope: DiffScope) -> Result<Vec<DiffFile>, GitError> {
        let mut args: Vec<&str> = vec!["diff", "--name-status", "-z", "--no-color"];
        let revs = self.scope_revs(scope);
        args.extend(revs.iter().map(String::as_str));
        let raw = self.run(&args)?;
        Ok(parse_diff_name_status(&raw))
    }

    fn diff_scope_file(&self, scope: DiffScope, path: &str) -> Result<String, GitError> {
        let mut args: Vec<&str> = vec!["diff", "--no-color"];
        let revs = self.scope_revs(scope);
        args.extend(revs.iter().map(String::as_str));
        args.push("--");
        args.push(path);
        self.run(&args)
    }
}

/// Run `git <args>` in `dir`, returning stdout on success.
fn run_git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| GitError::Io(e.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(GitError::Exit {
            cmd: format!("git {}", args.join(" ")),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// True if `dir` is inside a git work tree.
pub fn is_git_repo(dir: &Path) -> bool {
    run_git(dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Current branch name (or a short hash label when detached).
pub fn current_branch(dir: &Path) -> String {
    match run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(s) if s.trim() != "HEAD" => s.trim().to_string(),
        _ => run_git(dir, &["rev-parse", "--short", "HEAD"])
            .map(|s| format!("({})", s.trim()))
            .unwrap_or_else(|_| "HEAD".to_string()),
    }
}

/// Fetch URL of the `origin` remote, if any.
pub fn remote_url(dir: &Path) -> Option<String> {
    run_git(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve the repository's main branch: `origin/HEAD` symref -> cache file -> `git remote show`.
/// Only the last step touches the network, so repeat runs are instant.
pub fn detect_main_branch(dir: &Path) -> String {
    let symref = run_git(
        dir,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|s| s.trim().to_string());
    let cache = cache_lookup(dir);

    if let Some(branch) = resolve_main_branch(symref.as_deref(), cache.as_deref(), None) {
        return branch;
    }

    // Last resort: ask the remote (network), then cache.
    let remote_show = run_git(dir, &["remote", "show", "origin"])
        .ok()
        .and_then(|s| parse_remote_show(&s));
    if let Some(branch) = resolve_main_branch(None, None, remote_show.as_deref()) {
        cache_store(dir, &branch);
        return branch;
    }

    "main".to_string()
}

fn cache_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("gitt").join("main_branches"))
}

fn repo_key(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

fn cache_lookup(dir: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(cache_file()?).ok()?;
    let key = repo_key(dir);
    for line in contents.lines() {
        if let Some((path, branch)) = line.split_once('\t')
            && path == key
            && !branch.is_empty()
        {
            return Some(branch.to_string());
        }
    }
    None
}

fn cache_store(dir: &Path, branch: &str) {
    let Some(path) = cache_file() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let key = repo_key(dir);
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with(&format!("{key}\t")))
        .map(str::to_string)
        .collect();
    lines.push(format!("{key}\t{branch}"));
    let _ = std::fs::write(&path, lines.join("\n"));
}
