# gitt log — interactive fuzzy git log

- **ID prefix:** `LOG`
- **Status:** implemented (POC) — all criteria covered by unit + e2e tests; LOG-09 deferred, LOG-20 manual
- **Command:** `gitt log`

## Summary

`gitt log` presents an interactive, fuzzy-findable git log as a full-screen TUI. The user browses
commits with vim-like motions, filters them by typing a search query, previews the full diff of a
commit, toggles between the current branch and the remote main branch, and runs actions on a
selected commit (open on GitHub, open its PR via `gh`, copy the SHA, checkout, copy a revert
command). It ports the behavior of the `glogm` bash tool to a fast native implementation that feels
instant. Fuzzy matching is in-process (nucleo); there is no dependency on the `fzf` binary.

## Acceptance criteria

| ID     | Criterion (testable statement)                                                                                   | Tiers      |
| ------ | ---------------------------------------------------------------------------------------------------------------- | ---------- |
| LOG-01 | Running `gitt log` in a git repo renders a list of commits, one per line: short hash, relative date, author, subject, and ref decorations. | unit, e2e  |
| LOG-02 | Relative dates are computed from a `Clock`; e.g. a commit 3 days before "now" renders "3 days ago".              | unit       |
| LOG-03 | The log parser turns the pinned `--pretty=format` output into `Commit` values, splitting refs from `%D` into typed `Ref`s (HEAD, local, remote, tag). | unit       |
| LOG-04 | Pressing `/` enters search mode; typing appends to the filter; `Esc` leaves search mode but keeps the filter applied. | unit, e2e  |
| LOG-05 | With a filter active, only commits matching the query (fuzzy, smart-case) are shown, ordered by match score.     | unit, e2e  |
| LOG-06 | Vim motions move the selection: `j`/`k` (down/up), `g`/`G` (top/bottom), `Ctrl-d`/`Ctrl-u` (half page), `Ctrl-f`/`Ctrl-b` (page). Selection never leaves bounds. | unit       |
| LOG-07 | `→` switches to the `origin/<main>` view and `←` back to the current-branch (HEAD) view; each view's log loads once and is cached. | unit, e2e  |
| LOG-08 | `Tab` toggles a diff-preview pane showing `git show <hash>` (plain text) for the selected commit; toggling again hides it. | unit, e2e  |
| ~~LOG-09~~ | ~~Preview uses `delta` when available.~~ Deferred: an in-TUI pane can't render raw ANSI without an ANSI→spans parser; the POC shows plain `git show` text. See Out of scope. | —          |
| LOG-10 | `R` triggers a `git fetch` then reloads the current view (runs off the UI thread; UI stays responsive).          | unit       |
| LOG-11 | `Enter` opens an action menu for the selected commit with: Open in GitHub, Open PR, Copy SHA, Checkout, Copy revert command. `Esc` closes it. | unit, e2e  |
| LOG-12 | "Copy SHA" copies the **full** 40-char hash to the clipboard.                                                    | unit, e2e  |
| LOG-13 | "Checkout" checks out the selected commit; afterwards the repo `HEAD` points at that commit.                     | unit, e2e  |
| LOG-14 | "Open in GitHub" opens the browser at `https://github.com/<org>/<repo>/commit/<hash>`, normalizing SSH and HTTPS remotes. | unit       |
| LOG-15 | "Open PR" invokes `gh` for the commit (falling back to a `(#123)` reference parsed from the subject).            | unit       |
| LOG-16 | "Copy revert command" copies `git revert <hash>` to the clipboard.                                               | unit       |
| LOG-17 | Main-branch detection resolves in order: `origin/HEAD` symref → local cache file → `git remote show origin`.     | unit       |
| LOG-18 | `q` / `Ctrl-c` quit the TUI cleanly, restoring the terminal.                                                     | unit, e2e  |
| LOG-19 | Running `gitt log` outside a git repository prints a clear error and exits non-zero (no panic, no TUI).          | e2e        |
| LOG-20 | Rendering and keystroke handling are perceptibly instant; fuzzy matching runs off the UI thread and never blocks input. | (manual)   |

## Keybindings / UX

| Key             | Mode        | Action                                        |
| --------------- | ----------- | --------------------------------------------- |
| `j` / `↓`       | List        | Move selection down                           |
| `k` / `↑`       | List        | Move selection up                             |
| `g` / `G`       | List        | Jump to top / bottom                          |
| `Ctrl-d`/`Ctrl-u` | List      | Half page down / up                           |
| `Ctrl-f`/`Ctrl-b` | List      | Page down / up                                |
| `→` / `←`       | List        | Switch to origin-main view / back to HEAD     |
| `/`             | List        | Enter search mode                             |
| `<char>`        | Search      | Append to filter                              |
| `Backspace`     | Search      | Delete last filter char                       |
| `Esc`           | Search/Menu | Leave search (keep filter) / close menu       |
| `Tab`           | List        | Toggle diff preview                           |
| `R`             | List        | Fetch + reload current view                   |
| `Enter`         | List        | Open action menu for selected commit          |
| `j`/`k`,`Enter` | Menu        | Navigate / confirm action                     |
| `q` / `Ctrl-c`  | any         | Quit                                          |

## Errors / edge cases

- Not a git repo → clear message on stderr, exit code ≠ 0, no alt-screen.
- No `origin` remote → GitHub/PR actions report a friendly status; log still works on HEAD view.
- `gh` / `delta` missing → feature degrades gracefully (skip delta; PR action reports it), no crash.
- Empty repo (no commits) → renders an empty-state message, not a panic.

## Out of scope (for this POC)

- Syntax-highlighted / `delta`-colored diff preview (needs an ANSI→ratatui-spans parser). Preview is
  plain text for now. `Env::has_delta` plumbing is kept for when this lands.
- Per-character fuzzy match highlighting in the list (positions are tracked in state for later).
- Writing operations beyond checkout (rebase/cherry-pick/reset UI). Revert only copies a command.
- Multi-select. Configurable keybindings/theming. Graph (ASCII DAG) rendering.
