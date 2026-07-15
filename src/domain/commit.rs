//! Core commit data. Plain values — no I/O, no rendering.

/// Which log the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    /// The current branch (`HEAD`).
    LocalHead,
    /// The remote main branch (`origin/<main>`).
    OriginMain,
}

impl View {
    /// The other view (arrows toggle between exactly two views).
    pub fn toggled(self) -> View {
        match self {
            View::LocalHead => View::OriginMain,
            View::OriginMain => View::LocalHead,
        }
    }
}

/// A ref decoration attached to a commit (git `%D`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// `HEAD` itself.
    Head,
    /// A local branch, e.g. `main`.
    Local(String),
    /// A remote-tracking branch, e.g. `origin/main`.
    Remote(String),
    /// A tag, e.g. `v1.0`.
    Tag(String),
}

impl Ref {
    /// The text shown to the user for this ref.
    pub fn label(&self) -> &str {
        match self {
            Ref::Head => "HEAD",
            Ref::Local(s) | Ref::Remote(s) | Ref::Tag(s) => s,
        }
    }
}

/// A single commit as shown in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full 40-char object hash (used for actions like copy/checkout).
    pub hash: String,
    /// Abbreviated hash (`%h`) shown in the list.
    pub short: String,
    /// Committer timestamp (`%ct`, unix seconds).
    pub timestamp: i64,
    /// Author name (`%an`).
    pub author: String,
    /// Commit subject (`%s`).
    pub subject: String,
    /// Human relative time ("3 days ago"), computed at load time from a `Clock`.
    pub relative: String,
    /// Parsed ref decorations (`%D`).
    pub refs: Vec<Ref>,
    /// Lowercase-free search string fed to the fuzzy matcher.
    pub haystack: String,
}

/// The `git` argv for a pending commit, plus a copy-pasteable, shell-quoted line to print before it
/// runs. gitt hands the commit off to the real terminal — tearing down the TUI, printing this line,
/// then running `git` with inherited stdio — so pre-commit hooks (e.g. lefthook) stream their progress
/// live and a failure stays on screen and is easy to re-run. Pure, so it's tested here; the runtime is
/// a dumb pipe that prints `display` and spawns `git` with `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInvocation {
    /// The argv passed to `git` (each element is literal — no shell parsing/word-splitting).
    pub args: Vec<String>,
    /// A human-facing, shell-quoted command line, e.g. `git commit -m 'fix the bug'`.
    pub display: String,
}

/// Build the [`CommitInvocation`] for a commit (or `--amend`) with `message`.
pub fn commit_command(message: &str, amend: bool) -> CommitInvocation {
    let mut args = vec!["commit".to_string()];
    if amend {
        args.push("--amend".to_string());
    }
    args.push("-m".to_string());
    args.push(message.to_string());

    let amend_flag = if amend { " --amend" } else { "" };
    let display = format!("git commit{amend_flag} -m {}", shell_single_quote(message));
    CommitInvocation { args, display }
}

/// Wrap `s` in single quotes for a POSIX shell, escaping any embedded single quote as `'\''`, so the
/// printed command is safe to copy-paste and re-run verbatim.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // CMT-03: a normal commit builds `git commit -m <msg>` with a copy-pasteable display line.
    #[test]
    fn commit_command_normal() {
        let inv = commit_command("fix the bug", false);
        assert_eq!(inv.args, vec!["commit", "-m", "fix the bug"]);
        assert_eq!(inv.display, "git commit -m 'fix the bug'");
    }

    // CMT-05: amend adds `--amend`.
    #[test]
    fn commit_command_amend() {
        let inv = commit_command("reword", true);
        assert_eq!(inv.args, vec!["commit", "--amend", "-m", "reword"]);
        assert_eq!(inv.display, "git commit --amend -m 'reword'");
    }

    // A single quote in the message is escaped so the printed line stays runnable; the argv keeps the
    // literal message (git receives it directly, no shell in between).
    #[test]
    fn commit_command_quotes_safely() {
        let inv = commit_command("it's broken", false);
        assert_eq!(inv.args, vec!["commit", "-m", "it's broken"]);
        assert_eq!(inv.display, r"git commit -m 'it'\''s broken'");
    }
}
