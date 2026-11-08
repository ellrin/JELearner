Sources:
{% for s in sources %}
[{{ loop.index }}] {{ s.title }} ({{ s.url }})
{{ s.text }}

{% endfor %}
Write the article about: {{ topic_en }}
