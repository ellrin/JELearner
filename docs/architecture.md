# 架構

```
            瀏覽器（之後：Tauri 視窗）
                    │ HTTP（127.0.0.1）
┌───────────────────▼──────────────────────────┐
│ jl-server（axum）                             │
│  api.rs   handler（之後可改為 Tauri command） │
│  settings.rs  config.toml＋環境變數           │
│  secrets.rs   API key：環境變數／金鑰圈／檔案 │
│  git.rs       同步私人工作區（系統 git）      │
└──┬──────────────┬───────────────┬────────────┘
   │              │               │
┌──▼──────────────▼───────────────▼────────────┐
│ jl-core                                       │
│  workspace  私人工作區（使用者的 private repo）│
│  db         SQLite：生字、文法、進度、錯題…    │
│  import     CSV／每週 JSON、records 匯出還原  │
│  robots ─ crawler ─ news   新聞爬蟲            │
│  lesson     文章 → JSON 教材 → Markdown        │
│  provider   LlmProvider：Claude API／llama-server│
│  claude_code Claude 訂閱（官方 Claude Code CLI）│
│  google     Mailer／Calendar：REST／記憶體     │
│  report     週報、確認信、複習排程             │
│  catalog    模型清單與記憶體估算               │
└──┬────────────┬──────────────┬───────────────┘
   │            │              │
 新聞網站   Claude（訂閱經 Claude Code，或 API key）  Gmail／Calendar
（robots.txt）（之後 llama-server）                  （目前模擬）
```

## 資料放在哪裡

| 內容 | 位置 | 進 Git？ |
| --- | --- | --- |
| 程式、提示詞模板、範本 | 這個 repo | 是（公開） |
| 單字、文法、每週素材、教材、紀錄匯出 | 私人工作區（例如 `JELearner-private`） | 是（**你自己的 private repo**） |
| `config.toml`（工作區位置、模型選擇） | `<config dir>/JELearner/` | 否 |
| 這台電腦最後使用的使用者 | `config.toml` 的 `[active_users]` | 否 |
| API key（每位使用者一把） | 系統金鑰圈（帳號 `anthropic-api-key:<工作區 id>:<使用者 id>`），或 `<config dir>/JELearner/credentials/<工作區 id>/<使用者 id>.toml` | 否 |
| Claude Code 登入（每位使用者一個） | `<config dir>/JELearner/claude/<工作區 id>/<使用者 id>/`（`CLAUDE_CONFIG_DIR`） | 否 |
| SQLite（每位使用者一個） | `<config dir>/JELearner/data/<工作區 id>/<使用者 id>/learner.sqlite` | 否 |
| 示範工作區 | `<config dir>/JELearner/demo-workspace/` | 否 |

`<config dir>`：macOS `~/Library/Application Support`，Linux `~/.config`，Windows `%APPDATA%`。

## 啟動流程

1. 讀 `Settings`（`config.toml` → 環境變數覆蓋）。
2. 找工作區：`JELEARNER_WORKSPACE` → `config.toml` 的 `workspace_path` → 示範工作區。
3. `Workspace::open`（示範時 `init`，只補缺少的檔案），沒有 id 就產生一個寫回 `workspace.toml`。
4. 第一版結構（單人）→ 起始流程詢問名字並搬到 `profiles/<id>/`；否則選使用者：這台電腦上次用的、或唯一的那位；都沒有就顯示使用者選擇。
5. 開啟（或建立）「工作區 id＋使用者 id」的 SQLite，套用 schema（`PRAGMA user_version`）；第一版的資料庫交給擁有者。
6. `import::sync_workspace`：讀 `vocab/`、`grammar/`、`materials/`、`lessons/`；`restore_records`：資料庫是新的時從 `records/` 還原進度與錯題。

## 產生教材

```
選新聞／貼網址／貼文字
  → Fetcher（robots.txt、間隔、4 MB 上限）→ news::extract_article
  → articles 表
  → lesson::build_request（prompts/*.md 以 MiniJinja 填入；附已會的單字）
  → LlmProvider::complete（JSON Schema 結構化輸出）
  → lesson::parse_lesson（驗證：選擇題答案必須在選項中）
  → lesson::render_markdown（prompts/lesson.md.j2）＋ embed_data
  → lessons/<lang>/<date>-<slug>.md（不覆蓋）＋ lessons 表
  → 單字、文法 upsert（新卡片立即到期）
```

## 資料庫 schema

見 `crates/core/src/db.rs` 的 `MIGRATIONS`。主要表格：

- `words`（lang, term, reading 唯一）、`grammar`（lang, pattern 唯一）：重複匯入時，空欄位不會蓋掉既有內容。
- `progress`：每張卡片的 SM-2 狀態（ease、間隔、次數、遺忘次數、到期時間）；`reviews`：每次評分。
- `articles`（url 唯一）、`lessons`（含完整 Markdown 與 JSON）、`mistakes`、`materials`。

## 測試策略

- 單元測試在各模組（robots 規則、CSV 別名、SM-2、RFC 2822、Claude 請求形狀與拒絕處理…）。
- `crates/server/tests/e2e.rs`：真伺服器＋模擬新聞網站／Claude／Gmail／Calendar，走完整個流程，包括重新啟動與第二台電腦還原。
- 無法在這裡測的（Metal、llama-server 實機、Google OAuth）以模擬資料驗證，並在程式與文件標註「尚未實機測試」。
