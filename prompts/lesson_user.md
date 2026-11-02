Make a lesson from this {{ language }} article.

Title: {{ title }}
{% if source %}Source: {{ source }}
{% endif %}
{% if url %}URL: {{ url }}
{% endif %}
Target level: {{ level }}
{% if known_words %}
Words the learner already knows (do not include them): {{ known_words | join(", ") }}
{% endif %}

<article>
{{ text }}
</article>
