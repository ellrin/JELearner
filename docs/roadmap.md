# 路線圖

每個階段結束時都要能跑（`cargo run -p jl-server`）、能測（`cargo test`）。

## 第一版：文字管線（完成）

- [x] Rust workspace：`crates/core`、`crates/server`、`web/`
- [x] `CLAUDE.md`、`docs/`（決策、架構、資料格式、介面設計、路線圖）
- [x] 私人工作區：範本、`workspace.toml`（id、程度、週報信箱）、示範模式、開啟／建立／clone、Git 同步（沿用 PaperComposer）
- [x] SQLite：words、grammar、progress（SM-2）、reviews、mistakes、lessons、articles、materials；資料庫放設定資料夾、依工作區分開
- [x] 匯入：單字／文法 CSV（寬鬆標題、Tab 分隔）、每週 JSON；records 匯出與換機器還原
- [x] 爬蟲：RSS／Atom、文章內文擷取、robots.txt（RFC 9309）、每站間隔；來源 BBC Technology、NHK NEWS WEB
- [x] 教材：JSON Schema 結構化輸出 → Markdown，提示詞與版面模板放在 `prompts/`，可被工作區覆蓋
- [x] Claude provider（key：環境變數／金鑰圈／設定資料夾）；llama-server provider（模擬測試）
- [x] Gmail／Calendar trait、REST 實作（模擬伺服器測試）、記憶體實作與模擬寄件匣；週報、排複習、排課
- [x] 本機網頁：新聞或網址或貼文字 → 教材 → 存入 SQLite → 測驗 → 生字、文法、錯題、素材
- [x] 測試：單元測試＋模擬伺服器的離線端到端測試

**這個雲端開發環境連不到外部新聞網站與 Anthropic API**，所以真實新聞來源與真實 Claude 呼叫尚未實測；請在自己的電腦上執行一次「抓最新新聞」與「設定 → Claude → 測試連線」確認。

## 第二階段：向量庫＋RAG

- [ ] 評估 LanceDB（Rust 原生、檔案型）與 ChromaDB（需要 Python 伺服器）；傾向 LanceDB
- [ ] 嵌入模型：雲端期間用 API，Mac 到貨後改本機（llama-server 的 embedding 端點）
- [ ] 索引教材與新聞（切段、語言、日期）；可從工作區重建
- [ ] 產生教材時檢索相關的舊單字與文法；生字頁顯示真實例句
- [ ] 弱點教材（錯題＋高遺忘卡片）

## 第三階段：Google 真實串接

- [ ] OAuth 2.0 桌面流程（loopback redirect＋PKCE），token 存系統金鑰圈
- [ ] `TokenSource` 實作（自動更新 access token）
- [ ] 每週自動寄週報（App 開著時）、複習提醒
- [ ] 每位使用者用自己的 Google 帳號；不保存在任何 repo

## 第四階段：Mac 到貨

- [ ] 模型下載精靈（沿用 PaperComposer 的 `models.rs`：Hugging Face 檔名比對、下載 llama.cpp Metal 版、進度、確認後才下載）
- [ ] 啟動／停止 llama-server，設為預設引擎；Claude 保留為備援
- [ ] 實測記憶體：Q4_K_XL 與 Q8_0、KV cache、與 Whisper／Kokoro 同時執行
- [ ] 本機嵌入模型

## 第五階段：語音

- [ ] Whisper large-v3-turbo（whisper.cpp，Metal）STT
- [ ] Kokoro-82M TTS（英、日）
- [ ] 即時對話：盲聊、發音與文法即時糾正、結束後的糾正清單 → 生字／錯題
- [ ] Gemma 4 音訊輸入（mmproj）可行性評估

## 第六階段：Tauri 打包

- [ ] `apps/desktop`（沿用 PaperComposer：同程序啟動伺服器、隨機埠、WKWebView）
- [ ] 打包 `.app`／`.dmg`、圖示、簽章
- [ ] 需要原生能力的 handler 改為 Tauri command（麥克風、通知、深連結）

## 之後：數位人像

- [ ] 人像渲染與嘴型同步（由 TTS 音素時間軸驅動）
- [ ] `avatar/` 設定：外觀、聲音、語速
- [ ] 效能預算（與語音、大腦共用 GPU）
