# gitt branch — interactive fuzzy branch switcher

- **ID prefix:** `BR`
- **Status:** draft
- **Command:** `gitt branch`

## Summary

`gitt branch` presents the repository's **local branches** as an interactive, fuzzy-findable list —
the same full-screen TUI shell as `gitt log`, `gitt status`, and `gitt diff`, so a user who knows one
already knows this one. The user browses branches with vim-like motions, filters them by typing a
search query, and runs actions on the selected branch: **checkout**, **open its pull request** (via
`gh`), **copy the branch name**, and **delete the branch** (behind the same confirmation overlay every
destructive action uses). A top-level `n` **creates a new branch** off the current `HEAD` and switches
to it. Like `gitt log`, a persistent panel below the list shows an **AI summary** of the selected
branch — generated locally by Ollama from the branch's overall diff against the main branch (plus its
commit subjects), cached on disk so each branch tip is only summarized once. The branch summary keys
the cache distinctly from commit summaries so the two never collide.

## Acceptance criteria

| ID     | Criterion (testable statement)                                                                                     | Tiers      |
| ------ | ------------------------------------------------------------------------------------------------------------------ | ---------- |
| BR-01  | Running `gitt branch` renders the local branches with the **current (checked-out) branch pinned first**, then the rest most-recently-committed first, one per line: a current-branch marker, the branch name (given a wide, spaced-out column), the PR-status column, and the tip commit's relative date. The current branch is marked and styled distinctly. There is **no** commit-subject column — the name gets the freed room. The screen has **no header/title row** — the search bar (which already shows the match count) is the top row, so the redundant branch-name title and branch count are omitted. | unit, e2e  |
| BR-02  | The branch list parser turns the pinned `for-each-ref --format` output into `Branch` values (current flag, name, tip SHA, upstream, tip timestamp/subject). | unit       |
| BR-03  | Pressing `/` enters search mode; typing filters the branches with the same exact substring-per-term (smart-case) matcher as `gitt log`, over the branch name, upstream, and tip subject; `Esc` keeps the filter. | unit, e2e  |
| BR-04  | Vim motions move the selection: `j`/`k`, `g`/`G`, `Ctrl-d`/`Ctrl-u`, `Ctrl-f`/`Ctrl-b`. Selection never leaves bounds. | unit       |
| BR-05  | `Enter` opens an action menu for the selected branch with: Checkout, Open PR, Copy name, Delete branch. `Esc` closes it. | unit, e2e  |
| BR-06  | "Checkout" checks out the selected branch. On a **successful** checkout (git exits 0) `gitt` **quits immediately** — like a native `git checkout` — leaving the `Checked out <branch>` line as its exit report, with `HEAD` now on that branch. A **failed** checkout keeps the screen open and shows the error on the status line in **dominant red**, carrying only git's own message (`Checkout failed: fatal: …`) — the invoked command and exit code are dropped as noise. | unit, e2e  |
| BR-07  | "Open PR" invokes `gh` for the selected branch (the branch name reaches the PR opener).                            | unit, e2e  |
| BR-08  | "Copy name" copies the selected branch name to the clipboard.                                                      | unit, e2e  |
| BR-09  | "Delete branch" is gated by the shared confirmation overlay; `y`/`Enter` deletes and the list reloads, `n`/`Esc` cancels. Deleting the **current** branch is refused with a friendly status (git can't delete the checked-out branch). | unit, e2e  |
| BR-10  | `n` opens a "new branch" input; typing a name and pressing `Enter` creates the branch off `HEAD`, switches to it, and reloads the list; `Esc` cancels; an empty name is a no-op. | unit, e2e  |
| BR-11  | `gitt branch` always renders a bordered "ai summary" panel below the list reflecting the **selected** branch. Selecting a branch auto-loads its summary from the on-disk cache (keyed by the branch tip SHA, prefixed so it never collides with a commit summary); a hit shows instantly, a miss shows the "press @" hint. | unit, e2e  |
| BR-12  | Pressing `@` generates the branch summary off the UI thread (streaming into the panel); `s` toggles the expanded footer — identical behavior and rendering to `gitt log`'s summary. All progress/results show in the panel; the status line keeps the keymap legend. | unit, e2e  |
| BR-13  | The branch-summary prompt is built by a **pure** function from a system instruction + the base branch name + the branch's commit subjects + its (size-bounded) diff against the base. | unit       |
| BR-14  | A branch with no commits ahead of the base short-circuits to a friendly "no changes" summary without calling the model. | unit       |
| BR-15  | `R` reloads the branch list; `q` / `Ctrl-c` quit cleanly; running outside a git repository prints a clear error and exits non-zero (no TUI). | unit, e2e  |
| BR-16  | Empty repo / a single branch / no selection make motions and actions safe no-ops (no panic).                      | unit       |
| BR-17  | A per-branch PR-status column is filled from a single background `gh` fetch (open/draft/merged/closed, coloured), which never blocks the first paint. Until it lands the column reads `loading…`; a branch with no PR shows a dim `—`. | unit, e2e  |
| BR-18  | `gitt branch` opens in a small **inline window** (a fixed 20-row viewport in the current terminal) rather than taking over the whole screen, leaving the surrounding scrollback intact. On exit it leaves a git-native footprint: the UI is erased, the last action is printed as a one-line report, and the shell prompt resumes on the next line (no lingering blank block). | unit, e2e  |
| BR-19  | A branch whose AI summary is cached shows a one-character AI marker (`✦`) in a leading column; branches without a cached summary show a blank of the same width so rows stay aligned. Shared with `gitt log`. | unit       |
| BR-20  | `o` ("**o**nly open") toggles a filter down to branches with an **open or draft** PR, plus `main`, the current branch, and any branch whose PR was closed during this session. While active the search bar shows an `[only open]` badge. `p` is **not** bound — a stale press is an inert no-op. | unit, e2e  |

## Keybindings / UX

| Key               | Mode        | Action                                        |
| ----------------- | ----------- | --------------------------------------------- |
| `j` / `↓`         | List        | Move selection down                           |
| `k` / `↑`         | List        | Move selection up                             |
| `g` / `G`         | List        | Jump to top / bottom                          |
| `Ctrl-d`/`Ctrl-u` | List        | Half page down / up                           |
| `Ctrl-f`/`Ctrl-b` | List        | Page down / up                                |
| `/`               | List        | Enter search mode                             |
| `<char>`          | Search      | Append to filter                              |
| `Backspace`       | Search      | Delete last filter char                       |
| `Esc`             | Search      | Leave search (keep filter)                    |
| `Esc`             | List        | Quit (nothing open to dismiss)                |
| `o`               | List        | Only open: filter to branches with an open PR |
| `n`               | List        | Create a new branch (opens the name input)    |
| `d`               | List        | Delete the selected branch (opens confirm)    |
| `@`               | List        | Generate (or regenerate) the branch's AI summary |
| `s`               | List        | Expand / minimize the summary footer          |
| `R`               | List        | Reload the branch list                        |
| `Enter`           | List        | Open the action menu for the selected branch  |
| `j`/`k`,`Enter`   | Menu        | Navigate / confirm action                     |
| `Esc`             | Menu        | Close the menu                                 |
| `y`/`Enter`,`n`/`Esc` | Confirm | Confirm / cancel the deletion                 |
| `<char>`,`Enter`,`Esc` | Create | Type the name / create / cancel               |
| `q` / `Ctrl-c`    | any         | Quit                                          |

`Esc` is the universal exit: it dismisses whatever overlay/search is open, and quits from the base
list — so pressing `Esc` repeatedly always walks you out (alongside `q`).

The layout mirrors `gitt log`, minus the header row: search bar · branch list · AI-summary footer ·
status line. The summary footer states are identical to `gitt log`'s (hint / generating / streaming / ready /
failed), and `s` expands it in place.

## Errors / edge cases

- Not a git repo → clear message on stderr, exit code ≠ 0, no alt-screen.
- No `origin` remote / no PR → the "Open PR" action reports `gh`'s failure on the status line; the rest
  of the screen still works.
- The PR-status column is populated by a single `gh pr list --author @me` call, scoped to the current
  user's PRs. This keeps it correct and fast in a busy monorepo (an unscoped newest-N list is drowned
  out by unrelated org/bot PRs and never reaches your local branches). A branch whose PR was opened by
  someone else therefore shows no status (`—`); a non-GitHub repo or missing `gh` leaves the column
  at `—` too. While the fetch is still in flight the column reads `loading…`.
- Deleting the current branch → refused with a status message (git itself can't).
- `gh` / `ollama` missing → the affected action degrades gracefully (status message / failed panel), no
  crash.
- A branch with no commits ahead of the base → the summary shows a friendly "no changes" note instead of
  an empty/garbage model call.
- The base the summary diffs against is `origin/<main>` when it exists, else the local `<main>` — the
  same base the `gitt diff` "vs main" scope uses.

## Test seams (e2e determinism)

Reuses the shared seams: `GITT_NOW` (relative dates), `GITT_TEST_SINK_DIR` (clipboard / pr sinks),
`GITT_FAKE_SUMMARY` / `GITT_FAKE_SUMMARY_ERROR` (summarizer), `GITT_CACHE_DIR` (summary cache).

## Out of scope

- Remote-tracking branches in the list (local branches only for now).
- Renaming branches, setting upstreams, or pushing from this screen.
- A raw diff-preview pane (the branch's "content" view is the AI summary; no `Tab` pane here).
- Multi-select; configurable keybindings/theming.
