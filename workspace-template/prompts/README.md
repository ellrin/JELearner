# 自訂提示詞與教材模板

放一個和內建檔案同名的檔案，就會取代內建的版本（內建版見 JELearner repo 的 `prompts/`）：

| 檔名 | 用途 |
| --- | --- |
| `lesson_system.md` | 給模型的角色與規則 |
| `lesson_user.md` | 每篇文章的請求（可用 `{{ title }}`、`{{ text }}`、`{{ level }}`、`{{ known_words }}`…） |
| `lesson.md.j2` | 教材 Markdown 的版面（可用 `{{ lesson.vocabulary }}` 等欄位） |

模板語法是 [MiniJinja](https://docs.rs/minijinja)（與 Jinja2 相同）。
