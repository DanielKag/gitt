//! Input thread: block on terminal events and forward them into the reducer's channel.

use std::sync::mpsc::Sender;
use std::thread;

use crossterm::event::{self, Event as CtEvent, KeyEventKind};

use crate::state::Event;

/// Spawn the input reader. It exits when the channel receiver is dropped.
pub fn spawn(tx: Sender<Event>) {
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            let mapped = match event {
                // Ignore key-release/repeat (Windows) — act only on presses.
                CtEvent::Key(key) if key.kind == KeyEventKind::Press => Some(Event::Key(key)),
                CtEvent::Resize(w, h) => Some(Event::Resize(w, h)),
                _ => None,
            };
            if let Some(ev) = mapped
                && tx.send(ev).is_err()
            {
                break;
            }
        }
    });
}
