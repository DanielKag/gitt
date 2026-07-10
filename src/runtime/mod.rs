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

use crate::ports::Ports;
use crate::state::{AppState, Effect, Event, Load, update};
use crate::ui;
use terminal::TerminalGuard;

pub fn run(mut state: AppState, ports: Ports) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel::<Event>();

    if let Ok((w, h)) = crossterm::terminal::size() {
        state.size = (w, h);
        state.clamp_scroll();
    }

    input::spawn(tx.clone());

    // Kick off the initial log load for the current view.
    state.logs.insert(state.view, Load::Loading);
    workers::dispatch(Effect::LoadLog(state.view), &ports, &tx);

    guard.terminal.draw(|f| ui::draw(f, &state))?;

    while let Ok(event) = rx.recv() {
        let mut effects = update(&mut state, event);
        // Coalesce any already-queued events into this frame.
        while let Ok(ev) = rx.try_recv() {
            effects.extend(update(&mut state, ev));
        }
        for effect in effects {
            workers::dispatch(effect, &ports, &tx);
        }
        guard.terminal.draw(|f| ui::draw(f, &state))?;
        if state.should_quit {
            break;
        }
    }

    Ok(())
}
