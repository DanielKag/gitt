# gitt

### Pronounced **“Git-T”** — `git` plus the letter *T* (“git-tee”).

**An interactive git client for the terminal.**

`gitt` replaces the git commands you run twenty times a day with visible, navigable screens: fuzzy-find
a commit, stage a hunk-by-hunk mess, read a colorized diff, jump between branches — without
memorizing another flag. It's a single Rust binary, it starts instantly, and it leaves your terminal
exactly as a native git command would.

```
gitt log      # browse history, fuzzy search, preview diffs, checkout, copy SHA, open PR
gitt status   # stage / unstage / discard, then commit or amend
gitt diff     # read changes: unstaged, staged, working tree, or vs the main branch
gitt branch   # switch, create, delete branches; see each one's PR status
```

---

## Install

```bash
brew tap DanielKag/gitt
brew install gitt
```

Or build from source (Rust 1.85+, edition 2024):

```bash
cargo install --locked --git https://github.com/DanielKag/gitt
```

macOS only for now. Requires `git`; `gh` is optional (PR status and "open PR").

---

## The four screens

### `gitt log`

An interactive `git log` with the palette of [`glogm`](https://github.com/DanielKag/glogm): dim-cyan
hash, green relative date, blue author, then the subject. History loads **progressively** — the first
page paints immediately and the rest streams in behind it, so a monorepo feels the same as a toy repo.

Type `/` and search: every whitespace-separated term must appear as a literal substring (smart-case) of
the hash, author, subject, or branch decoration — and the matched characters are highlighted in place,
so you can see *why* a commit matched. `Enter` opens the action menu: open in GitHub, open the PR,
copy the full SHA, checkout, copy a revert command.

### `gitt status`

The working tree as a list. `Space` toggles staging for the file under the cursor, `s`/`u` stage and
unstage explicitly, `d` discards (behind a confirmation), `Tab` previews the file's diff. `c` commits
and `a` amends: `gitt` hands the commit off to your real terminal, so pre-commit hooks stream their
output live and a failure stays on screen where you can re-run it.

### `gitt diff`

A read-only diff browser over four scopes — unstaged, staged, whole working tree, and `main...HEAD`
(the PR view). `←`/`→` switches scope, `Tab` opens the pane, `f` expands it to most of the screen,
`Shift-j`/`Shift-k` scroll it.

### `gitt branch`

Your local branches, current one pinned first, each with the status of its PR (open / draft / merged /
closed) fetched in the background. `o` narrows the list to branches with an **o**pen PR — the ones you
actually still care about. `Enter` checks out, opens the PR, copies the name, or deletes. It renders in
a small inline window rather than taking over the screen, and on exit it erases itself and prints one
line saying what it did.

---

## Keys

A key means the same thing on every screen.

| Key | Action |
| --- | --- |
| `j` / `k` · `↓` / `↑` | Move the selection |
| `g` / `G` | Jump to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Half page down / up |
| `Ctrl-f` / `Ctrl-b` | Page down / up |
| `/` | Search (type to filter; `Esc` leaves search, keeps the filter) |
| `Tab` | Toggle the diff preview |
| `f` · `Shift-j` / `Shift-k` | Expand · scroll the diff pane |
| `←` / `→` | Switch view (log) or diff scope |
| `Enter` | Open the action menu for the selected row |
| `R` | Reload (fetch, in `gitt log`) |
| `Esc` | Dismiss whatever is open — and quit from the base list |
| `q` / `Ctrl-c` | Quit |

Screen-specific: `Space`/`s`/`u`/`d`/`c`/`a` in `status` · `o`/`n`/`d` in `branch` · `@` and `s` for AI
summaries.

---

## Colorized diffs

`gitt` renders diffs through whichever third-party differ you already like, and falls back to plain
`git` output when none is installed:

```bash
gitt diff --diff-tool difftastic     # or delta, git-split-diffs, none
export GITT_DIFF_TOOL=delta          # or set it once
```

## AI commit & branch summaries (optional)

Press `@` on a commit (`gitt log`) or a branch (`gitt branch`) for a plain-English summary of what it
changed. It runs **locally** through [Ollama](https://ollama.com) — nothing leaves your machine — and
is cached on disk by SHA, so a summary you've seen once appears instantly. `s` expands the panel.

Without Ollama running, the panel says so and everything else keeps working.

| Variable | Default | Purpose |
| --- | --- | --- |
| `GITT_OLLAMA_MODEL` | `qwen3-coder:30b` | Model used for summaries |
| `OLLAMA_HOST` | `http://127.0.0.1:11434` | Where Ollama listens |
| `GITT_CACHE_DIR` | `$XDG_CACHE_HOME/gitt/summaries` | Summary cache location |
| `GITT_DIFF_TOOL` | auto-detected | Diff renderer |

---

## Contributing

`gitt` is spec-driven and test-driven. Every user-facing behavior is a numbered criterion in
[`specs/`](./specs) (e.g. `LOG-25`, `BR-20`) and is covered by tests that name it. Start there.

The architecture is one rule — **functional core, imperative shell**. All logic is pure and
deterministic (`domain/`, `parse/`, `state/`, `ui/`); every side effect lives behind a trait in
`ports/` and is executed by `runtime/`. The reducer takes an event and returns state plus `Effect`s;
it never touches git, the clock, or the terminal. That's what makes the whole thing testable:

```bash
cargo test --lib                 # unit: parsers, reducers, rendering snapshots
cargo test                       # + e2e: the real binary, a real repo, over a PTY
cargo insta review               # review rendering snapshot changes
cargo clippy -- -D warnings && cargo fmt
```

E2E tests spawn the compiled binary against a throwaway git repo in a pseudo-terminal, drive real
keystrokes, and assert on both the rendered grid and the real side effects. They never sleep and never
touch your machine's git config.

See [CLAUDE.md](./CLAUDE.md) for the full working agreement, and [RELEASING.md](./RELEASING.md) for how
a release is cut.

## License

MIT — see [LICENSE](./LICENSE).
