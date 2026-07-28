//! `gitt` — an AI-first git TUI client. This binary wires the real ports and launches the TUI.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use gitt::domain::DiffTool;
use gitt::ports::git_cli::{self, RealGit};
use gitt::ports::system::{
    RealBrowser, RealClipboard, RealClock, RealPr, RealSummarizer, RealSummaryCache,
    resolve_diff_tool,
};
use gitt::ports::{Clock, Ports};
use gitt::runtime;
use gitt::state::{AppState, BranchState, DiffState, StatusState};

#[derive(Parser)]
#[command(
    name = "gitt",
    version,
    about = "gitt (\"git-tee\") — an interactive git client for the terminal"
)]
struct Cli {
    /// Third-party renderer for colorized diff previews: `difftastic`, `delta`, `git-split-diffs`,
    /// or `none`. Defaults to `$GITT_DIFF_TOOL`, else the first one installed on PATH.
    #[arg(long, global = true)]
    diff_tool: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Browse the git log interactively with fuzzy search.
    Log {
        /// Cap the total commits loaded per view (0 = unlimited). The log loads progressively — the
        /// first page paints instantly and the rest stream in — so the default is unlimited.
        #[arg(long, default_value_t = 0)]
        max_count: usize,
    },
    /// Stage, unstage, diff, and discard working-tree changes interactively.
    Status,
    /// Browse changes interactively: unstaged, staged, working-tree, or vs the main branch.
    Diff,
    /// Browse local branches with fuzzy search: checkout, open PR, delete, create, AI-summarize.
    Branch,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let diff_tool = resolve_diff_tool(cli.diff_tool.as_deref());
    match cli.command {
        Command::Log { max_count } => run_log(max_count, diff_tool),
        Command::Status => run_status(diff_tool),
        Command::Diff => run_diff(diff_tool),
        Command::Branch => run_branch(diff_tool),
    }
}

fn run_log(max_count: usize, diff_tool: DiffTool) -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let clock = RealClock;
    let now = clock.now_unix();
    let main_branch = git_cli::detect_main_branch(&dir);
    let current_branch = git_cli::current_branch(&dir);
    let remote_url = git_cli::remote_url(&dir);

    let mut state = AppState::new(current_branch, main_branch.clone(), remote_url);
    state.max_count = max_count;
    // Test seam: shrink the page size so e2e can exercise multi-page streaming on a tiny repo.
    if let Some(page) = std::env::var("GITT_LOG_PAGE")
        .ok()
        .and_then(|v| v.parse().ok())
        && page > 0
    {
        state.log_page = page;
    }

    let ports = Ports {
        git: Arc::new(RealGit::new(dir, main_branch, now, diff_tool)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        summarizer: Arc::new(RealSummarizer),
        summary_cache: Arc::new(RealSummaryCache),
    };

    runtime::run(state, ports)
}

fn run_status(diff_tool: DiffTool) -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let current_branch = git_cli::current_branch(&dir);
    let state = StatusState::new(current_branch);

    // Status needs neither main-branch detection nor a clock (no relative dates), so those are
    // placeholders; the same `Ports`/`RealGit` seam is reused so effect dispatch is identical.
    let ports = Ports {
        git: Arc::new(RealGit::new(dir, "main".to_string(), 0, diff_tool)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        summarizer: Arc::new(RealSummarizer),
        summary_cache: Arc::new(RealSummaryCache),
    };

    runtime::run(state, ports)
}

fn run_diff(diff_tool: DiffTool) -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let main_branch = git_cli::detect_main_branch(&dir);
    let state = DiffState::new(main_branch.clone());

    // The diff viewer is read-only and shows no relative dates, so the clock is a placeholder; the
    // same `Ports`/`RealGit` seam is reused so effect dispatch is identical to the other screens.
    let ports = Ports {
        git: Arc::new(RealGit::new(dir, main_branch, 0, diff_tool)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        summarizer: Arc::new(RealSummarizer),
        summary_cache: Arc::new(RealSummaryCache),
    };

    runtime::run(state, ports)
}

fn run_branch(diff_tool: DiffTool) -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let clock = RealClock;
    let now = clock.now_unix();
    let main_branch = git_cli::detect_main_branch(&dir);
    let current_branch = git_cli::current_branch(&dir);

    let state = BranchState::new(current_branch, main_branch.clone());

    let ports = Ports {
        git: Arc::new(RealGit::new(dir, main_branch, now, diff_tool)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        summarizer: Arc::new(RealSummarizer),
        summary_cache: Arc::new(RealSummaryCache),
    };

    runtime::run(state, ports)
}
