//! Effect dispatch: each side-effect runs on its own thread so the UI never blocks, and the result
//! is routed back into the reducer as an [`Event`].

use std::sync::mpsc::Sender;
use std::thread;

use crate::domain::summary::{build_branch_prompt, build_prompt, strip_preamble};
use crate::ports::{ColorMode, GitError, GitRepo, Ports, Summarizer, SummaryCache};
use crate::state::{Effect, Event};

pub fn dispatch(effect: Effect, ports: &Ports, tx: &Sender<Event>) {
    match effect {
        Effect::LoadLogPage {
            view,
            skip,
            limit,
            epoch,
        } => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.log_page(view, skip, limit) {
                    Ok(commits) => Event::LogBatch {
                        view,
                        skip,
                        epoch,
                        commits,
                    },
                    Err(e) => Event::LogPageFailed {
                        view,
                        epoch,
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
                let ev = match git.show(&hash, ColorMode::Never, false) {
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
        Effect::LoadSummary { hash } => {
            let cache = ports.summary_cache.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match cache.get(&hash) {
                    Some(text) => Event::SummaryLoaded { hash, text },
                    None => Event::SummaryMissing { hash },
                };
                let _ = tx.send(ev);
            });
        }
        Effect::PrefetchSummaries(keys) => {
            // One background thread reads all the keys from the on-disk cache and reports the hits in
            // a single event, so first paint can show the AI marker on every summarized entry.
            let cache = ports.summary_cache.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let hits: Vec<(String, String)> = keys
                    .into_iter()
                    .filter_map(|key| cache.get(&key).map(|text| (key, text)))
                    .collect();
                if !hits.is_empty() {
                    let _ = tx.send(Event::SummariesPrefetched(hits));
                }
            });
        }
        Effect::GenerateSummary { hash, subject } => {
            let git = ports.git.clone();
            let summarizer = ports.summarizer.clone();
            let cache = ports.summary_cache.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                generate_summary(&*git, &*summarizer, &*cache, &hash, &subject, &tx)
            });
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
        Effect::LoadBranches => {
            let git = ports.git.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match git.branches() {
                    Ok(branches) => Event::BranchesLoaded(branches),
                    Err(e) => Event::BranchesFailed(e.to_string()),
                };
                let _ = tx.send(ev);
            });
        }
        Effect::LoadPrStatuses => {
            let pr = ports.pr.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let ev = match pr.statuses() {
                    Ok(map) => Event::PrStatusesLoaded(map),
                    Err(e) => Event::PrStatusesFailed(e.to_string()),
                };
                let _ = tx.send(ev);
            });
        }
        Effect::CheckoutBranch(name) => {
            let git = ports.git.clone();
            branch_mutation(tx, "Checked out", move || git.checkout(&name));
        }
        Effect::CreateBranch(name) => {
            let git = ports.git.clone();
            branch_mutation(tx, "Created branch", move || git.create_branch(&name));
        }
        Effect::DeleteBranch(name) => {
            let git = ports.git.clone();
            branch_mutation(tx, "Deleted branch", move || git.delete_branch(&name));
        }
        Effect::GenerateBranchSummary { key, branch, base } => {
            let git = ports.git.clone();
            let summarizer = ports.summarizer.clone();
            let cache = ports.summary_cache.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                generate_branch_summary(&*git, &*summarizer, &*cache, &key, &branch, &base, &tx)
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

/// Fetch the commit's diff, build the prompt, stream the summary from the model (emitting a
/// `SummaryChunk` per token so the panel fills in live), then cache the full result and emit
/// `SummaryReady`. Any failure — including an empty completion — emits `SummaryFailed`.
fn generate_summary(
    git: &dyn GitRepo,
    summarizer: &dyn Summarizer,
    cache: &dyn SummaryCache,
    hash: &str,
    subject: &str,
    tx: &Sender<Event>,
) {
    // Ignore whitespace-only churn: less noise for the model, fewer prompt tokens (faster).
    let diff = match git.show(hash, ColorMode::Never, true) {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(Event::SummaryFailed {
                hash: hash.to_string(),
                error: e.to_string(),
            });
            return;
        }
    };
    let prompt = build_prompt(subject, &diff);

    let mut acc = String::new();
    let result = summarizer.summarize(&prompt, &mut |tok| {
        acc.push_str(tok);
        let _ = tx.send(Event::SummaryChunk {
            hash: hash.to_string(),
            delta: tok.to_string(),
        });
    });

    let event = match result {
        // An empty completion is a failure, not a cached empty file (which `get` reads back as a
        // miss, so the summary would silently never persist).
        Ok(()) if acc.trim().is_empty() => Event::SummaryFailed {
            hash: hash.to_string(),
            error: "ollama returned an empty response".to_string(),
        },
        Ok(()) => {
            let summary = strip_preamble(acc.trim());
            // Cache write is best-effort: a failure here must not lose the summary for this run.
            let _ = cache.put(hash, &summary);
            Event::SummaryReady {
                hash: hash.to_string(),
                text: summary,
            }
        }
        Err(e) => Event::SummaryFailed {
            hash: hash.to_string(),
            error: e.to_string(),
        },
    };
    let _ = tx.send(event);
}

/// Compute the branch's diff-vs-base and commit subjects, build the branch prompt, stream the
/// summary (emitting a `SummaryChunk` per token, keyed by the branch's cache `key`), then cache the
/// result and emit `SummaryReady`. A branch with no changes ahead of the base short-circuits to a
/// friendly note without calling the model (BR-14). Any failure emits `SummaryFailed`.
fn generate_branch_summary(
    git: &dyn GitRepo,
    summarizer: &dyn Summarizer,
    cache: &dyn SummaryCache,
    key: &str,
    branch: &str,
    base: &str,
    tx: &Sender<Event>,
) {
    let fail = |error: String| Event::SummaryFailed {
        hash: key.to_string(),
        error,
    };

    let subjects = match git.branch_commit_subjects(branch) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(fail(e.to_string()));
            return;
        }
    };
    let diff = match git.branch_diff(branch) {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(fail(e.to_string()));
            return;
        }
    };

    // BR-14: nothing ahead of the base → a friendly note, cached, no model call.
    if diff.trim().is_empty() && subjects.is_empty() {
        let text = format!("No changes relative to `{base}`.");
        let _ = cache.put(key, &text);
        let _ = tx.send(Event::SummaryReady {
            hash: key.to_string(),
            text,
        });
        return;
    }

    let prompt = build_branch_prompt(base, &subjects, &diff);

    let mut acc = String::new();
    let result = summarizer.summarize(&prompt, &mut |tok| {
        acc.push_str(tok);
        let _ = tx.send(Event::SummaryChunk {
            hash: key.to_string(),
            delta: tok.to_string(),
        });
    });

    let event = match result {
        Ok(()) if acc.trim().is_empty() => fail("ollama returned an empty response".to_string()),
        Ok(()) => {
            let summary = strip_preamble(acc.trim());
            let _ = cache.put(key, &summary);
            Event::SummaryReady {
                hash: key.to_string(),
                text: summary,
            }
        }
        Err(e) => fail(e.to_string()),
    };
    let _ = tx.send(event);
}

/// Run a branch mutation (checkout/create/delete) on a thread, reporting via `BranchMutated` so the
/// branch view reloads afterward (mirrors `mutation` for the status screen).
fn branch_mutation<F>(tx: &Sender<Event>, label: &str, f: F)
where
    F: FnOnce() -> Result<(), GitError> + Send + 'static,
{
    let tx = tx.clone();
    let label = label.to_string();
    thread::spawn(move || {
        let result = f().map_err(|e| e.to_string());
        let _ = tx.send(Event::BranchMutated { label, result });
    });
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
