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
        Effect::LoadStatus => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.status() {
                    Ok(entries) => Event::StatusLoaded(entries),
                    Err(e) => Event::StatusFailed(e.to_string()),
                };
                let _ = tx.send(ev);
            });
        }
        Effect::LoadFileDiff { path, kind } => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.file_diff(&path, kind) {
                    Ok(text) => Event::FileDiffLoaded { path, text },
                    Err(e) => Event::FileDiffFailed {
                        path,
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::Stage(path) => {
            let git = ports.git.clone();
            mutation(tx, "Staged", move || git.stage(&path));
        }
        Effect::Unstage(path) => {
            let git = ports.git.clone();
            mutation(tx, "Unstaged", move || git.unstage(&path));
        }
        Effect::Discard { path, untracked } => {
            let git = ports.git.clone();
            mutation(tx, "Discarded", move || git.discard(&path, untracked));
        }
        Effect::LoadDiffFiles(scope) => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.diff_files(scope) {
                    Ok(files) => Event::DiffFilesLoaded { scope, files },
                    Err(e) => Event::DiffFilesFailed {
                        scope,
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::LoadDiffText { scope, path } => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.diff_scope_file(scope, &path) {
                    Ok(text) => Event::DiffTextLoaded { scope, path, text },
                    Err(e) => Event::DiffTextFailed {
                        scope,
                        path,
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::CopyScopeDiff { scope, path } => {
            let git = ports.git.clone();
            let cb = ports.clipboard.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let result = git
                    .diff_scope_file(scope, &path)
                    .and_then(|text| cb.copy(&text))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Event::ActionFinished {
                    label: "Copied diff".to_string(),
                    result,
                });
            });
        }
        Effect::Quit => {}
    }
}

/// Run a working-tree mutation on a thread, reporting via `StatusMutated` so the status view reloads.
fn mutation<F>(tx: &Sender<Event>, label: &str, f: F)
where
    F: FnOnce() -> Result<(), GitError> + Send + 'static,
{
    let tx = tx.clone();
    let label = label.to_string();
    thread::spawn(move || {
        let result = f().map_err(|e| e.to_string());
        let _ = tx.send(Event::StatusMutated { label, result });
    });
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
