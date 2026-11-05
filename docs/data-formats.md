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
id = "member-2"
name = "第二位使用者"
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

課程與對話會把使用者的興趣和這些通識主題混合（比例在 `profile.toml` 的 `interest_ratio`），可自由增減。

## `lessons/<lang>/*.md`：教材

App 產生，YAML front matter（title、language、level、date、source、engine）＋ Markdown 本文，最後一行是內嵌資料的 HTML 註解（`<!-- jelearner:lesson {…} -->`），用來在其他電腦重建資料庫。可以手動修改本文；內嵌資料只在資料庫還沒有這份教材時讀取。

## `records/`：紀錄匯出

- `progress.csv`：`kind,lang,key,reading,ease,interval_days,reps,lapses,due_at,last_reviewed_at`（以文字對應單字／文法，不用資料庫 id）。
- `mistakes.csv`：`lang,question,user_answer,correct_answer,explanation,created_at,resolved_at`。

按「同步」或「設定 → 匯出複習紀錄與錯題」時寫出。新電腦第一次開啟工作區、資料庫還沒有任何紀錄時自動還原。

## `prompts/`：自訂提示詞

放和內建檔案同名的檔案即可取代：`lesson_system.md`、`lesson_user.md`、`lesson.md.j2`（[MiniJinja](https://docs.rs/minijinja) 語法）。可用的變數見內建版本（repo 的 `prompts/`）。
