# gitt log — AI commit summaries (Ollama)

- **ID prefix:** `SUM`
- **Status:** implemented
- **Command:** `gitt log`

## Summary

`gitt log` can show a short, AI-generated plain-language summary of what the selected commit does.
Summaries are generated **locally** by [Ollama](https://ollama.com) and cached on disk so each commit
is only ever summarized once. A persistent panel below the commit list shows the summary for the
selected commit: if one is already cached it appears instantly (no model call); otherwise the panel
shows a hint and the user presses `@` to generate it. Generation runs off the UI thread — while it is
in flight the panel says so, then the result replaces it. This keeps the tool's "instant" feel: the
common case (cached) is a cheap file read, and the expensive case (an LLM call) never blocks input.

The model is fed a system instruction plus the commit's subject and its (size-bounded) diff, all built
by a pure function. The cache is keyed by the commit's **content hash** — its full git SHA, which git
itself computes from the commit's content — so two runs over the same repo reuse the same summary, and
distinct commits never collide.

## Acceptance criteria

| ID      | Criterion (testable statement)                                                                                          | Tiers      |
| ------- | ----------------------------------------------------------------------------------------------------------------------- | ---------- |
| SUM-01  | When the summary panel is showing (see SUM-13 for when that is), `gitt log` renders it as a bordered "ai summary" panel below the commit list, and its content reflects the **selected** commit's summary state. | unit, e2e  |
| SUM-02  | Selecting a commit auto-loads its summary from the on-disk cache (keyed by the commit's full SHA); a cache **hit** displays immediately, with no AI call. | unit, e2e  |
| SUM-03  | When no summary is cached, the panel shows a hint to press `@`; the reducer emits a cache-load effect once per commit and records the miss. | unit       |
| SUM-04  | Pressing `@` on the selected commit starts generation off the UI thread; the model's tokens **stream** into the panel as they arrive (a "summarizing…" placeholder shows until the first token). All progress/results show in the panel — the status line keeps showing the keymap legend. | unit, e2e  |
| SUM-05  | The generation prompt is built by a **pure** function from a system instruction + the commit subject + its diff; the diff is truncated to a bounded number of lines. | unit       |
| SUM-06  | A completed generation shows the model's summary in the panel and writes it to the cache directory under the user's home, keyed by the commit SHA, so a later run reuses it without calling Ollama. | unit, e2e  |
| SUM-07  | Generation calls Ollama's HTTP API (`POST /api/generate`, streaming) with model `qwen3-coder:30b` by default (overridable via `GITT_OLLAMA_MODEL`) at `http://127.0.0.1:11434` (overridable via `OLLAMA_HOST`); the clean `response` text is used (not `ollama run`, whose piped output carries terminal control codes). | unit (model + URL resolution), manual |
| SUM-08  | A generation failure (Ollama missing or erroring) surfaces in the panel as a failed state without crashing; pressing `@` again retries. | unit, e2e  |
| SUM-09  | Pressing `@` while a summary is already generating for the selected commit is ignored (no duplicate effect / AI call).  | unit       |
| SUM-10  | The cache directory resolves as `GITT_CACHE_DIR` → `$XDG_CACHE_HOME/gitt/summaries` → `$HOME/.cache/gitt/summaries`.    | unit       |
| SUM-11  | A list entry (commit in `gitt log`, branch in `gitt branch`) whose summary is cached (`Ready`) shows a one-character AI marker (`✦`) in a leading column; entries without a cached summary reserve the same width blank so rows stay aligned. | unit       |
| SUM-12  | Markdown `code` spans (backtick pairs) render styled (backticks stripped) identically in **both** the collapsed teaser and the expanded footer — the teaser is the expanded view cut to fit, not plain text. | unit       |
| SUM-13  | The summary panel is **hidden** until it has something to say: on a fresh launch no panel is drawn and the list gets those rows. It appears when the user presses `@`, or when the **selected** entry turns out to have a summary already (a cache hit) — never for an entry that is merely a cache miss. | unit, e2e  |
| SUM-14  | Once the panel has appeared it stays for the rest of the session, including while the selection sits on entries with no summary (where it shows the `press @` hint). It never flickers open and shut as the user moves the cursor. | unit, e2e  |
| SUM-15  | The list's page/scroll math follows the panel: while it is hidden the list occupies the rows the panel would have taken, and opening it re-clamps the scroll so the selected row stays visible. Identical in `gitt log` and `gitt branch`. | unit       |

## Keybindings / UX

| Key   | Mode | Action                                                        |
| ----- | ---- | ------------------------------------------------------------- |
| `@`   | List | Generate (or regenerate) an AI summary of the selected entry — also what first reveals the panel |
| `s`   | List | Expand / minimize the summary panel (once it is open)          |

The summary panel starts **hidden** (SUM-13): a fresh `gitt log` is all list. It opens the moment it
has something to show — `@` pressed, or the selection landing on an entry whose summary is already
cached — and then stays open for the rest of the session (SUM-14), so it never flickers as the cursor
moves. While open it reserves a fixed number of rows below the commit list, and the list's page/scroll
math accounts for it either way (SUM-15). Panel states:

- **cached / generated** → the summary text (wrapped).
- **not cached** → dim hint: `press @ for an AI summary`.
- **generating** → `summarizing with ollama…` until the first token, then the streaming text.
- **failed** → `summary failed: <reason>` (retry with `@`).

The status line always shows the keymap legend (or a transient result from other actions like copy/checkout); summary progress and failures never occupy it.

## Errors / edge cases

- Empty repo / no selection → `@` is a safe no-op and the panel stays hidden (nothing to summarize).
- `ollama` not installed or the call fails → failed state on the (now open) panel, no crash; `@` retries.
- Very large diffs → truncated to a bounded size before being sent to the model (SUM-05).
- Cache directory not writable → generation still displays the summary; the cache write error is
  swallowed (the summary is transient for that run).

## Test seams (e2e determinism)

- `GITT_FAKE_SUMMARY=<text>` — when set, the real summarizer returns `<text>` instead of shelling out
  to `ollama`, and (when `GITT_TEST_SINK_DIR` is set) writes the exact prompt it *would* have sent to
  `summary_prompt.txt` in the sink — so e2e can assert both the rendered summary and the built context.
- `GITT_FAKE_SUMMARY_ERROR=<msg>` — when set, the summarizer fails deterministically with `<msg>`
  (so the failure path is testable without depending on ollama's real behavior).
- `GITT_CACHE_DIR=<dir>` — overrides the summary cache directory, so a test can point two separate
  binary runs at one shared directory and assert cross-run cache reuse.

## Out of scope

- Streaming/token-by-token rendering of the model output (we show the completed summary).
- Summarizing ranges, branches, or the working tree (commit-only for now).
- Configurable prompt/system-instruction or summary length via flags.
- Remote LLM providers (Ollama-only, local).
