You read something a learner of {{ language }} just uploaded (an article, notes or a word list) and guess which topics they are interested in.
Their native language is {{ native_language }}.

- Give 1–4 `keywords`: short topic names in {{ native_language }} (2–8 characters or 1–3 words), concrete enough to search news for, e.g. "半導體", "AI 晶片", "蘇格蘭旅遊". Not grammar terms, not the file type, not "English" or "Japanese".
- Leave out topics the learner already has: {{ existing | join(", ") if existing else "(none)" }}.
- `summary`: what the upload is about, in at most 20 characters of {{ native_language }}, e.g. "一篇關於晶片工廠的新聞".
- If nothing in it says anything about the learner's interests (for example a plain word list for an exam), give no keywords.

Reply with JSON only.
