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
- **2026-11 更新（Mac 到貨）**：耳朵與嘴巴改用 macOS 內建（第 14 節），不再佔模型預算；大腦可選 Gemma 4 或 Qwen（第 15 節）。實測 M6 32 GB 的 Metal 可用上限（`recommendedMaxWorkingSetSize`）是 26.8 GB，比上面的估算寬。

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

## 12. 多使用者工作區（第二版 0-1）

- **決定**：一個工作區可有多位使用者（`profiles/<id>/`），選名字進入、不用密碼；第一位使用者是擁有者與管理員。設定分三層：這台電腦（`config.toml`，含 `[active_users]`）、使用者（`profile.toml`）、工作區（`workspace.toml`）。每位使用者有自己的資料庫、Claude Code 登入資料夾（`CLAUDE_CONFIG_DIR`）與 API key；Claude Code 子程序另外移除 `CLAUDE_CODE_OAUTH_TOKEN`，避免繞過登入資料夾。
- **原因**：家人共用一個 private repo；同一台電腦上不能用到別人的 Claude 帳號（docs/plan.md §2–§4）。
- **第一版相容**：偵測到舊的單人結構時，由使用者確認後搬到 `profiles/<id>/`（`git mv`，不刪檔），第一版的 `workspace.toml` 設定移到 `profile.toml`；舊的資料庫與單一 API key 交給擁有者。
- **尚未實測**：macOS 鑰匙圈中，不同 `CLAUDE_CONFIG_DIR` 的 Claude Code 登入是否確實分開（已確認 Claude Code 會使用指定資料夾）。

## 13. 積分、等級與打字版對話（第二版 A 階段）

- **決定**：積分以 `records/points/YYYY-MM-DD.csv` 為準（一天一檔、只新增，未通過也記 0 分），資料庫只是索引、每次開啟都從檔案補齊；規則數值放 `config/progress.toml`，工作區可覆蓋；等級只升不降、小階由條件 1–3 自動判斷、跨大級等 B 階段的檢核；教材難度與等級分開調整；`LlmProvider` 加 `chat`（多回合），Claude 訂閱把整段對話寫進提示詞；對話完成後存成 Markdown＋內嵌資料；大頭貼只存在本機。細節與理由見 `plan.md` 第 20 節「實作時決定的細節」。
- **原因**：多台電腦同步不衝突、資料庫可重建；門檻需要實際使用後校準，所以放設定檔；介面三原則（完成感、正向回饋、不說「你錯了」）。
- **尚未實測**：真實 Claude（API 與訂閱）的對話品質與回覆格式；目前以 mock 伺服器與假的 Claude Code 測試。

## 14. Mac 的語音：Apple 內建辨識與系統聲音（第二版 D 階段，2026-11）

- **決定**：作者的 Mac 上，語音辨識用 macOS 的 `SpeechAnalyzer`／`SpeechTranscriber`（英、日，模型由系統下載與管理，錄音不離開電腦），取代原定的 Whisper；語音合成用系統聲音（瀏覽器 `speechSynthesis` 本來就用它們），優先選 Premium／加強版，取代原定的 Kokoro。
  - Rust 透過小工具 `jl-speech`（`tools/macos-speech/`，Swift）呼叫 Apple 的 API：`status`（語言是否支援、是否已安裝）、`install`（下載語言模型）、`transcribe`（WAV → JSON）。`jl_core::speech::MacSpeech` 實作 `SpeechProvider`，上層不指名引擎。
  - 網頁錄成 16 kHz 單聲道 WAV（Web Audio，不用 `MediaRecorder`：Chrome 錄出 WebM，Apple 的 API 讀不了），上傳給伺服器辨識；15 秒倒數、時間到自動送出。
  - 有本機辨識時，麥克風與跟讀預設就出現；瀏覽器的雲端辨識改為沒有本機引擎時的備案（daidai 的 Windows），仍需在設定打開。
- **原因**：Apple 的辨識不必另外下載大模型、幾乎不佔記憶體、很快，不會和本機 LLM 搶 GPU；Kokoro 的日文明顯弱於英文，系統 Premium 聲音英日品質一致且免費。兩者都在本機，符合「錄音不離開電腦」。
- **取捨**：只有 macOS 26 以上能用；Windows 仍走系統內建或瀏覽器。辨識引擎會自動修正口音，跟讀分數仍是近似值（第 15 節）。
- **測試**：`cargo test` 用假的 `jl-speech`（寫成 shell script）測參數、錯誤與 JSON；實機以 `say` 產生英、日語音檔再辨識，驗證整條路。**麥克風錄音需人工測試。**

## 15. 共用模型資料夾與本機引擎（2026-11）

- **決定**：本機模型與 llama.cpp 放在所有專案共用的資料夾，位置由環境變數 `LOCAL_LLM_HOME` 決定（或 `config.toml` 的 `[llama] home`，從 Finder 開啟的 App 讀不到 shell 變數）；模型在 `<home>/gguf/<owner>/<repo>/<檔名>`，與 `hf download --local-dir` 相同，和 PaperComposer 共用同一份檔案。`[engine] kind = "llama-server"` 時，伺服器自己用 `<home>/runtimes/llama.cpp` 的 `llama-server` 啟動 `[llama] model` 指定的模型（`--reasoning-budget 0`：Gemma 4 與 Qwen 不先思考，教材與對話都要固定 JSON）；`[llama] url` 有填時改連那台已經在跑的伺服器。
- **目錄**：`models/catalog.json` 的大腦加上 Qwen（3.5 4B、3.5 9B、3.8 27B）與 Gemma 4 E4B；耳朵、嘴巴改為 macOS 內建（0 GB）。JELearner 是家人共用的學習工具，**不放 abliterated（移除拒答）版本**（只在 PaperComposer 提供）。設定 → 05 本機模型列出共用資料夾裡已下載的大腦，「使用這個」會寫入 `[llama] model`、把引擎切到本機並立即啟動；關閉伺服器（Ctrl+C、SIGTERM）時一併結束 llama-server。
- **實測**（M6 32 GB）：從共用資料夾啟動 Qwen3.5 4B 到回答 JSON 約 2.7 秒；在設定頁切換 1.1 秒、測試連線 0.26 秒。
- **原因**：模型動輒十幾 GB，每個專案各存一份浪費空間；作者在 PaperComposer 的實測（M6 32 GB）：Qwen3.5 4B 每秒 30.8 token、Gemma 4 12B Q8 11.5、Qwen3.8 27B 8.2，關掉思考後一次回覆從數分鐘降到 10–30 秒。
