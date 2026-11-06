# 私人工作區的檔案格式

所有檔案都是 UTF-8。CSV 可以用逗號或 Tab 分隔（看標題列哪一種比較多），第一列必須是標題；以 `#` 開頭的列會略過。

## 結構

一個工作區可以有好幾位使用者，每人一個 `profiles/<id>/`：

```
workspace.toml          工作區 id、名稱、管理員、成員
sources.toml            新聞來源（共用）
topics.toml             通識主題（共用）
prompts/                自訂提示詞（共用）
profiles/<id>/
  profile.toml          這位使用者的設定
  interests.csv         興趣關鍵字
  vocab/ grammar/ materials/ lessons/ conversations/ records/
  recipes/              課程（大綱＋進度，docs/courses.md §8）
    lists/vocab|grammar/  為固定課程上傳的單字表、文法表（不直接進複習）
  notebook/<lang>/      復習筆記本（一課一頁、一段對話一頁）
```

以下各節中的 `vocab/`、`grammar/`、`materials/`、`lessons/`、`records/` 都是指 `profiles/<id>/` 底下。

第一版（單人）把這些資料夾放在最上層。App 偵測到時會徵求同意，把它們搬到 `profiles/<id>/`（Git repository 裡用 `git mv`），不刪任何檔案，並把第一版 `workspace.toml` 裡的名字、語言、程度、週報信箱移到 `profile.toml`。

## `vocab/*.csv`：想背的單字

| 欄位 | 也接受的標題 | 必填 |
| --- | --- | --- |
| `term` | word、vocab、front、expression、單字、單詞、単語 | ✓ |
| `reading` | kana、furigana、pronunciation、ipa、讀音、読み | |
| `meaning` | translation、definition、back、中文、意思、解釋、意味 | |
| `pos` | part_of_speech、詞性、品詞 | |
| `example` | sentence、例句、例文 | |
| `tags` | tag、標籤、タグ | |
| `note` | notes、memo、備註、メモ | |
| `lang` | language、語言（`en`／`ja`） | |

沒有 `lang` 欄時，語言由檔名判斷（`*-ja.csv`、`*_en.csv`、`ja/…`），再不行就用這位使用者的第一個學習語言。同一個（語言、單字、讀音）只會有一筆；重複匯入時，空白欄位不會蓋掉已有的內容。

在 App 裡手動新增的單字寫進 `vocab/my-words-<lang>.csv`。

```csv
term,reading,meaning,pos,example,tags,note
挑戦,ちょうせん,挑戰,名詞・サ変,新しい技術に挑戦する。,N3,
```

## `grammar/*.csv`：想學的文法

| 欄位 | 也接受的標題 | 必填 |
| --- | --- | --- |
| `pattern` | grammar、point、structure、文法、句型、文型 | ✓ |
| `meaning` | translation、意思、意味 | |
| `explanation` | usage、description、說明、解説、用法 | |
| `example` | sentence、例句、例文 | |
| `level` | jlpt、cefr、程度、級 | |
| `tags`、`lang` | 同上 | |

手動新增的文法寫進 `grammar/my-grammar-<lang>.csv`。

## `materials/<週>/*.json`：每週素材

```json
{
  "week": "2026-W45",
  "title": "本週素材",
  "language": "ja",
  "items": [
    { "kind": "word", "term": "規制", "reading": "きせい", "meaning": "管制", "example": "…", "tags": "" },
    { "kind": "grammar", "pattern": "〜をめぐって", "meaning": "圍繞著…", "explanation": "…", "example": "…" },
    { "kind": "article", "title": "短文", "url": "", "text": "本文…" },
    { "kind": "note", "text": "自己的備註" }
  ]
}
```

- `week` 是 ISO 週（`YYYY-Www`），`language` 是 `en` 或 `ja`。
- `word`、`grammar` 加入資料庫與複習；`article` 出現在「新聞」頁（可以做成教材）；`note` 只保存。
- 從 App 上傳時會存成 `materials/<week>/<檔名>.json`，同名時自動加 `-2`。

## `sources.toml`：新聞來源

```toml
[[source]]
id = "bbc-technology"
name = "BBC News – Technology"
lang = "en"
feed = "https://feeds.bbci.co.uk/news/technology/rss.xml"
enabled = true            # 可省略，預設 true
body_selector = "article" # 可省略：該網站文章內文的 CSS selector
```

## `workspace.toml`

```toml
id = "ws-…"               # 第一次開啟時自動產生；決定本機資料庫，請勿更改
name = "Ellrin 的工作區"
admins = ["ellrin"]       # 第一位使用者自動成為管理員

[[member]]
id = "ellrin"             # = profiles/ 底下的資料夾名稱
name = "Ellrin"
kind = "owner"            # owner | household

[[member]]
id = "daidai"
name = "daidai"
kind = "household"
```

成員由 App 管理（設定 → 使用者、頂列的使用者選單）。其他裝置新增、但還沒列在這裡的 `profiles/<id>/` 也會出現在使用者清單中。

## `profiles/<id>/profile.toml`

```toml
id = "ellrin"
name = "Ellrin"
color = "#ff6b35"
languages = ["en", "ja"]
native_language = "zh-TW" # 解說用的語言
daily_minutes = 30
report_email = "you@example.com"   # 可省略
timezone = "Asia/Taipei"

[lang.en]
level = "B1-3"            # 等級（只升不降）
difficulty = 255          # 教材難度分數：每個大級 100 分（英文 A1–C2 = 0–600，日文 N5–N1 = 0–500）
early_feedback_left = 5   # 還要在幾課結束時詢問難度
level_since = "2026-11-06T09:00:00+08:00"  # 升到目前小階的時間（App 寫入；之後的積分算進下一階）

[lang.ja]
level = "N5-1"
difficulty = 0
early_feedback_left = 5

[conversation]
seconds_per_turn = 15
turns = 20
interest_ratio = 0.6      # 興趣主題比例，其餘為通識
```

等級顯示為「大級-小階」：每個大級 4 小階，日文 N5 為 6 小階（`level.rs`）。自評時只選大級；在設定中改成另一個大級，該語言會從那個大級的第一小階重新開始。

## `profiles/<id>/interests.csv`

```csv
keyword,lang,weight,updated_at
半導體,,1,2026-11-05T12:00:00+08:00
```

`lang` 留空表示兩種語言都適用。起始流程與設定中選的主題會寫進這裡；課後的關鍵字回饋（之後的階段）也會更新權重。

## `topics.toml`：通識主題

```toml
[[topic]]
id = "school"
name = "校園與社團"
group = "生活"
```

課程與對話會把使用者的興趣和這些通識主題混合（比例在 `profile.toml` 的 `interest_ratio`：讓最近十段對話中興趣主題的比例最接近它），同類主題中選最久沒聊的，所以通識主題會輪流出現。可自由增減；檔案不存在或格式錯誤時用內建清單。

## `lessons/<lang>/*.md`：教材

App 產生，YAML front matter（title、language、level、date、source、engine）＋ Markdown 本文，最後一行是內嵌資料的 HTML 註解（`<!-- jelearner:lesson {…} -->`），用來在其他電腦重建資料庫。可以手動修改本文；內嵌資料只在資料庫還沒有這份教材時讀取。

課程的課（`<日期>-<課程>-<課號>.md`）front matter 多 `course`、`course_lesson`、`course_role`、`units`，內嵌資料多課文（`text`，日文以 `{漢字|かんじ}` 標音）、輸出題與對話計畫；重建資料庫時整份讀回，並記下它屬於哪個課程（docs/courses.md §8.2）。

教材檔只在那一課產生（打開）後才存在，所以**重建資料庫時教材裡的新字與文法會重新排進複習**（單課教材與課程的課都一樣；複習進度再由 `records/progress.csv` 還原）。

## `records/`：紀錄匯出

- `progress.csv`：`kind,lang,key,reading,ease,interval_days,reps,lapses,due_at,last_reviewed_at`（以文字對應單字／文法，不用資料庫 id）。
- `mistakes.csv`：`lang,question,user_answer,correct_answer,explanation,created_at,resolved_at`。

按「同步」或「設定 → 匯出複習紀錄與錯題」時寫出。新電腦第一次開啟工作區、資料庫還沒有任何紀錄時自動還原。

## `records/points/YYYY-MM-DD.csv`：積分紀錄

一天一個檔案（以紀錄時間的當地日期命名），**只新增、不修改**，多台電腦同步不會衝突；資料庫的 `points`、`units` 表每次開啟工作區時從這些檔案補齊，換電腦也能還原。

```csv
time,lang,unit,skill,points,score,detail
2026-11-06T09:12:00+08:00,en,conversation,writing,1.5,0.87,conversations/en/2026-11-06-校園與社團.md
2026-11-06T09:30:00+08:00,en,quiz,reading,0,0.5,lessons/en/2026-11-06-chips.md
2026-11-06T10:00:00+08:00,en,reading,reading,1,,50 min
```

- `unit`：`conversation`（對話）、`quiz`（教材測驗）、`review`（一輪複習卡片）、`reading`／`listening`（被動時間，`score` 空白）；課程的單元另有 `reading`（閱讀理解，有 `score`）、`translation`、`sentence-making`、`cloze`、`paraphrase`。
- `skill`：`listening`、`reading`、`speaking`、`writing`、`vocab`（單字文法；也接受 `grammar`）。打字版對話記為 `writing`。
- `points`：未通過＝0（照樣記錄，作為近期表現）；通過＝1；正確率 ≥ 95% 或對話評估優秀再 +0.5。
- `detail`：相對於 `profiles/<id>/` 的檔案路徑，或 `50 min`、`20 張`。課程單元是 `<教材檔>#<單元序號>`（例如 `lessons/en/2026-11-06-ai-chips-02.md#1`），資料庫的 `units` 表從這裡補上 `course`、`course_lesson`、`unit_index`，換電腦也一樣。

## `conversations/<lang>/*.md`：對話紀錄

完成（評估過）的對話存成 `<日期>-<主題>.md`（不覆蓋，同名加 `-2`）：front matter（title、language、level、topic、date、turns、engine）、逐回合對話（AI 的換句話說以 `> ↺` 標示）、回饋（評分、可以說得更好的句子、目標字、建議加入的字、關鍵字），最後一行是內嵌資料 `<!-- jelearner:conversation {…} -->`，用來在其他電腦重建資料庫（含目標字的主動使用次數與主題紀錄）。版面模板是 `prompts/conversation.md.j2`。

## `notebook/<lang>/*.md`：復習筆記本

一課一頁、一段對話一頁，檔名與教材（或對話）檔同名；頁面以 front matter 的 `source` 對應它的教材或對話。分兩部分，**App 重新產生時只換第一部分**：

```markdown
---
title: "Inside the fab"
kind: lesson            # lesson | conversation
lang: en
date: 2026-11-06
source: "profiles/ellrin/lessons/en/2026-11-06-ai-chips-02.md"
---

# Inside the fab

<!-- jelearner:auto 這一段由 App 產生，重新產生時會整段取代；自己的筆記請寫在「我的筆記」下面 -->
## 錯題（1）            答錯的題目、你的答案、正確答案與解析
## 可以這樣說           對話的「原句 → 建議」
## 新單字／新文法       這一課的字與文法（日文的漢字加 <ruby> 標音）
<!-- /jelearner:auto -->

## 我的筆記

自己寫的內容……

![](jl-image:3f9c0a1b2c3d4e5f6a7b8c9d)

<!-- jelearner:notebook {…App 的資料…} -->
```

- 課的單元每次結算、對話完成時，App 建立或更新這一頁的 App 區段；「我的筆記」只有在你於 App 儲存（或自己用編輯器改）時才會變。
- 自己把 `jelearner:auto` 標記刪掉的頁面，App 就不再改寫它。
- **圖片不在工作區、不會同步**：`![](jl-image:<id>)` 只是參照，圖片檔存在加圖片那台電腦的 `<config dir>/JELearner/notebook-images/<工作區 id>/<使用者 id>/<id>.jpg`（或 `.webp`／`.png`），與大頭貼一樣不進 Git。瀏覽器先壓縮（長邊 ≤ 1600 px、WebP 或 JPEG 約 0.8），上限 3 MB。在別台電腦打開同一頁會顯示「圖片只在原本的電腦上」。

## `recipes/*.toml`：課程

課程的大綱檔與進度檔（`<課程 id>.toml`、`<課程 id>.progress.toml`）格式見 [`courses.md`](courses.md) §8。大綱檔建立後 App 不再改寫（延伸主題課程時只在檔尾加課）；進度檔由 App 寫。每日課程（`kind = "daily"`）也存在這裡，一天一個。

`recipes/lists/vocab/*.csv`、`recipes/lists/grammar/*.csv`：建立固定課程時上傳的清單（欄位與 `vocab/`、`grammar/` 相同，或一行一個字），存成不重複的檔名（`toeic-en.csv`、`toeic-en-2.csv`…），從不覆蓋。**工作區匯入不讀這個資料夾**：清單裡的字要等那一課打開、產生教材後才進資料庫與複習排程，建立 300 個字的課程不會讓今天多出 300 張卡片。已經放在 `vocab/` 的清單本來就是自己的單字庫，選它建課程時不會重複排程。

## `progress.toml`：積分、等級、難度規則（可選）

內建數值在程式 repo 的 `config/progress.toml`（積分門檻依第二版規劃 §9.1）。工作區最上層放同名檔案，只寫想改的鍵即可覆蓋，例如：

```toml
[points]
bonus = 0            # 關閉高分加成
[levels.en]
B1 = 1200
```

## 大頭貼（不在工作區）

使用者的大頭貼只存在**每台電腦**的 `<config dir>/JELearner/avatars/<工作區 id>/<使用者 id>.jpg`（與資料庫放在一起），不進 Git、不會同步；換電腦要重新上傳。

## `prompts/`：自訂提示詞

放和內建檔案同名的檔案即可取代：`lesson_system.md`、`lesson_user.md`、`lesson.md.j2`、`conversation_en.md`、`conversation_ja.md`、`conversation_review.md`、`conversation.md.j2`（[MiniJinja](https://docs.rs/minijinja) 語法）。可用的變數見內建版本（repo 的 `prompts/`）。
