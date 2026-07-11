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
use crate::state::{AppState, DiffLoad, DiffState, Effect, Event, Load, StatusLoad, StatusState};
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
        self.logs.insert(self.view, Load::Loading);
        vec![Effect::LoadLog(self.view)]
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

/// Drive any [`Screen`] to completion: terminal setup, the event loop, and effect dispatch.
pub fn run<S: Screen>(mut screen: S, ports: Ports) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel::<Event>();

    if let Ok((w, h)) = crossterm::terminal::size() {
        // Uniform path: seed the initial size through the reducer like any other event.
        let _ = screen.update(Event::Resize(w, h));
    }

    input::spawn(tx.clone());

    for effect in screen.init_effects() {
        workers::dispatch(effect, &ports, &tx);
    }

    guard.terminal.draw(|f| screen.draw(f))?;

    while let Ok(event) = rx.recv() {
        let mut effects = screen.update(event);
        // Coalesce any already-queued events into this frame.
        while let Ok(ev) = rx.try_recv() {
            effects.extend(screen.update(ev));
        }
        for effect in effects {
            workers::dispatch(effect, &ports, &tx);
        }
        guard.terminal.draw(|f| screen.draw(f))?;
        if screen.should_quit() {
            break;
        }
    }

    Ok(())
}
