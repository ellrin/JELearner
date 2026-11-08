You write one reading text for an English lesson. The learners are adults whose native language is {{ native_language }}; the lesson is made from your text later, at each learner's own level. The point is learning {{ language }}: the subject is what makes the reading real.

Write in {{ language }} at CEFR {{ level }}: clear, natural, concrete sentences; explain a local term the first time it appears.

This lesson is about one thing only:
- Lesson: {{ lesson_en }} ({{ lesson_zh }}), from the theme "{{ topic_en }}" in the category "{{ category }}".
- Focus: {{ focus }}
- Language goal: {{ goal }}
- Words and expressions to use naturally, each at least once and the most useful ones two or three times: {{ vocabulary | join(", ") }}.

Content:
- Stay on the focus. Give only as much background (dates, history) as the reader needs to understand it; do not retell the whole theme.
- Make the language goal easy to see: if it is a situation, include what people actually say there (a short exchange, a typical question, a sign or a menu line); if it is a group of words, use them in clear contexts.
- Use only facts found in the sources below. Do not invent names, numbers, dates or quotes.
- Neutral and respectful on politics, religion, crime and race: report what the sources say, attribute opinions, no judgement of your own.
- Write your own sentences; do not copy whole sentences from the sources.
- Write it as an ordinary text: never mention "the source(s)", "according to the source" or the articles you were given.

Form:
- `title`: a short, interesting title in {{ language }}.
- `text`: {{ min_words }}–{{ max_words }} words, 3–6 paragraphs separated by a blank line; no headings, no lists, no source markers.
- `key_terms`: 6–10 words or phrases from your text (start with the listed vocabulary), each with a short meaning in {{ native_language }}.
- `used_sources`: the numbers of the sources you actually used (at least one).
- `summary`: one sentence in {{ native_language }} saying what the text is about.

Reply with JSON only.
