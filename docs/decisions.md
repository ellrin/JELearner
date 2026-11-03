# 決策紀錄

每一項：決定了什麼、為什麼、什麼情況下要重新考慮。

## 1. 推論引擎：llama.cpp 的 llama-server，不用 candle

- **決定**：本機模型一律透過 `llama-server`（OpenAI 相容 HTTP API）呼叫，Rust 端只是 HTTP client。
- **原因**：Metal 支援成熟、Gemma 4 的 GGUF 與 mmproj（圖片、音訊）現成；PaperComposer 已經用同樣做法，模型下載精靈與 `catalog.json` 可以沿用。candle 的 Gemma 4 多模態支援與 Metal 效能都還要自己追。
- **重新考慮**：llama-server 不支援某個必要功能（例如即時語音串流）時。

## 2. Provider trait：現在 Claude，之後 llama-server

- **決定**：`jl_core::provider::LlmProvider`（`complete(CompletionRequest) -> String`），請求可附 JSON Schema。實作：`ClaudeProvider`（API key）、`ClaudeCodeProvider`（Claude 訂閱，見第 11 節）、`LlamaServerProvider`、`MockProvider`。
- **原因**：Mac 到貨前沒有本機 GPU；上層（教材生成、之後的對話與糾錯）不需要知道引擎。
- **現狀**：`LlamaServerProvider` 只以模擬伺服器測試、`ClaudeCodeProvider` 只以假的 `claude` 執行檔測試，**都尚未實機測試**。

## 3. 雲端模型：名稱放設定檔

- **決定**：可選模型與預設值放在 `models/catalog.json` 的 `cloud`；`config.toml` 的 `[claude] model`、`effort` 或環境變數 `JELEARNER_CLAUDE_MODEL` 可以覆蓋。程式碼不寫死模型名稱。
- **目前清單**：`claude-opus-5-5`（預設）、`claude-sonnet-5-5`、`claude-haiku-5-5`。
- **API 細節**：`anthropic-version: 2026-10-21`；不送 `temperature`（目前的模型不接受取樣參數）；結構化輸出用 `output_config.format`（JSON Schema）；`output_config.effort` 預設 `medium`；開啟 `fallbacks: "default"`（beta `server-side-fallback-2026-07-01`），被安全分類器拒絕時由伺服器改用建議的備援模型；讀內容前先檢查 `stop_reason`（`refusal`、`max_tokens`）。

## 4. 地端模型預算（Mac 32 GB）

| 角色 | 模型 | 大小（約） |
| --- | --- | --- |
| 大腦 | Gemma 4 12B UD-Q4_K_XL（或 Q8_0） | 7.4–7.6 GB（Q8_0 約 12.7–12.9 GB）＋ mmproj 約 0.2 GB |
| 耳朵 | Whisper large-v3-turbo | 約 1.5–1.6 GB |
| 嘴巴 | Kokoro-82M | 約 0.35 GB |

- 總預算 ≤ 20 GB，留給系統與 KV cache。macOS 預設只讓 GPU 用約 2/3（36 GB 以下）到 3/4 的統一記憶體，32 GB 約 21 GB。
- 預設用 Q4_K_XL；確認 KV cache 與語音同時執行還有餘裕後再評估 Q8_0。`catalog::plan` 會算總和並檢查兩個上限（**估算，尚未實機測試**）。

## 5. Gmail／Google Calendar：先 trait，OAuth 之後

- **決定**：`Mailer`、`Calendar` trait；`GmailApi`、`GoogleCalendarApi` 照真正的 REST 形狀實作（`users.messages.send` 的 RFC 2822 `raw`、`events.insert`／`events.list`），token 由 `TokenSource` 提供；預設用 `MemoryMailer`／`MemoryCalendar`，畫面上顯示模擬寄件匣。
- **原因**：OAuth（桌面 App 的 loopback 流程、token 保存在金鑰圈）需要另外設計，不該擋住第一版。
- **測試**：以模擬伺服器檢查請求內容（Bearer token、UTF-8 主旨編碼、標頭注入防護、提醒設定）。

## 6. 介面：axum + 靜態網頁，之後 Tauri

- **決定**：`jl-server` 只綁 127.0.0.1，`web/` 是不需要建置的 HTML/CSS/JS；handler 都是對 `Shared` 的普通 async 函式。
- **之後**：照 PaperComposer 的 `apps/desktop`，Tauri 啟動同一個伺服器（隨機埠）並開視窗；需要原生能力的部分再改成 Tauri command。

## 7. 隱私與多使用者

- **決定**：
  - 使用者自己寫的東西（單字、文法、每週素材、教材、複習紀錄匯出）放在**使用者自己的 private repo**（工作區）。
  - App 設定、金鑰、SQLite 放在 `dirs::config_dir()/JELearner/`；資料庫路徑是 `data/<工作區 id>/learner.sqlite`。
  - 其他人使用時指向他們自己的工作區，自動得到自己的資料庫；不會讀到別人的資料。
- **原因**：沿用 PaperComposer 的做法；工作區是純文字檔，可以 diff、可以多台電腦同步，資料庫隨時可重建。
- **SQLite 不進 Git**：二進位檔無法合併。換電腦時，教材從 Markdown 內嵌的資料重建，複習進度與錯題從 `records/*.csv` 還原。

## 8. 爬蟲禮儀

- 只讀 RSS／Atom feed 與 feed 指到的文章頁。
- 每個網址先查 `robots.txt`（RFC 9309：最具體的 user-agent 群組、最長規則、`*` 與 `$`）；4xx 視為允許，5xx 或連不上視為禁止。
- 同一網站請求間隔至少 1.5 秒或網站的 Crawl-delay；誠實的 User-Agent；回應上限 4 MB。
- 教材只供個人學習，不轉載原文；工作區是私人 repo。

## 9. 教材格式

- 模型輸出固定 JSON（`lesson::lesson_schema`），Markdown 由模板產生，所以換版面不必重跑模型。
- Markdown 末尾以 HTML 註解內嵌 JSON（`<!-- jelearner:lesson … -->`），渲染時看不到，但能在另一台電腦重建資料庫。
- 文章超過 24,000 字時拒絕並請使用者貼段落，不會默默截斷。

## 10. Rust 技術棧

tokio、axum 0.8、reqwest（rustls）、sqlx 0.8（SQLite，執行期查詢，不需要 `DATABASE_URL`）、serde、feed-rs、scraper、minijinja。Agent 框架（rig-core）目前不需要：第一版只有單次呼叫；之後做對話與工具呼叫時再評估。

## 11. Claude 的兩種連線方式：訂閱與 API key（比照 PaperComposer）

- **決定**：
  - **訂閱**：`ClaudeCodeProvider` 執行使用者自己安裝、自己登入的官方 Claude Code（`claude -p --output-format json --safe-mode --no-session-persistence --tools ""`，加上 `--model`、`--effort`、`--system-prompt`、`--json-schema`），提示詞走 stdin，每次在空的暫存資料夾執行；移除 `ANTHROPIC_API_KEY`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL` 與雲端供應商變數，確保計入訂閱。登入用 `claude auth login --claudeai`，在 Anthropic 的頁面完成；狀態用 `claude auth status --json` 判斷（`authMethod` 為 `claude.ai`／`oauth_token` 才算訂閱）。
  - **API key**：`ClaudeProvider`（第 3 節）。
  - **選擇**：`[engine] kind`，預設 `auto`：訂閱已登入就用訂閱，否則用 API key。兩條路都能單獨測試連線。
- **原因**：作者自己用訂閱額度，不必另外付 API 費用；API key 留給沒有訂閱、或想避免規定疑慮的使用者。
- **界線**：App 不接觸、不保存、不轉送任何 Claude 帳號憑證或 token，也不提供自己的 claude.ai 登入頁；不代付、不轉售用量；不修改 Claude Code。README 說明 Anthropic 的相關規定（訂閱以個人一般使用為前提、第三方不得代替使用者透過訂閱送出請求），由使用者自行判斷。規定若改變，拿掉訂閱模式、只留 API key 即可，上層不受影響。
- **測試**：`claude_code::fake` 產生假的 `claude` 執行檔（模擬 `--version`、`auth status`、`auth login`、`-p`），檢查參數、stdin、空資料夾、環境變數移除與登入流程。測試設定一律把 CLI 指向不存在的路徑，絕不呼叫真的 Claude Code。**尚未用真實帳號從 App 測試。**
