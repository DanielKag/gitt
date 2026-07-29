//! The optional `~/.gitt` config file — the pure half: parsing its text into a [`Config`].
//!
//! The format is deliberately the smallest thing that reads well without documentation: `key = value`
//! lines, `#` comments, blank lines ignored, no sections. Two settings live here (`diff_tool`,
//! `ollama_model`); anything else is ignored rather than rejected, so a config written for a newer
//! `gitt` never stops an older one from opening. See `specs/config.md`.
//!
//! Nothing here touches the filesystem — the shell (`ports::system::load_config`) reads the file and
//! hands the text in.

/// The settings `gitt` accepts from `~/.gitt`. `None` means "not configured" — the caller falls
/// through to the next precedence level (env var, then built-in default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// Which third-party diff renderer to pipe diffs through (`difftastic`, `delta`,
    /// `git-split-diffs`, `none`).
    pub diff_tool: Option<String>,
    /// Which Ollama model writes the AI summaries.
    pub ollama_model: Option<String>,
}

/// Parse the contents of a `~/.gitt` file. Never fails: an unrecognized or malformed line is skipped
/// (CFG-02), so a stray character can't stop `gitt` from starting. When a key repeats, the last
/// occurrence wins (CFG-05) — appending an override to the end of the file does what it looks like.
pub fn parse_config(text: &str) -> Config {
    let mut config = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue; // Not a `key = value` line; ignore it rather than guessing.
        };
        // A key may be written with dashes or underscores, in any case (CFG-03).
        let key = key.trim().to_ascii_lowercase().replace('-', "_");
        let value = value.trim();
        // An empty value means "unset", so it falls through instead of forcing an empty string (CFG-04).
        let value = if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };
        match key.as_str() {
            "diff_tool" => config.diff_tool = value,
            "ollama_model" => config.ollama_model = value,
            _ => {} // Unknown key: forward-compatible, not an error.
        }
    }
    config
}

/// Pick the first value that is present and not blank. The shared shape of every setting's precedence
/// chain (flag → env → config file → default), kept in one place so each setting reads the same.
pub fn first_configured<I, S>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<S>>,
    S: AsRef<str>,
{
    candidates
        .into_iter()
        .flatten()
        .map(|s| s.as_ref().trim().to_string())
        .find(|s| !s.is_empty())
}

/// Which diff-tool *name* the user asked for, if any: `--diff-tool` flag → `GITT_DIFF_TOOL` →
/// `~/.gitt` (CFG-06). `None` means "nobody said" — the shell then auto-detects. Pure, so the
/// precedence is unit-tested without touching the real environment; the shell only supplies `env`.
pub fn diff_tool_choice(
    explicit: Option<&str>,
    env: Option<String>,
    config: &Config,
) -> Option<String> {
    first_configured([explicit.map(str::to_string), env, config.diff_tool.clone()])
}

/// Which Ollama model the user asked for, if any: `GITT_OLLAMA_MODEL` → `~/.gitt` (CFG-07). `None`
/// falls back to the built-in default in [`crate::domain::summary::ollama_model`].
pub fn ollama_model_choice(env: Option<String>, config: &Config) -> Option<String> {
    first_configured([env, config.ollama_model.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;

    // CFG-01: `key = value` lines become a Config; whitespace around each part is trimmed.
    #[test]
    fn cfg_01_parses_known_keys() {
        let c = parse_config("diff_tool = delta\nollama_model = qwen3-coder:30b\n");
        assert_eq!(c.diff_tool.as_deref(), Some("delta"));
        assert_eq!(c.ollama_model.as_deref(), Some("qwen3-coder:30b"));
    }

    #[test]
    fn cfg_01_trims_generously() {
        let c = parse_config("   diff_tool   =    difftastic   \n");
        assert_eq!(c.diff_tool.as_deref(), Some("difftastic"));
    }

    // CFG-02: comments, blank lines, `=`-less lines, and unknown keys are all ignored.
    #[test]
    fn cfg_02_ignores_noise() {
        let c = parse_config(
            "# gitt config\n\
             \n\
                # indented comment\n\
             this line has no equals sign\n\
             future_setting = 42\n\
             diff_tool = delta\n",
        );
        assert_eq!(c.diff_tool.as_deref(), Some("delta"));
        assert_eq!(c.ollama_model, None);
    }

    #[test]
    fn cfg_02_garbage_only_is_empty_not_an_error() {
        assert_eq!(parse_config("!!!\n\u{0}\n???"), Config::default());
        assert_eq!(parse_config(""), Config::default());
    }

    // CFG-03: keys match case-insensitively and accept `-` for `_`.
    #[test]
    fn cfg_03_key_spelling_is_forgiving() {
        let c = parse_config("DIFF-TOOL = delta\nOllama_Model = mistral\n");
        assert_eq!(c.diff_tool.as_deref(), Some("delta"));
        assert_eq!(c.ollama_model.as_deref(), Some("mistral"));
    }

    // CFG-04: a key with an empty value is unset, not an empty string.
    #[test]
    fn cfg_04_empty_value_is_unset() {
        let c = parse_config("diff_tool =\nollama_model =    \n");
        assert_eq!(c.diff_tool, None);
        assert_eq!(c.ollama_model, None);
    }

    // CFG-05: the last occurrence of a repeated key wins.
    #[test]
    fn cfg_05_last_occurrence_wins() {
        let c = parse_config("diff_tool = delta\ndiff_tool = difftastic\n");
        assert_eq!(c.diff_tool.as_deref(), Some("difftastic"));
    }

    // A value may contain `=` (e.g. a model tag); only the first `=` separates key from value.
    #[test]
    fn cfg_01_value_may_contain_equals() {
        let c = parse_config("ollama_model = weird=name:7b\n");
        assert_eq!(c.ollama_model.as_deref(), Some("weird=name:7b"));
    }

    // CFG-06: the diff tool name resolves flag → env → config file, and "nobody said" stays None so
    // the shell can auto-detect.
    #[test]
    fn cfg_06_diff_tool_precedence() {
        let cfg = parse_config("diff_tool = git-split-diffs");
        let flag = Some("difftastic");
        let env = || Some("delta".to_string());

        // Flag beats everything.
        assert_eq!(
            diff_tool_choice(flag, env(), &cfg).as_deref(),
            Some("difftastic")
        );
        // Env beats the file.
        assert_eq!(
            diff_tool_choice(None, env(), &cfg).as_deref(),
            Some("delta")
        );
        // The file is the last word before autodetect.
        assert_eq!(
            diff_tool_choice(None, None, &cfg).as_deref(),
            Some("git-split-diffs")
        );
        // Nothing configured anywhere → autodetect (None), not a silent default.
        assert_eq!(diff_tool_choice(None, None, &Config::default()), None);
        // A blank env var doesn't shadow the file.
        assert_eq!(
            diff_tool_choice(None, Some("  ".into()), &cfg).as_deref(),
            Some("git-split-diffs")
        );
    }

    // CFG-07: the model resolves env → config file → built-in default.
    #[test]
    fn cfg_07_ollama_model_precedence() {
        use crate::domain::summary::{DEFAULT_MODEL, ollama_model};

        let cfg = parse_config("ollama_model = codellama");
        assert_eq!(
            ollama_model_choice(Some("mistral".into()), &cfg).as_deref(),
            Some("mistral")
        );
        assert_eq!(
            ollama_model_choice(None, &cfg).as_deref(),
            Some("codellama")
        );
        assert_eq!(ollama_model_choice(None, &Config::default()), None);

        // Wired through the model resolver: the file's value is used, and absence means the default.
        assert_eq!(ollama_model(ollama_model_choice(None, &cfg)), "codellama");
        assert_eq!(
            ollama_model(ollama_model_choice(None, &Config::default())),
            DEFAULT_MODEL
        );
    }

    // The shared precedence helper: first present, non-blank value wins; blanks fall through.
    #[test]
    fn first_configured_skips_absent_and_blank() {
        assert_eq!(
            first_configured([None, Some("  "), Some("delta"), Some("difft")]),
            Some("delta".to_string())
        );
        assert_eq!(first_configured::<_, &str>([None, None]), None);
        assert_eq!(first_configured([Some("  \n ")]), None);
    }
}
