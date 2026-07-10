---
name: e2e
description: Build the gitt binary and run the PTY-driven end-to-end test suite that exercises the real compiled binary against a throwaway git repo, then summarize failures. Use to validate that gitt actually works as a system, or to debug a flaky/failing e2e scenario.
---

# /e2e — run the end-to-end suite

E2E tests spawn the **real** `gitt` binary against a **real** temp git repo over a PTY and assert on
the rendered grid and real side effects. Harness: `tests/common/tui_tester.rs`; fixtures:
`tests/common/fixture.rs`.

## Steps

1. Build first so the binary is current: `cargo build`.
2. Run the suite: `cargo test --test e2e_log -- --nocapture`.
3. **On failure**, before touching product code, check the usual determinism culprits (see
   `CLAUDE.md` → "E2E determinism rules"):
   - A fixed `sleep` where a `wait_for(needle)` belongs → race. Replace it.
   - Leaked developer git config (missing `GIT_CONFIG_GLOBAL=/dev/null` / `HOME` override).
   - Non-pinned `GITT_NOW` or `delta` enabled (set `GITT_NO_DELTA=1`).
   - Viewport not 24×80, or asserting on color while running `--color=never`.
   To see what the app actually rendered, print `tui.screen()` at the failing step.
4. Only after ruling out harness/nondeterminism, treat it as a real product bug and fix the core.
5. Summarize: which criterion IDs passed/failed and the concrete cause of each failure.
