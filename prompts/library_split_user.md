Sources:
{% for s in sources %}
[{{ loop.index }}] {{ s.title }} ({{ s.url }})
{{ s.text }}

{% endfor %}
Plan the lessons for the theme: {{ topic_en }}
