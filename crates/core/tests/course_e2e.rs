//! End to end through the core with the mock provider: create a topic
//! course outline from keywords and a page the learner gave, save it, open
//! lesson 2 (generated from its plan, based on that page), check the target
//! words are in the content, the progress is recorded and the outline file
//! is left as it was.

use jl_core::course::outline::{create_topic_course, recurrence, TopicRequest, MIN_RECURRENCE};
use jl_core::course::plan::{needs_search, open_lesson, LessonContext, SourceText};
use jl_core::course::{store, topics, CourseTemplates, Learner, LessonRole, LessonStatus};
use jl_core::lang::Lang;
use jl_core::lesson;
use jl_core::provider::MockProvider;
use jl_core::workspace::{Profile, Root};
use serde_json::{json, Value};

fn word(term: &str, reading: &str, meaning: &str) -> Value {
    json!({ "term": term, "reading": reading, "meaning": meaning })
}

fn outline() -> String {
    let lesson = |title: &str, angle: &str, words: Vec<Value>, grammar: &str| {
        json!({
            "title": title, "angle": angle, "purpose": "認識主題", "search": ["AI chips"],
            "conversation_topic": "", "words": words,
            "grammar": [{ "pattern": grammar, "meaning": "" }]
        })
    };
    json!({
        "title": "AI Chips This Week",
        "summary": "一週認識 AI 晶片",
        "keywords": ["AI chips"],
        "enough_material": true,
        "lessons": [
            lesson("What is an AI chip?", "", vec![word("semiconductor", "/ˌsemikənˈdʌktər/", "半導體"), word("processor", "/ˈprəʊsesər/", "處理器")], "be used to + V"),
            lesson("Inside the fab", "", vec![word("subsidy", "/ˈsʌbsədi/", "補助"), word("yield", "/jiːld/", "良率"), word("capacity", "/kəˈpæsəti/", "產能")], "so ... that ..."),
            lesson("Chips and jobs", "", vec![word("shortage", "", "短缺")], "not only ... but also ..."),
            lesson("My phone's chip", "", vec![word("battery", "", "電池")], "used to"),
            lesson("Review", "", vec![word("ignored", "", "")], "x"),
        ]
    })
    .to_string()
}

/// The model's lesson 2, which uses the new words and the lesson-1 words
/// the outline put into its review list.
fn lesson_two() -> String {
    json!({
        "title": "Inside the fab",
        "summary": "政府補助新晶圓廠，提高產能與良率。",
        "text": "The government gave a large subsidy to a new semiconductor fab. The fab was so busy that it doubled its capacity. Better yield means each processor costs less.",
        "vocabulary": [
            { "term": "subsidy", "reading": "/ˈsʌbsədi/", "part_of_speech": "n.", "meaning": "補助", "example": "The subsidy helped farmers.", "example_translation": "補助幫助了農民。" },
            { "term": "capacity", "reading": "/kəˈpæsəti/", "part_of_speech": "n.", "meaning": "產能", "example": "The plant runs at full capacity.", "example_translation": "工廠滿載運作。" }
        ],
        "grammar": [{ "pattern": "so ... that ...", "meaning": "如此…以致於…", "explanation": "表示程度與結果。", "example": "It was so cold that we stayed in.", "example_translation": "太冷了，我們待在家。" }],
        "questions": [
            { "kind": "choice", "question": "誰給了補助？", "options": ["政府", "工廠"], "answer": "政府", "explanation": "第一句。" }
        ],
        "output": [
            { "kind": "sentence-making", "prompt": "用 subsidy 造句", "answer": "The city offers a subsidy for solar panels.", "hint": "subsidy" }
        ],
        "conversation": { "topic": "Should governments pay for chip factories?", "opening": "Do you think a subsidy for a fab is a good idea?", "target_words": ["subsidy", "yield"] }
    })
    .to_string()
}

#[tokio::test]
async fn topic_course_outline_then_lesson_two() {
    // A workspace with the first user.
    let tpl = tempfile::tempdir().unwrap();
    std::fs::write(tpl.path().join("workspace.toml"), "id = \"\"\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut root = Root::init(dir.path(), tpl.path()).unwrap();
    let ws = root.create_profile(tpl.path(), Profile::new("", "ellrin")).unwrap();
    let learner = Learner::from_profile(&ws.profile, Lang::En, vec![]);
    let templates = CourseTemplates::load(Some(&ws.prompts_dir()));
    let p = MockProvider::new(vec![outline(), lesson_two()]);

    // 1. Create: only the outline.
    let interests = topics::parse_interests("keyword,lang,weight,updated_at\nsemiconductor,,1,\n");
    let page = SourceText { title: "New fab".into(), url: "https://example.com/fab".into(), source: "Example".into(), text: "A new fab got a subsidy.".into() };
    let req = TopicRequest { keywords: vec!["AI chips".into()], sources: vec![page.clone()], ..Default::default() };
    let mut course = create_topic_course(&p, &templates, &learner, &req, &interests, "2026-11-05").await.unwrap();
    assert_eq!(course.lessons.len(), 5);
    assert!(course.lessons.iter().all(|l| l.status == LessonStatus::Planned && l.file.is_none()));
    assert_eq!(course.lessons[4].role, LessonRole::Review);
    assert!(course.lessons[4].words.is_empty());
    assert!(course.lessons[3].words.is_empty(), "lesson 4 introduces no new words (battery dropped)");
    for (term, _, count) in recurrence(&course.lessons) {
        assert!(count >= MIN_RECURRENCE, "{term}");
    }
    assert_eq!(course.lessons[1].article.as_ref(), Some(&page), "the learner's page is lesson 2's text");
    let id = store::save_new(&ws, &mut course).unwrap();
    assert_eq!(id, "2026-11-05-ai-chips-this-week");
    assert!(ws.profile_dir().join("recipes/2026-11-05-ai-chips-this-week.toml").exists());
    assert_eq!(p.seen.lock().unwrap().len(), 1, "no lesson content yet");
    let outline_file = ws.profile_dir().join("recipes/2026-11-05-ai-chips-this-week.toml");
    let outline_text = std::fs::read_to_string(&outline_file).unwrap();

    // 2. Open lesson 2: generated from its plan.
    let plan = course.lesson(2).unwrap().clone();
    let targets: Vec<String> = plan.words.iter().map(|w| w.term.clone()).collect();
    assert_eq!(targets, vec!["subsidy", "yield", "capacity"]);
    assert!(plan.review.contains(&"semiconductor".to_string()), "lesson-1 words come back: {:?}", plan.review);
    assert!(!needs_search(&plan), "no news search for a lesson with the learner's page");
    let ctx = LessonContext { recent_accuracy: Some(0.8), article: None, focus: None };
    let opened = open_lesson(&p, &templates, &ws, &id, 2, &learner, &ctx, "2026-11-06").await.unwrap();
    let g = opened.generated.as_ref().expect("generated now");
    assert!(g.missing.is_empty(), "missing in text: {:?}", g.missing);

    // The request carried the plan.
    let seen = p.seen.lock().unwrap()[1].clone();
    for t in &targets {
        assert!(seen.user.contains(t.as_str()), "{t} in the prompt");
    }
    assert!(seen.user.contains("so ... that ..."));
    assert!(seen.user.contains("A new fab got a subsidy."), "the learner's page is sent");
    assert!(seen.system.contains("Weave the new and review"));
    assert!(seen.system.contains("80 %"));
    assert!(seen.json_schema.as_ref().unwrap()["properties"]["text"].is_object());

    // The Markdown: target words, plan metadata, still a readable lesson file.
    let md = &opened.markdown;
    for t in &targets {
        assert!(md.contains(t.as_str()), "{t} in the lesson");
    }
    assert!(md.contains("| yield |"), "a word the model left out of the vocabulary is added from the plan");
    assert_eq!(lesson::front_matter(md, "course").as_deref(), Some(id.as_str()));
    assert_eq!(lesson::front_matter(md, "course_lesson").as_deref(), Some("2"));
    assert_eq!(lesson::front_matter(md, "units").as_deref(), Some("reading:10,review:5,sentence-making:5,conversation:10"));
    assert_eq!(lesson::front_matter(md, "source").as_deref(), Some("https://example.com/fab"));
    let data = lesson::extract_data(md).expect("embedded data reads as a plain lesson");
    assert_eq!(data.vocabulary.len(), 3);
    assert!(md.contains("Do you think a subsidy"));
    assert!(opened.file.starts_with("profiles/ellrin/lessons/en/2026-11-06-ai-chips-this-week-02"));
    assert!(dir.path().join(&opened.file).exists());

    // Progress is recorded in its own file; the outline file is untouched.
    assert_eq!(std::fs::read_to_string(&outline_file).unwrap(), outline_text);
    assert!(ws.profile_dir().join("recipes/2026-11-05-ai-chips-this-week.progress.toml").exists());
    // Opening again reads the lesson file, no new call.
    let saved = store::load(&ws, &id).unwrap();
    let l2 = saved.lesson(2).unwrap();
    assert_eq!(l2.status, LessonStatus::Generated);
    assert_eq!(l2.file.as_deref(), Some(opened.file.as_str()));
    assert_eq!(saved.lesson(3).unwrap().status, LessonStatus::Planned);
    let again = open_lesson(&p, &templates, &ws, &id, 2, &learner, &ctx, "2026-11-07").await.unwrap();
    assert!(again.generated.is_none());
    assert_eq!(again.markdown, opened.markdown);
    assert_eq!(p.seen.lock().unwrap().len(), 2);

    // Mark the units done: 1 point each, then the lesson is done.
    let mut c = store::load(&ws, &id).unwrap();
    let n = c.lesson(2).unwrap().units.len() as u32;
    let mut points = 0.0;
    for u in 0..n {
        points += store::mark_unit(&mut c, 2, u, true, Some(0.9), "2026-11-06").unwrap().points;
    }
    store::save_progress(&ws, &c).unwrap();
    assert_eq!(std::fs::read_to_string(&outline_file).unwrap(), outline_text);
    assert_eq!(points, n as f32);
    assert_eq!(store::load(&ws, &id).unwrap().lesson(2).unwrap().status, LessonStatus::Done);
    let (list, _) = store::list(&ws);
    assert_eq!((list[0].done, list[0].next), (1, Some(1)));
}
