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
    guard.finish_inline(screen.exit_report())?;

    Ok(())
}
