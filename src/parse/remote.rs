//! Parse `git remote show origin` output to find the remote's HEAD branch.

/// Extract the branch name from the `HEAD branch: <name>` line of `git remote show origin`.
pub fn parse_remote_show(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("HEAD branch:") {
            let name = rest.trim();
            if !name.is_empty() && name != "(unknown)" {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // LOG-17: remote-show fallback parsing.
    #[test]
    fn log_17_parses_head_branch() {
        let raw = "\
* remote origin
  Fetch URL: git@github.com:org/repo.git
  Push  URL: git@github.com:org/repo.git
  HEAD branch: main
  Remote branches:
    main tracked";
        assert_eq!(parse_remote_show(raw).as_deref(), Some("main"));
    }

    #[test]
    fn log_17_unknown_head_is_none() {
        assert_eq!(parse_remote_show("  HEAD branch: (unknown)"), None);
    }

    #[test]
    fn log_17_missing_line_is_none() {
        assert_eq!(parse_remote_show("no head branch line here"), None);
    }
}
