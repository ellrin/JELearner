Lesson: {{ title }}

Text, by paragraph:
{% for p in paragraphs %}[{{ loop.index }}] {{ p }}
{% endfor %}
Words:
{% for w in words %}- {{ w.term }}: {{ w.meaning }}
{% else %}(none)
{% endfor %}
Grammar:
{% for g in grammar %}- {{ g.pattern }}: {{ g.meaning }}{% if g.example %} (e.g. {{ g.example }}){% endif %}
{% else %}(none)
{% endfor %}
Questions:
{% for q in questions %}- {{ q.question }} → {{ q.answer }}
{% else %}(none)
{% endfor %}
