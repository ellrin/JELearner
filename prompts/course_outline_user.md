{% if kind == "topic" %}
Plan a {{ lessons }}-lesson {{ language }} topic series.

{% if angles %}
Angles of lessons 2 to {{ lessons - 1 }}: {{ angles | join(", ") }}
{% endif %}
{% if keywords %}
Keywords: {{ keywords | join(", ") }}
{% endif %}
{% if interests %}
The learner's interests (for the personal angle): {{ interests | join(", ") }}
{% endif %}
{% if url %}
Source page: {{ url }}
{% endif %}
Level: {{ level }}
New words per lesson: {{ topic_words_min }}–{{ topic_words_max }}; grammar per lesson: {{ grammar_min }}–{{ grammar_max }}
Each text will be about {{ text_length }}.
{% if previous_words %}

This series continues an earlier one on the same topic. These words were already taught; do not make
them new again (they will be reviewed): {{ previous_words | join(", ") }}
{% endif %}
{% if known_words %}

Words the learner already knows (do not make them new): {{ known_words | join(", ") }}
{% endif %}
{% if source_text %}

<source>
{{ source_text }}
</source>
{% endif %}
{% else %}
Course: {{ name }} ({{ language }}, level {{ level }}, {{ lessons }} lessons)

{% for l in plan %}
- Lesson {{ l.number }}{% if l.role == "review" %} (review){% endif %}: {% if l.words %}words: {{ l.words | join(", ") }}{% endif %}{% if l.grammar %}; grammar: {{ l.grammar | join(", ") }}{% endif %}

{% endfor %}
{% endif %}
