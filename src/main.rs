//! `gitt` — an AI-first git TUI client. This binary wires the real ports and launches the TUI.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use gitt::ports::git_cli::{self, RealGit};
use gitt::ports::system::{RealBrowser, RealClipboard, RealClock, RealPr};
use gitt::ports::{Clock, Ports};
use gitt::runtime;
use gitt::state::{AppState, DiffState, StatusState};

#[derive(Parser)]
#[command(name = "gitt", version, about = "Git-ty — an interactive git TUI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Browse the git log interactively with fuzzy search.
    Log {
        /// Maximum number of commits to load per view.
        #[arg(long, default_value_t = 5000)]
        max_count: usize,
    },
    /// Stage, unstage, diff, and discard working-tree changes interactively.
    Status,
    /// Browse changes interactively: unstaged, staged, working-tree, or vs the main branch.
    Diff,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Log { max_count } => run_log(max_count),
        Command::Status => run_status(),
        Command::Diff => run_diff(),
    }
}

fn run_log(max_count: usize) -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let clock = RealClock;
    let now = clock.now_unix();
    let main_branch = git_cli::detect_main_branch(&dir);
    let current_branch = git_cli::current_branch(&dir);
    let remote_url = git_cli::remote_url(&dir);

    let state = AppState::new(current_branch, main_branch.clone(), remote_url);

    let ports = Ports {
        git: Arc::new(RealGit::new(dir, main_branch, now)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        log_limit: max_count,
    };

    runtime::run(state, ports)
}

fn run_status() -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let current_branch = git_cli::current_branch(&dir);
    let state = StatusState::new(current_branch);

    // Status needs neither main-branch detection nor a clock (no relative dates), so those are
    // placeholders; the same `Ports`/`RealGit` seam is reused so effect dispatch is identical.
    let ports = Ports {
        git: Arc::new(RealGit::new(dir, "main".to_string(), 0)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        log_limit: 0,
    };

    runtime::run(state, ports)
}

fn run_diff() -> Result<()> {
    let dir = std::env::current_dir().context("cannot determine current directory")?;

    if !git_cli::is_git_repo(&dir) {
        bail!("not a git repository (or any of the parent directories)");
    }

    let main_branch = git_cli::detect_main_branch(&dir);
    let state = DiffState::new(main_branch.clone());

    // The diff viewer is read-only and shows no relative dates, so the clock is a placeholder; the
    // same `Ports`/`RealGit` seam is reused so effect dispatch is identical to the other screens.
    let ports = Ports {
        git: Arc::new(RealGit::new(dir, main_branch, 0)),
        clipboard: Arc::new(RealClipboard),
        browser: Arc::new(RealBrowser),
        pr: Arc::new(RealPr),
        log_limit: 0,
    };

    runtime::run(state, ports)
}
