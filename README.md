# gitt (“git-tee”)

**An interactive git client for the terminal.**

`gitt` replaces the git commands you run twenty times a day with visible, navigable screens: fuzzy-find
a commit, stage what you meant to stage, read a colorized diff, jump between branches — without
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
brew tap danielkag/gitt
brew trust danielkag/gitt   # Homebrew 6+ requires this for any third-party tap
brew install gitt
```

Or build from source (Rust 1.85+, edition 2024):

```bash
cargo install --locked --git https://github.com/danielkag/gitt
```

macOS only for now. Requires `git`; `gh` is optional (PR status and "open PR").

---

## The four screens

Every screen prints its own keymap along the bottom, so you never have to come back here to use one.

### `gitt log`

Finding the commit you're thinking of. Type a few fragments — part of a message, a teammate's name,
half a SHA — and the list narrows as you type, showing you which characters matched. From there you can
read its diff, check it out, grab the full SHA, or open its PR. History streams in progressively, so a
monorepo opens as fast as a toy repo. Colors follow [`glogm`](https://github.com/danielkag/glogm).

![gitt log](docs/images/log.png)

### `gitt status`

Deciding what goes into this commit. Your working tree as one list, each file's diff a keypress away,
staging and unstaging as you review. When you commit or amend, `gitt` hands off to your real terminal —
so pre-commit hooks stream their output live and a failure stays on screen where you can re-run it.

![gitt status](docs/images/status.png)

### `gitt diff`

Reading changes before you commit or push. Four scopes you can flip between: unstaged, staged, the whole
working tree, and `main...HEAD` — the last one being what your reviewer will see.

![gitt diff](docs/images/diff.png)

### `gitt branch`

Getting back to the branch you were on. Your local branches with the current one pinned first, each
showing the state of its PR (open / draft / merged / closed), so you can filter down to the work that's
still live. Opens as a small inline window instead of taking over the screen, and on exit it erases
itself and prints one line saying what it did.

![gitt branch](docs/images/branch.png)

---

## Keys

A key means the same thing on every screen.

| Key | Action |
| --- | --- |
| `j` / `k` · `↓` / `↑` | Move the selection |
| `/` | Search (type to filter; `Esc` leaves search, keeps the filter) |
| `Enter` | Open the action menu for the selected row |
| `Tab` | Toggle the diff preview |
| `Esc` | Dismiss whatever is open — and quit from the base list |
| `q` | Quit |
| `←` / `→` | Switch view (log) or diff scope |
| `g` / `G` | Jump to top / bottom |
| `R` | Reload (fetch, in `gitt log`) |
| `f` · `Shift-j` / `Shift-k` | Expand · scroll the diff pane |

---

## Configuration

Everything works out of the box. When you want to pin something, drop a `~/.gitt`:

```ini
# ~/.gitt
diff_tool    = delta            # difftastic · delta · git-split-diffs · none
ollama_model = qwen3-coder:30b  # model for the AI summaries below
```

`key = value`, `#` for comments, no sections. A missing or malformed file is never an error — `gitt`
falls back to its defaults rather than refusing to start. Anything here can be overridden for one run:

```bash
gitt diff --diff-tool difftastic   # a flag beats the file
export GITT_DIFF_TOOL=delta        # so does an env var
```

Precedence is **flag → environment variable → `~/.gitt` → default**.

### Colorized diffs

`gitt` renders diffs through whichever third-party differ you already have — difftastic, delta, or
git-split-diffs — auto-detecting one on your `PATH` and falling back to plain `git` output when there
isn't one. Set `diff_tool` to pick deliberately, or `none` to always stay plain.

---

## Local AI summaries (optional)

`gitt` can tell you what a commit or a branch actually *did*, in a sentence, when the message doesn't.
Press `@` on any row and a summary streams into a panel below the list; `s` expands it. The panel isn't
there until it has something to say — it appears when you ask for a summary or land on a row that
already has one, and then stays for the rest of the session.

This is off unless you opt in, and it stays on your machine — the model runs locally via
[Ollama](https://ollama.com), so no diff is ever sent anywhere. Set it up once:

```bash
brew install ollama && ollama serve
ollama pull qwen3-coder:30b        # or set `ollama_model` to something smaller
```

Every summary is cached on disk by SHA, so one you've already read appears instantly and costs nothing
the second time. Without Ollama running, the panel just says so — nothing else changes.

| Variable | Default | Purpose |
| --- | --- | --- |
| `GITT_OLLAMA_MODEL` | `qwen3-coder:30b` | Overrides `ollama_model` for one run |
| `OLLAMA_HOST` | `http://127.0.0.1:11434` | Where Ollama is listening |
| `GITT_CACHE_DIR` | `$XDG_CACHE_HOME/gitt/summaries` | Where summaries are cached |
| `GITT_CONFIG` | `~/.gitt` | Read the config from somewhere else |

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

See [CLAUDE.md](./CLAUDE.md) for the full working agreement. Cutting a release is one command —
`scripts/release.sh 0.2.0` — see [RELEASING.md](./RELEASING.md).

## License

MIT — see [LICENSE](./LICENSE).
