---
name: feature
description: Run the gitt TDD loop for a feature spec — write failing unit + e2e tests that encode each acceptance criterion, then implement against the ports until green, then review. Use when implementing or extending a gitt feature that already has a spec in specs/.
---

# /feature — implement a spec, test-first

Argument: a spec path (e.g. `specs/log.md`). If none given, ask which spec.

Follow the architecture rule in `CLAUDE.md`: **logic in the pure core (`domain`/`parse`/`state`/`ui`/
`fuzzy`), I/O only behind `ports` traits, reducer does no I/O.**

## Loop

1. **Read the spec.** List the acceptance criteria and their tiers. Pick the next unimplemented
   criteria (smallest coherent slice).
2. **Write failing tests FIRST**, naming them after criterion IDs (e.g. `log_05_filter_orders_by_score`):
   - Unit tests colocated in the target module's `#[cfg(test)]`. For reducer behavior, drive
     `update()` with a `Vec<Event>` and assert resulting state + emitted `Effect`s. For rendering,
     use `ratatui::TestBackend` + `insta`. Inject **fake** ports (`FakeGit`, `FixedClock`, …).
   - If the criterion is user-facing (`e2e` tier), add a scenario in `tests/e2e_log.rs` using the
     `tui_tester` + `fixture` harness (real binary, real temp repo, PTY). Obey the determinism rules
     in `CLAUDE.md`.
3. **Run them; confirm they fail for the right reason** (`cargo test`).
4. **Implement** the minimum in the pure core plus a thin real port impl to go green.
5. **Green:** `cargo test`. Review new snapshots: `cargo insta review`.
6. **Refactor** with tests green. Then `cargo fmt` and `cargo clippy -- -D warnings` (zero warnings).
7. **Verify & review:** run `/verify` to drive the real binary for the changed behavior, then
   `/code-review` on the diff. Fix findings.
8. Repeat for remaining criteria. When all of a criterion's tiers are green, it's done; update the
   spec **Status** if the whole feature is complete.

Never mark work done with failing/ignored tests or clippy warnings. If blocked, say so with the
failing output.
