# gitt log — interactive fuzzy git log

- **ID prefix:** `LOG`
- **Status:** implemented — LOG-01..18, 21..30 covered by unit + e2e tests; LOG-20 manual
- **Command:** `gitt log`

## Summary

`gitt log` presents an interactive, fuzzy-findable git log as a full-screen TUI. The user browses
commits with vim-like motions, filters them by typing a search query, previews the full diff of a
commit, toggles between the current branch and the remote main branch, and runs actions on a
selected commit (open on GitHub, open its PR via `gh`, copy the SHA, checkout, copy a revert
command). It ports the behavior of the `glogm` bash tool to a fast native implementation that feels
instant. Matching is in-process and exact (substring-per-term, fzf `'exact` semantics); there is no
dependency on the `fzf` binary.

## Acceptance criteria

| ID     | Criterion (testable statement)                                                                                   | Tiers      |
| ------ | ---------------------------------------------------------------------------------------------------------------- | ---------- |
| LOG-01 | Running `gitt log` in a git repo renders a list of commits, one per line: short hash, relative date, author, subject, and ref decorations. | unit, e2e  |
| LOG-02 | Relative dates are computed from a `Clock`; e.g. a commit 3 days before "now" renders "3 days ago".              | unit       |
| LOG-03 | The log parser turns the pinned `--pretty=format` output into `Commit` values, splitting refs from `%D` into typed `Ref`s (HEAD, local, remote, tag). | unit       |
| LOG-04 | Pressing `/` enters search mode; typing appends to the filter; `Esc` leaves search mode but keeps the filter applied. | unit, e2e  |
| LOG-05 | With a filter active, only commits are shown where **every** whitespace-separated term appears as a literal substring (smart-case) of the commit's searchable text (hash, author, subject, refs); matches keep reverse-chronological order. A term is not a fuzzy subsequence — `dankag` matches `daniel kagan` only when typed as `dan kag`, and never matches `daniel nagn`. | unit, e2e  |
| LOG-06 | Vim motions move the selection: `j`/`k` (down/up), `g`/`G` (top/bottom), `Ctrl-d`/`Ctrl-u` (half page), `Ctrl-f`/`Ctrl-b` (page). Selection never leaves bounds. | unit       |
| LOG-07 | `→` switches to the `origin/<main>` view and `←` back to the current-branch (HEAD) view; each view's log loads once and is cached. | unit, e2e  |
| LOG-08 | `Tab` toggles a diff-preview pane showing `git show <hash>` for the selected commit; toggling again hides it. The pane is colorized via the shared configurable diff tool and behaves like `gitt diff`'s: it sits **below** the list (full width), `f` expands it to 90% height, and `Shift+j`/`Shift+k` (`Shift+↓`/`↑`) scroll it. See `specs/diff.md` DIFF-15..19. | unit, e2e  |
| LOG-09 | Preview is colorized through a configurable third-party diff tool (`difftastic`/`delta`/`git-split-diffs`), converted to styled spans by the shared `ui::ansi` parser; plain `git show` text when no tool is set/installed. Shared with `gitt diff`/`gitt status`. See `specs/diff.md` DIFF-15..17. | unit |
| LOG-10 | `R` triggers a `git fetch` then reloads the current view (runs off the UI thread; UI stays responsive).          | unit       |
| LOG-11 | `Enter` opens an action menu for the selected commit with: Open in GitHub, Open PR, Copy SHA, Checkout, Copy revert command. `Esc` closes it. | unit, e2e  |
| LOG-12 | "Copy SHA" copies the **full** 40-char hash to the clipboard.                                                    | unit, e2e  |
| LOG-13 | "Checkout" checks out the selected commit; afterwards the repo `HEAD` points at that commit.                     | unit, e2e  |
| LOG-14 | "Open in GitHub" opens the browser at `https://github.com/<org>/<repo>/commit/<hash>`, normalizing SSH and HTTPS remotes. | unit       |
| LOG-15 | "Open PR" opens the commit's PR via `gh pr view … --web`. Since `gh` can't resolve a bare commit SHA to a PR, it targets the `(#N)` number parsed from the subject (GitHub's squash-merge convention) when present, else falls back to the SHA. On failure, `gh`'s own error is surfaced on the status line. | unit       |
| LOG-16 | "Copy revert command" copies `git revert <hash>` to the clipboard.                                               | unit       |
| LOG-17 | Main-branch detection resolves in order: `origin/HEAD` symref → local cache file → `git remote show origin`.     | unit       |
| LOG-18 | `q` / `Ctrl-c` quit the TUI cleanly, restoring the terminal.                                                     | unit, e2e  |
| LOG-19 | Running `gitt log` outside a git repository prints a clear error and exits non-zero (no panic, no TUI).          | e2e        |
| LOG-20 | Rendering and keystroke handling are perceptibly instant; fuzzy matching runs off the UI thread and never blocks input. | (manual)   |
| LOG-21 | The log loads **progressively**: the first batch renders immediately (instant first paint) and the remaining history is loaded in the background, appended to the same view newest-first, without re-fetching earlier batches. | unit, e2e  |
| LOG-22 | While a background load is in flight the status line shows progress (count loaded so far); the indicator clears once the full history is loaded. | unit       |
| LOG-23 | Search operates over every commit loaded so far and re-filters as new batches arrive; appending a batch preserves the selected commit (cursor does not jump). | unit       |
| LOG-24 | `--max-count N` caps total commits loaded (`0` = unlimited); the default is unlimited via progressive loading. Batches stop once the cap is reached or history is exhausted. | unit       |
| LOG-25 | While a filter is active, the substrings that matched are visually highlighted in-place on each shown row (across hash, author, subject, and ref fields), so the user sees *why* a commit matched. | unit       |
| LOG-26 | `Tab` toggles the diff preview in **search mode** too (not only in list mode), so the user can peek at a diff without leaving the search they're typing. | unit, e2e  |
| LOG-27 | When an overlay (action menu / confirmation) is open, the base screen behind it is dimmed so the modal stands out. Shared by every screen's overlays (`gitt log`/`status`/`diff`/`branch`). | unit       |
| LOG-28 | **Tags are never rendered** in the commit list — in a repo with release tags they crowd out the subject and say nothing about the branch you're on. `HEAD`/local/remote decorations are still shown; a commit decorated *only* by tags gets no decoration group at all (no empty `()`). Tags remain **searchable** (they stay in the commit's haystack), so `/v1.0` still finds the tagged commit. | unit, e2e |
| LOG-29 | Every commit column is padded to a minimum width **and** followed by an explicit separator space, so a value that overflows its column never runs into the next one — `%h` abbreviates to 10+ characters in a large repo and must still be separated from the relative date. | unit      |
| LOG-30 | The commit row uses `glogm`'s palette one-for-one (bold-dim-cyan hash · green relative date · bold-blue author · default subject) and decorations carry git's own `%C(auto)` colour per kind: `HEAD` cyan, local branch green, remote-tracking red, tag yellow. | unit      |

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
| `Esc`           | List        | Quit (nothing open to dismiss)                |
| `Tab`           | List/Search | Toggle diff preview                           |
| `f`             | List        | Expand the diff pane to 90% height            |
| `Shift-j`/`Shift-k` | List    | Scroll the diff pane down / up                 |
| `@`             | List        | Generate (or regenerate) the commit's AI summary |
| `s`             | List        | Expand / minimize the summary footer           |
| `R`             | List        | Fetch + reload current view (restarts progressive load) |
| `Enter`         | List        | Open action menu for selected commit          |
| `j`/`k`,`Enter` | Menu        | Navigate / confirm action                     |
| `q` / `Ctrl-c`  | any         | Quit                                          |

## Errors / edge cases

- Not a git repo → clear message on stderr, exit code ≠ 0, no alt-screen.
- No `origin` remote → GitHub/PR actions report a friendly status; log still works on HEAD view.
- `gh` missing → PR action reports it, no crash. Diff tool missing → preview falls back to plain text.
- Empty repo (no commits) → renders an empty-state message, not a panic.
- Very large history (20k–200k commits) → first paint stays instant; background batches fill in the
  rest so search reaches the full history within a moment (LOG-21..24). A batch load failure after
  the first paint reports on the status line and stops paging without discarding loaded commits (for
  a background/non-current view the partial commits are kept and paging stops silently).
- Search with no match while still streaming → the empty-state reads "…yet — still loading…", not a
  definitive "No matches", since the match may live in a page that hasn't arrived.
- Known limitation: pages are separate `git log --skip=N` invocations, so if `HEAD`/`origin` moves
  between pages the boundary commit can be duplicated or missed until the next reload (`R`). Rare in
  the seconds-long load window; acceptable for the POC. A future fix pins the tip SHA at load start.

## Out of scope (for this POC)

- (Delivered) Syntax-highlighted / tool-colored diff preview — now implemented via the shared
  configurable diff tool + `ui::ansi` (see `specs/diff.md` DIFF-15..17).
- Fuzzy (subsequence) matching. Search is exact substring-per-term (fzf `'exact` semantics, LOG-05);
  matched substrings are highlighted (LOG-25).
- Writing operations beyond checkout (rebase/cherry-pick/reset UI). Revert only copies a command.
- Multi-select. Configurable keybindings/theming. Graph (ASCII DAG) rendering.
