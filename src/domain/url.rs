//! Pure URL helpers: normalize a git remote to an https base, build a commit URL, and pull a PR
//! number out of a commit subject.

/// Normalize a git remote URL to an `https://host/org/repo` base (no `.git`, no trailing slash).
///
/// Handles the common forms:
/// - `git@github.com:org/repo.git`        (scp-like SSH)
/// - `ssh://git@github.com/org/repo.git`
/// - `https://github.com/org/repo.git`
/// - `http://github.com/org/repo`
///
/// Returns `None` if it doesn't look like something we can turn into a browseable base.
pub fn normalize_remote(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }

    let without_scheme = if let Some(rest) = remote.strip_prefix("git@") {
        // scp-like: git@host:org/repo(.git) — turn the first ':' into '/'.
        rest.replacen(':', "/", 1)
    } else if let Some(rest) = remote.strip_prefix("ssh://") {
        strip_userinfo(rest)
    } else if let Some(rest) = remote.strip_prefix("https://") {
        strip_userinfo(rest)
    } else if let Some(rest) = remote.strip_prefix("http://") {
        strip_userinfo(rest)
    } else if remote.contains('@') && remote.contains(':') {
        // Bare scp-like with a non-git user, e.g. user@host:org/repo.
        let rest = remote.split_once('@')?.1;
        rest.replacen(':', "/", 1)
    } else {
        return None;
    };

    let trimmed = without_scheme
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| without_scheme.trim_end_matches('/'));

    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty() || !trimmed.contains('/') {
        return None;
    }
    Some(format!("https://{trimmed}"))
}

/// Drop any `user@` prefix from a `host/...` string.
fn strip_userinfo(host_and_path: &str) -> String {
    match host_and_path.split_once('@') {
        Some((_, rest)) => rest.to_string(),
        None => host_and_path.to_string(),
    }
}

/// Build the browser URL for a commit given a raw remote URL.
pub fn commit_url(remote: &str, hash: &str) -> Option<String> {
    let base = normalize_remote(remote)?;
    Some(format!("{base}/commit/{hash}"))
}

/// Extract a PR number from a commit subject of the form `... (#123)`.
///
/// Used as a fallback for the "Open PR" action when `gh` can't resolve one directly.
pub fn pr_number_from_subject(subject: &str) -> Option<u32> {
    let start = subject.rfind("(#")? + 2;
    let rest = &subject[start..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // LOG-14: normalize SSH and HTTPS remotes to the same https base.
    #[test]
    fn log_14_normalize_ssh_scp() {
        assert_eq!(
            normalize_remote("git@github.com:org/repo.git").as_deref(),
            Some("https://github.com/org/repo")
        );
    }

    #[test]
    fn log_14_normalize_https_with_git_suffix() {
        assert_eq!(
            normalize_remote("https://github.com/org/repo.git").as_deref(),
            Some("https://github.com/org/repo")
        );
    }

    #[test]
    fn log_14_normalize_ssh_url_form() {
        assert_eq!(
            normalize_remote("ssh://git@github.com/org/repo.git").as_deref(),
            Some("https://github.com/org/repo")
        );
    }

    #[test]
    fn log_14_normalize_http_no_suffix() {
        assert_eq!(
            normalize_remote("http://github.com/org/repo").as_deref(),
            Some("https://github.com/org/repo")
        );
    }

    #[test]
    fn log_14_commit_url() {
        assert_eq!(
            commit_url("git@github.com:org/repo.git", "abc123").as_deref(),
            Some("https://github.com/org/repo/commit/abc123")
        );
    }

    #[test]
    fn log_14_rejects_garbage() {
        assert_eq!(normalize_remote(""), None);
        assert_eq!(normalize_remote("not a url"), None);
    }

    // LOG-15: PR number fallback parsed from subject.
    #[test]
    fn log_15_pr_number_from_subject() {
        assert_eq!(pr_number_from_subject("Fix the bug (#123)"), Some(123));
        assert_eq!(
            pr_number_from_subject("Merge pull request (#7) into main"),
            Some(7)
        );
        // Takes the last (#N) if several appear.
        assert_eq!(pr_number_from_subject("Revert (#1) (#456)"), Some(456));
        assert_eq!(pr_number_from_subject("no pr here"), None);
    }
}
