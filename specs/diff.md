# gitt diff — interactive scoped diff viewer

- **ID prefix:** `DIFF`
- **Status:** implemented — all criteria covered by unit + e2e tests; DIFF-14 is a design invariant (see CLAUDE.md "one tool, one feel")
- **Command:** `gitt diff`

## Summary

`gitt diff` presents the repository's changes as an interactive, full-screen diff **viewer** in the
shape of GitHub's pull-request "Files changed" tab: a list of changed files on one side, the diff of
the selected file in a pane beside it. The user browses files with the same vim motions as `gitt log`
and `gitt status`, and switches the **scope** of the diff with `←`/`→` — exactly the way `gitt log`
switches between its HEAD and origin views. Four scopes are offered:

| Scope (tab) | Meaning | git |
| ----------- | ------- | --- |
| **Unstaged** (default) | worktree ↔ index — what you haven't staged yet | `git diff` |
| **Staged** | index ↔ HEAD — what's staged for the next commit | `git diff --staged` |
| **Working** | worktree ↔ HEAD — everything uncommitted (staged **and** unstaged) | `git diff HEAD` |
| **vs `<main>`** | merge-base(`<main>`, HEAD)…HEAD — the GitHub-PR "Files changed" diff | `git diff <main>...HEAD` |

It is a **read-only** viewer: it never mutates the working tree or index (staging/unstaging/discarding
is what `gitt status` is for). By design it **looks and behaves like `gitt log`/`gitt status`** — same
theme, same list/overlay/preview/help-bar components, same navigation, preview toggle, and quit keys —
so all three commands feel like one tool. Each scope's file list loads once and is cached, so flicking
between scopes is instant.

## Acceptance criteria

| ID      | Criterion (testable statement)                                                                                                     | Tiers      |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| DIFF-01 | `gitt diff` renders a flat list of the changed files for the active scope (default **Unstaged**), one row per file: a one-letter change badge (`M`/`A`/`D`/`R`/`C`/`T`) followed by the path (a rename shows `old → new`). | unit, e2e  |
| DIFF-02 | A pure `parse_diff_name_status` turns `git diff --name-status -z` output into typed `DiffFile` values (change status char, path, and rename/copy original path). | unit       |
| DIFF-03 | Vim motions move the selection across the file list exactly as in `gitt log`/`gitt status`: `j`/`k`/`↓`/`↑`, `g`/`G`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`. Selection never leaves bounds. | unit       |
| DIFF-04 | `→` moves to the next scope and `←` to the previous, cycling through **Unstaged → Staged → Working → vs `<main>`** (wrapping at both ends); the header highlights the active scope tab. Each scope's file list loads once and is cached. | unit, e2e  |
| DIFF-05 | Each scope maps to the correct git diff: Unstaged = `git diff`, Staged = `git diff --staged`, Working = `git diff HEAD`, vs-main = `git diff <main>...HEAD` (three-dot / merge-base, matching GitHub's PR file-changes semantics). The vs-main base resolves to `origin/<main>` when it exists, else the local `<main>`. | unit, e2e  |
| DIFF-06 | The diff pane is **open by default** and shows the selected file's diff for the active scope (plain `git diff` text). `Tab` toggles it: hidden → the file list takes the full width; toggling again re-opens it. | unit, e2e  |
| DIFF-07 | Moving the selection reloads the diff pane for the newly selected file; switching scope reloads both the file list and (unconditionally) the diff pane, so the pane always reflects the active scope's diff for the current file. | unit       |
| DIFF-08 | `Enter` opens a per-file action menu (reusing the shared overlay) with **Copy path** and **Copy diff**; `Esc` closes it. | unit, e2e  |
| DIFF-09 | "Copy path" copies the selected file's path to the clipboard; "Copy diff" copies that file's diff text (for the active scope) to the clipboard. | unit, e2e  |
| DIFF-10 | `R` reloads the active scope's file list from git (and refreshes the diff pane when open).                                          | unit       |
| DIFF-11 | A scope with no changes renders a clear empty state (e.g. "no unstaged changes", "no changes vs main"), not a panic; navigation and actions are safe no-ops. | unit, e2e  |
| DIFF-12 | `q` / `Ctrl-c` quit the TUI cleanly, restoring the terminal.                                                                        | unit, e2e  |
| DIFF-13 | Running `gitt diff` outside a git repository prints a clear error and exits non-zero (no panic, no TUI).                            | e2e        |
| DIFF-14 | `gitt diff` reuses the shared `theme`, the shared list / overlay / preview / help-bar components, and the shared navigation + preview + quit keybindings, so it is visually and behaviorally consistent with `gitt log` and `gitt status`. | (reference) |

## Keybindings / UX

| Key                | Mode    | Action                                                        |
| ------------------ | ------- | ------------------------------------------------------------- |
| `j` / `↓`          | List    | Move selection down                                           |
| `k` / `↑`          | List    | Move selection up                                             |
| `g` / `G`          | List    | Jump to first / last file                                     |
| `Ctrl-d`/`Ctrl-u`  | List    | Half page down / up                                           |
| `Ctrl-f`/`Ctrl-b`  | List    | Page down / up                                                |
| `→` / `←`          | List    | Next / previous diff scope (wraps)                            |
| `Tab`              | List    | Toggle the diff pane                                          |
| `R`                | List    | Reload the active scope from git                              |
| `Enter`            | List    | Open the per-file action menu                                 |
| `j`/`k`, `Enter`   | Menu    | Navigate / confirm action                                     |
| `Esc`              | Menu    | Close menu                                                    |
| `Esc`              | List    | Quit (nothing open to dismiss)                                |
| `q` / `Ctrl-c`     | any     | Quit                                                          |

_Keys shared verbatim with `gitt log`/`gitt status`: `j`/`k`/`g`/`G`/`Ctrl-d`/`u`/`f`/`b`
(navigation), `←`/`→` (switch view/scope, as in `gitt log`), `Tab` (preview), `R` (reload), `Enter`
(action menu), `Esc` (dismiss the open overlay, or quit from the base list), `q`/`Ctrl-c` (quit)._

## Ports (behind `GitRepo`)

- `diff_files(scope) -> Vec<DiffFile>` — parsed `git diff --name-status -z <scope-args>` (parsing is
  the pure `parse_diff_name_status`; the port only shells out, resolves the vs-main base ref, and calls
  it — mirroring how `log`/`status` return already-parsed values).
- `diff_scope_file(scope, path) -> String` — the plain-text diff of one file for the scope
  (`git diff [--staged|HEAD|<base>...HEAD] --no-color -- <path>`).

## Errors / edge cases

- Not a git repo → clear message on stderr, exit code ≠ 0, no alt-screen (same path as `gitt log`).
- A scope with no changes → empty-state message, not a panic; motions/actions are no-ops.
- vs-main scope when `<main>` cannot be resolved (no such ref) → the load fails gracefully into the
  scope's empty/error state; the other scopes keep working.
- Diff/list load failures surface as a transient status-line message; the view still renders.
- Binary files: git reports them in `--name-status`; the per-file diff shows git's "Binary files
  differ" text as-is (no special handling needed).

## Out of scope (for this POC)

- **Vertical scrolling inside the diff pane.** Like `gitt log`/`gitt status`, the pane shows the diff
  from the top and clips; `j`/`k` pages between files. In-pane scrolling is a separate later spec.
- **Mutations.** No staging/unstaging/discarding here — that is `gitt status`. `gitt diff` is a viewer.
- Syntax-highlighted / `delta`-colored diffs (plain text, exactly as the other screens' previews).
- Fuzzy file filtering with `/` (the shared fuzzy component exists; deferred to keep scope tight).
- Hunk-/line-level navigation, comments, side-by-side (split) diffs, word-level intraline highlight.
- Configurable scope set / arbitrary `gitt diff <revA> <revB>` ref arguments.
