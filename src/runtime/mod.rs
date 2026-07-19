//! The imperative shell: terminal setup, the event loop, and effect dispatch.
//!
//! A keystroke goes Key -> `update` -> `draw` synchronously (pure functions + one buffer diff);
//! every expensive effect runs on a worker thread and streams its result back as an `Event`. Events
//! are drained before each draw so bursts coalesce into a single frame.

pub mod input;
pub mod terminal;
pub mod workers;

use std::sync::mpsc;

use anyhow::Result;
use ratatui::Frame;

use crate::ports::Ports;
use crate::state::{
    AppState, BranchLoad, BranchState, DiffLoad, DiffState, Effect, Event, StatusLoad, StatusState,
};
use crate::ui;
use terminal::TerminalGuard;

/// A full-screen interactive screen (`gitt log`, `gitt status`, …). Each screen is a pure state
/// machine: fold events into state, return effects, render from state. The runtime below is
/// identical for every screen, which is what keeps the whole tool feeling the same.
pub trait Screen {
    /// Fold an event into the state, returning side effects for the shell to perform.
    fn update(&mut self, event: Event) -> Vec<Effect>;
    /// Render the current state into the frame.
    fn draw(&self, frame: &mut Frame);
    /// Whether the app should exit.
    fn should_quit(&self) -> bool;
    /// The effect(s) to kick off once, at startup (e.g. the initial load).
    fn init_effects(&mut self) -> Vec<Effect>;
    /// Fixed height (rows) of a small inline window to run in, or `None` for fullscreen. Most screens
    /// take the whole terminal; `gitt branch` opts into a compact inline picker.
    fn viewport_height(&self) -> Option<u16> {
        None
    }
    /// A one-line summary of what happened, printed to the terminal as the inline screen exits (so an
    /// interactive command leaves a git-native trace, e.g. "Checked out wip-parser"). `None` prints
    /// nothing (a clean prompt). Only consulted for inline screens.
    fn exit_report(&self) -> Option<String> {
        None
    }
    /// A command to run in the **restored** terminal after the screen exits, with inherited stdio so
    /// its progress is visible and it's re-runnable — used for `gitt status`'s commit, which hands off
    /// to `git commit` in the shell so pre-commit hooks stream live. `None` = exit normally.
    fn exit_command(&self) -> Option<ExitCommand> {
        None
    }
}

/// A command gitt runs after tearing down the TUI, in the plain terminal (see [`Screen::exit_command`]).
pub struct ExitCommand {
    pub program: String,
    pub args: Vec<String>,
    /// A copy-pasteable, shell-quoted line printed before the command runs.
    pub display: String,
}

impl Screen for AppState {
    fn update(&mut self, event: Event) -> Vec<Effect> {
        crate::state::update(self, event)
    }
    fn draw(&self, frame: &mut Frame) {
        ui::draw(frame, self);
    }
    fn should_quit(&self) -> bool {
        self.should_quit
    }
    fn init_effects(&mut self) -> Vec<Effect> {
        vec![crate::state::reducer::start_log_load(self, self.view)]
    }
}

/// Height of the small inline window `gitt status` runs in.
const STATUS_VIEWPORT_ROWS: u16 = 20;

impl Screen for StatusState {
    fn update(&mut self, event: Event) -> Vec<Effect> {
        crate::state::update_status(self, event)
    }
    fn draw(&self, frame: &mut Frame) {
        ui::draw_status(frame, self);
    }
    fn should_quit(&self) -> bool {
        self.should_quit
    }
    fn init_effects(&mut self) -> Vec<Effect> {
        self.load = StatusLoad::Loading;
        vec![Effect::LoadStatus]
    }
    fn viewport_height(&self) -> Option<u16> {
        Some(STATUS_VIEWPORT_ROWS)
    }
    fn exit_report(&self) -> Option<String> {
        self.status.clone()
    }
    fn exit_command(&self) -> Option<ExitCommand> {
        // On Enter-to-commit the reducer records a `PendingCommit` and quits; we run the actual
        // `git commit` here, in the restored terminal, so hooks stream live and a failure is visible.
        self.pending_commit.as_ref().map(|pc| {
            let inv = crate::domain::commit_command(&pc.message, pc.amend);
            ExitCommand {
                program: "git".to_string(),
                args: inv.args,
                display: inv.display,
            }
        })
    }
}

impl Screen for DiffState {
    fn update(&mut self, event: Event) -> Vec<Effect> {
        crate::state::update_diff(self, event)
    }
    fn draw(&self, frame: &mut Frame) {
        ui::draw_diff(frame, self);
    }
    fn should_quit(&self) -> bool {
        self.should_quit
    }
    fn init_effects(&mut self) -> Vec<Effect> {
        self.loads.insert(self.scope, DiffLoad::Loading);
        vec![Effect::LoadDiffFiles(self.scope)]
    }
}

impl Screen for BranchState {
    fn update(&mut self, event: Event) -> Vec<Effect> {
        crate::state::update_branch(self, event)
    }
    fn draw(&self, frame: &mut Frame) {
        ui::draw_branch(frame, self);
    }
    fn should_quit(&self) -> bool {
        self.should_quit
    }
    fn init_effects(&mut self) -> Vec<Effect> {
        self.load = BranchLoad::Loading;
        // Fetch the branch list and the PR statuses in parallel; neither blocks the first paint.
        vec![Effect::LoadBranches, Effect::LoadPrStatuses]
    }
    fn viewport_height(&self) -> Option<u16> {
        // The branch switcher runs as a small inline window rather than taking over the terminal.
        Some(BRANCH_VIEWPORT_ROWS)
    }
    fn exit_report(&self) -> Option<String> {
        // Leave a git-native trace of the last action (checkout/create/delete/…), if any.
        self.status.clone()
    }
}

/// Height of the small inline window `gitt branch` runs in.
const BRANCH_VIEWPORT_ROWS: u16 = 20;

/// Drive any [`Screen`] to completion: terminal setup, the event loop, and effect dispatch.
pub fn run<S: Screen>(mut screen: S, ports: Ports) -> Result<()> {
    let inline_height = screen.viewport_height();
    let mut guard = match inline_height {
        Some(h) => TerminalGuard::enter_inline(h)?,
        None => TerminalGuard::enter()?,
    };
    let (tx, rx) = mpsc::channel::<Event>();

    if let Ok((w, h)) = crossterm::terminal::size() {
        // Uniform path: seed the initial size through the reducer like any other event. In an inline
        // window the drawable area is only the viewport height, so clamp to it.
        let h = inline_height.map_or(h, |vh| vh.min(h));
        let _ = screen.update(Event::Resize(w, h));
    }

    input::spawn(tx.clone());

    for effect in screen.init_effects() {
        workers::dispatch(effect, &ports, &tx);
    }

    guard.terminal.draw(|f| screen.draw(f))?;

    // In an inline window, a resize must be clamped to the viewport height (the drawable area).
    let clamp = |ev: Event| match (ev, inline_height) {
        (Event::Resize(w, h), Some(vh)) => Event::Resize(w, vh.min(h)),
        (ev, _) => ev,
    };

    while let Ok(event) = rx.recv() {
        let mut effects = screen.update(clamp(event));
        // Coalesce any already-queued events into this frame.
        while let Ok(ev) = rx.try_recv() {
            effects.extend(screen.update(clamp(ev)));
        }
        for effect in effects {
            workers::dispatch(effect, &ports, &tx);
        }
        guard.terminal.draw(|f| screen.draw(f))?;
        if screen.should_quit() {
            break;
        }
    }

    // Leave a clean, git-native footprint for an inline screen (erase the UI, print a one-line
    // report, drop to a fresh prompt). Fullscreen screens restore themselves via the alternate screen.
    let exit_cmd = screen.exit_command();
    guard.finish_inline(screen.exit_report())?;
    // Restore the terminal (leave raw mode / the alternate screen) BEFORE running any deferred
    // command, so its output lands on the normal terminal, not the torn-down TUI.
    drop(guard);

    if let Some(cmd) = exit_cmd {
        run_exit_command(cmd);
    }

    Ok(())
}

/// Run a deferred [`ExitCommand`] in the (now restored) terminal: echo the command, run it with
/// inherited stdio so its output — including pre-commit hook progress — streams live, then exit with
/// its status code so a failing commit is reflected in `$?` and the user can simply re-run the printed
/// line. Diverges (never returns) — it's the last thing gitt does.
fn run_exit_command(cmd: ExitCommand) -> ! {
    use std::io::Write;
    use std::process::Command;

    println!("$ {}", cmd.display);
    let _ = std::io::stdout().flush();

    let code = Command::new(&cmd.program)
        .args(&cmd.args)
        .status()
        .map(|s| s.code().unwrap_or(1))
        .unwrap_or_else(|e| {
            eprintln!("gitt: failed to run {}: {e}", cmd.program);
            1
        });
    std::process::exit(code);
}
