# JELearner

本機運作的英文／日文學習秘書：自動抓英日新聞、整理成單字與文法教材、排複習、記錯題、寄週報、排行事曆。之後加上即時語音對話與數位人像，並包成 Mac app。

> 開發初期（第一版：文字管線）。已完成：私人工作區、SQLite（生字／文法／複習進度／錯題本）、英日新聞爬蟲（遵守 robots.txt）、新聞 → Markdown 教材（模板可替換）、Claude 兩種連線方式（訂閱／API key）、Gmail／Calendar 介面與模擬實作、本機網頁、離線端到端測試。
> 尚未完成：向量庫與 RAG、Google OAuth、本機模型（等 Mac）、語音、Tauri 打包、數位人像。見 [`docs/roadmap.md`](docs/roadmap.md)。
> 第二版進行中：**0-1 起始流程與多使用者、A 階段（積分、等級、難度調整、打字版 AI 對話、今日任務與進度頁）已完成**。完整規劃（多使用者、積分與等級、AI 對話、每日課程、課程配方、語音、學習群組）：[`docs/plan.md`](docs/plan.md)。

## 執行

需要 [Rust](https://rustup.rs)（1.85 以上）。

```sh
cargo run -p jl-server
```

開啟 <http://127.0.0.1:7979>（只監聽本機）。第一次開啟會用內建的示範資料，並跳出「設定」讓你連接自己的私人工作區。

**沒有 API key、也不想連網時**（看介面、改前端）：

```sh
scripts/dev.sh --mock
```

會同時啟動模擬的 Claude API 與模擬新聞網站（`cargo run -p jl-server --example mock_services`），把私人工作區 `sources.toml` 的 feed 換成畫面上印出的網址即可。

**macOS 開發**：`scripts/dev.sh` 會用系統的 `security` 保存 API key，避免每次重新編譯都被問鑰匙圈密碼。

## 私人工作區

生字、文法、每週素材、教材都是**個人資料，放在你自己的 private repository**（例如 `JELearner-private`），不要放進這個 repo。一個工作區可以有好幾位使用者（例如你和家人），每人一個資料夾。結構（範本見 [`workspace-template/`](workspace-template/)）：

```
JELearner-private/
  workspace.toml        工作區 id、名稱、管理員、成員
  sources.toml          新聞來源（RSS / Atom，共用）
  topics.toml           通識主題（共用）
  prompts/              自訂提示詞與教材模板（可選，共用）
  profiles/<使用者>/
    profile.toml        學習語言、各語言程度、每日目標、對話設定、週報信箱
    interests.csv       興趣關鍵字
    vocab/  grammar/    想背的單字、想學的文法（CSV，可放多個檔案）
    materials/          每週素材（materials/2026-W45/*.json）
    lessons/en|ja/      產生的教材（Markdown）
    conversations/      對話紀錄
    records/            積分（records/points/）、複習進度與錯題本（CSV）
```

**使用者**：第一次開啟時有起始流程（歡迎 → 工作區 → 使用者 → 學習語言 → Claude → 興趣 → 完成）。之後從頂列的使用者選單切換或新增使用者；選名字即可進入，不需要密碼。同一個工作區的成員看得到彼此的所有檔案（包括單字庫），適合家人；朋友請各自用自己的工作區（學習群組在之後的階段加入）。第一版的單人工作區會在開啟時詢問名字，搬到 `profiles/〈你〉/`（Git repository 裡用 `git mv`，不刪任何檔案）。

**每位使用者有自己的 Claude**：Claude Code 的登入資料夾（`CLAUDE_CONFIG_DIR`）與 API key 都依使用者分開，同一台電腦上也不會用到別人的帳號。

在「設定 → 工作區與同步」三選一：**開啟資料夾**（已有的 clone）、**建立新的**（從範本建立並推到你空白的 private repo）、**從 repo 下載**（第二台電腦）。也可以手動指定：

- 環境變數 `JELEARNER_WORKSPACE=/path/to/JELearner-private`
- 或設定檔 `JELearner/config.toml` 寫 `workspace_path = "…"`（macOS：`~/Library/Application Support/`；Linux：`~/.config/`；Windows：`%APPDATA%`）

**資料庫不在任何 repo 裡**：SQLite 放在設定資料夾的 `JELearner/data/<工作區 id>/<使用者 id>/learner.sqlite`，每位使用者各有一個。開啟工作區時會把檔案讀進資料庫，所以資料庫隨時可以重建；複習進度與錯題按「同步」或「匯出複習紀錄」時寫進 `records/`，換電腦也能還原。

欄位格式見 [`docs/data-formats.md`](docs/data-formats.md)。CSV 的標題很寬鬆（`term`／`word`／`單字`、`reading`／`読み`、`meaning`／`中文`…），從試算表或單字卡 App 匯出的檔案通常可以直接用。

## 新聞與教材

- 新聞來源寫在工作區的 `sources.toml`（預設 BBC Technology、NHK NEWS WEB；ITmedia、NPR 預設關閉）。每個網址抓取前都會檢查該網站的 `robots.txt`，同一網站的請求至少間隔 1.5 秒，User-Agent 標明 `JELearnerBot`。教材只供個人學習，請勿轉載原文。
- 「產生教材」可以選新聞、貼網址或貼文字。模型回傳固定格式的 JSON（單字、文法、問答），再由 [`prompts/lesson.md.j2`](prompts/lesson.md.j2) 排成 Markdown；提示詞也在 [`prompts/`](prompts/)。想改格式就在工作區的 `prompts/` 放同名檔案，會取代內建版本。
- 教材存成 `profiles/<使用者>/lessons/<語言>/<日期>-<標題>.md`，單字與文法同時加入複習；問答答錯的自動進錯題本。

## Claude

和 PaperComposer 一樣有兩種連線方式，在「設定 → Claude」選擇，隨時可以切換（狀態列會顯示目前用哪一種）：

- **登入帳號（訂閱）**：透過 [Claude Code](https://code.claude.com/docs/en/setup) 用你自己的 Claude Pro／Max 帳號，用量算在你的訂閱額度內。App 會偵測是否安裝 Claude Code、顯示官方安裝指令，按「用瀏覽器登入」後自動偵測登入完成。每次呼叫都在空的暫存資料夾執行 `claude -p`，關閉所有工具、不載入任何外掛設定（`--safe-mode`），並移除 `ANTHROPIC_API_KEY` 等環境變數，以免改走 API 計費。
- **API key**：按用量計費。每位使用者的 key 分開存在系統金鑰圈（macOS 鑰匙圈），沒有金鑰圈時才存在設定資料夾的 `credentials/`（僅本人可讀）；也可以用環境變數 `ANTHROPIC_API_KEY`（開發用，對這個程序的所有使用者生效）。

「使用」預設為**自動**：Claude Code 已用訂閱帳號登入就用訂閱，否則用 API key；也可以固定其中一種（`config.toml` 的 `[engine] kind = "auto" | "subscription" | "api" | "llama-server"`）。兩種方式都有「測試連線」，會標明走的是訂閱還是 API。

模型清單與預設值在 [`models/catalog.json`](models/catalog.json) 的 `cloud`，或 `config.toml` 的 `[claude] model`／`effort`，兩種連線方式共用，程式裡不寫死模型名稱。

### 關於「登入帳號（訂閱）」模式

這個模式在你的電腦上執行官方、未修改的 Claude Code，用你自己的 Claude 帳號登入。登入在 Anthropic 的頁面完成，JELearner 不會接觸、保存或轉送你的帳號憑證；請求不經過任何第三方伺服器，用量只計入你自己的訂閱額度。

Anthropic 的[使用規定](https://code.claude.com/docs/en/legal-and-compliance)寫明，訂閱額度以個人的一般使用為前提，並且不允許第三方應用代替使用者透過訂閱方案送出請求。這個模式是作者為自己的使用情境所寫；是否使用請自行判斷。若想避免疑慮，請改用 API key 模式（依用量計費，這是 Anthropic 建議開發者使用的方式）。

JELearner 是非營利的個人專案，不代付、不轉售任何 Claude 用量。

## Gmail 與 Google 日曆

目前是**模擬模式**：週報、課程確認信、複習時段都放在「信件與行事曆」頁的模擬寄件匣，內容與真正寄出的一樣。程式已實作 Gmail `users.messages.send` 與 Calendar `events.insert`／`events.list` 的 REST 介面（以模擬伺服器測試），OAuth 登入在下一階段加入。

## 本機模型（Mac 到貨後）

[`models/catalog.json`](models/catalog.json) 列出預定的本機模型：大腦 Gemma 4 12B（llama.cpp `llama-server`，Q4_K_XL 或 Q8_0）、耳朵 Whisper large-v3-turbo、嘴巴 Kokoro-82M，總預算 20 GB。「設定 → 本機模型」會顯示估算；llama-server provider 已寫好並以模擬伺服器測試，**尚未在實機上測試**。

## 測試

```sh
cargo test
```

全部離線：核心邏輯的單元測試，加上 [`crates/server/tests/e2e.rs`](crates/server/tests/e2e.rs) 的端到端測試——用模擬的新聞網站（含 robots.txt）、Claude API、Gmail 與 Calendar，透過 HTTP 走完「連接工作區 → 抓新聞 → 被 robots.txt 擋下 → 產生教材 → 測驗與錯題 → 匯入 CSV／JSON → 複習 → 匯出紀錄 → 週報與排課 → 重新啟動 → 第二台電腦還原」。

## 專案結構

```
crates/core         工作區、SQLite、匯入、爬蟲、robots.txt、provider trait（Claude API、Claude 訂閱、llama-server）、教材生成、Google trait
crates/server       本機伺服器（127.0.0.1），之後的 Tauri 版共用同一組 handler
web/                介面（HTML / CSS / JS，不需要建置）
prompts/            教材的提示詞與 Markdown 模板（可被工作區覆蓋）
models/catalog.json 雲端與本機模型清單
workspace-template/ 私人工作區範本與示範資料
docs/               架構、決策、資料格式、介面設計、路線圖
scripts/            開發啟動與 Git hooks
```

## API

| 方法 | 路徑 | 說明 |
| --- | --- | --- |
| GET | `/api/status` | 工作區、結構（v1／v2）、使用者清單、目前使用者與等級、資料庫位置、引擎、各語言統計 |
| POST | `/api/users` `{ name, color, languages, majors, daily_minutes, interests }` | 新增使用者並以他進入 |
| POST | `/api/users/select` `{ id }` | 切換使用者（記在這台電腦） |
| PUT | `/api/user` `{ name, color, languages, majors, daily_minutes, report_email, seconds_per_turn, turns, interest_ratio, interests }` | 修改目前使用者的 profile.toml |
| POST | `/api/workspace/migrate` `{ name }` | 第一版單人工作區轉成多使用者結構 |
| POST | `/api/workspace/reindex` | 重新讀取工作區檔案 |
| POST | `/api/setup/workspace` `{ path, init }` | 開啟資料夾 |
| POST | `/api/setup/new` `{ path, remote? }` | 從範本建立（可推到 private repo） |
| POST | `/api/setup/clone` `{ url, parent }` | clone 自己的 private repo |
| GET / POST | `/api/git/status`、`/api/git/pull`、`/api/git/sync` | 同步私人工作區 |
| GET | `/api/sources` | 新聞來源 |
| GET | `/api/news?lang=&refresh=` | 新聞列表（refresh 時先抓 feed） |
| GET / POST | `/api/articles`（POST `{ url, lang }`） | 文章；POST 抓單一網址 |
| GET / POST | `/api/lessons`（POST `{ article_id \| url \| text, lang, level }`） | 教材列表／產生教材 |
| GET | `/api/lessons/{id}` | 教材內容 |
| POST | `/api/lessons/{id}/answers` `{ answers: [{ index, answer, correct? }] }` | 作答；答錯進錯題本 |
| GET / POST / DELETE | `/api/words`、`/api/grammar`（`?lang=&q=`） | 生字與文法；手動新增會寫進工作區 CSV |
| GET / POST | `/api/review`（POST `{ kind, id, grade }`） | 到期卡片／評分（1–5） |
| GET / POST | `/api/mistakes`、`/api/mistakes/{id}/resolve` | 錯題本 |
| GET | `/api/materials` | 每週素材 |
| POST | `/api/import?kind=vocab\|grammar\|materials\|auto&name=&lang=`（本文為檔案內容） | 匯入並存進工作區；`auto` 自動判斷類型（拖放） |
| GET | `/api/today?lang=` | 今日任務（複習、對話、教材測驗、錯題）與等級、今日積分、連續天數 |
| GET | `/api/progress?lang=` | 進度頁：升級條件 1–3、分技能積分、最近 14 天、主題、最近得分 |
| POST | `/api/progress/assess` `{ lang }` | 立即檢查升級（每個單元結束後也會自動檢查） |
| POST | `/api/difficulty/feedback` `{ lang, feedback: too_easy\|right\|too_hard }` | 前 5 課的難度回饋（±15） |
| POST | `/api/passive` `{ lang, kind: reading\|listening, seconds }` | App 內閱讀／聆聽時間（每 50 分鐘 1 分，無法手動登記） |
| POST | `/api/lessons/{id}/finish` `{ answers }` | 教材測驗結算：積分（每份教材一次）、難度與升級 |
| GET / POST | `/api/conversations`（POST `{ lang, topic_id? }`） | 對話列表／開始一段對話（選題、目標字、AI 開場） |
| GET | `/api/conversations/next?lang=&skip=` | 下一段對話的主題（不呼叫模型） |
| GET | `/api/conversations/{id}` | 對話內容 |
| POST | `/api/conversations/{id}/turns` `{ text }` | 一回合：換句話說＋下一個問題；最後一回合後鎖定 |
| POST | `/api/conversations/{id}/finish` | 評估、積分、存成 `conversations/<lang>/*.md` |
| POST / DELETE | `/api/user/avatar`（本文為圖片，≤ 2 MB） | 上傳或移除大頭貼（存在這台電腦，不進工作區） |
| GET | `/api/avatars/{user}` | 大頭貼 |
| POST | `/api/records/export` | 寫出 records/progress.csv、mistakes.csv |
| GET | `/api/claude?fresh=` | 兩種連線方式的狀態（Claude Code 是否安裝、登入）、目前使用的引擎 |
| POST | `/api/settings/claude` `{ key, model, effort }` | 存入或移除 API key、選擇模型 |
| POST | `/api/settings/engine` `{ kind }` | 選擇引擎：auto、subscription、api、llama-server |
| POST | `/api/claude-code/login` | 啟動 `claude auth login --claudeai`（在瀏覽器登入） |
| POST | `/api/engine/test` `{ engine? }` | 測試目前的引擎，或指定 subscription／api |
| GET | `/api/models` | 模型清單與記憶體估算 |
| GET | `/api/google`、`/api/google/outbox` | Google 狀態、模擬寄件匣 |
| POST | `/api/google/weekly-report`、`/api/google/schedule-reviews`、`/api/google/book-lesson` | 週報、排複習、排課 |

## 防止洩漏個人資料

```sh
sh scripts/install-hooks.sh
```

安裝 commit 前的金鑰掃描（需要 [gitleaks](https://github.com/gitleaks/gitleaks)）。同步私人工作區時，App 也會在 commit 前檢查新增內容，看起來像 API key 或 token 就拒絕 commit。

## 開發說明

本專案由 ellrin 開發與維護，程式碼在 Claude Code（Anthropic）協助下撰寫。
