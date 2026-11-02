# JELearner 私人工作區

這是 [JELearner](https://github.com/ellrin/JELearner) 的私人資料夾，**請放在你自己的 private repository**。
生字、文法、每週素材、產生的教材都是一般檔案，可以直接用試算表或編輯器修改；App 開啟時會讀進本機資料庫。

```
workspace.toml   名字、各語言程度、週報信箱
sources.toml     新聞來源（RSS / Atom）
vocab/           想背的單字（CSV，可有多個檔案）
grammar/         想學的文法（CSV）
materials/       每週素材（materials/2026-W45/*.json）
lessons/en|ja/   App 產生的教材（Markdown）
records/         複習進度與錯題本的匯出備份（CSV）
prompts/         自訂提示詞與教材模板（可選）
avatar/          數位人像設定（之後）
```

欄位格式見 JELearner repo 的 `docs/data-formats.md`。

本機資料庫（SQLite）不在這裡，而是在系統設定資料夾（macOS：`~/Library/Application Support/JELearner/data/<id>/`），
可以隨時從這些檔案重建；複習進度與錯題請按 App 裡的「匯出紀錄」寫進 `records/` 再同步。
