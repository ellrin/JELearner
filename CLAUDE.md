# JELearner — notes for Claude Code

English / Japanese learning secretary, Rust, local-first, later a Mac app (Tauri).
Same structure and habits as the author's PaperComposer project.

## Commands

```sh
cargo test                                   # everything, offline (mock servers)
cargo run -p jl-server                       # http://127.0.0.1:7979
cargo run -p jl-server --example mock_services   # mock Claude + news site on :7980
scripts/dev.sh --mock                        # both of the above together
cd apps/desktop && APPLE_SIGNING_IDENTITY="…" cargo tauri build --bundles app   # the Mac app
```

`apps/desktop` is not a default member: `cargo test` / `cargo build` skip it (it bundles
`jl-speech`, built by `scripts/build-speech.sh` before `cargo tauri build`).

`cargo test` must pass before every commit.

## Layout

- `crates/core` (`jl-core`): workspace, SQLite (`db.rs`), import, robots/crawler/news,
  `provider.rs` (LlmProvider trait: Claude API, llama-server, mock), `claude_code.rs`
  (Claude subscription through Claude Code), `lesson.rs`,
  `google.rs` (Mailer / Calendar traits: REST + in-memory), `report.rs`, `catalog.rs`,
  `speech.rs` (SpeechProvider; `MacSpeech` = Apple's on-device recognition through the
  `jl-speech` helper; speaking stays in `web/speech.js`), `local_llm.rs` (models in the
  shared `LOCAL_LLM_HOME` folder; starts llama-server).
- `tools/macos-speech/`: `jl-speech`, a Swift helper (`swift build -c release` there);
  status / install / transcribe, one JSON object per call.
- `crates/server` (`jl-server`): axum on 127.0.0.1, `api.rs` handlers, `settings.rs`,
  `secrets.rs`, `git.rs`. Handlers are plain async fns over `Shared` so they can become
  Tauri commands later.
- `web/`: static HTML/CSS/JS, no build step. Visual language from PaperComposer
  (tokens, icons, panels, dialogs). UI text in Traditional Chinese.
- `prompts/`: lesson prompt + Markdown templates (MiniJinja). Overridable per user by
  files of the same name in the workspace `prompts/`.
- `models/catalog.json`: cloud (Claude) model list and local models for the Mac.
- `workspace-template/`: starting layout of a private workspace.
- `docs/`: decisions, architecture, data formats, UI design, roadmap. Update them when a
  decision changes. `docs/plan.md` is the agreed second-version plan (profiles, points and
  levels, AI conversation, daily lessons, recipes, voice, study groups); follow its stages
  (section 20) and update it first if a decision changes.

## Decisions (see docs/decisions.md for the reasons)

- Inference: llama.cpp `llama-server` over HTTP (not candle). Claude until the Mac
  arrives. Everything goes through `LlmProvider`; upper layers never name an engine.
- Claude has two ways in, like PaperComposer: the user's own **subscription** through the
  official Claude Code CLI (`claude_code.rs`: `claude -p --safe-mode … --json-schema`,
  empty temp folder, billing env vars stripped, sign-in via `claude auth login --claudeai`)
  and an **API key** (`ClaudeProvider`). `[engine] kind = auto` prefers the subscription.
  Never touch, store or forward Claude account credentials; never call the real CLI in
  tests (test settings point `claude_bin` at a nonexistent path; use `claude_code::fake`).
- Model names live in `models/catalog.json` / `config.toml`, never in code (tests excepted).
  Two models by task (decisions §19): `Task::Generate` (lessons, courses; `cloud.default`,
  Sonnet) and `Task::Realtime` (conversation turns and their evaluation; `cloud.realtime`,
  Haiku, no refusal fallbacks) — get a provider with `provider_task`.
  Current Claude API: `anthropic-version: 2026-10-21`, no `temperature`/`top_p`,
  structured output via `output_config.format`, effort via `output_config.effort`,
  `fallbacks: "default"` with beta `server-side-fallback-2026-07-01`, check
  `stop_reason` (`refusal`, `max_tokens`) before reading content.
- Local models live in the shared folder `LOCAL_LLM_HOME` (`[llama] home`), shared with
  PaperComposer: `<home>/gguf/<owner>/<repo>/<file>`, llama.cpp in `<home>/runtimes/`.
  Without `[llama] url` the server starts llama-server itself (`--reasoning-budget 0`)
  for `[llama] model` (a catalog id) and stops it on exit. Brains: Gemma 4 and Qwen; no
  abliterated models in this app. Ears and mouth are macOS built-in (decisions §14):
  Apple's SpeechTranscriber and the system voices, no Whisper or Kokoro.
- Tests never call Apple's engine or a real llama-server: `speech_bin` points at a
  missing path (or `speech::fake`), `llama_external` stays on. Real-hardware checks are
  `#[ignore]` tests (`cargo test -p jl-core real_ -- --ignored`).
- Gmail / Calendar: traits first, mock implementations and mock servers; OAuth later.
- UI: axum on 127.0.0.1 + static web now; Tauri later.
- Privacy / multi-user: personal data lives in the user's own private repo (the
  workspace, e.g. `JELearner-private`), one `profiles/<id>/` per user. App settings,
  credentials and the SQLite databases live in `<config dir>/JELearner/`
  (`dirs::config_dir()`): one database, one Claude Code sign-in folder and one API key
  per workspace id + user id. A user never falls back to another user's Claude.
  Nothing personal ever goes into this repo.

## Rules

- Never commit keys, tokens, personal data, real user paths or the user's e-mail.
  `.gitignore` and `scripts/pre-commit` (gitleaks) back this up.
- Never invent names that look like real people in examples, tests or docs. The first
  user is `ellrin`; the second user is always `daidai` (the author's choice).
- Workspace files are the source of truth for things the user writes (vocab, grammar,
  materials, lessons); the database can always be rebuilt from them. Never overwrite a
  user's file: new files get a free name (`free_path`), template copies use
  `copy_missing`.
- The crawler must check robots.txt (`Fetcher::get_text` does) and keep the per-host delay.
- Anything that cannot be tested here (Metal, Apple-only features, real llama-server,
  real Google OAuth) is verified with mock data and marked **not tested on real
  hardware** in code comments and docs.
- Propose a task list before large changes; each stage must build and test.
- Every frontend UI change (web/) is reviewed afterwards by an advisor (a separate review
  agent that runs the app, takes screenshots across themes and widths, and checks the three
  UI principles in docs/plan.md §0.1); fix its findings before calling the UI done.
  New CSS uses the `--accent*-text` tokens for accent text, line icons and thin borders.
- Commits: the author (ellrin) is the only author. Do not add `Co-Authored-By` or other
  AI attribution trailers to commit messages; the README's 開發說明 section already
  notes that Claude Code helped write the code.
- Replies to the author in Traditional Chinese (繁體中文).
