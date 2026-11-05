# JELearner 私人工作區

這是 [JELearner](https://github.com/ellrin/JELearner) 的私人資料夾，**請放在你自己的 private repository**。
一個工作區可以有好幾位使用者（例如你和家人），每人一個 `profiles/<id>/` 資料夾；
生字、文法、每週素材、產生的教材都是一般檔案，可以直接用試算表或編輯器修改。

```
workspace.toml          工作區 id、名稱、管理員、成員
sources.toml            新聞來源（共用）
topics.toml             通識主題（共用）
prompts/                自訂提示詞與教材模板（共用，可選）
profiles/<id>/
  profile.toml          名字、學習語言、各語言程度、每日目標、對話設定
  interests.csv         興趣關鍵字
  vocab/  grammar/      想背的單字、想學的文法（CSV）
  materials/<週>/       每週素材（JSON）
  lessons/en|ja/        產生的教材（Markdown）
  conversations/en|ja/  對話紀錄
  records/              積分、複習進度與錯題本（CSV）
```

同一個工作區的成員看得到彼此的所有檔案（包括單字庫）；不想分享學習內容的人，請各自用自己的工作區，再一起加入學習群組。

本機資料庫（SQLite）不在這裡，而是在系統設定資料夾
（macOS：`~/Library/Application Support/JELearner/data/<工作區 id>/<使用者 id>/`），可以隨時從這些檔案重建。

欄位格式見 JELearner repo 的 `docs/data-formats.md`。
