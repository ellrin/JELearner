# 課程引擎

課程引擎的設計、檔案格式與整合方式。對應 `docs/plan.md` §5（難度）、§7（主題）、§8（積分）、§11（每日課程）、§12（題型）、§13（配方）、§14（興趣）。程式在 `crates/core/src/course/`。

## 1. 三層結構

| 層 | 意思 | 例子 |
| --- | --- | --- |
| 課程 Course | 一系列課，有主題或素材來源 | 「AI 晶片」主題課程（5 課）、「TOEIC 單字」固定課程（38 課） |
| 課 Lesson | 坐下來一次做完的份量，3–5 個單元 | 第 2 課：Inside the fab |
| 單元 Unit | 一個可完成、可判斷通過的練習；**完成並通過＝1 分** | 閱讀理解 5 題、對話 20 回合 |

## 2. 課程種類（`CourseKind`）

| 種類 | 檔案值 | 狀態 |
| --- | --- | --- |
| 每日課程 | `daily` | 資料模型＋主題選擇函式（興趣／通識輪替）；每日組合介面之後做 |
| 主題課程 | `topic` | **完整**：大綱產生、驗證、延伸、開課產生內容 |
| 固定課程 | `fixed` | **完整**：單字表／文法表切課（不需模型）、可選模型命名、開課產生內容 |
| 單課 | `single` | 第一版已有（`lesson.rs`），行為不變 |
| 對話課程 | `conversation` | 資料模型＋單元配置（整課一段對話） |
| 弱項複習 | `weakness` | 資料模型（每週，從錯題與忘記的卡片） |
| 程度檢核 | `check` | 資料模型＋單元配置（測驗＋翻譯＋對話） |

課在課程中的角色（`LessonRole`）決定單元配置與提示詞：`regular`、`background`（主題第 1 課）、`article`（主題第 2–4 課）、`review`、`conversation`、`check`。

## 3. 產生時機

- **建立課程時只產生大綱**：每課的標題、角度、目的、目標單字（含讀音／意思）、目標文法、單元配置、搜尋關鍵字、對話主題。
- **學習者打開某一課時才產生完整內容**：新聞比較新、難度依最近表現調整、不浪費額度。
- 已產生的課再次打開時直接讀檔，不再呼叫模型；教材檔被手動刪掉時會重新產生。

## 4. 每課份量（`sizing.rs`）

### 4.1 新字、文法、課文長度（依語言與教材難度的大級，一張表）

| 語言・大級 | 固定課程新字 | 主題課程新字 | 文法 | 課文 | 假名標音 |
| --- | --- | --- | --- | --- | --- |
| 英文 A1 | 6 | 4–6 | 1 | 80–150 字 | |
| 英文 A2 | 8 | 5–8 | 1 | 150–200 字 | |
| 英文 B1–B2 | 10 | 6–10 | 1–2 | 200–300 字 | |
| 英文 C1–C2 | 12 | 8–12 | 1–2 | 300–450 字 | |
| 日文 N5 | 6 | 4–6 | 1 | 100–200 字元，短句 | ✓ |
| 日文 N4 | 8 | 5–7 | 1 | 200–300 字元 | ✓ |
| 日文 N3 | 10 | 6–8 | 1–2 | 300–500 字元 | |
| 日文 N2–N1 | 12 | 8–10 | 1–2 | 500–800 字元 | |

作者訂的是英文 B1–B2 與日文 N5 兩列；其他列是推估，實際使用後調整。

### 4.2 單元配置（依 `profile.daily_minutes`）

基本分鐘數：

| 每日目標 | 複習 | 課文 | 輸出 | 對話 | 第二個輸出 |
| --- | --- | --- | --- | --- | --- |
| < 15 | 3 | 5 | 2 | – | – |
| 15–24 | 3 | 7 | – | 5 | – |
| 25–29 | 3 | 7 | 5 | 10 | – |
| 30–44 | 5 | 10 | 5 | 10 | – |
| ≥ 45 | 5 | 10 | 5 | 10 | 5 |

剩下的分鐘一次一分鐘依序分給「對話、課文、輸出、複習、第二個輸出」，各有上限（15、15、10、10、10），全部到上限時課程短於目標（最長 60 分鐘）。

預設 30 分鐘：複習 5 分（20 張卡）、課文 10 分（閱讀理解 5 題）、輸出 5 分（3 題）、對話 10 分（20 回合，主題延續課文）。

- 數量：複習卡＝分鐘×4；閱讀題＝分鐘÷2（3–8）；對話回合＝分鐘×2（6–30）；輸出題＝分鐘×3÷5（2–6）；克漏字＝分鐘（4–12）。
- 輸出題型依課號輪替：單數課「中翻英／中翻日」（`translation`），雙數課「造句」（`sentence-making`）；第二個輸出單數課克漏字、雙數課換句話說。
- 複習課：課文換成整理前面課文的克漏字，輸出用造句。
- 對話課：一段對話（5–30 分鐘）。檢核：測驗 40%、翻譯 20%、對話其餘。
- 技能：複習／克漏字＝單字文法，閱讀＝讀，翻譯／造句／換句話說＝寫，對話＝說（`UnitKind::skill`）。

## 5. 主題課程

1. **關鍵字**：請求中的關鍵字；請求沒有關鍵字、網址或照片文字時，用 `interests.csv` 權重最高的 3 個（同語言或未指定語言）。
2. **長度**：預設 5 課（約一週）。伺服器先搜尋新聞時可傳入找到的篇數，少於 3 篇改為 3 課；模型回 `enough_material = false` 時也縮成 3 課（保留第 1 課、第一篇文章課與最後的複習課）。
3. **結構**：第 1 課背景與核心單字；第 2–4 課三篇不同角度的文章（technical、social、personal life）；第 5 課複習＋用本週單字的主題對話（沒有新字，文法＝本系列全部文法）。
4. **後處理（不靠模型，確定性）**：課數正確、角色依位置、新字不重複、去掉已會的字、新字與文法不超過該級上限、補角度與搜尋關鍵字。
5. **單字重現**：每個新字至少出現在 `min(3, 從引入那課到最後剩下的課數)` 課。最後的複習課包含全部新字；其餘不足的次數加到後面課的 `review`，同一課的單字單雙號分別先排到下一課與下下課，讓複習字分散。重跑結果不變。第 4 課的字只能出現 2 次（第 4、5 課），延伸時會帶入下一個系列。
6. **延伸**：全部課完成後（`Course::can_extend`）可以再延伸一個系列（預設 5 課，課號接續，`series` +1）；前一系列出現不到 3 次的字列入新系列第 1 課與最後一課的複習。也可以改選新主題（建立新課程，舊的保持 `finished`）。

## 6. 固定課程

1. 讀單字表（與 `vocab/*.csv` 相同欄位，或一行一個字）與文法表（與 `grammar/*.csv` 相同欄位）。已會的字略過。
2. **切課（不需模型）**：單字課數＝`ceil(單字數 ÷ 每課新字)`，文法需要更多課時取較大者（每課最多 `grammar.max` 個）；文法平均分配，除不盡時前面的課多一個。
3. **複習課**：每第 5 個位置（5、10、15…）放一課複習，複習上一次複習後的所有單字與文法；最後沒有剛好結束在複習課時，再加一課複習。例：英文 B1 300 個字 → 30 課單字課＋8 課複習＝38 課。
4. 每課單字課也複習前 1 課與前 3 課的字（間隔重複），最多一課新字的量。
5. **命名（可選）**：一次模型呼叫寫出各課標題、角度、目的與對話主題；不改單字、文法與單元。失敗時保留「第 3 課：w1、w2、w3」這種標題。
6. **課文**：開課時若有含這些字的真實新聞（`plan::pick_article`：至少 2 個目標字且至少 30%），把新聞改寫成學習者的程度；否則生成一篇含這些字的短文。提示詞要求超出程度的句子簡化或替換。

## 7. 興趣與通識輪替（`topics.rs`）

- `pick_topic(day, interest_ratio, interests, general, lang)`：第 `n` 天是興趣日若且唯若 `floor((n+1)p) − floor(np) = 1`（`p`＝比例百分比），因此任何一段天數中興趣日的比例都接近設定值，且不會連續好幾天同一類。
- 興趣關鍵字用平滑加權輪替（權重 2 的字出現次數是權重 1 的兩倍，平均分散）；通識主題依 `topics.toml` 順序輪流。
- 只有一種主題時每天在那一種裡輪替；兩種都沒有回傳 `None`。
- `day_number(date)`＝1970-01-01 起的天數；可以加上每位使用者的位移當種子。

## 8. 檔案格式

### 8.1 課程大綱：`profiles/<id>/recipes/<course-id>.toml`

選 TOML：與 `docs/plan.md` §13 的配方檔同一個資料夾、同一種格式，可以手動修改（課程檔是配方的超集合：配方欄位＋`[[lesson]]` 大綱）。檔名就是課程 id：`<建立日期>-<名稱 slug>`，已存在時加 `-2`、`-3`，絕不覆蓋別的檔案。App 只會改寫課程自己的檔案（更新狀態）；改寫時保留檔頭說明，但手寫的其他註解會消失。

```toml
id = "2026-11-05-ai-chips-this-week"
kind = "topic"                 # daily | topic | fixed | single | conversation | weakness | check
name = "AI Chips This Week"
lang = "en"
level = "B1-1"                 # 建立時的教材難度
created = "2026-11-05"
status = "active"              # active | finished | archived
daily_minutes = 30
summary = "一週認識 AI 晶片"
keywords = ["AI chips"]
source_url = ""                # 主題課程：來源網址
# vocab = ["vocab/toeic-en.csv"]   固定課程：單字表
# grammar = []                     固定課程：文法表
# new_per_lesson = 10              固定課程
# review_every = 5                 固定課程
series = 1
engine = "claude:…"

[[lesson]]
number = 2
role = "article"               # regular | background | article | review | conversation | check
title = "Inside the fab"
angle = "technical"
purpose = "認識晶圓廠的補助與產能"
search = ["chip fab subsidy"]
conversation_topic = "Should governments pay for chip factories?"
status = "generated"           # planned | generated | done
minutes = 30
review = ["semiconductor"]     # 前面課的字在這課重現
file = "profiles/ellrin/lessons/en/2026-11-06-ai-chips-this-week-02.md"
generated = "2026-11-06"
units_done = [0, 1]            # 已通過的單元（從 0 起）

[[lesson.word]]
term = "subsidy"
reading = "/ˈsʌbsədi/"
meaning = "補助"

[[lesson.grammar]]
pattern = "so ... that ..."

[[lesson.unit]]
kind = "review"                # review | reading | translation | sentence-making | cloze | paraphrase | listening | dictation | shadowing | conversation | quiz
skill = "vocab-grammar"        # listening | reading | speaking | writing | vocab-grammar
minutes = 5
items = 20
```

所有欄位都有預設值，手寫的簡短檔案也能讀。`recipes/` 裡不是課程的配方（沒有 `[[lesson]]`）或格式錯的檔案，列表時略過並回報原因。

### 8.2 產生的教材：`profiles/<id>/lessons/<lang>/<日期>-<課程名>-<課號>.md`

沿用現有教材格式（`docs/data-formats.md`），向後相容：

- front matter 多四個欄位：`course`（課程 id）、`course_lesson`（課號）、`course_role`、`units`（例如 `"review:5,reading:10,sentence-making:5,conversation:10"`）。
- 本文多「本課安排」「今日課文」「輸出練習」「對話」。日文 N5–N4 課文以 `{漢字|かんじ}` 標音，Markdown 中轉成 `<ruby>`。
- 結尾的 `<!-- jelearner:lesson {…} -->` 內嵌資料是 `Lesson` 的超集合（多 `text`、`output`、`conversation`、`course`）；`lesson::extract_data` 照樣讀成一般教材，資料庫匯入不必改。

### 8.3 提示詞（`prompts/`，可用工作區同名檔案取代）

| 檔案 | 用途 |
| --- | --- |
| `course_outline_system.md`、`course_outline_user.md` | 主題課程大綱（`kind == "topic"`）與固定課程命名（`kind == "fixed"`） |
| `course_lesson_system.md`、`course_lesson_user.md` | 依課程計畫產生一課 |
| `course_lesson.md.j2` | 課程教材 Markdown |

全部用 JSON Schema 結構化輸出，經過 `LlmProvider`；程式裡沒有模型名稱。

## 9. 核心 API（`jl_core::course`）

| 函式 | 用途 |
| --- | --- |
| `Learner::from_profile(&profile, lang, known_words)` | 學習者資料（難度、每日目標、母語、已會的字） |
| `outline::create_topic_course(provider, templates, learner, &TopicRequest, interests, date)` | 主題課程大綱（一次模型呼叫） |
| `outline::extend_topic_course(...)` | 延伸一個系列 |
| `outline::create_fixed_course(learner, &FixedRequest, date)` | 固定課程（不呼叫模型） |
| `outline::describe_fixed(provider, templates, learner, &mut course)` | 可選：模型命名固定課程各課 |
| `fixed::read_word_list(text)`、`fixed::read_grammar_list(text)` | 讀上傳的清單 |
| `store::save_new` / `save` / `load` / `list` | 大綱檔案 |
| `plan::pick_article(candidates, terms, lang)` | 從新聞候選中挑含最多目標字的 |
| `plan::open_lesson(provider, templates, ws, course_id, number, learner, &LessonContext, date)` | 打開一課：未產生就產生並存檔、更新大綱 |
| `store::mark_unit(&mut course, number, unit, passed, score, date)` | 單元結果 → `UnitResult`（積分、技能、課完成、課程完成） |
| `topics::pick_topic(day, ratio, interests, general, lang)` | 每日課程的主題 |
| `sizing::content_size(lang, difficulty)`、`sizing::unit_plan(role, minutes, number)` | 份量表與單元配置 |

## 10. 整合步驟（伺服器／介面，尚未做）

伺服器需要的端點（handler 照現有慣例寫成 `Shared` 上的 async fn）：

| 端點 | 動作 |
| --- | --- |
| `POST /api/courses` | 建立課程。`{kind: "topic", lang, keywords, url?, text?, lessons?}`：可先用 `plan.search`／關鍵字搜尋新聞計算篇數（`available_articles`），再呼叫 `create_topic_course`。`{kind: "fixed", lang, name, vocab: [path], grammar: [path] 或上傳內容}`：`read_word_list` → `create_fixed_course` →（可選）`describe_fixed`。最後 `store::save_new`。網址要先用 `Fetcher::get_text`（遵守 robots.txt）抓文字；照片先由支援圖片的模型讀出文字 |
| `GET /api/courses` | `store::list`，附略過的檔案原因 |
| `GET /api/courses/:id` | `store::load`（大綱、每課狀態與單元） |
| `POST /api/courses/:id/lessons/:n/open` | 依 `plan.search` 從資料庫新聞挑候選並抓全文 → `pick_article` → `open_lesson`。`generated` 有值時照 `api.rs` 產生教材的做法寫入資料庫：`insert_lesson` 與新字、文法 `upsert`。`recent_accuracy` 由 A 階段的 `units` 表計算 |
| `POST /api/courses/:id/lessons/:n/units/:u` | `{passed, score}` → `mark_unit` → `store::save`；`UnitResult.points > 0` 時交給 A 階段的積分模組寫 `records/points/`（`unit`＝`UnitKind::code`、`skill`＝`Skill::code`、`detail`＝教材檔路徑） |
| `POST /api/courses/:id/extend` | `extend_topic_course` → `store::save` |

資料庫（之後由整合步驟改 `db.rs`，這次沒有動）：

- `lessons` 表加 `course`、`course_lesson` 欄（plan.md §17 已列 `recipe`，可用同一欄）；重建時從教材 front matter 讀。
- 不需要新的課程表：課程以檔案為準，列表直接讀檔即可；若要加快，可加 `courses(id, kind, lang, name, status, file, updated_at)` 快取表，從 `recipes/*.toml` 重建。
- 單元嘗試記錄在 A 階段的 `units` 表（類型、開始／結束、通過、正確率），加 `course`、`course_lesson`、`unit_index` 欄。

## 11. 尚未決定

1. **固定課程的複習位置**：目前解讀為「30 課單字課＋每第 5 個位置一課複習＋結尾複習」＝38 課。若作者的意思是「共 30 課、其中每第 5 課是複習」（每課新字變多），只要改 `fixed::split`。
2. **單字重現 ≥3 的範圍**：最後兩課引入的字不可能在同一系列出現 3 次；目前是「剩下幾課就出現幾課，最多 3 課」，不足的在延伸時帶入。另一種做法是第 4 課不放新字。
3. 表 4.1 中作者沒指定的級別（英文 A1–A2、C1–C2，日文 N4–N1）數值是推估。
4. 每天超過 60 分鐘時是否改成一天兩課，而不是拉長單元。
5. 課程檔的手寫註解在 App 更新狀態時會消失；若作者常手動改大綱，可改成狀態存另一個檔（`<id>.status.toml`）。
6. 主題課程的照片、網址文字只用於大綱；開課時的文章仍由 `plan.search` 搜尋。是否把原網址的文章直接當第 2 課的素材？
7. 弱項、每日、檢核課程的大綱產生與每日組合介面留待 B 階段。
