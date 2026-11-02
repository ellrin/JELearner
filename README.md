# JELearner

本機運作的英文／日文學習秘書：自動抓英日新聞、整理成單字與文法教材、排複習、記錯題、寄週報、排行事曆。之後加上即時語音對話與數位人像，並包成 Mac app。

> 開發初期（第一版：文字管線）。已完成：私人工作區、SQLite（生字／文法／複習進度／錯題本）、英日新聞爬蟲（遵守 robots.txt）、新聞 → Markdown 教材（模板可替換）、Claude provider、Gmail／Calendar 介面與模擬實作、本機網頁、離線端到端測試。
> 尚未完成：向量庫與 RAG、Google OAuth、本機模型（等 Mac）、語音、Tauri 打包、數位人像。見 [`docs/roadmap.md`](docs/roadmap.md)。

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

生字、文法、每週素材、教材都是**個人資料，放在你自己的 private repository**（例如 `JELearner-private`），不要放進這個 repo。結構（範本見 [`workspace-template/`](workspace-template/)）：

```
JELearner-private/
  workspace.toml   名字、各語言程度、週報信箱
  sources.toml     新聞來源（RSS / Atom）
  vocab/           想背的單字（CSV，可放多個檔案）
  grammar/         想學的文法（CSV）
  materials/       每週素材（materials/2026-W45/*.json）
  lessons/en|ja/   產生的教材（Markdown）
  records/         複習進度與錯題本的匯出（CSV）
  prompts/         自訂提示詞與教材模板（可選）
  avatar/          數位人像設定（之後）
```

在「設定 → 工作區與同步」三選一：**開啟資料夾**（已有的 clone）、**建立新的**（從範本建立並推到你空白的 private repo）、**從 repo 下載**（第二台電腦）。也可以手動指定：

- 環境變數 `JELEARNER_WORKSPACE=/path/to/JELearner-private`
- 或設定檔 `JELearner/config.toml` 寫 `workspace_path = "…"`（macOS：`~/Library/Application Support/`；Linux：`~/.config/`；Windows：`%APPDATA%`）

**資料庫不在任何 repo 裡**：SQLite 放在設定資料夾的 `JELearner/data/<工作區 id>/learner.sqlite`，每個工作區（也就是每位使用者）各有一個。開啟工作區時會把檔案讀進資料庫，所以資料庫隨時可以重建；複習進度與錯題按「同步」或「匯出複習紀錄」時寫進 `records/`，換電腦也能還原。

欄位格式見 [`docs/data-formats.md`](docs/data-formats.md)。CSV 的標題很寬鬆（`term`／`word`／`單字`、`reading`／`読み`、`meaning`／`中文`…），從試算表或單字卡 App 匯出的檔案通常可以直接用。

## 新聞與教材

- 新聞來源寫在工作區的 `sources.toml`（預設 BBC Technology、NHK NEWS WEB；ITmedia、NPR 預設關閉）。每個網址抓取前都會檢查該網站的 `robots.txt`，同一網站的請求至少間隔 1.5 秒，User-Agent 標明 `JELearnerBot`。教材只供個人學習，請勿轉載原文。
- 「產生教材」可以選新聞、貼網址或貼文字。模型回傳固定格式的 JSON（單字、文法、問答），再由 [`prompts/lesson.md.j2`](prompts/lesson.md.j2) 排成 Markdown；提示詞也在 [`prompts/`](prompts/)。想改格式就在工作區的 `prompts/` 放同名檔案，會取代內建版本。
- 教材存成 `lessons/<語言>/<日期>-<標題>.md`，單字與文法同時加入複習；問答答錯的自動進錯題本。

## Claude

在「設定 → Claude」貼上 API key：存在系統金鑰圈（macOS 鑰匙圈），沒有金鑰圈時才存在設定資料夾的 `credentials.toml`（僅本人可讀）；也可以用環境變數 `ANTHROPIC_API_KEY`。模型清單與預設值在 [`models/catalog.json`](models/catalog.json) 的 `cloud`，或 `config.toml` 的 `[claude] model`／`effort`，程式裡不寫死模型名稱。

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
crates/core         工作區、SQLite、匯入、爬蟲、robots.txt、provider trait、教材生成、Google trait
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
| GET | `/api/status` | 工作區、資料庫位置、引擎、各語言統計 |
| PUT | `/api/workspace` `{ owner, report_email, levels }` | 修改 workspace.toml |
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
| POST | `/api/import?kind=vocab\|grammar\|materials&name=&lang=`（本文為檔案內容） | 匯入並存進工作區 |
| POST | `/api/records/export` | 寫出 records/progress.csv、mistakes.csv |
| GET / POST | `/api/claude`、`/api/settings/claude` `{ key, model, effort }` | Claude 設定 |
| POST | `/api/engine/test` | 測試目前的引擎 |
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
