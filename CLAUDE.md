# JELearner — notes for Claude Code

English / Japanese learning secretary, Rust, local-first, later a Mac app (Tauri).
Same structure and habits as the author's PaperComposer project.

## Commands

```sh
cargo test                                   # everything, offline (mock servers)
cargo run -p jl-server                       # http://127.0.0.1:7979
cargo run -p jl-server --example mock_services   # mock Claude + news site on :7980
scripts/dev.sh --mock                        # both of the above together
```

`cargo test` must pass before every commit.

## Layout

- `crates/core` (`jl-core`): workspace, SQLite (`db.rs`), import, robots/crawler/news,
  `provider.rs` (LlmProvider trait: Claude API, llama-server, mock), `claude_code.rs`
  (Claude subscription through Claude Code), `lesson.rs`,
  `google.rs` (Mailer / Calendar traits: REST + in-memory), `report.rs`, `catalog.rs`.
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
  decision changes.

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
  Current Claude API: `anthropic-version: 2026-10-21`, no `temperature`/`top_p`,
  structured output via `output_config.format`, effort via `output_config.effort`,
  `fallbacks: "default"` with beta `server-side-fallback-2026-07-01`, check
  `stop_reason` (`refusal`, `max_tokens`) before reading content.
- Local budget ≤ 20 GB on a 32 GB Mac: Gemma 4 12B (Q4_K_XL ~7.6 GB or Q8_0 ~12.9 GB,
  mmproj ~0.2 GB), Whisper large-v3-turbo ~1.6 GB, Kokoro-82M ~0.35 GB.
- Gmail / Calendar: traits first, mock implementations and mock servers; OAuth later.
- UI: axum on 127.0.0.1 + static web now; Tauri later.
- Privacy / multi-user: personal data lives in the user's own private repo (the
  workspace, e.g. `JELearner-private`). App settings, credentials and the SQLite
  database live in `<config dir>/JELearner/` (`dirs::config_dir()`), the database keyed
  by the workspace id. Nothing personal ever goes into this repo.

## Rules

- Never commit keys, tokens, personal data, real user paths or the user's e-mail.
  `.gitignore` and `scripts/pre-commit` (gitleaks) back this up.
- Workspace files are the source of truth for things the user writes (vocab, grammar,
  materials, lessons); the database can always be rebuilt from them. Never overwrite a
  user's file: new files get a free name (`free_path`), template copies use
  `copy_missing`.
- The crawler must check robots.txt (`Fetcher::get_text` does) and keep the per-host delay.
- Anything that cannot be tested here (Metal, Apple-only features, real llama-server,
  real Google OAuth) is verified with mock data and marked **not tested on real
  hardware** in code comments and docs.
- Propose a task list before large changes; each stage must build and test.
- Commits: the author (ellrin) is the only author. Do not add `Co-Authored-By` or other
  AI attribution trailers to commit messages; the README's 開發說明 section already
  notes that Claude Code helped write the code.
- Replies to the author in Traditional Chinese (繁體中文).
