Sources:
{% for s in sources %}
[{{ loop.index }}] {{ s.title }} ({{ s.url }})
{{ s.text }}

{% endfor %}
Write the text for the lesson: {{ lesson_en }} — {{ focus }}
