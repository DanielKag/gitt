//! A throwaway git repository with a known commit graph, built with real `git` and pinned dates so
//! commit SHAs and relative times are reproducible.
//!
//! Graph (oldest → newest), all on `main`:
//!   init project → add parser → fix flaky test → refactor parser  ── pushed to a bare `origin`
//!   → local only change                                            ── local HEAD only
//!
//! So the local HEAD view contains "local only change" but the `origin/main` view does not, which
//! makes the view-toggle behavior assertable. `origin/HEAD` is set so main-branch detection resolves
//! via the symref (no network).

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::{DAY, NOW};

pub struct TempRepo {
    work: TempDir,
    _origin: TempDir,
    shas: HashMap<String, String>,
}

impl TempRepo {
    pub fn path(&self) -> &Path {
        self.work.path()
    }

    /// Full SHA of the commit with the given subject.
    pub fn sha(&self, subject: &str) -> String {
        self.shas
            .get(subject)
            .unwrap_or_else(|| panic!("no commit with subject {subject:?}"))
            .clone()
    }

    /// `git rev-parse HEAD` in the repo (used to check checkout moved HEAD).
    pub fn head(&self) -> String {
        let out = git(self.work.path(), &["rev-parse", "HEAD"], NOW);
        out.trim().to_string()
    }

    /// The current branch name (`git rev-parse --abbrev-ref HEAD`).
    pub fn current_branch(&self) -> String {
        git(
            self.work.path(),
            &["rev-parse", "--abbrev-ref", "HEAD"],
            NOW,
        )
        .trim()
        .to_string()
    }

    /// The local branch names (`git branch --format=%(refname:short)`).
    pub fn branch_names(&self) -> Vec<String> {
        git(
            self.work.path(),
            &["branch", "--format=%(refname:short)"],
            NOW,
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    /// True if a local branch named `name` exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        self.branch_names().iter().any(|b| b == name)
    }

    /// True if `rel` exists in the working tree (used to check discard of untracked files).
    pub fn exists(&self, rel: &str) -> bool {
        self.work.path().join(rel).exists()
    }

    /// True if `rel` has changes staged in the index (`git diff --cached --name-only`).
    pub fn is_staged(&self, rel: &str) -> bool {
        let out = git(self.work.path(), &["diff", "--cached", "--name-only"], NOW);
        out.lines().any(|l| l == rel)
    }

    /// HEAD's commit subject (`git log -1 --pretty=%s`).
    pub fn head_subject(&self) -> String {
        git(self.work.path(), &["log", "-1", "--pretty=%s"], NOW)
            .trim()
            .to_string()
    }

    /// Number of commits reachable from HEAD (`git rev-list --count HEAD`).
    pub fn commit_count(&self) -> usize {
        git(self.work.path(), &["rev-list", "--count", "HEAD"], NOW)
            .trim()
            .parse()
            .unwrap_or(0)
    }

    /// True if `rel` is committed at HEAD (`git ls-files` would list it; check via `git cat-file`).
    pub fn is_tracked_at_head(&self, rel: &str) -> bool {
        !git(
            self.work.path(),
            &["ls-tree", "HEAD", "--name-only", rel],
            NOW,
        )
        .trim()
        .is_empty()
    }

    /// A repo with one committed file plus a dirty working tree covering the interesting states:
    /// a staged-new file (`A `), a modified-unstaged tracked file (` M`), and an untracked file
    /// (`??`). `git status` sorts by path, so the list order is deterministic:
    ///   `newstaged.txt`, `tracked.txt`, `untracked.txt`.
    pub fn with_dirty() -> TempRepo {
        let work = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        let wp = work.path();

        git(wp, &["init", "-b", "main"], NOW);
        git(wp, &["config", "user.name", "Tester"], NOW);
        git(wp, &["config", "user.email", "tester@example.com"], NOW);

        std::fs::write(wp.join("tracked.txt"), "original line\n").unwrap();
        git(wp, &["add", "-A"], NOW);
        git(wp, &["commit", "-m", "base"], NOW);

        let mut shas = HashMap::new();
        shas.insert(
            "base".to_string(),
            git(wp, &["rev-parse", "HEAD"], NOW).trim().to_string(),
        );

        // Dirty state:
        std::fs::write(wp.join("tracked.txt"), "changed line\n").unwrap(); //  M
        std::fs::write(wp.join("newstaged.txt"), "staged content\n").unwrap();
        git(wp, &["add", "newstaged.txt"], NOW); // A
        std::fs::write(wp.join("untracked.txt"), "brand new\n").unwrap(); // ??

        TempRepo {
            work,
            _origin: origin,
            shas,
        }
    }

    /// A repo with a `main` baseline and a `feature` branch one commit ahead, its working tree clean.
    /// So `git diff main...HEAD` (the `gitt diff` vs-main scope) shows the feature commit's file,
    /// while the unstaged/staged/working scopes are empty — exercising the PR "Files changed" view.
    pub fn with_feature_branch() -> TempRepo {
        let work = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        let wp = work.path();

        git(wp, &["init", "-b", "main"], NOW);
        git(wp, &["config", "user.name", "Tester"], NOW);
        git(wp, &["config", "user.email", "tester@example.com"], NOW);

        std::fs::write(wp.join("base.txt"), "base\n").unwrap();
        git(wp, &["add", "-A"], NOW);
        git(wp, &["commit", "-m", "base"], NOW);

        // A feature branch, one commit ahead of main.
        git(wp, &["checkout", "-b", "feature"], NOW);
        std::fs::write(wp.join("feature.txt"), "feature work\n").unwrap();
        git(wp, &["add", "-A"], NOW);
        git(wp, &["commit", "-m", "add feature"], NOW);

        TempRepo {
            work,
            _origin: origin,
            shas: HashMap::new(),
        }
    }

    /// A repo with a `main` baseline (pushed to a bare `origin`, so `origin/main` resolves without a
    /// network) plus two feature branches each one commit ahead of `main`: `wip-parser` (newest) and
    /// `bugfix`. `HEAD` stays on `main`, so the branch list is deterministic (the current `main` is
    /// pinned first, then by commit date: `wip-parser`, `bugfix`) and the feature branches are safely
    /// deletable.
    pub fn with_branches() -> TempRepo {
        let work = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        let wp = work.path();

        git(wp, &["init", "-b", "main"], NOW);
        git(wp, &["config", "user.name", "Tester"], NOW);
        git(wp, &["config", "user.email", "tester@example.com"], NOW);

        let mut shas = HashMap::new();

        std::fs::write(wp.join("base.txt"), "base\n").unwrap();
        git(wp, &["add", "-A"], NOW - 40 * DAY);
        git(wp, &["commit", "-m", "base"], NOW - 40 * DAY);
        shas.insert(
            "base".to_string(),
            git(wp, &["rev-parse", "HEAD"], NOW).trim().to_string(),
        );

        // Wire a bare origin and push main, then set origin/HEAD (main-branch detection via symref).
        let origin_path = origin.path().join("repo.git");
        git(wp, &["init", "--bare", origin_path.to_str().unwrap()], NOW);
        git(
            wp,
            &["remote", "add", "origin", origin_path.to_str().unwrap()],
            NOW,
        );
        git(wp, &["push", "-u", "origin", "main"], NOW);
        git(wp, &["remote", "set-head", "origin", "main"], NOW);

        // `bugfix`, one commit ahead of main (older).
        git(wp, &["checkout", "-b", "bugfix"], NOW - 5 * DAY);
        std::fs::write(wp.join("bug.txt"), "bug fix\n").unwrap();
        git(wp, &["add", "-A"], NOW - 5 * DAY);
        git(wp, &["commit", "-m", "fix the bug"], NOW - 5 * DAY);
        shas.insert(
            "fix the bug".to_string(),
            git(wp, &["rev-parse", "HEAD"], NOW).trim().to_string(),
        );

        // `wip-parser`, one commit ahead of main (newest → sorts first).
        git(wp, &["checkout", "main"], NOW);
        git(wp, &["checkout", "-b", "wip-parser"], NOW - 2 * DAY);
        std::fs::write(wp.join("parser.txt"), "parser work\n").unwrap();
        git(wp, &["add", "-A"], NOW - 2 * DAY);
        git(wp, &["commit", "-m", "refactor parser"], NOW - 2 * DAY);
        shas.insert(
            "refactor parser".to_string(),
            git(wp, &["rev-parse", "HEAD"], NOW).trim().to_string(),
        );

        // Land back on `main` as the current branch.
        git(wp, &["checkout", "main"], NOW);

        TempRepo {
            work,
            _origin: origin,
            shas,
        }
    }

    pub fn with_graph() -> TempRepo {
        let work = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        let wp = work.path();

        git(wp, &["init", "-b", "main"], NOW);
        git(wp, &["config", "user.name", "Tester"], NOW);
        git(wp, &["config", "user.email", "tester@example.com"], NOW);

        let mut shas = HashMap::new();
        let mut commit = |subject: &str, days_ago: i64| {
            let ts = NOW - days_ago * DAY;
            std::fs::write(wp.join("file.txt"), format!("{subject}\n")).unwrap();
            git(wp, &["add", "-A"], ts);
            git(wp, &["commit", "-m", subject], ts);
            let sha = git(wp, &["rev-parse", "HEAD"], ts).trim().to_string();
            shas.insert(subject.to_string(), sha);
        };

        commit("init project", 40);
        commit("add parser", 20);
        commit("fix flaky test", 10);
        commit("refactor parser", 5);

        // Wire a bare origin and push everything so far, then set origin/HEAD.
        let origin_path = origin.path().join("repo.git");
        git(wp, &["init", "--bare", origin_path.to_str().unwrap()], NOW);
        git(
            wp,
            &["remote", "add", "origin", origin_path.to_str().unwrap()],
            NOW,
        );
        git(wp, &["push", "-u", "origin", "main"], NOW);
        git(wp, &["remote", "set-head", "origin", "main"], NOW);

        // One more commit that lives only on the local HEAD.
        commit("local only change", 2);

        TempRepo {
            work,
            _origin: origin,
            shas,
        }
    }
}

/// Run a git command in `dir` with fully isolated config and pinned author/committer identity+date.
fn git(dir: &Path, args: &[&str], date_ts: i64) -> String {
    let date = format!("{date_ts} +0000");
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@example.com")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("failed to run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Path type helper re-export so tests can name it if needed.
pub type RepoPath = PathBuf;
