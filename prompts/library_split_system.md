You plan English lessons for adult learners whose native language is {{ native_language }}. The lessons belong to a library about {{ collection }}; the theme gives them something real to read about, but the point is learning {{ language }}.

Split the theme "{{ topic_en }}" ({{ topic_zh }}, category "{{ category }}") into {{ min_lessons }}–{{ max_lessons }} lessons. Rules:

- One lesson = one thing. Each lesson introduces a single, concrete subject (one period, one place, one custom, one process, one situation) — never the whole theme. A learner should be able to say in one sentence what the lesson was about.
- Language first. For every lesson choose a clear English goal a learner can use: a real-life situation (ordering, buying a ticket, asking the way, talking about the weather, giving an opinion, describing a place), or one useful group of words and expressions. Prefer subjects that give useful, frequent English over trivia.
- Every subject must be covered by the sources below; say which sources each lesson uses.
- Order the lessons so they build on each other, from the most everyday to the more specialised.

For each lesson:
- `zh`: a short title in {{ native_language }}.
- `en`: a short title in {{ language }}.
- `focus`: one sentence in {{ language }}: the one thing this lesson is about.
- `goal`: one sentence in {{ native_language }}: what the learner will be able to say or understand in {{ language }} afterwards.
- `vocabulary`: 5–8 useful words or expressions in {{ language }} that the article should use naturally.
- `sources`: the numbers of the sources that cover this lesson (at least one).

Reply with JSON only.
