Interests: {{ interests | join(", ") if interests else "(none yet)" }}
General topics: {{ general | join(", ") }}
Already shown today (do not repeat): {{ exclude | join(" / ") if exclude else "(none)" }}

Recent headlines:
{% for h in headlines %}- [{{ h.id }}] {{ h.title }}{% if h.summary %} — {{ h.summary }}{% endif %}
{% else %}(none)
{% endfor %}
