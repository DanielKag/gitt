//! Parse git ref decorations (`%D`) into typed [`Ref`]s.

use crate::domain::Ref;

/// Parse a `%D` decoration string like `HEAD -> main, origin/main, origin/HEAD, tag: v1.0`.
///
/// Classification is heuristic (git's decoration string does not fully disambiguate local vs remote
/// branches): a `tag:` prefix is a tag, `HEAD` is HEAD, `HEAD -> x` yields both HEAD and local `x`,
/// a name containing `/` is treated as remote-tracking, otherwise it's a local branch.
pub fn parse_refs(decorations: &str) -> Vec<Ref> {
    let mut refs = Vec::new();
    for raw in decorations.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(branch) = token.strip_prefix("HEAD -> ") {
            refs.push(Ref::Head);
            push_branch(&mut refs, branch.trim());
        } else if token == "HEAD" {
            refs.push(Ref::Head);
        } else if let Some(tag) = token.strip_prefix("tag: ") {
            refs.push(Ref::Tag(tag.trim().to_string()));
        } else {
            push_branch(&mut refs, token);
        }
    }
    refs
}

fn push_branch(refs: &mut Vec<Ref>, name: &str) {
    if name.is_empty() {
        return;
    }
    // A remote-tracking ref's origin/HEAD pointer is noise; keep the branch, drop `origin/HEAD`.
    if name.ends_with("/HEAD") {
        return;
    }
    if name.contains('/') {
        refs.push(Ref::Remote(name.to_string()));
    } else {
        refs.push(Ref::Local(name.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // LOG-03: split %D into typed refs.
    #[test]
    fn log_03_head_arrow_local_and_remote_and_tag() {
        let refs = parse_refs("HEAD -> main, origin/main, tag: v1.0");
        assert_eq!(
            refs,
            vec![
                Ref::Head,
                Ref::Local("main".into()),
                Ref::Remote("origin/main".into()),
                Ref::Tag("v1.0".into()),
            ]
        );
    }

    #[test]
    fn log_03_empty_is_no_refs() {
        assert_eq!(parse_refs(""), vec![]);
    }

    #[test]
    fn log_03_drops_origin_head_pointer() {
        let refs = parse_refs("origin/main, origin/HEAD");
        assert_eq!(refs, vec![Ref::Remote("origin/main".into())]);
    }

    #[test]
    fn log_03_bare_head() {
        assert_eq!(parse_refs("HEAD"), vec![Ref::Head]);
    }
}
