//! Pure precedence logic for resolving the repository's main branch.
//!
//! The actual lookups (reading `origin/HEAD`, the cache file, `git remote show`) are I/O and live in
//! `ports`. This function just encodes the *order* and the branch-name extraction, so the precedence
//! is unit-testable without touching git or the filesystem.

/// Resolve the main branch name from three already-fetched, optional sources, in priority order:
/// 1. the `origin/HEAD` symbolic ref (fastest, authoritative when present),
/// 2. a cached value (previously resolved for this repo),
/// 3. the output line from `git remote show origin`.
///
/// Each input may be a full ref (`refs/remotes/origin/main`), a short ref (`origin/main`), or a bare
/// branch name (`main`); all are reduced to the bare branch name.
pub fn resolve_main_branch(
    symref: Option<&str>,
    cache: Option<&str>,
    remote_show: Option<&str>,
) -> Option<String> {
    symref
        .and_then(branch_name)
        .or_else(|| cache.and_then(branch_name))
        .or_else(|| remote_show.and_then(branch_name))
}

/// Reduce any ref form to a bare branch name.
fn branch_name(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let s = s
        .strip_prefix("refs/remotes/origin/")
        .or_else(|| s.strip_prefix("refs/heads/"))
        .or_else(|| s.strip_prefix("origin/"))
        .unwrap_or(s);
    let s = s.trim();
    if s.is_empty() || s == "HEAD" {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // LOG-17: precedence symref -> cache -> remote_show.
    #[test]
    fn log_17_symref_wins() {
        assert_eq!(
            resolve_main_branch(
                Some("refs/remotes/origin/main"),
                Some("develop"),
                Some("master")
            )
            .as_deref(),
            Some("main")
        );
    }

    #[test]
    fn log_17_falls_back_to_cache() {
        assert_eq!(
            resolve_main_branch(None, Some("develop"), Some("master")).as_deref(),
            Some("develop")
        );
    }

    #[test]
    fn log_17_falls_back_to_remote_show() {
        assert_eq!(
            resolve_main_branch(None, None, Some("origin/master")).as_deref(),
            Some("master")
        );
    }

    #[test]
    fn log_17_none_when_all_missing() {
        assert_eq!(resolve_main_branch(None, None, None), None);
    }

    #[test]
    fn log_17_ignores_bare_head_symref() {
        // A detached `origin/HEAD` pointing at nothing useful should not win.
        assert_eq!(
            resolve_main_branch(Some("origin/HEAD"), Some("main"), None).as_deref(),
            Some("main")
        );
    }
}
