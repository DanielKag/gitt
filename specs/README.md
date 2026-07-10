# Specs

Specs are the **source of truth** for `gitt`. Code and tests trace back to a spec. No user-facing
behavior ships without a spec entry and tests referencing its criterion IDs.

## Format

Each spec is one Markdown file (`specs/<feature>.md`) created from [`_template.md`](./_template.md):

- **Status** — `draft` → `in-progress` → `implemented` → `stable`.
- **Summary** — one paragraph: what and why.
- **Acceptance criteria** — a numbered table. Each row has a stable ID (`<AREA>-NN`, e.g. `LOG-07`),
  a testable statement, and the test tier(s) that cover it (`unit`, `e2e`).
- **Keybindings / UX** — concrete key → action mapping where relevant.
- **Out of scope** — explicitly deferred behavior.

## Rules

- Criterion IDs are **stable and append-only**. Never renumber; mark removed rows `~~struck~~`.
- Every criterion must name at least one test tier. A criterion with no test is not "done".
- Reference IDs in test names/comments (e.g. `fn log_07_view_toggle_loads_origin()`), so coverage is
  auditable.

## Index

- [`log.md`](./log.md) — `gitt log`: interactive fuzzy git log (POC / feature #1).
