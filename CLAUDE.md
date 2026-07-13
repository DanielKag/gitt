# gitt — AI-first git TUI client

`gitt` (pronounced "Git-ty") is a terminal UI git client that replaces native git operations with
interactive, visible flows. It is **AI-first, spec-driven, and TDD-driven**: every feature begins as
a spec in `specs/`, is covered by unit **and** e2e tests, and is developed and tested entirely by AI.

**Stack:** Rust (edition 2024) · `ratatui` + `crossterm` (TUI) · `nucleo` (fuzzy matcher) · `clap`.

---

## The one architectural rule: Functional Core, Imperative Shell

All logic lives in a **pure core**. All side effects live in a thin **shell** behind traits.

- **Pure core** — `domain/`, `parse/`, `state/`, `ui/`, `fuzzy/` ranking. No I/O. No `std::process`,
  no terminal, no clock, no network, no filesystem. Deterministic in, deterministic out.
- **Shell** — `ports/` (real trait impls) and `runtime/` (event loop, threads, terminal). The
  **only** place `git`/`gh`/clipboard/browser/terminal are touched.
- **The reducer never does I/O.** `state::reducer::update(&mut AppState, Event) -> Vec<Effect>`
  mutates state and returns `Effect`s. The shell executes effects and feeds results back as `Event`s.

If you're about to call `std::process::Command`, `SystemTime::now()`, or touch the terminal from
anywhere outside `ports/` or `runtime/`, stop — it belongs behind a trait in `ports/mod.rs`.

### Injection seams (`src/ports/mod.rs`)
`GitRepo`, `Clock`, `Clipboard`, `Browser`, `PrOpener`, `Env`. Tests inject fakes; `ports/*.rs`
provide the real impls. `GitRepo::log` returns already-**parsed** `Commit`s (parsing is a separate
pure function tested against fixtures). `Clock` feeds relative dates so they're deterministic.

---

## The second rule: one tool, one feel

Every `gitt` subcommand must feel like it was built by the same team to the same standard — a user who
learns `gitt log` already knows `gitt status`. New features **reuse**, never re-invent:

- **Colors & styles** come from `ui/theme.rs` — the single source of truth. Never hardcode a `Color`
  or `Style` in a screen. Need a new semantic style? Add a named function to `theme` and use it
  everywhere that role appears.
- **Components** are shared. The commit/file list, the centered overlay (menu / confirmation), the
  header/tabs, the search bar, and the help/status line are common widgets — factor them into shared
  `ui` helpers rather than copying a screen's rendering. A second screen that needs a list uses the
  same list renderer.
- **Keybindings are a shared vocabulary.** A key means the same thing everywhere: `j`/`k`/`g`/`G`/
  `Ctrl-d`/`u`/`f`/`b` navigate, `Tab` toggles preview, `Enter` opens the action menu, `Esc` dismisses
  the open overlay/search and quits from the base list (so repeated `Esc` is always the way out),
  `R` reloads, `/` searches, `q`/`Ctrl-c` quit. Don't repurpose a key for something a user
  wouldn't expect from its meaning in another screen; if a screen adds a key, prefer one that's free
  everywhere.
- **Interaction patterns match.** Destructive actions are always gated by the same confirmation
  overlay; long/expensive work runs off the UI thread as an `Effect` and reports via the status line;
  the view reflects real state after a mutation rather than optimistically guessing.
- **Clean, git-native footprint.** Every command must leave the terminal exactly as a native git
  command would: run, do its thing, report what happened, and return the cursor to a fresh prompt on
  the next line — **never** a lingering blank block, half-erased UI, or a scrambled prompt. A
  fullscreen screen uses the alternate screen (so it restores the pre-launch terminal on exit, leaving
  no trace). A **small inline screen** (`Viewport::Inline`, e.g. `gitt branch`) must, on exit, erase
  its own drawing from the viewport's top row down, print a one-line report of the last action (the
  `Screen::exit_report`), and drop to a new line for the shell prompt — see
  `runtime::terminal::TerminalGuard::finish_inline`. If you add an interactive command, verify this on
  the **real binary** (an alt-screen leftover or an inline blank block is a bug), not just in tests.

When you add a screen, the diff should be mostly new *behavior* (reducer + a thin port), not new
*chrome*. If you find yourself writing a second color palette, a second list widget, or a second
meaning for `Tab`, stop — lift the existing one into a shared spot instead.

---

## TDD loop (do this for every change)

1. **Spec first.** Write/update the feature spec in `specs/` (use `/spec`). Each acceptance
   criterion gets an ID (e.g. `LOG-07`).
2. **Failing tests first.** Write the unit tests (and e2e scenario, if user-facing) that encode the
   criteria. Run them; watch them fail for the right reason.
3. **Implement** the minimum in the pure core + a thin port impl to go green.
4. **Green.** `cargo test` passes. Review snapshots with `cargo insta review`.
5. **Refactor** with tests green. Then `cargo clippy -- -D warnings` and `cargo fmt`.
6. **Verify & review.** `/verify` (drive the real binary) and `/code-review` the diff.

`/feature <spec>` runs this loop for you.

---

## Test tiers

- **Unit** — colocated `#[cfg(test)]`. Cover parsing, `relative_time`/`url`/`main_branch`, fuzzy
  ranking (sync `nucleo::Matcher`), the **reducer** (events in → state + effects out), and rendering
  via `ratatui::TestBackend` + `insta` snapshots. No terminal, no real git.
- **E2E (`tests/`)** — spawn the **real compiled binary** (`env!("CARGO_BIN_EXE_gitt")`) against a
  **real throwaway git repo** over a PTY (`portable-pty` + `vt100`), drive keystrokes, assert on the
  rendered grid **and real side effects** (checkout actually moved `HEAD`; SHA landed in the sink).
  Harness: `tests/common/tui_tester.rs`; fixtures: `tests/common/fixture.rs`.

### E2E determinism rules (non-negotiable — flaky e2e is worse than none)
- Fixed **24×80** viewport. `--color=never` / `NO_COLOR=1`.
- Isolate git: `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_SYSTEM=/dev/null`, `HOME`/`XDG_CONFIG_HOME`
  → tempdir. Pin author/committer name + `GIT_*_DATE` in fixtures (reproducible SHAs & dates).
- Pin "now" via `GITT_NOW=<unix>` (read by `RealClock`). Force plain preview via `GITT_NO_DELTA=1`.
- **Never `sleep`.** Synchronize with `wait_for(needle)` that polls the vt100 grid until it appears.
- Capture side effects via `GITT_TEST_SINK_DIR`: `RealClipboard`/`Browser`/`PrOpener` write their
  payload to `clipboard.txt`/`browser.txt`/`pr.txt` there instead of doing the real OS action.

---

## Commands

```bash
cargo test                 # unit + e2e
cargo test --lib           # unit only (fast)
cargo test --test e2e_log  # e2e only
cargo insta review         # review/accept snapshot changes  (cargo install cargo-insta)
cargo clippy -- -D warnings
cargo fmt
cargo run -- log           # run the app
```

---

## Starting a session

When the user says **"add a feature"** (or names one):
1. `/spec <name>` → write `specs/<name>.md` from `specs/_template.md` (ask for the name/scope if
   unclear; give each criterion a stable `<AREA>-NN` id).
2. `/feature specs/<name>.md` → run the TDD loop below (or the `implement-spec` Workflow for a
   fanned-out, adversarially-verified run — only when the user opts into orchestration).
3. Keep logic in the pure core; add I/O only behind a `ports` trait.

When the user says **"fix a bug"** (or describes one):
1. Reproduce it with a **failing test first** — a reducer/parse/fuzzy unit test if it's core logic,
   or an `tests/e2e_log.rs` scenario if it's user-visible. This is the regression guard.
2. Find the root cause in the pure core (that's where behavior lives); fix it there, not in the shell.
3. Green + `cargo clippy -- -D warnings` + `cargo fmt`, then `/code-review` the diff.
4. If the bug reveals the spec was wrong or silent, update `specs/log.md` (or the relevant spec).

Either way: unit + e2e must be green, clippy clean, and every user-facing change traces to a spec
criterion id. Run a **release** build to sanity-check performance (`cargo run --release -- log`) —
the debug build is not representative of the "instant" target.

## Log format contract
`git log --color=never --pretty=format:'%H%x1f%h%x1f%ct%x1f%an%x1f%s%x1f%D%x1e'`
`%x1f` = field separator (US), `%x1e` = record separator (RS), `%ct` = committer unix time.
`parse_log(raw, now)` is the single pure parser; do not parse git output anywhere else.
