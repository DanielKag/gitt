---
name: spec
description: Scaffold or update a gitt feature spec in specs/ from the template — captures summary, numbered acceptance criteria with stable IDs, keybindings, and edge cases. Use when starting a new gitt feature or revising an existing feature's behavior before writing code.
---

# /spec — author a gitt feature spec

Specs are the source of truth (see `specs/README.md`). Code and tests trace to criterion IDs.

## Steps

1. **Determine the feature name and ID prefix.** Ask the user if ambiguous. The prefix is a short
   uppercase area code (`LOG`, `DIFF`, `STATUS`, …). The file is `specs/<name>.md`.
2. **If the spec exists**, read it and update in place — do NOT renumber existing criterion IDs (they
   are stable and append-only). Add new rows with the next free number; strike removed rows.
3. **If new**, copy `specs/_template.md` and fill in:
   - **Summary** — what and why, one paragraph.
   - **Acceptance criteria** — a numbered table. Each row: stable ID (`<PREFIX>-NN`), a statement
     phrased so a test can assert it (observable behavior, not implementation), and the test
     tier(s): `unit`, `e2e`, or `(manual)` for things like "feels instant".
   - **Keybindings / UX**, **Errors / edge cases**, **Out of scope**.
4. **Every criterion names at least one tier.** If you can't state how it's tested, sharpen it until
   you can.
5. Add the spec to the index in `specs/README.md`.
6. Report the criterion IDs so the user can reference them. Do NOT write code — this skill only
   produces the spec. Suggest `/feature specs/<name>.md` to implement.
