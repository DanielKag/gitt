//! Parse `git status --porcelain=v1 -z` output into [`StatusEntry`]s.
//!
//! Format contract: entries are NUL-terminated. Each entry is `XY<space><path>`, where `X` is the
//! index (staged) status and `Y` the worktree (unstaged) status. For a rename/copy the original path
//! follows as a **separate** NUL-terminated token (`XY <new>` `\0` `<old>`), so those consume an
//! extra token. `-z` means paths are raw and unquoted, which keeps this parser deterministic.

use crate::domain::status::StatusEntry;

/// The `git status` invocation whose output [`parse_status`] expects.
pub const STATUS_ARGS: &[&str] = &["status", "--porcelain=v1", "-z", "-uall"];

/// Parse raw `git status --porcelain=v1 -z` output into entries, in git's reported order.
pub fn parse_status(raw: &str) -> Vec<StatusEntry> {
    let tokens: Vec<&str> = raw.split('\0').filter(|t| !t.is_empty()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        i += 1;

        // A valid entry is at least "XY<space><1-char path>" = 4 bytes. X/Y and the separator are
        // ASCII, so byte index 3 is a safe char boundary for the path.
        if tok.len() < 4 {
            continue;
        }
        let mut chars = tok.chars();
        let (Some(index), Some(worktree)) = (chars.next(), chars.next()) else {
            continue;
        };
        let path = tok[3..].to_string();

        // Renames/copies carry their original path in the following token.
        let is_rename = matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C');
        let orig_path = if is_rename {
            let orig = tokens.get(i).map(|s| s.to_string());
            if orig.is_some() {
                i += 1;
            }
            orig
        } else {
            None
        };

        out.push(StatusEntry {
            index,
            worktree,
            path,
            orig_path,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // STAT-02: index + worktree codes, untracked, and paths parse from the -z stream.
    #[test]
    fn parses_mixed_entries() {
        let raw = "A  staged_new.txt\0 M tracked.txt\0MM both.txt\0?? untracked.txt\0";
        let entries = parse_status(raw);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].index, 'A');
        assert_eq!(entries[0].worktree, ' ');
        assert_eq!(entries[0].path, "staged_new.txt");
        assert!(entries[0].is_staged());

        assert_eq!(entries[1].badge(), " M");
        assert_eq!(entries[1].path, "tracked.txt");

        assert_eq!(entries[2].badge(), "MM");

        assert!(entries[3].is_untracked());
        assert_eq!(entries[3].path, "untracked.txt");
    }

    // STAT-02: rename entries consume the trailing original-path token.
    #[test]
    fn parses_rename_with_orig_path() {
        let raw = "R  new_name.txt\0old_name.txt\0 M after.txt\0";
        let entries = parse_status(raw);
        assert_eq!(
            entries.len(),
            2,
            "orig-path token must not become its own entry"
        );
        assert_eq!(entries[0].index, 'R');
        assert_eq!(entries[0].path, "new_name.txt");
        assert_eq!(entries[0].orig_path.as_deref(), Some("old_name.txt"));
        assert_eq!(entries[1].path, "after.txt");
    }

    #[test]
    fn paths_with_spaces_survive() {
        let raw = " M my notes.txt\0";
        let entries = parse_status(raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "my notes.txt");
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_status("").is_empty());
        assert!(parse_status("\0\0").is_empty());
    }
}
