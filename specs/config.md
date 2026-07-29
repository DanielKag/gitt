# gitt config file (`~/.gitt`)

- **ID prefix:** `CFG`
- **Status:** draft
- **Command:** all (`gitt log` / `status` / `diff` / `branch`)

## Summary

The two settings a user actually wants to pin — which third-party diff renderer to pipe diffs through,
and which Ollama model writes the AI summaries — currently live only in environment variables. Exporting
them from a shell profile works but is invisible and easy to lose. `gitt` therefore reads a small
optional file at `~/.gitt`:

```ini
# ~/.gitt — gitt configuration
diff_tool = delta
ollama_model = qwen3-coder:30b
```

The format is deliberately minimal: `key = value` lines, `#` comments, blank lines ignored. There are
no sections and no nesting, because there are only a handful of settings and a flat file needs no
documentation to read. Parsing is a **pure** function over the file's text; the shell does nothing but
read the file (and a missing, unreadable, or malformed file is never an error — it degrades to
defaults, so a typo can never stop `gitt` from opening).

Precedence follows the usual convention — the more specific and more immediate wins:
**CLI flag → environment variable → `~/.gitt` → built-in default.**

## Acceptance criteria

| ID      | Criterion (testable statement)                                                                                       | Tiers      |
| ------- | -------------------------------------------------------------------------------------------------------------------- | ---------- |
| CFG-01  | `parse_config` turns `key = value` lines into a `Config` value, recognizing `diff_tool` and `ollama_model`. Whitespace around keys, values, and the `=` is trimmed. | unit       |
| CFG-02  | Lines that are blank or start with `#` (after leading whitespace) are ignored; so are lines with no `=`. An unknown key is ignored rather than being an error, so a newer config never breaks an older binary. | unit       |
| CFG-03  | Keys are matched case-insensitively and accept `-` as an alias for `_` (`diff-tool` == `diff_tool`), so the file is forgiving about the two ways a user would naturally spell it. | unit       |
| CFG-04  | A key present with an empty value is treated as **unset** (it falls through to the next precedence level) rather than as an empty string. | unit       |
| CFG-05  | When a key appears more than once, the **last** occurrence wins (so a user can append an override to the end of the file). | unit       |
| CFG-06  | The diff tool resolves as `--diff-tool` flag → `GITT_DIFF_TOOL` → `~/.gitt` `diff_tool` → autodetect the first tool installed on `PATH`. A configured-but-not-installed tool still degrades to plain text. | unit       |
| CFG-07  | The summary model resolves as `GITT_OLLAMA_MODEL` → `~/.gitt` `ollama_model` → the built-in default (`qwen3-coder:30b`). | unit       |
| CFG-08  | The config file is read from `$HOME/.gitt`, overridable with `GITT_CONFIG=<path>` (which also lets e2e point at a fixture). A missing or unreadable file yields an empty `Config`, never an error. | unit, e2e  |
| CFG-09  | An `ollama_model` set only in `~/.gitt` reaches the **real** summarizer in a real `gitt` run (observable via the test sink), and a config file whose `diff_tool` names an unknown/uninstalled tool still opens and previews diffs as plain text rather than failing. | e2e        |

## Keybindings / UX

None — the file is read once at startup.

## Errors / edge cases

- No `~/.gitt` → every setting falls through to its env var or default (the overwhelmingly common case).
- Unreadable file (permissions, a directory at that path) → treated as absent, no message, no failure.
- Garbage content → each unparseable line is skipped; recognized lines still apply.
- A `diff_tool` naming a tool that isn't installed → plain diffs (same as the env-var path, CFG-06).
- An unknown `diff_tool` name → plain diffs (`DiffTool::parse` already maps unknown → `None`).

## Test seams (e2e determinism)

- `GITT_CONFIG=<path>` — read the config from an explicit path instead of `$HOME/.gitt`. E2E already
  points `HOME` at a tempdir, so this is belt-and-braces isolation plus a way to test a fixture file.
- `ollama_model.txt` in `GITT_TEST_SINK_DIR` — the real summarizer records the model it resolved to on
  every call, including the faked ones, so e2e can assert the config actually reached it (CFG-09).
- Note the e2e harness pins `GITT_DIFF_TOOL=none` for determinism; a test that wants the *file's*
  `diff_tool` to apply passes `GITT_DIFF_TOOL=""`, since a blank value counts as unset (CFG-04).

## Out of scope

- Per-repository config (`.gitt` in the repo root) — global only for now.
- Configuring keybindings, colors, page size, or the summary prompt.
- Sections, arrays, or nested values (no TOML/YAML dependency).
- Writing the file from inside `gitt` (no `gitt config set`).
