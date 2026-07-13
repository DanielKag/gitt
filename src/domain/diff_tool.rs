//! Which third-party renderer `gitt` pipes its diffs through, and the pure recipe for invoking it.
//!
//! `gitt` never renders diff colors itself: it shells out to a well-known tool and converts that
//! tool's ANSI output into styled spans (see `ui::ansi`). This module is the pure half — it names
//! the supported tools, parses the user's choice, and, given a pane width, produces the
//! [`RenderRecipe`] (program, args, env) the shell runs. No I/O here; the shell (`ports`) resolves
//! availability and actually spawns the process.

/// A supported diff renderer. `None` means "no tool" — show git's plain text (also the fallback
/// when the chosen tool isn't installed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffTool {
    /// difftastic (`difft`): structural, syntax-aware; a git external-diff engine.
    Difftastic,
    /// delta: pager over a unified diff.
    Delta,
    /// git-split-diffs: pager over a unified diff (GitHub-style split/unified).
    GitSplitDiffs,
    #[default]
    None,
}

/// How a tool consumes a diff — the only per-tool branch the shell must handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Reads a unified diff on **stdin** and writes colored output (delta, git-split-diffs).
    Pager,
    /// A git **external-diff** engine: the shell re-runs the `git diff` command with
    /// `GIT_EXTERNAL_DIFF` set, and the tool is invoked by git per file (difftastic).
    ExternalDiff,
    /// No tool — the shell returns git's plain diff unchanged.
    Plain,
}

/// The concrete, width-resolved instructions for rendering one diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRecipe {
    pub shape: Shape,
    /// The tool binary (for `Pager` it is spawned directly; for `ExternalDiff` it is the value of
    /// `GIT_EXTERNAL_DIFF`). Empty for `Plain`.
    pub program: String,
    /// Extra args for a `Pager` program (unused for `ExternalDiff`, which is configured via `env`).
    pub args: Vec<String>,
    /// Environment variables to set on the child process (width, force-color, `GIT_EXTERNAL_DIFF`).
    pub env: Vec<(String, String)>,
    /// For a `Pager`: whether the piped `git diff` input should itself be colored
    /// (`--color=always`) rather than `--no-color`. delta only emits color to a non-TTY pipe when
    /// its input is already colored; git-split-diffs wants plain input and colors it itself.
    pub color_input: bool,
}

/// A pane at least this wide (columns) is worth a side-by-side layout for tools we drive explicitly
/// (delta). Two ~40-col columns plus gutters need roughly this much; below it we render unified.
const SIDE_BY_SIDE_MIN: u16 = 100;

impl DiffTool {
    /// Parse the configured tool name (from `GITT_DIFF_TOOL` or `--diff-tool`). Unknown/empty →
    /// `None`. Case-insensitive; a couple of natural aliases are accepted.
    pub fn parse(s: &str) -> DiffTool {
        match s.trim().to_ascii_lowercase().as_str() {
            "difftastic" | "difft" | "dft" => DiffTool::Difftastic,
            "delta" => DiffTool::Delta,
            "git-split-diffs" | "split-diffs" | "gsd" => DiffTool::GitSplitDiffs,
            "none" | "plain" | "off" | "" => DiffTool::None,
            _ => DiffTool::None,
        }
    }

    /// The binary that must exist on `PATH` for this tool (used by the shell to auto-detect and to
    /// fall back to plain when missing). `None` for [`DiffTool::None`].
    pub fn binary(self) -> Option<&'static str> {
        match self {
            DiffTool::Difftastic => Some("difft"),
            DiffTool::Delta => Some("delta"),
            DiffTool::GitSplitDiffs => Some("git-split-diffs"),
            DiffTool::None => None,
        }
    }

    pub fn shape(self) -> Shape {
        match self {
            DiffTool::Difftastic => Shape::ExternalDiff,
            DiffTool::Delta | DiffTool::GitSplitDiffs => Shape::Pager,
            DiffTool::None => Shape::Plain,
        }
    }

    /// The tools in auto-detection preference order (native single-binary Rust tools first). The
    /// shell picks the first one whose [`binary`](Self::binary) is on `PATH`.
    pub const AUTODETECT_ORDER: [DiffTool; 3] = [
        DiffTool::Difftastic,
        DiffTool::Delta,
        DiffTool::GitSplitDiffs,
    ];

    /// Build the width-resolved recipe for rendering a diff into `width` columns.
    pub fn recipe(self, width: u16) -> RenderRecipe {
        let w = width.max(1).to_string();
        match self {
            // difftastic is driven via GIT_EXTERNAL_DIFF and configured through env. It defaults to
            // side-by-side and does NOT auto-collapse on a narrow width (it just wraps), so we pick
            // the display explicitly: unified (`inline`) when the pane is narrow. When wide we leave
            // DFT_DISPLAY unset so difft's default applies and a user's own `DFT_DISPLAY` override
            // (e.g. `side-by-side-show-both`) is still honored.
            DiffTool::Difftastic => {
                let mut env = vec![
                    ("GIT_EXTERNAL_DIFF".to_string(), "difft".to_string()),
                    ("DFT_WIDTH".to_string(), w.clone()),
                    ("DFT_COLOR".to_string(), "always".to_string()),
                ];
                if width < SIDE_BY_SIDE_MIN {
                    env.push(("DFT_DISPLAY".to_string(), "inline".to_string()));
                }
                RenderRecipe {
                    shape: Shape::ExternalDiff,
                    program: "difft".to_string(),
                    args: vec![],
                    env,
                    color_input: false,
                }
            }
            // delta reads a unified diff on stdin. It has no auto width-collapse, so we choose the
            // layout: side-by-side only when the pane is wide enough, else unified.
            DiffTool::Delta => {
                let mut args = vec![
                    "--paging=never".to_string(),
                    "--width".to_string(),
                    w.clone(),
                ];
                if width >= SIDE_BY_SIDE_MIN {
                    args.push("--side-by-side".to_string());
                }
                RenderRecipe {
                    shape: Shape::Pager,
                    program: "delta".to_string(),
                    args,
                    env: vec![("COLUMNS".to_string(), w.clone())],
                    // delta only colorizes a redirected (non-TTY) pipe when its input is colored.
                    color_input: true,
                }
            }
            // git-split-diffs reads a plain unified diff on stdin and colors it itself (`--color`);
            // it gets its width from `COLUMNS` (its terminal-size lib honors the env var even when
            // piped) and auto-collapses split→unified by the `split-diffs.min-line-width` git config.
            DiffTool::GitSplitDiffs => RenderRecipe {
                shape: Shape::Pager,
                program: "git-split-diffs".to_string(),
                args: vec!["--color".to_string()],
                env: vec![("COLUMNS".to_string(), w)],
                color_input: false,
            },
            DiffTool::None => RenderRecipe {
                shape: Shape::Plain,
                program: String::new(),
                args: vec![],
                env: vec![],
                color_input: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_names_and_aliases() {
        assert_eq!(DiffTool::parse("difftastic"), DiffTool::Difftastic);
        assert_eq!(DiffTool::parse("DIFFT"), DiffTool::Difftastic);
        assert_eq!(DiffTool::parse(" delta "), DiffTool::Delta);
        assert_eq!(DiffTool::parse("git-split-diffs"), DiffTool::GitSplitDiffs);
        assert_eq!(DiffTool::parse("gsd"), DiffTool::GitSplitDiffs);
        assert_eq!(DiffTool::parse("none"), DiffTool::None);
        assert_eq!(DiffTool::parse(""), DiffTool::None);
        assert_eq!(DiffTool::parse("whatever"), DiffTool::None);
    }

    #[test]
    fn difftastic_is_an_external_diff_engine() {
        let r = DiffTool::Difftastic.recipe(120);
        assert_eq!(r.shape, Shape::ExternalDiff);
        // Driven via git's external-diff hook, configured entirely through env.
        assert!(
            r.env
                .iter()
                .any(|(k, v)| k == "GIT_EXTERNAL_DIFF" && v == "difft")
        );
        assert!(r.env.iter().any(|(k, v)| k == "DFT_WIDTH" && v == "120"));
        // Wide: no forced display, so difft's side-by-side default (and any user DFT_DISPLAY) applies.
        assert!(!r.env.iter().any(|(k, _)| k == "DFT_DISPLAY"));
    }

    #[test]
    fn difftastic_forces_inline_when_narrow() {
        let r = DiffTool::Difftastic.recipe(60);
        assert!(
            r.env
                .iter()
                .any(|(k, v)| k == "DFT_DISPLAY" && v == "inline"),
            "a narrow pane should render unified (inline), not a cramped split"
        );
    }

    #[test]
    fn delta_is_a_pager_and_splits_only_when_wide() {
        let narrow = DiffTool::Delta.recipe(60);
        assert_eq!(narrow.shape, Shape::Pager);
        assert_eq!(narrow.program, "delta");
        assert!(
            !narrow.args.iter().any(|a| a == "--side-by-side"),
            "a narrow pane stays unified"
        );

        let wide = DiffTool::Delta.recipe(160);
        assert!(
            wide.args.iter().any(|a| a == "--side-by-side"),
            "a wide pane goes side-by-side"
        );
        // Width is pinned both as a flag and via COLUMNS (delta reads a non-TTY pipe).
        assert!(wide.args.iter().any(|a| a == "160"));
        assert!(wide.env.iter().any(|(k, v)| k == "COLUMNS" && v == "160"));
        // delta needs colored git input to emit color into a pipe.
        assert!(wide.color_input, "delta is fed --color=always input");
    }

    #[test]
    fn git_split_diffs_pipes_with_columns() {
        let r = DiffTool::GitSplitDiffs.recipe(90);
        assert_eq!(r.shape, Shape::Pager);
        assert_eq!(r.program, "git-split-diffs");
        assert!(r.env.iter().any(|(k, v)| k == "COLUMNS" && v == "90"));
        // git-split-diffs colors plain input itself, so it is fed --no-color.
        assert!(!r.color_input);
    }

    #[test]
    fn none_is_plain() {
        let r = DiffTool::None.recipe(80);
        assert_eq!(r.shape, Shape::Plain);
        assert!(r.program.is_empty());
        assert_eq!(DiffTool::None.binary(), None);
    }
}
