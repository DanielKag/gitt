//! Parse `git diff --name-status -z` output into [`DiffFile`]s.
//!
//! Format contract: records are NUL-separated. A normal record is `<status>` `\0` `<path>`, where
//! `<status>` is a single letter (`M`/`A`/`D`/`T`/`U`). A rename/copy record is `<status><score>` `\0`
//! `<old>` `\0` `<new>` — the status starts with `R`/`C` (e.g. `R100`) and consumes **two** path
//! tokens, old then new. `-z` means paths are raw and unquoted, keeping this parser deterministic.

use crate::domain::diff::DiffFile;

/// The base `git diff --name-status` invocation; the scope-specific revision args are appended by the
/// port before this parser runs on the output.
pub const DIFF_NAME_STATUS_ARGS: &[&str] = &["diff", "--name-status", "-z", "--no-color"];

/// Parse raw `git diff --name-status -z` output into changed files, in git's reported order.
pub fn parse_diff_name_status(raw: &str) -> Vec<DiffFile> {
    let tokens: Vec<&str> = raw.split('\0').filter(|t| !t.is_empty()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let status_tok = tokens[i];
        i += 1;
        let Some(status) = status_tok.chars().next() else {
            continue;
        };

        if matches!(status, 'R' | 'C') {
            // Rename/copy: the next two tokens are the old then the new path.
            let (Some(orig), Some(new)) = (tokens.get(i), tokens.get(i + 1)) else {
                break; // truncated stream; nothing sensible to emit.
            };
            i += 2;
            out.push(DiffFile {
                status,
                path: new.to_string(),
                orig_path: Some(orig.to_string()),
            });
        } else {
            let Some(path) = tokens.get(i) else {
                break;
            };
            i += 1;
            out.push(DiffFile {
                status,
                path: path.to_string(),
                orig_path: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // DIFF-02: statuses + paths parse from the -z stream.
    #[test]
    fn parses_mixed_entries() {
        let raw = "M\0src/reducer.rs\0A\0src/diff.rs\0D\0old.rs\0";
        let files = parse_diff_name_status(raw);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, 'M');
        assert_eq!(files[0].path, "src/reducer.rs");
        assert!(files[0].orig_path.is_none());
        assert_eq!(files[1].status, 'A');
        assert_eq!(files[1].path, "src/diff.rs");
        assert_eq!(files[2].status, 'D');
        assert_eq!(files[2].path, "old.rs");
    }

    // DIFF-02: a rename record consumes its old + new path tokens.
    #[test]
    fn parses_rename_with_orig_path() {
        let raw = "R100\0old_name.rs\0new_name.rs\0M\0after.rs\0";
        let files = parse_diff_name_status(raw);
        assert_eq!(
            files.len(),
            2,
            "old-path token must not become its own entry"
        );
        assert_eq!(files[0].status, 'R');
        assert_eq!(files[0].orig_path.as_deref(), Some("old_name.rs"));
        assert_eq!(files[0].path, "new_name.rs");
        assert!(files[0].is_rename());
        assert_eq!(files[1].status, 'M');
        assert_eq!(files[1].path, "after.rs");
    }

    #[test]
    fn paths_with_spaces_survive() {
        let raw = "M\0my notes.txt\0";
        let files = parse_diff_name_status(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "my notes.txt");
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_diff_name_status("").is_empty());
        assert!(parse_diff_name_status("\0\0").is_empty());
    }
}
