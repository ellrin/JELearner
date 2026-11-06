Write lesson {{ number }} of {{ total }} of the {{ language }} course "{{ course_name }}".

Lesson title: {{ title }}
{% if angle %}
Angle: {{ angle }}
{% endif %}
{% if purpose %}
Purpose: {{ purpose }}
{% endif %}
{% if keywords %}
Course keywords: {{ keywords | join(", ") }}
{% endif %}
Level: {{ level }}
Text length: {{ text_length }}

New words:
{% for w in new_words %}
- {{ w.term }}{% if w.reading %} [{{ w.reading }}]{% endif %}{% if w.meaning %}: {{ w.meaning }}{% endif %}

{% else %}
- (none)
{% endfor %}
{% if review_words %}
Review words (use them in the text too): {{ review_words | join(", ") }}
{% endif %}
Grammar:
{% for g in grammar %}
- {{ g.pattern }}{% if g.meaning %}: {{ g.meaning }}{% endif %}

{% else %}
- (none planned; pick one pattern from the text that suits the level)
{% endfor %}

Exercises:
- Reading questions: {{ questions }}
{% for o in output %}
- {{ o.kind }}: {{ o.items }} items
{% endfor %}
{% if turns %}
- Conversation: {{ turns }} turns{% if conversation_topic %}, topic: {{ conversation_topic }}{% endif %}

{% endif %}
{% if known_words %}

Words the learner already knows: {{ known_words | join(", ") }}
{% endif %}
{% if article %}

<article title="{{ article.title }}"{% if article.url %} url="{{ article.url }}"{% endif %}>
{{ article.text }}
</article>
{% endif %}
