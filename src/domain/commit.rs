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
