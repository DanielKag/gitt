//! Effect dispatch: each side-effect runs on its own thread so the UI never blocks, and the result
//! is routed back into the reducer as an [`Event`].

use std::sync::mpsc::Sender;
use std::thread;

use crate::ports::{ColorMode, GitError, Ports};
use crate::state::{Effect, Event};

pub fn dispatch(effect: Effect, ports: &Ports, tx: &Sender<Event>) {
    match effect {
        Effect::LoadLog(view) => {
            let git = ports.git.clone();
            let limit = ports.log_limit;
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.log(view, limit) {
                    Ok(commits) => Event::LogLoaded { view, commits },
                    Err(e) => Event::LogFailed {
                        view,
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::LoadDiff(hash) => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.show(&hash, ColorMode::Never) {
                    Ok(text) => Event::DiffLoaded { hash, text },
                    Err(e) => Event::DiffFailed {
                        hash,
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::Fetch => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let result = git.fetch().map_err(|e| e.to_string());
                let _ = tx.send(Event::FetchFinished(result));
            });
        }
        Effect::Checkout(hash) => {
            let git = ports.git.clone();
            action(tx, "Checked out", move || git.checkout(&hash));
        }
        Effect::CopyToClipboard(text) => {
            let cb = ports.clipboard.clone();
            action(tx, "Copied to clipboard", move || cb.copy(&text));
        }
        Effect::OpenBrowser(url) => {
            let browser = ports.browser.clone();
            action(tx, "Opened in browser", move || browser.open(&url));
        }
        Effect::OpenPr(hash) => {
            let pr = ports.pr.clone();
            action(tx, "Opened PR", move || pr.open_pr(&hash));
        }
        Effect::Quit => {}
    }
}

/// Run a fire-and-report action on a thread, sending an `ActionFinished` event with `label`.
fn action<F>(tx: &Sender<Event>, label: &str, f: F)
where
    F: FnOnce() -> Result<(), GitError> + Send + 'static,
{
    let tx = tx.clone();
    let label = label.to_string();
    thread::spawn(move || {
        let result = f().map_err(|e| e.to_string());
        let _ = tx.send(Event::ActionFinished { label, result });
    });
}
