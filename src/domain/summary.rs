//! Pure helpers for AI commit summaries: building the model prompt and resolving config. No I/O.
//!
//! The prompt sent to the local model is deterministic given the commit subject and diff, so it is a
//! pure function tested against expectations. The port layer (`ports/system.rs`) is a dumb pipe that
//! feeds this string to `ollama` and returns the completion.

use std::path::PathBuf;

/// Default Ollama model, overridable via `GITT_OLLAMA_MODEL`. A small code-trained model is the
/// sweet spot for one-line commit summaries: benchmarked fastest *and* correct (right identifiers,
/// scopes, backticks) vs larger models, whose extra quality isn't worth the latency.
pub const DEFAULT_MODEL: &str = "qwen2.5-coder:3b";

/// Default Ollama server, overridable via `OLLAMA_HOST` (ollama's own convention).
pub const DEFAULT_OLLAMA_HOST: &str = "http://127.0.0.1:11434";

/// Upper bound on diff lines fed to the model, so a huge commit can't blow up the prompt.
pub const MAX_DIFF_LINES: usize = 200;

/// Upper bound on diff *characters* — the real proxy for input tokens, which dominate latency
/// (prompt prefill). A one-sentence summary needs the gist, not the whole patch; keeping this small
/// keeps generation fast even on large commits.
pub const MAX_DIFF_CHARS: usize = 4000;

/// System instruction prepended to every summary prompt.
const SYSTEM: &str = "You are a senior software engineer reviewing a git commit. In one or two plain \
sentences, describe what the commit changes and why. Wrap file names, paths, code identifiers, \
commands, tools, and version numbers in `backticks`. Reply with only the summary — no preamble, no \
headings, no bullet points, no code fences.";

/// Resolve the Ollama model name from the `GITT_OLLAMA_MODEL` value (if any), falling back to the
/// default. Blank/whitespace values fall back too.
pub fn ollama_model(configured: Option<String>) -> String {
    match configured {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => DEFAULT_MODEL.to_string(),
    }
}

/// Resolve the summary cache directory from environment values, in precedence order:
/// `GITT_CACHE_DIR` (used verbatim) → `$XDG_CACHE_HOME/gitt/summaries` → `$HOME/.cache/gitt/summaries`.
/// Returns `None` only when none of the three are set (so there is nowhere to cache).
pub fn resolve_cache_dir(
    gitt_cache_dir: Option<&str>,
    xdg_cache_home: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    let nonempty = |s: &&str| !s.trim().is_empty();
    if let Some(dir) = gitt_cache_dir.filter(nonempty) {
        return Some(PathBuf::from(dir));
    }
    let base = xdg_cache_home
        .filter(nonempty)
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(nonempty)
                .map(|h| PathBuf::from(h).join(".cache"))
        })?;
    Some(base.join("gitt").join("summaries"))
}

/// Build the Ollama `/api/generate` URL from the `OLLAMA_HOST` value (if any). Accepts a bare
/// `host:port` (assumes `http://`) or a full URL; falls back to the local default.
pub fn ollama_generate_url(host: Option<&str>) -> String {
    let host = host
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_OLLAMA_HOST);
    let base = if host.starts_with("http://") || host.starts_with("https://") {
        host.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", host.trim_end_matches('/'))
    };
    format!("{base}/api/generate")
}

/// Bound `diff` for the prompt: cap by lines first, then by characters (the real token proxy),
/// appending a marker when anything was cut. Keeping the diff small keeps prompt prefill — the
/// dominant cost of generation — fast.
pub fn truncate_diff(diff: &str, max_lines: usize, max_chars: usize) -> String {
    let mut out: String = if diff.lines().count() > max_lines {
        diff.lines().take(max_lines).collect::<Vec<_>>().join("\n")
    } else {
        diff.to_string()
    };
    let mut cut = out.len() < diff.len();
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
        cut = true;
    }
    if cut {
        out.push_str("\n… [diff truncated to keep the prompt small]");
    }
    out
}

/// Build the full prompt for summarizing a commit: system instruction + subject + (bounded) diff.
pub fn build_prompt(subject: &str, diff: &str) -> String {
    let truncated = truncate_diff(diff, MAX_DIFF_LINES, MAX_DIFF_CHARS);
    format!("{SYSTEM}\n\nCommit subject:\n{subject}\n\nDiff:\n{truncated}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // SUM-07: model resolution prefers the env value, falls back to the default.
    #[test]
    fn sum_07_model_resolution() {
        assert_eq!(ollama_model(None), DEFAULT_MODEL);
        assert_eq!(ollama_model(Some("  ".into())), DEFAULT_MODEL);
        assert_eq!(ollama_model(Some("qwen2.5-coder".into())), "qwen2.5-coder");
        assert_eq!(ollama_model(Some("  mistral \n".into())), "mistral");
    }

    // SUM-07: the generate URL is built from OLLAMA_HOST (bare or full), else the local default.
    #[test]
    fn sum_07_generate_url() {
        assert_eq!(
            ollama_generate_url(None),
            "http://127.0.0.1:11434/api/generate"
        );
        assert_eq!(
            ollama_generate_url(Some("localhost:11434")),
            "http://localhost:11434/api/generate"
        );
        assert_eq!(
            ollama_generate_url(Some("http://box:1234/")),
            "http://box:1234/api/generate"
        );
        assert_eq!(
            ollama_generate_url(Some("  ")),
            "http://127.0.0.1:11434/api/generate"
        );
    }

    // SUM-05: the prompt carries the system instruction, the subject, and the diff.
    #[test]
    fn sum_05_prompt_includes_context() {
        let prompt = build_prompt("fix flaky test", "diff --git a/x b/x\n+added\n-removed");
        assert!(
            prompt.contains("senior software engineer"),
            "system instruction present"
        );
        assert!(prompt.contains("fix flaky test"), "subject present");
        assert!(prompt.contains("+added"), "diff body present");
        assert!(prompt.contains("Commit subject:"));
        assert!(prompt.contains("Diff:"));
    }

    // SUM-05/09: a short diff is left intact; a diff over either cap is truncated with a marker.
    #[test]
    fn sum_05_diff_truncation() {
        let short = "line1\nline2\nline3";
        assert_eq!(truncate_diff(short, 200, 10_000), short);

        // Line cap (generous char cap): only the first N lines survive, plus the marker line.
        let big: String = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let cut = truncate_diff(&big, 200, 1_000_000);
        assert!(cut.contains("truncated"));
        assert_eq!(cut.lines().count(), 201);
        assert!(cut.contains("line 0") && !cut.contains("line 400"));

        // Char cap (generous line cap): the body is cut to the char budget (+ a marker).
        let wide = "x".repeat(9000);
        let cut = truncate_diff(&wide, 10_000, 4000);
        assert!(cut.contains("truncated"));
        assert_eq!(cut.chars().filter(|&c| c == 'x').count(), 4000);
    }

    // SUM-10: cache-dir precedence GITT_CACHE_DIR → XDG_CACHE_HOME → HOME/.cache.
    #[test]
    fn sum_10_cache_dir_resolution() {
        assert_eq!(
            resolve_cache_dir(Some("/explicit"), Some("/xdg"), Some("/home")),
            Some(PathBuf::from("/explicit"))
        );
        assert_eq!(
            resolve_cache_dir(None, Some("/xdg"), Some("/home")),
            Some(PathBuf::from("/xdg/gitt/summaries"))
        );
        assert_eq!(
            resolve_cache_dir(None, None, Some("/home")),
            Some(PathBuf::from("/home/.cache/gitt/summaries"))
        );
        assert_eq!(resolve_cache_dir(None, None, None), None);
        // Blank values are treated as unset.
        assert_eq!(
            resolve_cache_dir(Some("  "), Some(""), Some("/home")),
            Some(PathBuf::from("/home/.cache/gitt/summaries"))
        );
    }

    #[test]
    fn build_prompt_truncates_large_diffs() {
        let big: String = (0..1000)
            .map(|i| format!("+l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = build_prompt("huge commit", &big);
        assert!(prompt.contains("truncated"));
    }
}
