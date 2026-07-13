//! Parse `gh pr list --json headRefName,state,isDraft` output into a per-branch [`PrStatus`] map.
//!
//! Pure: feed the captured JSON in, get `head-branch → status` out. Keeping it a separate pure
//! function (the port layer just shells out to `gh` and hands the stdout here) means the mapping is
//! tested against fixtures with no network. A branch with several PRs keeps its most-actionable one
//! (open/draft > merged > closed, via [`PrStatus::rank`]).

use std::collections::HashMap;

use crate::domain::PrStatus;

/// Parse `gh`'s PR-list JSON into a `head-branch → status` map. Unrecognised or malformed entries are
/// skipped rather than aborting the whole parse (one odd PR should never blank the column).
pub fn parse_pr_list(json: &str) -> HashMap<String, PrStatus> {
    let mut out: HashMap<String, PrStatus> = HashMap::new();
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return out;
    };
    for entry in entries {
        let Some(branch) = entry.get("headRefName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(status) = status_of(&entry) else {
            continue;
        };
        // Keep the most-actionable status when a branch has more than one PR.
        out.entry(branch.to_string())
            .and_modify(|cur| {
                if status.rank() > cur.rank() {
                    *cur = status;
                }
            })
            .or_insert(status);
    }
    out
}

/// Map one PR's `state`/`isDraft` fields to a [`PrStatus`] (`gh` reports `OPEN`/`CLOSED`/`MERGED`).
fn status_of(entry: &serde_json::Value) -> Option<PrStatus> {
    let state = entry.get("state")?.as_str()?;
    let is_draft = entry
        .get("isDraft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match state.to_ascii_uppercase().as_str() {
        "OPEN" if is_draft => Some(PrStatus::Draft),
        "OPEN" => Some(PrStatus::Open),
        "MERGED" => Some(PrStatus::Merged),
        "CLOSED" => Some(PrStatus::Closed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // BR-17: gh states map to PR statuses (open, draft, merged, closed).
    #[test]
    fn br_17_maps_states() {
        let json = r#"[
            {"headRefName":"feature","state":"OPEN","isDraft":false},
            {"headRefName":"wip","state":"OPEN","isDraft":true},
            {"headRefName":"done","state":"MERGED","isDraft":false},
            {"headRefName":"stale","state":"CLOSED","isDraft":false}
        ]"#;
        let map = parse_pr_list(json);
        assert_eq!(map.get("feature"), Some(&PrStatus::Open));
        assert_eq!(map.get("wip"), Some(&PrStatus::Draft));
        assert_eq!(map.get("done"), Some(&PrStatus::Merged));
        assert_eq!(map.get("stale"), Some(&PrStatus::Closed));
        assert_eq!(map.get("nope"), None);
    }

    // BR-17: a branch with several PRs keeps the most-actionable one (open beats a prior closed).
    #[test]
    fn br_17_multiple_prs_keep_most_actionable() {
        let json = r#"[
            {"headRefName":"feature","state":"CLOSED","isDraft":false},
            {"headRefName":"feature","state":"OPEN","isDraft":false}
        ]"#;
        assert_eq!(parse_pr_list(json).get("feature"), Some(&PrStatus::Open));

        // Order-independent: the open one wins even when it appears first.
        let json2 = r#"[
            {"headRefName":"feature","state":"OPEN","isDraft":false},
            {"headRefName":"feature","state":"CLOSED","isDraft":false}
        ]"#;
        assert_eq!(parse_pr_list(json2).get("feature"), Some(&PrStatus::Open));
    }

    #[test]
    fn br_17_malformed_is_empty_not_panic() {
        assert!(parse_pr_list("").is_empty());
        assert!(parse_pr_list("not json").is_empty());
        assert!(parse_pr_list("[]").is_empty());
        // Missing fields are skipped, valid neighbours survive.
        let json = r#"[{"state":"OPEN"},{"headRefName":"ok","state":"MERGED"}]"#;
        let map = parse_pr_list(json);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("ok"), Some(&PrStatus::Merged));
    }
}
