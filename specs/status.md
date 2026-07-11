# gitt status — interactive working-tree status

- **ID prefix:** `STAT`
- **Status:** implemented — all criteria covered by unit + e2e tests; STAT-16 is a design invariant (see CLAUDE.md "one tool, one feel")
- **Command:** `gitt status`

## Summary

`gitt status` presents the working tree as an interactive, full-screen TUI: a flat list of the files
git would report in `git status`, each shown with a two-letter `XY` status badge (index + worktree,
e.g. `M `, ` M`, `MM`, `A `, `??`, ` D`). The user browses files with the same vim motions as
`gitt log`, stages/unstages a file with a single key, previews the diff of the selected file in a side
pane (reusing the log preview), and discards a file's changes behind a confirmation. It is the staging
half of a commit workflow (committing itself is a later feature). Every stage/unstage/discard re-reads
real git state so the view never drifts from reality. By design it **looks and behaves like
`gitt log`** — same theme, same list/overlay/help-bar components, same navigation, preview, and quit
keys — so the two commands feel like one tool.

## Acceptance criteria

| ID      | Criterion (testable statement)                                                                                                    | Tiers      |
| ------- | --------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| STAT-01 | `gitt status` renders a flat list of changed files, one per row: a two-letter `XY` status badge (index + worktree) followed by the path (e.g. `MM src/reducer.rs`, `A  src/status.rs`, `?? notes.txt`). | unit, e2e  |
| STAT-02 | A pure `parse_status` turns `git status --porcelain=v1 -z` output into typed `StatusEntry` values (index status, worktree status, untracked flag, and rename original path). | unit       |
| STAT-03 | Vim motions move the selection across the file list exactly as in `gitt log`: `j`/`k`/`↓`/`↑`, `g`/`G`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`. Selection never leaves bounds. | unit       |
| STAT-04 | `Space` toggles staging of the selected file: if it has worktree/untracked changes (`Y != ' '`) it is staged (`git add -- <path>`), otherwise (fully staged, `Y == ' '`) it is unstaged (`git restore --staged -- <path>`). Afterwards the list reloads and the badge flips. | unit, e2e  |
| STAT-05 | `s` always stages the selected file (`git add`) and `u` always unstages it (`git restore --staged`), regardless of its current badge. | unit       |
| STAT-06 | `Tab` toggles a diff-preview pane for the selected file: an untracked row shows the file's contents, a row with worktree changes (`Y != ' '`) shows the worktree diff (`git diff -- <path>`), and a fully-staged row shows the cached diff (`git diff --staged -- <path>`). Toggling again hides it. | unit, e2e  |
| STAT-07 | `d` (or **Discard changes** in the action menu) opens a confirmation overlay naming the file; confirming discards that file's changes — a tracked file is restored (`git restore -- <path>`), an untracked file is deleted from disk. The list then reloads. | unit, e2e  |
| STAT-08 | The confirmation overlay is mandatory for discard: `Esc` / `n` cancels leaving the file untouched; `Enter` / `y` confirms. No other key discards. | unit       |
| STAT-09 | `Enter` opens a per-file action menu (reusing the log action-menu overlay) with **Stage**/**Unstage** (label reflects the file's side), **Discard changes**, and **Copy path**. `Esc` closes it. | unit, e2e  |
| STAT-10 | After any stage/unstage/discard, the file list is reloaded from git so the view always reflects real repo state (no optimistic drift); the selection is clamped to remain valid. | unit       |
| STAT-11 | A file with **both** staged and unstaged changes shows a two-letter badge (e.g. `MM`); `Space`/`s` stages its worktree portion (badge → `M `), and the diff preview shows the worktree diff while worktree changes remain. | unit       |
| STAT-12 | `R` reloads the status from git. | unit       |
| STAT-13 | `q` / `Ctrl-c` quit the TUI cleanly, restoring the terminal. | unit, e2e  |
| STAT-14 | Running `gitt status` in a clean repo renders a "nothing to commit, working tree clean" empty state, not a panic. | unit, e2e  |
| STAT-15 | Running `gitt status` outside a git repository prints a clear error and exits non-zero (no panic, no TUI). | e2e        |
| STAT-16 | `gitt status` reuses the shared `theme`, the shared list / overlay / help-bar components, and the shared navigation + preview + quit keybindings, so it is visually and behaviorally consistent with `gitt log`. | (reference) |

## Keybindings / UX

| Key                | Mode    | Action                                                        |
| ------------------ | ------- | ------------------------------------------------------------- |
| `j` / `↓`          | List    | Move selection down                                           |
| `k` / `↑`          | List    | Move selection up                                             |
| `g` / `G`          | List    | Jump to first / last file                                     |
| `Ctrl-d`/`Ctrl-u`  | List    | Half page down / up                                           |
| `Ctrl-f`/`Ctrl-b`  | List    | Page down / up                                                |
| `Space`            | List    | Toggle staging of selected file (stage if dirty, else unstage)|
| `s` / `u`          | List    | Stage / unstage selected file (section-independent)           |
| `d`                | List    | Discard selected file's changes (opens confirmation)          |
| `Tab`              | List    | Toggle diff preview for the selected file                     |
| `R`                | List    | Reload status                                                 |
| `Enter`            | List    | Open action menu for the selected file                        |
| `j`/`k`, `Enter`   | Menu    | Navigate / confirm action                                     |
| `Enter` / `y`      | Confirm | Confirm discard                                               |
| `Esc` / `n`        | Confirm | Cancel discard                                                |
| `Esc`              | Menu    | Close menu                                                    |
| `q` / `Ctrl-c`     | any     | Quit                                                          |

_Keys shared verbatim with `gitt log`: `j`/`k`/`g`/`G`/`Ctrl-d`/`u`/`f`/`b` (navigation), `Tab`
(preview), `R` (reload), `Enter` (action menu), `Esc` (dismiss overlay), `q`/`Ctrl-c` (quit)._

## Ports (behind `GitRepo`)

- `status() -> Vec<StatusEntry>` — parsed `git status --porcelain=v1 -z` (parsing is the pure
  `parse_status`; the port only shells out and calls it, mirroring how `log` returns parsed commits).
- `diff_file(path, staged: bool) -> String` — `git diff [--staged] -- <path>` (plain text).
- `stage(path)` — `git add -- <path>` (stages modifications, additions, and deletions).
- `unstage(path)` — `git restore --staged -- <path>`.
- `discard(path, untracked: bool)` — tracked: `git restore -- <path>`; untracked: delete the file.

## Errors / edge cases

- Not a git repo → clear message on stderr, exit code ≠ 0, no alt-screen (same path as `gitt log`).
- Clean working tree → empty-state message, not a panic; stage/unstage/discard/preview are no-ops.
- Untracked file: `Space`/`s` runs `git add`; the diff preview shows the file's contents; `d` deletes it.
- Deleted-in-worktree file (` D`): staging stages the deletion; discard restores the file.
- Discard is destructive and therefore **always** gated by the confirmation overlay.
- Stage/unstage/discard failures surface as a transient status-line message; the list still reloads.

## Out of scope (for this POC)

- Partial / hunk-level or line-level staging (whole-file staging only).
- Creating a commit, amend, push (this feature only stages; committing is a separate spec).
- Fuzzy file filtering with `/` (the shared fuzzy component is available; deferred to keep scope tight).
- Multi-select / bulk stage-all.
- Syntax-highlighted / `delta`-colored diffs (plain text, exactly as `gitt log`'s preview).
