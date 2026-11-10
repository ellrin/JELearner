//! Stage B end to end, offline: courses wired into the server (topic course
//! from keywords and a pasted text, fixed course from an uploaded CSV),
//! opening lessons, unit results into the points ledger and the course
//! progress file, today's daily lesson, the review notebook and its
//! pictures. The Claude API is a mock that answers by the JSON schema of the
//! request and puts the planned words into the lesson it writes.

use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use jl_server::{GoogleMode, Options, Paths, Settings};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

const EN_POOL: &[(&str, &str, &str)] = &[
    ("semiconductor", "/ˌsemikənˈdʌktər/", "半導體"),
    ("processor", "/ˈprəʊsesər/", "處理器"),
    ("subsidy", "/ˈsʌbsədi/", "補助"),
    ("capacity", "/kəˈpæsəti/", "產能"),
    ("wafer", "/ˈweɪfər/", "晶圓"),
    ("foundry", "/ˈfaʊndri/", "晶圓代工廠"),
    ("throughput", "/ˈθruːpʊt/", "產出量"),
    ("shortage", "/ˈʃɔːrtɪdʒ/", "短缺"),
    ("efficiency", "/ɪˈfɪʃnsi/", "效率"),
    ("benchmark", "/ˈbentʃmɑːrk/", "基準"),
];

fn word(t: &(&str, &str, &str)) -> Value {
    json!({ "term": t.0, "reading": t.1, "meaning": t.2 })
}

/// A topic outline: five lessons, two new words each.
fn outline(user: &str) -> Value {
    let kw = user.lines().find_map(|l| l.strip_prefix("Keywords: ")).unwrap_or("Today").to_string();
    let lessons: Vec<Value> = (0..5)
        .map(|i| {
            json!({
                "title": format!("{kw} part {}", i + 1), "angle": "", "purpose": "認識主題", "search": [kw],
                "conversation_topic": format!("What do you think about {kw}?"),
                "words": [word(&EN_POOL[(2 * i) % EN_POOL.len()]), word(&EN_POOL[(2 * i + 1) % EN_POOL.len()])],
                "grammar": [{ "pattern": format!("pattern {}", i + 1), "meaning": "句型" }]
            })
        })
        .collect();
    json!({ "title": format!("{kw} This Week"), "summary": "一週認識主題", "keywords": [kw], "enough_material": true, "lessons": lessons })
}

/// Lesson titles for a fixed course.
fn fixed_titles() -> Value {
    json!({ "lessons": (1..=60).map(|n| json!({ "number": n, "title": format!("Office day {n}"), "angle": "work", "purpose": "職場單字", "conversation_topic": "Your workplace" })).collect::<Vec<_>>() })
}

/// A course lesson written from the plan in the prompt: every new and
/// review word goes into the text; one output item per requested item.
fn course_lesson(user: &str) -> Value {
    let mut words: Vec<(String, String, String)> = vec![];
    let mut in_words = false;
    let mut review: Vec<String> = vec![];
    let mut outputs: Vec<(String, usize)> = vec![];
    let mut questions = 2;
    for line in user.lines() {
        if line.starts_with("New words:") {
            in_words = true;
            continue;
        }
        if in_words {
            match line.strip_prefix("- ") {
                Some("(none)") => {}
                Some(w) => {
                    let (head, meaning) = w.split_once(": ").unwrap_or((w, ""));
                    let (term, reading) = match head.split_once(" [") {
                        Some((t, r)) => (t.to_string(), r.trim_end_matches(']').to_string()),
                        None => (head.to_string(), String::new()),
                    };
                    words.push((term, reading, meaning.to_string()));
                    continue;
                }
                None => in_words = false,
            }
        }
        if let Some(r) = line.strip_prefix("Review words (use them in the text too): ") {
            review = r.split(", ").map(String::from).collect();
        }
        if let Some(q) = line.strip_prefix("- Reading questions: ") {
            questions = q.trim().parse().unwrap_or(2).clamp(1, 4);
        }
        for kind in ["translation", "sentence-making", "cloze", "paraphrase"] {
            if let Some(n) = line.strip_prefix(&format!("- {kind}: ")) {
                outputs.push((kind.to_string(), n.split(' ').next().unwrap_or("2").parse().unwrap_or(2)));
            }
        }
    }
    let all: Vec<String> = words.iter().map(|w| w.0.clone()).chain(review.iter().cloned()).collect();
    let text = format!("Today we read about chips. {} Everyone talked about it.", all.iter().map(|t| format!("The word {t} matters here.")).collect::<Vec<_>>().join(" "));
    let vocabulary: Vec<Value> = words
        .iter()
        .map(|(t, r, m)| json!({ "term": t, "reading": r, "part_of_speech": "n.", "meaning": m, "example": format!("This {t} is new."), "example_translation": "例句。" }))
        .collect();
    let qs: Vec<Value> = (0..questions)
        .map(|i| json!({ "kind": "choice", "question": format!("Question {}?", i + 1), "options": ["Yes", "No", "Maybe"], "answer": "Yes", "explanation": "課文第一句。" }))
        .collect();
    let output: Vec<Value> = outputs
        .iter()
        .flat_map(|(k, n)| (0..*n).map(move |i| json!({ "kind": k, "prompt": format!("{k} item {}", i + 1), "answer": format!("Model answer {}", i + 1), "hint": "用今天的字" })))
        .collect();
    json!({
        "title": "Inside the fab", "summary": "今天的課文。", "text": text, "vocabulary": vocabulary,
        "grammar": [{ "pattern": "so ... that ...", "meaning": "如此…以致於", "explanation": "程度與結果。", "example": "It was so cold that we stayed in.", "example_translation": "太冷了。" }],
        "questions": qs, "output": output,
        "conversation": { "topic": "Should governments pay for chip factories?", "opening": "Do you think a subsidy is a good idea?", "target_words": all.iter().take(3).collect::<Vec<_>>() }
    })
}

fn turn(b: &Value) -> Value {
    let last = b["messages"].as_array().and_then(|m| m.last()).and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
    json!({ "recast": format!("So you mean: {last}"), "question": "Why do you think so?", "used_targets": [], "starters": [] })
}

fn review() -> Value {
    json!({
        "fluency": 4, "target_use": 4, "grammar": 4, "overall": "good", "summary": "很好。",
        "better": [{ "original": "I very like it", "suggestion": "I really like it.", "why": "very 不修飾動詞" }],
        "suggested_words": [], "keywords": ["chips"]
    })
}

/// Topic cards (plan §19.2.6): the first headline in the request becomes a
/// news card; titles already shown are left out.
fn topic_cards(user: &str) -> Value {
    let shown: Vec<String> = user
        .lines()
        .find_map(|l| l.strip_prefix("Already shown today (do not repeat): "))
        .map(|l| l.split(" / ").map(|x| x.trim().to_string()).collect())
        .unwrap_or_default();
    let first = user.lines().find_map(|l| l.strip_prefix("- [").and_then(|r| r.split(']').next()).and_then(|n| n.parse::<i64>().ok()));
    let pool = ["愛丁堡的電車", "AI 晶片的新工廠", "蘇格蘭早餐", "雨天的城市生活", "博物館之夜", "高地的健行路線", "超市裡的自有品牌", "威士忌蒸餾廠"];
    let cards: Vec<Value> = pool
        .iter()
        .filter(|t| !shown.iter().any(|s| s == *t))
        .take(4)
        .enumerate()
        .map(|(i, t)| {
            let news = i == 0 && first.is_some();
            json!({ "title": t, "why": "跟你的程度剛好", "kind": if news { "news" } else { "general" }, "article_id": if news { json!(first) } else { Value::Null }, "keywords": ["Edinburgh trams"], "category": "交通" })
        })
        .collect();
    json!({ "cards": cards })
}

fn focus_sheet() -> Value {
    json!({ "summary": "蘇格蘭與英國", "cities": ["Edinburgh", "Glasgow"], "streets": ["Princes Street", "Royal Mile"], "shops": ["Tesco", "Greggs"],
        "transport": ["ScotRail", "Edinburgh Trams"], "foods": ["haggis"], "drinks": ["whisky"], "landmarks": ["Edinburgh Castle"],
        "daily_life": ["pounds and pence"], "news_keywords": ["Edinburgh", "Holyrood"] })
}

fn mock_claude(calls: Arc<Mutex<Vec<Value>>>) -> Router {
    mock_claude_slow(calls, 0)
}

/// The mock, writing a course lesson in `lesson_ms` (a slow local model).
fn mock_claude_slow(calls: Arc<Mutex<Vec<Value>>>, lesson_ms: u64) -> Router {
    Router::new().route(
        "/v1/messages",
        post(move |Json(b): Json<Value>| {
            let calls = calls.clone();
            async move {
                calls.lock().unwrap().push(b.clone());
                let props = &b["output_config"]["format"]["schema"]["properties"];
                if lesson_ms > 0 && props["text"].is_object() && props["conversation"].is_object() {
                    tokio::time::sleep(std::time::Duration::from_millis(lesson_ms)).await;
                }
                let user = b["messages"][0]["content"].as_str().unwrap_or("").to_string();
                let text = if props["cards"].is_object() {
                    topic_cards(&user)
                } else if props["news_keywords"].is_object() {
                    focus_sheet()
                } else if props["keywords"].is_object() && props["summary"].is_object() && props.as_object().map_or(0, |o| o.len()) == 2 {
                    json!({ "summary": "一篇關於晶片工廠的新聞", "keywords": ["半導體", "music", "AI 晶片"] })
                } else if props["lessons"]["items"]["properties"]["goal"].is_object() {
                    json!({ "lessons": [
                        { "zh": "點一份哈吉斯", "en": "Ordering haggis", "focus": "How to order haggis in a restaurant.", "goal": "能在餐廳點哈吉斯", "vocabulary": ["savoury", "neeps and tatties"], "sources": [1] },
                        { "zh": "彭斯之夜", "en": "Burns Night", "focus": "Why haggis is eaten on Burns Night.", "goal": "能說明節日習俗", "vocabulary": ["supper"], "sources": [1] }
                    ] })
                } else if props["used_sources"].is_object() {
                    let text = (0..9).map(|i| format!("Haggis has been eaten in Scotland for centuries and paragraph {i} explains how cooks prepared it then and how restaurants in Edinburgh serve it to visitors today.")).collect::<Vec<_>>().join(" ");
                    json!({ "title": "A Pudding with a Past", "text": text, "key_terms": [{ "term": "savoury", "meaning": "鹹的" }], "used_sources": [1], "summary": "哈吉斯從古到今" })
                } else if props["enough_material"].is_object() {
                    outline(&user)
                } else if props["lessons"]["items"]["properties"]["number"].is_object() {
                    fixed_titles()
                } else if props["text"].is_object() && props["conversation"].is_object() {
                    course_lesson(&user)
                } else if props["recast"].is_object() {
                    turn(&b)
                } else if props["fluency"].is_object() {
                    review()
                } else {
                    json!("OK")
                };
                Json(json!({
                    "id": "msg_test", "type": "message", "role": "assistant", "model": b["model"],
                    "content": [{ "type": "text", "text": text.to_string() }], "stop_reason": "end_turn",
                    "usage": { "input_tokens": 1, "output_tokens": 1 }
                }))
            }
        }),
    )
}

struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let r = self.http.get(format!("{}{path}", self.base)).send().await.unwrap();
        (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
    }
    async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.http.request(method, format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::POST, path, body).await
    }
    async fn bytes(&self, path: &str, body: Vec<u8>) -> (StatusCode, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).body(body).send().await.unwrap();
        (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
    }
}

/// A workspace with Ellrin (English B1, Japanese N5) and the mock Claude API.
async fn setup(app_dir: &Path, ws_path: &Path) -> (Client, Arc<Mutex<Vec<Value>>>) {
    setup_slow(app_dir, ws_path, 0).await
}

async fn setup_slow(app_dir: &Path, ws_path: &Path, lesson_ms: u64) -> (Client, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(vec![]));
    let claude = serve(mock_claude_slow(calls.clone(), lesson_ms)).await;
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.to_path_buf());
    s.claude.base_url = claude;
    s.claude_api_key = Some("sk-ant-test-key-0000".into());
    s.google.mode = GoogleMode::Mock;
    let app = jl_server::router(s, Options { provider: None }).await;
    let c = Client { base: serve(app).await, http: reqwest::Client::new() };
    let (code, r) = c.post("/api/setup/workspace", json!({ "path": ws_path, "init": true })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let (code, r) = c.post("/api/users", json!({ "name": "Ellrin", "languages": ["en", "ja"], "majors": { "en": "B1", "ja": "N5" }, "interests": ["music"] })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    (c, calls)
}

fn read_dir_text(dir: &Path) -> String {
    let mut out = String::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            out.push_str(&std::fs::read_to_string(p).unwrap_or_default());
        }
    }
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// Create a topic course → outline → open lesson 2 (its target words in the
/// text) → every unit of it (review, reading, output with a wrong answer,
/// the conversation) → points CSV and course progress file; the notebook
/// page with the wrong answer, notes surviving a refresh, pictures kept
/// outside the workspace; extending the finished course by five.
#[tokio::test]
async fn topic_course_lesson_units_points_and_notebook() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;

    // Quick create: keywords plus a pasted text.
    let (code, course) = c
        .post("/api/courses", json!({ "kind": "topic", "lang": "en", "keywords": ["AI chips"], "sources": [{ "text": "A new fab got a large subsidy from the city.", "title": "New fab" }] }))
        .await;
    assert_eq!(code, StatusCode::OK, "{course}");
    let id = course["id"].as_str().unwrap().to_string();
    assert!(id.ends_with("-ai-chips-this-week"), "{id}");
    assert_eq!(course["total"], 5);
    assert_eq!(course["kind"], "topic");
    assert_eq!(course["next"], 1);
    assert!(course["lessons"][1]["has_article"] == true, "the pasted text is lesson 2's text");
    assert!(ws_path.join(format!("profiles/ellrin/recipes/{id}.toml")).exists());
    {
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "only the outline at creation");
        assert!(calls[0]["messages"][0]["content"].as_str().unwrap().contains("A new fab got a large subsidy"));
    }

    // List and outline.
    let (_, list) = c.get("/api/courses?lang=en").await;
    assert_eq!(list["courses"].as_array().unwrap().len(), 1);
    assert_eq!(list["courses"][0]["done"], 0);
    let (code, outline) = c.get(&format!("/api/courses/{id}")).await;
    assert_eq!(code, StatusCode::OK);
    assert!(outline["lessons"].as_array().unwrap().iter().all(|l| l["status"] == "planned"));
    let plan2 = outline["lessons"][1].clone();
    let targets: Vec<String> = plan2["words"].as_array().unwrap().iter().map(|w| w["term"].as_str().unwrap().to_string()).collect();
    assert_eq!(targets, vec!["subsidy", "capacity"]);
    let kinds: Vec<&str> = plan2["units"].as_array().unwrap().iter().map(|u| u["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["reading", "review", "sentence-making", "conversation"], "the text first, then its words and grammar");

    // Open lesson 2: generated now, target words in the text, words added for review.
    let (code, opened) = c.post(&format!("/api/courses/{id}/lessons/2/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{opened}");
    assert_eq!(opened["generated"], true);
    let text = opened["lesson"]["text"].as_str().unwrap();
    for t in &targets {
        assert!(text.contains(t.as_str()), "{t} in the text: {text}");
    }
    assert!(text.contains("semiconductor"), "a lesson-1 word comes back for review: {text}");
    assert_eq!(opened["missing"], json!([]));
    assert!(opened["file"].as_str().unwrap().starts_with("profiles/ellrin/lessons/en/"));
    let lesson_id = opened["lesson_id"].as_i64().unwrap();
    let (_, words) = c.get("/api/words?lang=en&q=subsidy").await;
    assert_eq!(words.as_array().unwrap().len(), 1, "new words go into review");
    let cards = opened["review_cards"].as_array().unwrap();
    assert!(!cards.is_empty() && cards.len() <= 20);
    let (_, lessons) = c.get("/api/lessons?lang=en").await;
    assert_eq!(lessons.as_array().unwrap().len(), 1);
    // Opening again reads the file: no new model call.
    let n_calls = calls.lock().unwrap().len();
    let (code, again) = c.post(&format!("/api/courses/{id}/lessons/2/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{again}");
    assert_eq!(again["generated"], false);
    assert_eq!(again["lesson_id"], lesson_id);
    assert_eq!(calls.lock().unwrap().len(), n_calls);

    // Words and grammar (unit 1; done here before the text, which is allowed):
    // grade the cards as part of the course (no separate review round).
    for card in cards.iter().filter(|c| c["item_id"].is_i64()).take(3) {
        let (code, r) = c.post("/api/review", json!({ "kind": card["item_kind"], "id": card["item_id"], "grade": 4, "in_course": true })).await;
        assert_eq!(code, StatusCode::OK);
        assert!(r.get("award").is_none() && r.get("round").is_none(), "{r}");
    }
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/1"), json!({ "cards": 3, "score": 1.0 })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["passed"], true);
    assert_eq!(r["points"], 1.5, "all remembered: bonus");
    assert_eq!(r["next"]["unit"], 0, "the text is still to read");

    // The text (unit 0): answer every question (choice answers checked by the server).
    let qn = opened["lesson"]["questions"].as_array().unwrap().len();
    for i in 0..qn {
        let (code, _) = c.post(&format!("/api/lessons/{lesson_id}/answers"), json!({ "answers": [{ "index": i, "answer": "Yes" }] })).await;
        assert_eq!(code, StatusCode::OK);
    }
    let answers: Vec<Value> = (0..qn).map(|i| json!({ "index": i, "answer": "Yes" })).collect();
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/0"), json!({ "answers": answers })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["score"], 1.0);
    assert!(r["notebook"].is_string(), "the notebook page exists after a unit");

    // 3. Output: one of three wrong → a mistake, still passed (2/3 < 0.7? no).
    let items = vec![
        json!({ "prompt": "sentence-making item 1", "answer": "A subsidy helps.", "model": "Model answer 1", "correct": true }),
        json!({ "prompt": "sentence-making item 2", "answer": "I capacity much.", "model": "Model answer 2", "hint": "capacity 是名詞", "correct": false }),
        json!({ "prompt": "sentence-making item 3", "answer": "Fine.", "model": "Model answer 3", "correct": true }),
    ];
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/2"), json!({ "items": items })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let output_passed = r["passed"].as_bool().unwrap();
    let (_, mistakes) = c.get("/api/mistakes?lang=en").await;
    assert!(mistakes.as_array().unwrap().iter().any(|m| m["user_answer"] == "I capacity much."), "{mistakes}");
    let stem = r["notebook"].as_str().unwrap().to_string();

    // 4. Conversation: started from the lesson's plan (opening without a model call), until locked.
    let link = json!({ "course": id, "lesson": 2, "unit": 3 });
    let n_calls = calls.lock().unwrap().len();
    let (code, conv) = c.post("/api/conversations", json!({ "lang": "en", "course": link })).await;
    assert_eq!(code, StatusCode::OK, "{conv}");
    assert_eq!(calls.lock().unwrap().len(), n_calls, "the opening comes from the lesson");
    assert_eq!(conv["conversation"]["turns"][0]["text"], "Do you think a subsidy is a good idea?");
    assert_eq!(conv["conversation"]["topic"]["name"], "Should governments pay for chip factories?");
    let limit = conv["turns_limit"].as_u64().unwrap();
    assert_eq!(limit, 20, "10 minutes = 20 turns");
    let cid = conv["id"].as_i64().unwrap();
    let (_, same) = c.post("/api/conversations", json!({ "lang": "en", "course": link })).await;
    assert_eq!(same["id"], cid, "the unit's conversation is resumed, not started again");
    for i in 1..=limit {
        let (code, r) = c.post(&format!("/api/conversations/{cid}/turns"), json!({ "text": format!("I think the subsidy is useful, answer {i}.") })).await;
        assert_eq!(code, StatusCode::OK, "{r}");
    }
    let (code, done) = c.post(&format!("/api/conversations/{cid}/finish"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["result"]["course"]["lesson"], 2);
    assert_eq!(done["result"]["course"]["lesson_done"], output_passed, "{done}");
    assert!(done["result"]["notebook"].is_string(), "a page for the conversation too");

    // Points CSV: one line per unit, per skill, the lesson file with the unit index.
    let csv = read_dir_text(&ws_path.join("profiles/ellrin/records/points"));
    assert!(csv.contains(",en,review,vocab,1.5,1,lessons/en/"), "{csv}");
    assert!(csv.contains(",en,reading,reading,1.5,1,lessons/en/") && csv.contains("-02.md#0"), "{csv}");
    assert!(csv.contains(",en,sentence-making,writing,"), "{csv}");
    assert!(csv.contains(",en,conversation,writing,"), "{csv}");
    assert_eq!(csv.matches(",conversation,").count(), 1, "the conversation's point is written once");
    // Course progress file.
    let progress = std::fs::read_to_string(ws_path.join(format!("profiles/ellrin/recipes/{id}.progress.toml"))).unwrap();
    assert!(progress.contains("number = 2"), "{progress}");
    if output_passed {
        assert!(progress.contains("units_done = [") && progress.contains("status = \"done\""), "{progress}");
    }
    let (_, outline) = c.get(&format!("/api/courses/{id}")).await;
    assert_eq!(outline["lessons"][1]["units_done"], if output_passed { 4 } else { 3 });
    // The database knows which course each unit attempt was.
    let (_, st) = c.get("/api/status").await;
    let db = jl_core::db::Db::open(Path::new(st["db"].as_str().unwrap())).await.unwrap();
    let linked: Vec<(String, i64, i64)> = sqlx::query_as("SELECT course, course_lesson, unit_index FROM units WHERE course != '' ORDER BY unit_index").fetch_all(db.pool()).await.unwrap();
    assert_eq!(linked.iter().map(|x| x.2).collect::<Vec<_>>(), vec![0, 1, 2], "{linked:?}");
    assert!(linked.iter().all(|x| x.0 == id && x.1 == 2));

    // Notebook: the page lists the wrong answer, new words; the learner's notes survive a refresh.
    let (code, list) = c.get("/api/notebook?lang=en").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2, "one lesson page, one conversation page: {list}");
    let (code, page) = c.get(&format!("/api/notebook/en/{stem}")).await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(page["lesson_id"], lesson_id);
    assert_eq!(page["course"]["id"], id.as_str());
    assert!(page["page"]["data"]["mistakes"].as_array().unwrap().iter().any(|m| m["your_answer"] == "I capacity much." && m["answer"] == "Model answer 2"));
    assert!(page["page"]["data"]["words"].as_array().unwrap().iter().any(|w| w["term"] == "subsidy"));
    let file = ws_path.join(page["page"]["path"].as_str().unwrap());
    assert!(file.starts_with(ws_path.join("profiles/ellrin/notebook/en")));

    // A picture: compressed by the browser, kept on this computer only.
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
    jpeg.extend(std::iter::repeat_n(3u8, 4000));
    let (code, img) = c.bytes("/api/notebook/images", jpeg.clone()).await;
    assert_eq!(code, StatusCode::OK, "{img}");
    let img_id = img["id"].as_str().unwrap().to_string();
    let notes = format!("capacity 是名詞，不是動詞。\n\n![白板](jl-image:{img_id})");
    let (code, saved) = c.send(reqwest::Method::PUT, &format!("/api/notebook/en/{stem}"), json!({ "notes": notes })).await;
    assert_eq!(code, StatusCode::OK, "{saved}");
    assert_eq!(saved["page"]["images"], json!([img_id]));
    assert_eq!(saved["local_images"], json!([img_id]));
    let got = c.http.get(format!("{}/api/notebook/images/{img_id}", c.base)).send().await.unwrap();
    assert_eq!(got.headers()["content-type"], "image/jpeg");
    assert_eq!(got.bytes().await.unwrap().to_vec(), jpeg);
    let stored: Vec<PathBuf> = walk(app_dir.path()).into_iter().filter(|p| p.to_string_lossy().contains(&img_id)).collect();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].to_string_lossy().contains("/notebook-images/ws-") && stored[0].to_string_lossy().contains("/ellrin/"));
    assert!(walk(&ws_path).iter().all(|p| !p.to_string_lossy().contains(&img_id)), "never in the workspace");
    let (code, e) = c.bytes("/api/notebook/images", b"GIF89a not accepted".to_vec()).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    let mut big = vec![0xFF, 0xD8, 0xFF, 0xE0];
    big.extend(std::iter::repeat_n(1u8, 3 * 1024 * 1024 + 10));
    let (code, e) = c.bytes("/api/notebook/images", big).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "over 3 MB: {e}");
    let got = c.http.get(format!("{}/api/notebook/images/..%2F..%2Fx", c.base)).send().await.unwrap();
    assert_eq!(got.status().as_u16(), 404);

    // Another wrong answer later: the app section is regenerated, the notes stay.
    let (code, _) = c.post(&format!("/api/lessons/{lesson_id}/answers"), json!({ "answers": [{ "index": 0, "answer": "No" }] })).await;
    assert_eq!(code, StatusCode::OK);
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/0"), json!({ "answers": [{ "index": 0, "answer": "No" }] })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["points"], 0.0, "a passed unit earns once");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("capacity 是名詞，不是動詞。") && text.contains(&format!("jl-image:{img_id}")), "{text}");
    assert!(text.contains("Question 1?"), "the new wrong answer is in the app section: {text}");
    let (_, page) = c.get(&format!("/api/notebook/en/{stem}")).await;
    assert_eq!(page["page"]["notes"], notes);

    // On another computer (a fresh app folder) the picture is not there: the page says so.
    let other = tempfile::tempdir().unwrap();
    let mut s2 = Settings::for_tests(Paths::from_repo(), other.path().to_path_buf());
    s2.workspace = Some(ws_path.clone());
    let app2 = jl_server::router(s2, Options { provider: None }).await;
    let c2 = Client { base: serve(app2).await, http: reqwest::Client::new() };
    let (code, page2) = c2.get(&format!("/api/notebook/en/{stem}")).await;
    assert_eq!(code, StatusCode::OK, "{page2}");
    assert_eq!(page2["page"]["images"], json!([img_id]));
    assert_eq!(page2["local_images"], json!([]), "only on the computer it was added on");
    // The course lesson and its units are rebuilt from the files there.
    let (_, outline2) = c2.get(&format!("/api/courses/{id}")).await;
    assert!(outline2["lessons"][1]["lesson_id"].is_i64(), "{outline2}");
    let (_, st2) = c2.get("/api/status").await;
    let db2 = jl_core::db::Db::open(Path::new(st2["db"].as_str().unwrap())).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM units WHERE course = ?").bind(&id).fetch_one(db2.pool()).await.unwrap();
    assert!(n >= 3, "course units linked after a rebuild ({n})");

    // Extend: refused until every lesson is done; then five more lessons.
    let (code, _) = c.post(&format!("/api/courses/{id}/extend"), json!({})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    {
        let root = jl_core::workspace::Root::open(&ws_path).unwrap();
        let ws = root.user("ellrin").unwrap();
        let mut course = jl_core::course::store::load(&ws, &id).unwrap();
        for l in course.lessons.iter_mut() {
            l.status = jl_core::course::LessonStatus::Done;
            l.done = Some("2026-11-06".into());
        }
        jl_core::course::store::save_progress(&ws, &course).unwrap();
    }
    let (code, ext) = c.post(&format!("/api/courses/{id}/extend"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{ext}");
    assert_eq!(ext["total"], 10);
    assert_eq!(ext["next"], 6);
    assert_eq!(ext["series"], 2);
}

/// A fixed course from an uploaded word list (saved into the workspace
/// under a free name), with lesson titles from the model.
#[tokio::test]
async fn fixed_course_from_uploaded_csv() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    let mut csv = String::from("term,reading,meaning\n");
    for i in 0..25 {
        csv.push_str(&format!("officeword{i},,辦公室單字 {i}\n"));
    }
    let (code, course) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "vocab_text": csv, "file_name": "toeic.csv" })).await;
    assert_eq!(code, StatusCode::OK, "{course}");
    assert_eq!(course["kind"], "fixed");
    // B1: 10 new words per lesson → 3 word lessons, then a final review lesson.
    assert_eq!(course["total"], 4, "{course}");
    assert_eq!(course["lessons"][3]["role"], "review");
    assert_eq!(course["lessons"][0]["title"], "第 1 課：Office day 1", "titles from the model");
    assert_eq!(course["name"], "toeic", "the name the create page shows as its placeholder");
    assert_eq!(course["vocab"], json!(["profiles/ellrin/recipes/lists/vocab/toeic-en.csv"]));
    assert!(ws_path.join("profiles/ellrin/recipes/lists/vocab/toeic-en.csv").exists());
    assert!(!ws_path.join("profiles/ellrin/vocab/toeic-en.csv").exists(), "not a review list");
    // Uploading the same name again never overwrites the first file.
    let (code, again) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "vocab_text": csv, "file_name": "toeic.csv", "describe": false })).await;
    assert_eq!(code, StatusCode::OK, "{again}");
    assert_eq!(again["vocab"], json!(["profiles/ellrin/recipes/lists/vocab/toeic-en-2.csv"]));
    assert!(again["lessons"][0]["title"].as_str().unwrap().starts_with("第 1 課"));
    assert_ne!(again["id"], course["id"]);
    // From a list already in the workspace (an uploaded course list, or the learner's own vocab list).
    let (_, lists) = c.get("/api/courses/lists?lang=en").await;
    assert!(lists.as_array().unwrap().iter().any(|l| l["path"] == "profiles/ellrin/recipes/lists/vocab/toeic-en.csv" && l["items"] == 25 && l["course_only"] == true), "{lists}");
    std::fs::write(ws_path.join("profiles/ellrin/vocab/office-en.csv"), &csv).unwrap();
    let (_, lists) = c.get("/api/courses/lists?lang=en").await;
    assert!(lists.as_array().unwrap().iter().any(|l| l["path"] == "profiles/ellrin/vocab/office-en.csv" && l["course_only"] == false), "{lists}");
    let (code, from_ws) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "name": "My words", "vocab": ["profiles/ellrin/vocab/office-en.csv"], "describe": false })).await;
    assert_eq!(code, StatusCode::OK, "{from_ws}");
    assert_eq!(from_ws["name"], "My words");
    let (code, e) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "vocab": ["../../etc/passwd"] })).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    // Open the first lesson: its words are in the text.
    let id = course["id"].as_str().unwrap();
    let (code, opened) = c.post(&format!("/api/courses/{id}/lessons/1/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{opened}");
    assert!(opened["lesson"]["text"].as_str().unwrap().contains("officeword0"));
}

/// A fixed course's word list is scheduled lesson by lesson: creating a
/// 300-word course leaves today's due cards as they were; opening lesson 1
/// adds only its words; a rebuild from the files schedules the same words.
#[tokio::test]
async fn fixed_course_words_wait_until_taught() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    let (_, before) = c.get("/api/today?lang=en").await;
    let due0 = before["due"].as_i64().unwrap();
    let (_, words0) = c.get("/api/words?lang=en").await;
    let n0 = words0.as_array().unwrap().len();

    let mut csv = String::from("term,reading,meaning\n");
    for i in 0..300 {
        csv.push_str(&format!("listword{i:03},,清單單字 {i}\n"));
    }
    let (code, course) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "vocab_text": csv, "file_name": "toeic-words.csv", "describe": false })).await;
    assert_eq!(code, StatusCode::OK, "{course}");
    assert_eq!(course["name"], "toeic-words");
    assert!(course["id"].as_str().unwrap().ends_with("-toeic-words"), "{course}");
    let (_, after) = c.get("/api/today?lang=en").await;
    assert_eq!(after["due"].as_i64().unwrap(), due0, "nothing scheduled at creation");
    let (_, words) = c.get("/api/words?lang=en&q=listword").await;
    assert_eq!(words.as_array().unwrap().len(), 0, "{words}");
    // A workspace re-read does not import the course list either.
    let (code, r) = c.post("/api/workspace/reindex", json!({})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let (_, words) = c.get("/api/words?lang=en&q=listword").await;
    assert_eq!(words.as_array().unwrap().len(), 0, "{words}");

    let id = course["id"].as_str().unwrap().to_string();
    let (code, opened) = c.post(&format!("/api/courses/{id}/lessons/1/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{opened}");
    let planned: Vec<String> = course["lessons"][0]["words"].as_array().unwrap().iter().map(|w| w["term"].as_str().unwrap().to_string()).collect();
    assert_eq!(planned.len(), 10);
    let (_, words) = c.get("/api/words?lang=en&q=listword").await;
    let mut got: Vec<String> = words.as_array().unwrap().iter().map(|w| w["term"].as_str().unwrap().to_string()).collect();
    got.sort();
    assert_eq!(got, planned, "only lesson 1's words");
    let (_, all) = c.get("/api/words?lang=en").await;
    assert_eq!(all.as_array().unwrap().len(), n0 + 10);

    // Another computer rebuilds the database from the files: the same ten words.
    let other = tempfile::tempdir().unwrap();
    let mut s2 = Settings::for_tests(Paths::from_repo(), other.path().to_path_buf());
    s2.workspace = Some(ws_path.clone());
    let app2 = jl_server::router(s2, Options { provider: None }).await;
    let c2 = Client { base: serve(app2).await, http: reqwest::Client::new() };
    let (_, words2) = c2.get("/api/words?lang=en&q=listword").await;
    let mut got2: Vec<String> = words2.as_array().unwrap().iter().map(|w| w["term"].as_str().unwrap().to_string()).collect();
    got2.sort();
    assert_eq!(got2, planned, "a rebuild schedules only opened lessons' words");
}

/// Today's tasks come from today's lesson: the daily lesson when there is no
/// course (topic by the interest ratio, created on start), the next lesson
/// of the course the learner is doing once there is one; two lessons when
/// the daily goal is over 60 minutes.
#[tokio::test]
async fn daily_lesson_on_today() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;

    let (code, today) = c.get("/api/today?lang=en").await;
    assert_eq!(code, StatusCode::OK, "{today}");
    assert_eq!(today["lessons"], json!([]));
    assert!(today["daily"]["topic"]["name"].is_string(), "{today}");
    let tasks = today["tasks"].as_array().unwrap();
    assert!(tasks.iter().all(|t| t["daily"] == true));
    assert_eq!(tasks.iter().map(|t| t["kind"].as_str().unwrap()).collect::<Vec<_>>().first(), Some(&"reading"));
    assert_eq!(today["next"], 0, "one primary step");

    let (code, d) = c.post("/api/daily", json!({ "lang": "en" })).await;
    assert_eq!(code, StatusCode::OK, "{d}");
    assert_eq!(d["created"], true);
    let (_, d2) = c.post("/api/daily", json!({ "lang": "en" })).await;
    assert_eq!(d2["id"], d["id"], "one daily lesson a day");
    assert_eq!(d2["created"], false);
    let daily_id = d["id"].as_str().unwrap().to_string();
    let (_, today) = c.get("/api/today?lang=en").await;
    assert_eq!(today["lessons"][0]["course"], daily_id.as_str(), "{today}");
    assert_eq!(today["lessons"][0]["kind"], "daily");
    assert!(today["daily"].is_null());
    let (code, opened) = c.post(&format!("/api/courses/{daily_id}/lessons/1/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{opened}");
    let (_, list) = c.get("/api/courses?lang=en").await;
    assert_eq!(list["courses"], json!([]), "daily lessons are not listed as courses");

    // A course: today's lesson is its next lesson.
    let (code, course) = c.post("/api/courses", json!({ "kind": "topic", "lang": "en", "keywords": ["space travel"] })).await;
    assert_eq!(code, StatusCode::OK, "{course}");
    let (_, today) = c.get("/api/today?lang=en").await;
    assert_eq!(today["lessons"].as_array().unwrap().len(), 1);
    assert_eq!(today["lessons"][0]["course"], course["id"]);
    assert_eq!(today["lessons"][0]["number"], 1);
    let units = today["lessons"][0]["units"].as_array().unwrap().len();
    assert_eq!(today["tasks"].as_array().unwrap().iter().filter(|t| t["id"] == "unit").count(), units);

    // 90 minutes a day: two lessons (45 + 45).
    let r = c.http.put(format!("{}/api/user", c.base)).json(&json!({ "daily_minutes": 90 })).send().await.unwrap();
    assert!(r.status().is_success());
    let (_, today) = c.get("/api/today?lang=en").await;
    assert_eq!(today["per_day"], 2);
    let ls = today["lessons"].as_array().unwrap();
    assert_eq!(ls.len(), 2, "{today}");
    assert_eq!((ls[0]["number"].as_u64(), ls[1]["number"].as_u64()), (Some(1), Some(2)), "the same course twice");
}

/// A daily lesson started today stays on the today page when a course is
/// created afterwards (plan §11: today's lessons are listed as they are);
/// review steps never need Claude.
#[tokio::test]
async fn started_daily_lesson_stays_on_today() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;

    let (_, today) = c.get("/api/today?lang=en").await;
    let review = today["tasks"].as_array().unwrap().iter().find(|t| t["kind"] == "review").unwrap().clone();
    assert_eq!(review["needs_engine"], false, "review cards work without Claude: {review}");
    assert!(today["tasks"].as_array().unwrap().iter().any(|t| t["kind"] == "conversation" && t["needs_engine"] == true));

    let (_, d) = c.post("/api/daily", json!({ "lang": "en" })).await;
    let daily_id = d["id"].as_str().unwrap().to_string();
    let (code, opened) = c.post(&format!("/api/courses/{daily_id}/lessons/1/open"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{opened}");
    let (code, r) = c.post(&format!("/api/courses/{daily_id}/lessons/1/units/1"), json!({ "cards": 3, "score": 1.0 })).await;
    assert_eq!(code, StatusCode::OK, "{r}");

    // A new fixed course: the half-done daily lesson is still today's lesson.
    let mut csv = String::from("term,reading,meaning\n");
    for i in 0..12 {
        csv.push_str(&format!("officeword{i},,辦公室單字 {i}\n"));
    }
    let (code, course) = c.post("/api/courses", json!({ "kind": "fixed", "lang": "en", "vocab_text": csv, "file_name": "office.csv", "describe": false })).await;
    assert_eq!(code, StatusCode::OK, "{course}");
    let (_, today) = c.get("/api/today?lang=en").await;
    let ls = today["lessons"].as_array().unwrap();
    assert_eq!(ls[0]["course"], daily_id.as_str(), "{today}");
    assert_eq!(ls.len(), 1, "one lesson a day: the started one");
    let tasks = today["tasks"].as_array().unwrap();
    assert_eq!((tasks[0]["kind"].as_str(), tasks[1]["kind"].as_str(), tasks[1]["done"].as_bool()), (Some("reading"), Some("review"), Some(true)), "{today}");
    assert_eq!(today["next"], 0, "the text is still to read");
}

/// The daily lesson's review step before the lesson exists is a quick
/// review: it shows this round's progress, is done once nothing is due and
/// there were reviews today, and its points are the rounds actually awarded
/// (no point for fewer cards than a round). `next` moves past it.
#[tokio::test]
async fn quick_review_step_done_by_round_rules() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    std::fs::write(ws_path.join("progress.toml"), "[points]\nreview_round = 500\n").unwrap();
    let review_task = |t: &Value| t["tasks"].as_array().unwrap().iter().find(|x| x["kind"] == "review").unwrap().clone();

    let (_, today) = c.get("/api/today?lang=en").await;
    let rv = review_task(&today);
    assert_eq!(rv["done"], false, "{rv}");
    assert_eq!(rv["round"], json!({ "done": 0, "size": 500 }));
    assert_eq!((rv["unit"].as_u64(), today["next"].as_u64()), (Some(1), Some(0)), "words and grammar come after the text");
    let (_, due) = c.get("/api/review?lang=en&limit=100").await;
    let due = due.as_array().unwrap().clone();
    assert!(due.len() >= 2, "the starter workspace has due cards");

    // Two cards: still due cards left, the step stays next.
    for d in &due[..2] {
        let (code, r) = c.post("/api/review", json!({ "kind": d["item_kind"], "id": d["item_id"], "grade": 4 })).await;
        assert_eq!(code, StatusCode::OK, "{r}");
        assert!(r.get("award").is_none(), "{r}");
    }
    let (_, today) = c.get("/api/today?lang=en").await;
    let rv = review_task(&today);
    assert_eq!(rv["done"], false);
    assert_eq!(rv["round"]["done"], 2);

    // All due cards graded, fewer than a round: done, without points.
    for d in &due[2..] {
        c.post("/api/review", json!({ "kind": d["item_kind"], "id": d["item_id"], "grade": 4 })).await;
    }
    let (_, today) = c.get("/api/today?lang=en").await;
    assert_eq!(today["due"], 0, "{today}");
    let rv = review_task(&today);
    assert_eq!(rv["done"], true, "{rv}");
    assert!(rv["points"].is_null(), "no round finished, no point: {rv}");
    assert_eq!(today["next"], 0, "the text is still next");

    // A round finished: the step carries the points actually awarded.
    std::fs::write(ws_path.join("progress.toml"), "[points]\nreview_round = 1\n").unwrap();
    let (_, r) = c.post("/api/review", json!({ "kind": due[0]["item_kind"], "id": due[0]["item_id"], "grade": 4 })).await;
    let pts = r["award"]["points"].as_f64().expect("a round awarded");
    let (_, today) = c.get("/api/today?lang=en").await;
    let rv = review_task(&today);
    assert_eq!(rv["points"].as_f64(), Some(pts), "{rv}");
    assert_eq!(rv["round"], json!({ "done": 0, "size": 1 }));
}

/// News pages that robots.txt does not allow are marked, not offered.
#[tokio::test]
async fn news_blocked_by_robots_is_marked() {
    use axum::routing::get;
    let base = Arc::new(Mutex::new(String::new()));
    let b2 = base.clone();
    let site = Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /private/\n" }))
        .route(
            "/feed.xml",
            get(move || {
                let b = b2.lock().unwrap().clone();
                async move {
                    format!(
                        r#"<?xml version="1.0"?><rss version="2.0"><channel><title>T</title>
                        <item><title>Open story</title><link>{b}/articles/a.html</link><description>x</description></item>
                        <item><title>Members only</title><link>{b}/private/b.html</link><description>y</description></item></channel></rss>"#
                    )
                }
            }),
        );
    let url = serve(site).await;
    *base.lock().unwrap() = url.clone();
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _) = setup(app_dir.path(), &ws_path).await;
    std::fs::write(ws_path.join("sources.toml"), format!("[[source]]\nid = \"t\"\nname = \"T\"\nlang = \"en\"\nfeed = \"{url}/feed.xml\"\n")).unwrap();
    let (code, news) = c.get("/api/news?lang=en&refresh=true").await;
    assert_eq!(code, StatusCode::OK, "{news}");
    let arts = news["articles"].as_array().unwrap();
    let blocked = |t: &str| arts.iter().find(|a| a["title"] == t).map(|a| a["blocked"].clone());
    assert_eq!(blocked("Open story"), Some(json!(false)));
    assert_eq!(blocked("Members only"), Some(json!(true)));
    assert_eq!(arts.last().unwrap()["title"], "Members only", "blocked pages are listed last");
    // A topic course from a blocked page: what to do instead, not the URL again.
    let (code, e) = c.post("/api/courses", json!({ "kind": "topic", "lang": "en", "sources": [{ "url": format!("{url}/private/b.html") }] })).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    assert_eq!(e["error"], "這個網站不允許自動抓取，直接貼上文章文字吧");
}

/// The Claude subscription (fake Claude Code CLI) creates a course too.
#[cfg(unix)]
#[tokio::test]
async fn topic_course_through_the_subscription() {
    let cli_dir = tempfile::tempdir().unwrap();
    let cli = jl_core::claude_code::fake::write(cli_dir.path());
    std::fs::write(cli_dir.path().join("reply.json"), outline("Keywords: robots").to_string()).unwrap();
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    s.claude_bin = Some(cli.clone());
    let app = jl_server::router(s, Options { provider: None }).await;
    let c = Client { base: serve(app).await, http: reqwest::Client::new() };
    c.post("/api/setup/workspace", json!({ "path": ws_dir.path(), "init": true })).await;
    let (code, _) = c.post("/api/users", json!({ "name": "Ellrin", "languages": ["en"] })).await;
    assert_eq!(code, StatusCode::OK);
    c.post("/api/claude-code/login", json!({})).await;
    for _ in 0..50 {
        let (_, cl) = c.get("/api/claude?fresh=true").await;
        if cl["subscription"]["ready"] == true {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (code, course) = c.post("/api/courses", json!({ "kind": "topic", "lang": "en", "keywords": ["robots"] })).await;
    assert_eq!(code, StatusCode::OK, "{course}");
    assert_eq!(course["total"], 5);
    let args = std::fs::read_to_string(cli_dir.path().join("last-args")).unwrap();
    assert!(args.contains("enough_material") && args.contains("--safe-mode"), "outline schema passed: {args}");
}


/// A news site for the cards: robots.txt allows everything, one feed, one article.
fn news_site(base: Arc<Mutex<String>>) -> Router {
    use axum::routing::get;
    let feed = move || {
        let b = base.lock().unwrap().clone();
        async move {
            let xml = format!(r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Mock</title>
                <item><title>Edinburgh trams reach Newhaven</title><link>{b}/trams.html</link>
                  <description>The new line opens.</description><pubDate>Mon, 02 Nov 2026 08:00:00 GMT</pubDate></item>
                </channel></rss>"#);
            ([("content-type", "application/rss+xml")], xml)
        }
    };
    Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nAllow: /\n" }))
        .route("/feed.xml", get(feed))
        .route("/trams.html", get(|| async {
            axum::response::Html("<html><head><title>Edinburgh trams reach Newhaven</title></head><body><article>\
                <p>Edinburgh's trams now run all the way to Newhaven, linking Leith with the city centre.</p>\
                <p>Passengers can ride from the airport to the coast without changing.</p></article></body></html>")
        }))
}

/// 開始今天的課 (plan §19.2.6): cards from news, interests and general topics;
/// another batch without repeats; a focus theme with its setting sheet; the
/// lesson written from the news card's article and set in the focus place.
#[tokio::test]
async fn today_topic_cards_and_focus() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;
    let base = Arc::new(Mutex::new(String::new()));
    let site = serve(news_site(base.clone())).await;
    *base.lock().unwrap() = site.clone();
    std::fs::write(ws_path.join("sources.toml"), format!("[[source]]\nid = \"mock\"\nname = \"Mock News\"\nlang = \"en\"\nfeed = \"{site}/feed.xml\"\n")).unwrap();

    // No focus yet.
    let (_, f) = c.get("/api/focus?lang=en").await;
    assert_eq!(f["focus"], Value::Null, "{f}");

    // First batch: the feed is fetched; the headline becomes a news card.
    let (st, r) = c.post("/api/today/topics", json!({"lang": "en", "refresh": true})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let cards = r["cards"].as_array().unwrap().clone();
    assert_eq!(cards.len(), 4, "{r}");
    assert_eq!(cards[0]["kind"], "news");
    assert_eq!(cards[0]["article"]["title"], "Edinburgh trams reach Newhaven");
    let req = calls.lock().unwrap().last().unwrap().clone();
    assert!(req["messages"][0]["content"].as_str().unwrap().contains("Edinburgh trams reach Newhaven"), "headlines go to the model");

    // Another batch: nothing already shown comes back.
    let shown: Vec<String> = cards.iter().map(|c| c["title"].as_str().unwrap().to_string()).collect();
    let (_, r2) = c.post("/api/today/topics", json!({"lang": "en", "exclude": shown})).await;
    let again: Vec<&str> = r2["cards"].as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap()).collect();
    assert!(!again.is_empty() && again.iter().all(|t| !shown.iter().any(|s| s == t)), "{again:?} vs {shown:?}");

    // A focus theme: its setting sheet is written to the profile and steers the cards.
    let (st, f) = c.post("/api/focus", json!({"lang": "en", "name": "蘇格蘭／英國旅遊"})).await;
    assert_eq!(st, StatusCode::OK, "{f}");
    assert_eq!(f["focus"]["name"], "蘇格蘭／英國旅遊");
    let file = ws_path.join(f["file"].as_str().unwrap());
    assert!(std::fs::read_to_string(&file).unwrap().contains("Princes Street"), "editable file in the workspace");
    let (_, r3) = c.post("/api/today/topics", json!({"lang": "en"})).await;
    assert_eq!(r3["focus"], "蘇格蘭／英國旅遊");
    let sys = calls.lock().unwrap().last().unwrap()["system"].to_string();
    assert!(sys.contains("蘇格蘭／英國旅遊") && sys.contains("Royal Mile"), "the focus and its names go to the model");
    assert_eq!(c.post("/api/focus", json!({"lang": "en", "name": " "})).await.0, StatusCode::BAD_REQUEST);

    // Start with the news card: today's course, lesson 1 written from that article, set in Scotland.
    let card = r3["cards"][0].clone();
    assert_eq!(card["kind"], "news", "{r3}");
    let (st, s) = c.post("/api/today/start", json!({"lang": "en", "card": card})).await;
    assert_eq!(st, StatusCode::OK, "{s}");
    assert_eq!(s["created"], true);
    let id = s["id"].as_str().unwrap().to_string();
    let (st, again) = c.post("/api/today/start", json!({"lang": "en", "card": r3["cards"][1]})).await;
    assert_eq!((st, again["id"].as_str(), again["created"].as_bool()), (StatusCode::OK, Some(id.as_str()), Some(false)), "one daily course a day");
    let (st, l) = c.post(&format!("/api/courses/{id}/lessons/1/open"), json!({})).await;
    assert_eq!(st, StatusCode::OK, "{l}");
    let req = calls.lock().unwrap().iter().rev().find(|b| b["output_config"]["format"]["schema"]["properties"]["text"].is_object()).cloned().unwrap();
    let all = format!("{}{}", req["system"], req["messages"][0]["content"]);
    assert!(all.contains("Newhaven"), "the article is the lesson's source");
    assert!(all.contains("Princes Street") && all.contains("set the text there"), "the lesson is set in the focus place");

    // Cancel the focus.
    let (_, d) = c.send(reqwest::Method::DELETE, "/api/focus?lang=en", json!({})).await;
    assert_eq!(d["removed"], true);
    assert!(!file.exists());
}

/// An upload tells something (plan §19.2.7): the model suggests interests,
/// which go to interests.csv only when confirmed; a course from it is a
/// preview first and saved on confirm.
#[tokio::test]
async fn upload_feedback_interests_and_outline_preview() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;
    let article = "Chipmakers race to build AI accelerators. Several chipmakers announced new factories. Power supply is the bottleneck.";
    let (st, up) = c.bytes("/api/import/file?name=chips.md", article.as_bytes().to_vec()).await;
    assert_eq!(st, StatusCode::OK, "{up}");
    let file = up["file"].as_str().unwrap().to_string();
    let csv = ws_path.join("profiles/ellrin/interests.csv");
    let before = std::fs::read_to_string(&csv).unwrap_or_default();

    // Suggest: the model reads the upload; known interests are left out; nothing is written.
    let (st, g) = c.post("/api/interests/suggest", json!({"lang": "en", "file": file})).await;
    assert_eq!(st, StatusCode::OK, "{g}");
    assert_eq!(g["keywords"], json!(["半導體", "AI 晶片"]), "music is already an interest");
    assert_eq!(g["summary"], "一篇關於晶片工廠的新聞");
    let req = calls.lock().unwrap().last().unwrap().clone();
    assert!(req["messages"][0]["content"].as_str().unwrap().contains("Chipmakers race"), "the upload goes to the model");
    assert!(req["system"].as_str().unwrap().contains("music"), "the prompt knows the existing interests");
    assert_eq!(std::fs::read_to_string(&csv).unwrap_or_default(), before, "a suggestion writes nothing");
    // Only the learner's own files, and only text.
    assert_eq!(c.post("/api/interests/suggest", json!({"lang": "en", "file": "../../etc/passwd"})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(c.post("/api/interests/suggest", json!({"lang": "en", "file": "profiles/ellrin/materials/photo.png"})).await.0, StatusCode::BAD_REQUEST);

    // Confirmed (one edited away, one added by hand).
    let (st, a) = c.post("/api/interests", json!({"keywords": ["半導體", "電力", " ", "MUSIC"]})).await;
    assert_eq!(st, StatusCode::OK, "{a}");
    assert_eq!(a["added"], 2, "{a}");
    let after = std::fs::read_to_string(&csv).unwrap();
    assert!(after.contains("半導體") && after.contains("電力") && !after.contains("AI 晶片"), "{after}");
    let (_, status) = c.get("/api/status").await;
    let mine: Vec<&str> = status["user"]["interests"].as_array().unwrap().iter().map(|i| i["keyword"].as_str().unwrap()).collect();
    assert!(mine.contains(&"半導體"), "{mine:?}");
    assert_eq!(c.post("/api/interests", json!({"keywords": ["", "a,b"]})).await.0, StatusCode::BAD_REQUEST);

    // A course from the upload: the outline first, nothing saved until confirmed.
    let (st, pv) = c.post("/api/courses", json!({"kind": "topic", "lang": "en", "sources": [{"text": article}], "preview": true})).await;
    assert_eq!(st, StatusCode::OK, "{pv}");
    assert!(pv["total"].as_u64().unwrap() >= 3, "{pv}");
    let (_, list) = c.get("/api/courses?lang=en").await;
    assert_eq!(list["courses"].as_array().unwrap().len(), 0, "a preview is not saved");
    let (st, saved) = c.post("/api/courses/confirm", json!({"course": pv["preview"]})).await;
    assert_eq!(st, StatusCode::OK, "{saved}");
    let (_, list) = c.get("/api/courses?lang=en").await;
    assert_eq!(list["courses"].as_array().unwrap().len(), 1);
    assert_eq!(list["courses"][0]["id"], saved["id"]);
    // A tampered outline cannot point at files outside the learner's folder.
    let mut bad = pv["preview"].clone();
    bad["kind"] = json!("fixed");
    bad["vocab"] = json!(["../../outside.csv"]);
    assert_eq!(c.post("/api/courses/confirm", json!({"course": bad})).await.0, StatusCode::BAD_REQUEST);
}

/// The admin prepares a family member's lessons for the iPad (plan F5 step 2):
/// daily courses in the member's folder, a package per lesson with the
/// conversation prompts rendered, an index per day; only an admin may.
#[tokio::test]
async fn admin_prepares_lessons_for_a_member() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    let base = Arc::new(Mutex::new(String::new()));
    let site = serve(news_site(base.clone())).await;
    *base.lock().unwrap() = site.clone();
    std::fs::write(ws_path.join("sources.toml"), format!("[[source]]\nid = \"mock\"\nname = \"Mock News\"\nlang = \"en\"\nfeed = \"{site}/feed.xml\"\n")).unwrap();
    // The news is fetched on the admin's computer.
    assert_eq!(c.post("/api/today/topics", json!({"lang": "en", "refresh": true})).await.0, StatusCode::OK);
    let (_, me) = c.get("/api/status").await;
    let admin = me["user"]["id"].as_str().unwrap().to_string();
    let (st, r) = c.post("/api/users", json!({ "name": "daidai", "languages": ["en"], "majors": { "en": "A2" }, "interests": ["travel"] })).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let member = r["user"]["id"].as_str().unwrap().to_string();
    // As the member: not allowed.
    assert_eq!(c.post("/api/family/prepare", json!({"member": admin, "lang": "en"})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(c.post("/api/users/select", json!({"id": admin})).await.0, StatusCode::OK);
    assert_eq!(c.post("/api/family/prepare", json!({"member": "nobody", "lang": "en"})).await.0, StatusCode::BAD_REQUEST, "an unknown member");
    assert_eq!(c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 9})).await.0, StatusCode::BAD_REQUEST);

    let (st, r) = c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 2, "choices": 2})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let mut p = Value::Null;
    for _ in 0..200 {
        p = c.get("/api/family/prepare").await.1["progress"].clone();
        if p["running"] == false {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(p["running"], false, "{p}");
    assert_eq!(p["errors"], json!([]), "{p}");
    assert_eq!(p["done"], 4, "{p}");

    let dir = ws_path.join("profiles").join(&member).join("ipad");
    let index: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
    let days = index["days"].as_object().unwrap();
    assert_eq!(days.len(), 2, "{index}");
    assert_eq!(index["realtime_model"], "claude-haiku-5-5", "the iPad takes the model from here");
    let first = days.values().next().unwrap().as_array().unwrap();
    assert!(first.len() >= 2, "two topics to choose from each day: {index}");
    let titles: Vec<&str> = days.values().flat_map(|d| d.as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap())).collect();
    let mut uniq = titles.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), 4, "four different topics: {titles:?}");

    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(first[0]["file"].as_str().unwrap().trim_start_matches("ipad/"))).unwrap()).unwrap();
    assert_eq!(pkg["member"], member.as_str());
    assert!(pkg["data"].is_object() && pkg["plan"]["units"].as_array().is_some_and(|u| !u.is_empty()), "{pkg}");
    assert!(ws_path.join(pkg["file"].as_str().unwrap()).is_file(), "the lesson file is in the member's folder");
    assert!(pkg["file"].as_str().unwrap().starts_with(&format!("profiles/{member}/")));
    assert!(pkg["plan"]["units"].as_array().unwrap().iter().any(|u| u["kind"] == "conversation"), "a daily lesson has a conversation: {}", pkg["plan"]);
    {
        let conv = &pkg["conversation"];
        assert!(conv["system"].as_str().is_some_and(|s| !s.is_empty()) && conv["system_last"].is_string() && conv["review_system"].is_string(), "{conv}");
        assert!(conv["turn_schema"].is_object() && conv["opening"].is_string());
    }
    // The admin's own courses are untouched.
    let (_, mine) = c.get("/api/courses?lang=en").await;
    assert_eq!(mine["courses"].as_array().unwrap().len(), 0, "{mine}");

    // The iPad writes back the lesson's results; the admin's computer applies them.
    let course = pkg["course"].as_str().unwrap().to_string();
    let n = pkg["lesson"].as_u64().unwrap();
    let mut results = vec![];
    for u in pkg["plan"]["units"].as_array().unwrap() {
        let i = u["index"].as_u64().unwrap();
        let kind = u["kind"].as_str().unwrap();
        let base = json!({ "type": "unit", "course": course, "lesson": n, "unit": i });
        let mut r = base.clone();
        match kind {
            "review" => { r["cards"] = json!(5); r["score"] = json!(0.8); }
            "reading" | "quiz" => {
                let qs = pkg["data"]["questions"].as_array().map(|q| q.len()).unwrap_or(1);
                r["answers"] = json!((0..qs).map(|k| json!({ "index": k, "answer": "x", "correct": true })).collect::<Vec<_>>());
            }
            "conversation" => {
                let conv = &pkg["conversation"];
                let limit = conv["turns"].as_u64().unwrap();
                let mut turns = vec![json!({ "role": "ai", "text": conv["opening"] })];
                for k in 0..limit {
                    turns.push(json!({ "role": "me", "text": format!("I think chips are important for my job, reason {k}.") }));
                    if k + 1 < limit {
                        turns.push(json!({ "role": "ai", "text": "Why is that?", "recast": "" }));
                    }
                }
                r = json!({ "type": "conversation", "course": course, "lesson": n, "unit": i, "started_at": "2026-11-08T01:00:00Z", "turns": turns, "review": review() });
            }
            _ => { r["items"] = json!([{ "prompt": "p", "answer": "a", "model": "m", "correct": true }, { "prompt": "q", "answer": "b", "model": "n", "hint": "h", "correct": false }]); }
        }
        results.push(r);
    }
    let inbox_dir = dir.join("inbox");
    std::fs::create_dir_all(&inbox_dir).unwrap();
    let file = json!({ "version": 1, "id": "t1", "member": member, "results": results });
    std::fs::write(inbox_dir.join("2026-11-08T0100-t1.json"), file.to_string()).unwrap();
    std::fs::write(inbox_dir.join("2026-11-08T0101-bad.json"), "{ not json").unwrap();
    let (st, a) = c.post("/api/family/inbox", json!({})).await;
    assert_eq!(st, StatusCode::OK, "{a}");
    let ap = a["applied"].as_array().unwrap().iter().find(|x| x["member"] == member.as_str()).cloned().unwrap();
    assert_eq!(ap["files"], 1, "{ap}");
    assert_eq!(ap["conversations"], 1, "{ap}");
    assert_eq!(ap["units"].as_u64().unwrap() + 1, results.len() as u64, "{ap}");
    assert!(ap["points"].as_f64().unwrap() > 0.0, "{ap}");
    assert_eq!(ap["errors"].as_array().unwrap().len(), 1, "the unreadable file is reported: {ap}");
    assert!(inbox_dir.join("done/2026-11-08T0100-t1.json").is_file());
    assert!(inbox_dir.join("bad/2026-11-08T0101-bad.json").is_file());
    let points_dir = ws_path.join("profiles").join(&member).join("records");
    let all: String = walk(&points_dir).iter().map(|p| std::fs::read_to_string(p).unwrap_or_default()).collect();
    assert!(all.contains("conversation"), "points lines in the member's records");
    assert!(walk(&ws_path.join("profiles").join(&member).join("conversations")).iter().any(|p| p.extension().is_some_and(|e| e == "md")), "the conversation record");
    // The same file again: nothing more is counted.
    std::fs::write(inbox_dir.join("2026-11-08T0102-t1-again.json"), file.to_string()).unwrap();
    let (_, again) = c.post("/api/family/inbox", json!({})).await;
    let ap2 = again["applied"].as_array().unwrap().iter().find(|x| x["member"] == member.as_str()).cloned().unwrap();
    assert_eq!(ap2["points"], 0.0, "already counted: {ap2}");
    // Both at once, the admin's own iPad included: one more lesson each, at each one's level.
    let (st, r) = c.post("/api/family/prepare", json!({"members": [member, admin], "lang": "en", "days": 1, "choices": 1})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let mut p = Value::Null;
    for _ in 0..200 {
        p = c.get("/api/family/prepare").await.1["progress"].clone();
        if p["running"] == false {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!((p["running"].as_bool(), p["done"].as_u64(), p["errors"].as_array().map(|e| e.len())), (Some(false), Some(2), Some(0)), "{p}");
    assert_eq!(p["members"], json!([member, admin]));
    let own = ws_path.join("profiles").join(&admin).join("ipad");
    let mine: Value = serde_json::from_str(&std::fs::read_to_string(own.join("index.json")).unwrap()).unwrap();
    let entry = mine["days"].as_object().unwrap().values().next().unwrap()[0].clone();
    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(own.join(entry["file"].as_str().unwrap().trim_start_matches("ipad/"))).unwrap()).unwrap();
    assert_eq!(pkg["member"], admin.as_str());
    assert!(own.join("buddies.json").is_file() && dir.join("buddies.json").is_file(), "both iPads show the buddies");
    // The admin's own iPad results are applied too, and the open workspace sees them.
    std::fs::create_dir_all(own.join("inbox")).unwrap();
    let reading = pkg["plan"]["units"].as_array().unwrap().iter().find(|u| u["kind"] == "reading").unwrap()["index"].clone();
    let qs = pkg["data"]["questions"].as_array().map(|q| q.len()).unwrap_or(1);
    let answers: Vec<Value> = (0..qs).map(|k| json!({ "index": k, "answer": "x", "correct": true })).collect();
    let mine_result = json!({ "version": 1, "id": "m1", "member": admin, "results": [{ "type": "unit", "course": pkg["course"], "lesson": pkg["lesson"], "unit": reading, "answers": answers }] });
    std::fs::write(own.join("inbox/2026-11-08T0200-m1.json"), mine_result.to_string()).unwrap();
    let (_, a) = c.post("/api/family/inbox", json!({})).await;
    let ap = a["applied"].as_array().unwrap().iter().find(|x| x["member"] == admin.as_str()).cloned().unwrap();
    assert!(ap["points"].as_f64().unwrap() > 0.0, "{ap}");
    let (_, today) = c.get("/api/today?lang=en").await;
    assert!(today["summary"]["total"].as_f64().unwrap() > 0.0, "{today}");
    // Only an admin applies results.
    assert_eq!(c.post("/api/users/select", json!({"id": member})).await.0, StatusCode::OK);
    assert_eq!(c.post("/api/family/inbox", json!({})).await.0, StatusCode::BAD_REQUEST);
}

/// The topic library (decisions §22): a theme's references read (robots.txt
/// checked), split into one-thing lessons, a text per lesson with its
/// sources, a lesson made from a text with its key terms.
#[tokio::test]
async fn library_themes_split_into_lessons_with_texts() {
    use axum::routing::get;
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;
    let wiki = serve(
        Router::new()
            .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /private/\n" }))
            .route("/wiki/Haggis", get(|| async { axum::response::Html("<html><head><title>Haggis - Wikipedia</title><meta property=\"og:image\" content=\"/wikipedia/commons/thumb/a/ab/Haggis.png/960px-Haggis.png?utm_source=x\"></head><body><div id='mw-content-text'><div class='mw-parser-output'><p>Haggis is a savoury pudding containing sheep's pluck.[1][2] It is traditionally eaten on Burns Night.[citation needed]</p><p>Today it is served in restaurants across Scotland and sold in supermarkets.</p></div></div></body></html>") }))
            .route("/private/secret", get(|| async { "<p>should never be read, robots.txt forbids it</p>" }))
            .route("/wikipedia/commons/thumb/a/ab/Haggis.png/960px-Haggis.png", get(|| async {
                let img = image::RgbImage::from_fn(1200, 600, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 90]));
                let mut png = Vec::new();
                image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
                ([("content-type", "image/png")], png)
            }))
            .route("/wiki/File:Haggis.png", get(|| async { axum::response::Html("<table><tr><td id=\"fileinfotpl&#95;aut\">Author</td><td>A. Cook</td></tr></table><span class=\"licensetpl&#95;short\">CC BY-SA 4.0</span>") })),
    )
    .await;
    let dir = ws_path.join("library/uk");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("syllabus.toml"),
        format!(
            "name = \"蘇格蘭與英國\"\nlang = \"en\"\ntopic = [\n  {{ id = \"haggis\", cat = \"飲食\", zh = \"哈吉斯\", en = \"Haggis\", refs = [\"{wiki}/wiki/Haggis\", \"{wiki}/wiki/Missing\"], news = [\"haggis\"] }},\n  {{ id = \"secret\", cat = \"人文\", zh = \"禁止\", en = \"Forbidden\", refs = [\"{wiki}/private/secret\"] }},\n]\n"
        ),
    )
    .unwrap();
    let (_, l) = c.get("/api/library").await;
    assert_eq!((l["collections"][0]["themes"].as_u64(), l["collections"][0]["written"].as_u64()), (Some(2), Some(0)), "{l}");

    let wait = || async {
        let mut p = Value::Null;
        for _ in 0..200 {
            p = c.get("/api/library/build").await.1["progress"].clone();
            if p["running"] == false { break; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        p
    };
    // One text this time: the theme is split, its first lesson written.
    let (st, r) = c.post("/api/library/build", json!({"collection": "uk", "limit": 1})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let p = wait().await;
    assert_eq!(p["done"], 1, "{p}");
    let plan = std::fs::read_to_string(dir.join("plans/haggis.toml")).unwrap();
    assert!(plan.contains("haggis-1") && plan.contains("haggis-2") && plan.contains("Ordering haggis"), "{plan}");
    assert!(dir.join("飲食/haggis-1.md").exists() && !dir.join("飲食/haggis-2.md").exists());
    let split = calls.lock().unwrap().iter().find(|b| b["output_config"]["format"]["schema"]["properties"]["lessons"]["items"]["properties"]["goal"].is_object()).cloned().unwrap();
    assert!(split["system"].as_str().unwrap().contains("One lesson = one thing"));
    assert!(split["messages"][0]["content"].as_str().unwrap().contains("savoury pudding containing sheep's pluck. It is traditionally"), "citation marks removed");
    let text_req = calls.lock().unwrap().iter().rev().find(|b| b["output_config"]["format"]["schema"]["properties"]["used_sources"].is_object()).cloned().unwrap();
    assert!(text_req["system"].as_str().unwrap().contains("How to order haggis in a restaurant.") && text_req["system"].as_str().unwrap().contains("neeps and tatties"), "the lesson's focus and words");
    // The rest; the forbidden page is never read.
    let (st, _) = c.post("/api/library/build", json!({"collection": "uk"})).await;
    assert_eq!(st, StatusCode::OK);
    let p = wait().await;
    assert_eq!(p["done"], 1, "{p}");
    assert!(p["errors"][0].as_str().unwrap().contains("禁止"), "robots.txt keeps the forbidden page out: {p}");
    assert!(!calls.lock().unwrap().iter().any(|b| b.to_string().contains("should never be read")));
    let md = std::fs::read_to_string(dir.join("飲食/haggis-2.md")).unwrap();
    assert!(md.contains("## References") && md.contains("/wiki/Haggis") && md.contains("\"goal\": ") == false && md.contains("goal: "), "{md}");
    assert_eq!(c.post("/api/library/build", json!({"collection": "uk", "ids": ["haggis"]})).await.0, StatusCode::BAD_REQUEST, "nothing left; never rewritten");
    let (_, l) = c.get("/api/library").await;
    assert_eq!((l["collections"][0]["planned"].as_u64(), l["collections"][0]["written"].as_u64()), (Some(2), Some(2)), "{l}");

    // A lesson from a text, written from it when opened, with its key terms.
    let (st, les) = c.post("/api/library/lesson", json!({"collection": "uk", "id": "haggis-1"})).await;
    assert_eq!(st, StatusCode::OK, "{les}");
    let id = les["id"].as_str().unwrap().to_string();
    let (st, o) = c.post(&format!("/api/courses/{id}/lessons/1/open"), json!({})).await;
    assert_eq!(st, StatusCode::OK, "{o}");
    let req = calls.lock().unwrap().iter().rev().find(|b| b["output_config"]["format"]["schema"]["properties"]["text"].is_object() && b["output_config"]["format"]["schema"]["properties"]["conversation"].is_object()).cloned().unwrap();
    let all = format!("{}{}", req["system"], req["messages"][0]["content"]);
    assert!(all.contains("restaurants in Edinburgh serve it"), "the lesson is made from the text");
    assert!(all.contains("savoury"), "the text's key terms are the words to learn");
    assert_eq!(c.post("/api/library/lesson", json!({"collection": "uk", "id": "secret-1"})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(c.post("/api/library/lesson", json!({"collection": "../x", "id": "haggis-1"})).await.0, StatusCode::BAD_REQUEST);

    // The picture: its credit in the text; downloaded and shrunk for the lesson; gone when the lesson is done.
    let md = std::fs::read_to_string(dir.join("飲食/haggis-1.md")).unwrap();
    assert!(md.contains("image: ") && md.contains("A. Cook") && md.contains("Picture: A. Cook — CC BY-SA 4.0"), "{md}");
    assert_eq!(les["picture"], true, "{les}");
    let pic = o["picture"].clone();
    assert_eq!((pic["author"].as_str(), pic["licence"].as_str()), (Some("A. Cook"), Some("CC BY-SA 4.0")), "{pic}");
    let got = c.http.get(format!("{}{}", c.base, pic["src"].as_str().unwrap())).send().await.unwrap();
    assert_eq!(got.headers()["content-type"], "image/jpeg");
    let jpg = image::load_from_memory(&got.bytes().await.unwrap()).unwrap();
    assert_eq!((jpg.width(), jpg.height()), (800, 400), "shrunk to 800 wide");
    let units = o["plan"]["units"].as_array().unwrap().clone();
    for u in &units {
        let i = u["index"].as_u64().unwrap();
        match u["kind"].as_str().unwrap() {
            "reading" => {
                let qn = o["lesson"]["questions"].as_array().map(|q| q.len()).unwrap_or(0);
                let answers: Vec<Value> = (0..qn).map(|k| json!({ "index": k, "answer": "x", "correct": true })).collect();
                assert_eq!(c.post(&format!("/api/courses/{id}/lessons/1/units/{i}"), json!({ "answers": answers })).await.0, StatusCode::OK);
            }
            "review" => { assert_eq!(c.post(&format!("/api/courses/{id}/lessons/1/units/{i}"), json!({ "cards": 3, "score": 1.0 })).await.0, StatusCode::OK); }
            "conversation" => {
                let (_, conv) = c.post("/api/conversations", json!({ "lang": "en", "course": { "course": id, "lesson": 1, "unit": i } })).await;
                let cid = conv["id"].as_i64().unwrap();
                for k in 0..conv["turns_limit"].as_u64().unwrap() {
                    c.post(&format!("/api/conversations/{cid}/turns"), json!({ "text": format!("I would like haggis with neeps and tatties, please, number {k}.") })).await;
                }
                assert_eq!(c.post(&format!("/api/conversations/{cid}/finish"), json!({})).await.0, StatusCode::OK);
            }
            _ => { assert_eq!(c.post(&format!("/api/courses/{id}/lessons/1/units/{i}"), json!({ "items": [{ "prompt": "p", "answer": "a", "model": "m", "correct": true }] })).await.0, StatusCode::OK); }
        }
    }
    let (_, course) = c.get(&format!("/api/courses/{id}")).await;
    assert_eq!(course["lessons"][0]["status"], "done", "{course}");
    let got = c.http.get(format!("{}{}", c.base, pic["src"].as_str().unwrap())).send().await.unwrap();
    assert_eq!(got.status().as_u16(), 404, "the picture is removed once the lesson is done");
    assert!(std::fs::read_to_string(dir.join("飲食/haggis-1.md")).unwrap().contains("A. Cook"), "the credit stays in the text");
}

/// The admin prepares a member's English lessons from the topic library (the
/// default when the language has one): each day's choices from different
/// categories, texts not used before, packages for the iPad.
#[tokio::test]
async fn admin_prepares_library_lessons_for_a_member() {
    use jl_core::course::library::{self, ArticleOut, LessonFocus, SourceDoc, ThemePlan};
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;
    let dir = ws_path.join("library/uk");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("syllabus.toml"), "name = \"蘇格蘭與英國\"\nlang = \"en\"\ntopic = [\n  { id = \"whisky\", cat = \"飲食\", zh = \"威士忌\", en = \"Whisky\", refs = [\"w:Scotch whisky\"] },\n  { id = \"trains\", cat = \"交通\", zh = \"火車\", en = \"Trains\", refs = [\"w:ScotRail\"] },\n]\n").unwrap();
    let syl = library::load_syllabus(&ws_path, "uk").unwrap();
    let src = vec![SourceDoc { title: "Ref".into(), url: "https://en.wikipedia.org/wiki/Ref".into(), licence: "CC BY-SA 4.0".into(), text: String::new() }];
    for t in &syl.topic {
        let lessons: Vec<LessonFocus> = (1..=2)
            .map(|i| LessonFocus { id: format!("{}-{i}", t.id), zh: format!("{} 第 {i} 課", t.zh), en: format!("{} {i}", t.en), focus: "One thing.".into(), goal: "能說一句話".into(), vocabulary: vec!["ticket".into()], sources: vec![1], refs: vec![] })
            .collect();
        library::save_plan(&ws_path, "uk", t, &ThemePlan { theme: t.id.clone(), lesson: lessons.clone() }).unwrap();
        for l in &lessons {
            let text = (0..30).map(|k| format!("A day in Scotland sentence {k} about {}.", t.en)).collect::<Vec<_>>().join(" ");
            let a = ArticleOut { title: l.en.clone(), text, key_terms: vec![], used_sources: vec![1], summary: String::new() };
            let p = library::article_path(&ws_path, "uk", t, &l.id);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, library::render_article(&syl, "uk", t, l, &a, &src, None, "test", "2026-11-08")).unwrap();
        }
    }
    let (_, me) = c.get("/api/status").await;
    let admin = me["user"]["id"].as_str().unwrap().to_string();
    let (_, r) = c.post("/api/users", json!({ "name": "daidai", "languages": ["en"], "majors": { "en": "A2" } })).await;
    let member = r["user"]["id"].as_str().unwrap().to_string();
    c.post("/api/users/select", json!({"id": admin})).await;

    let wait = || async {
        let mut p = Value::Null;
        for _ in 0..200 {
            p = c.get("/api/family/prepare").await.1["progress"].clone();
            if p["running"] == false { break; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        p
    };
    let n_cards = calls.lock().unwrap().iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["cards"].is_object()).count();
    let (st, r) = c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 1, "choices": 2})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let p = wait().await;
    assert_eq!((p["done"].as_u64(), p["errors"].as_array().map(|e| e.len())), (Some(2), Some(0)), "{p}");
    assert_eq!(calls.lock().unwrap().iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["cards"].is_object()).count(), n_cards, "no news topic cards: the library is the source");
    let ipad = ws_path.join("profiles").join(&member).join("ipad");
    let index: Value = serde_json::from_str(&std::fs::read_to_string(ipad.join("index.json")).unwrap()).unwrap();
    let today = index["days"].as_object().unwrap().values().next().unwrap().as_array().unwrap().clone();
    let cats: Vec<&str> = today.iter().map(|e| e["category"].as_str().unwrap()).collect();
    assert_eq!(cats, vec!["飲食", "交通"], "the day's choices from different categories: {index}");
    assert!(today.iter().all(|e| e["kind"] == "library"));
    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(ipad.join(today[0]["file"].as_str().unwrap().trim_start_matches("ipad/"))).unwrap()).unwrap();
    assert!(pkg["data"]["text"].is_string() && pkg["conversation"]["system"].is_string(), "{pkg}");
    assert_eq!(pkg["card"]["why"], "能說一句話");
    let used_file = ws_path.join("profiles").join(&member).join("library-used.txt");
    let used = std::fs::read_to_string(&used_file).unwrap();
    assert_eq!(used.lines().collect::<Vec<_>>(), vec!["whisky-1", "trains-1"]);
    // Next time: the texts not used yet.
    c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 1, "choices": 2})).await;
    let p = wait().await;
    assert_eq!(p["done"], 2, "{p}");
    assert_eq!(std::fs::read_to_string(&used_file).unwrap().lines().count(), 4);
    // All used: said plainly.
    c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 1, "choices": 2})).await;
    let p = wait().await;
    assert!(p["errors"][0].as_str().unwrap().contains("寫更多課"), "{p}");
    // The news when asked for.
    c.post("/api/family/prepare", json!({"member": member, "lang": "en", "days": 1, "choices": 1, "source": "news"})).await;
    let _ = wait().await;
    assert!(calls.lock().unwrap().iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["cards"].is_object()).count() > n_cards, "news topic cards when asked");
}

/// Study buddies: the members of a workspace see each other's points, read
/// from the files they sync; the admin also sees how far a member's iPad
/// lessons are prepared, and the iPad gets the same cards as a file.
#[tokio::test]
async fn buddies_see_each_other() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    let (_, me) = c.get("/api/status").await;
    let admin = me["user"]["id"].as_str().unwrap().to_string();
    let (st, r) = c.post("/api/users", json!({ "name": "daidai", "languages": ["en"], "majors": { "en": "A2" } })).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let member = r["user"]["id"].as_str().unwrap().to_string();
    // What her device synced: today and yesterday.
    let today = chrono::Local::now().date_naive();
    let dir = ws_path.join("profiles").join(&member).join("records").join("points");
    std::fs::create_dir_all(&dir).unwrap();
    for (d, pts) in [(today, "1.5"), (today.pred_opt().unwrap(), "1")] {
        std::fs::write(dir.join(format!("{d}.csv")), format!("time,lang,unit,skill,points,score,detail\n{d}T09:00:00+08:00,en,review,vocab,{pts},1,x#0\n")).unwrap();
    }
    // As the member: both cards, hers marked, nothing about preparing.
    let (st, r) = c.get("/api/buddies").await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let list = r["buddies"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{r}");
    assert_eq!(r["admin"], false);
    let hers = list.iter().find(|b| b["id"] == member.as_str()).unwrap();
    assert_eq!((hers["me"].as_bool(), hers["today"].as_f64(), hers["week"].as_f64(), hers["streak"].as_u64()), (Some(true), Some(1.5), Some(2.5), Some(2)), "{hers}");
    assert_eq!(hers["levels"]["en"], "A2-1");
    assert_eq!(hers["days"].as_array().unwrap().len(), 7);
    assert!(list.iter().all(|b| b.get("prepared").is_none()), "{r}");
    // As the admin: her lessons on the iPad run out today.
    assert_eq!(c.post("/api/users/select", json!({"id": admin})).await.0, StatusCode::OK);
    let ipad = ws_path.join("profiles").join(&member).join("ipad");
    std::fs::create_dir_all(&ipad).unwrap();
    std::fs::write(ipad.join("index.json"), json!({ "version": 1, "days": { today.to_string(): [{ "title": "a" }], "2020-01-01": [{ "title": "old" }] } }).to_string()).unwrap();
    let (_, r) = c.get("/api/buddies").await;
    assert_eq!(r["admin"], true);
    let hers = r["buddies"].as_array().unwrap().iter().find(|b| b["id"] == member.as_str()).unwrap().clone();
    assert_eq!(hers["me"], false);
    assert_eq!(hers["prepared"], json!({ "until": today.to_string(), "days": 1 }), "{hers}");
    assert_eq!(r["buddies"].as_array().unwrap().iter().find(|b| b["id"] == admin.as_str()).unwrap()["today"], 0.0);
    // Collecting answers also writes the cards for her iPad (only hers: the admin has no iPad folder).
    assert_eq!(c.post("/api/family/inbox", json!({})).await.0, StatusCode::OK);
    let file: Value = serde_json::from_str(&std::fs::read_to_string(ipad.join("buddies.json")).unwrap()).unwrap();
    assert_eq!(file["as_of"], today.to_string());
    let hers = file["buddies"].as_array().unwrap().iter().find(|b| b["id"] == member.as_str()).unwrap();
    assert_eq!((hers["me"].as_bool(), hers["today"].as_f64()), (Some(true), Some(1.5)), "{file}");
    assert!(hers.get("prepared").is_none());
    assert!(!ws_path.join("profiles").join(&admin).join("ipad").exists());
}

/// A lesson the model takes long to write (a local model): opened twice at
/// once and once by a page that is left early, it is written once, kept,
/// and nothing else waits for it.
#[tokio::test]
async fn slow_lesson_is_written_once_and_kept() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup_slow(app_dir.path(), &ws_path, 1500).await;
    let (st, course) = c.post("/api/courses", json!({ "kind": "topic", "lang": "en", "keywords": ["AI chips"] })).await;
    assert_eq!(st, StatusCode::OK, "{course}");
    let id = course["id"].as_str().unwrap().to_string();
    let path = format!("/api/courses/{id}/lessons/1/open");
    let lesson_calls = || calls.lock().unwrap().iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["conversation"].is_object()).count();
    // A page left after a moment: the browser lets go of the request.
    let quick = reqwest::Client::builder().timeout(std::time::Duration::from_millis(200)).build().unwrap();
    assert!(quick.post(format!("{}{path}", c.base)).json(&json!({})).send().await.is_err(), "the left page gave up");
    // Two more opens of the same lesson: they wait for the first text. While
    // it is being written, the rest of the app answers at once (no lock held).
    let (a, b, waited) = tokio::join!(c.post(&path, json!({})), c.post(&path, json!({})), async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let t = std::time::Instant::now();
        assert_eq!(c.get("/api/status").await.0, StatusCode::OK);
        assert_eq!(c.send(reqwest::Method::PUT, "/api/user", json!({ "daily_minutes": 30 })).await.0, StatusCode::OK);
        t.elapsed()
    });
    assert!(waited < std::time::Duration::from_millis(800), "a settings change waited {waited:?} for the lesson");
    assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK), "{} {}", a.1, b.1);
    assert_eq!(a.1["lesson"]["text"], b.1["lesson"]["text"]);
    assert_eq!(lesson_calls(), 1, "written once");
    // Coming back later: the kept text, no model call.
    let (st, again) = c.post(&path, json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(again["generated"], false, "{again}");
    assert_eq!(lesson_calls(), 1);
}

/// A topic library with `per` written lessons for each theme (`id`, category).
fn write_library(ws_path: &Path, themes: &[(&str, &str)], per: usize) {
    use jl_core::course::library::{self, ArticleOut, LessonFocus, SourceDoc, ThemePlan};
    let dir = ws_path.join("library/uk");
    std::fs::create_dir_all(&dir).unwrap();
    let rows: String = themes.iter().map(|(id, cat)| format!("  {{ id = \"{id}\", cat = \"{cat}\", zh = \"{id} 主題\", en = \"{id}\", refs = [\"w:{id}\"], news = [\"{id}\"] }},\n")).collect();
    std::fs::write(dir.join("syllabus.toml"), format!("name = \"蘇格蘭與英國\"\nlang = \"en\"\ntopic = [\n{rows}]\n")).unwrap();
    let syl = library::load_syllabus(ws_path, "uk").unwrap();
    let src = vec![SourceDoc { title: "Ref".into(), url: "https://en.wikipedia.org/wiki/Ref".into(), licence: "CC BY-SA 4.0".into(), text: String::new() }];
    for t in &syl.topic {
        let lessons: Vec<LessonFocus> = (1..=per)
            .map(|i| LessonFocus { id: format!("{}-{i}", t.id), zh: format!("{} 第 {i} 課", t.id), en: format!("{} {i}", t.en), focus: "One thing.".into(), goal: "學完後，你能說一句話".into(), vocabulary: vec!["ticket".into()], sources: vec![1], refs: vec![] })
            .collect();
        library::save_plan(ws_path, "uk", t, &ThemePlan { theme: t.id.clone(), lesson: lessons.clone() }).unwrap();
        for l in &lessons {
            let text = (0..30).map(|k| format!("A day in Britain sentence {k} about {}.", t.en)).collect::<Vec<_>>().join(" ");
            let a = ArticleOut { title: l.en.clone(), text, key_terms: vec![], used_sources: vec![1], summary: String::new() };
            let p = library::article_path(ws_path, "uk", t, &l.id);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, library::render_article(&syl, "uk", t, l, &a, &src, None, "test", "2026-11-08")).unwrap();
        }
    }
}

fn card_calls(calls: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
    calls.lock().unwrap().iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["cards"].is_object()).map(|b| b["messages"][0]["content"].as_str().unwrap_or("").to_string()).collect()
}

/// About 70 % of today's cards are topic-library lessons (the syllabus's
/// `share`): three of four, no model needed for them; a library card starts
/// that lesson; lessons had are not offered again; when the library runs low
/// the share comes from British topics and the page is told.
#[tokio::test]
async fn today_cards_follow_the_library_share() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, calls) = setup(app_dir.path(), &ws_path).await;
    write_library(&ws_path, &[("whisky", "飲食"), ("trains", "交通"), ("castles", "觀光")], 1);

    let (st, r) = c.post("/api/today/topics", json!({ "lang": "en" })).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let cards = r["cards"].as_array().unwrap();
    let kinds: Vec<&str> = cards.iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(cards.len(), 4, "{r}");
    assert_eq!(kinds.iter().filter(|k| **k == "library").count(), 3, "70 % of 4 = 3 library cards: {kinds:?}");
    let cats: Vec<&str> = cards.iter().filter(|x| x["kind"] == "library").map(|x| x["category"].as_str().unwrap()).collect();
    assert_eq!(cats, vec!["飲食", "交通", "觀光"], "one per category");
    assert_eq!(r["library_short"], false);
    assert_eq!(card_calls(&calls).len(), 1, "one model call, for the fourth card");

    // Start a library card: that lesson, at this learner's level, kept as had.
    let card = cards.iter().find(|x| x["kind"] == "library").unwrap().clone();
    let (st, started) = c.post("/api/today/start", json!({ "lang": "en", "card": card })).await;
    assert_eq!(st, StatusCode::OK, "{started}");
    let (_, course) = c.get(&format!("/api/courses/{}", started["id"].as_str().unwrap())).await;
    assert_eq!(course["name"], card["title"], "{course}");
    let used = std::fs::read_to_string(ws_path.join("profiles/ellrin/library-used.txt")).unwrap();
    assert_eq!(used.trim(), card["library"].as_str().unwrap());

    // Two library lessons left, one of them shown already: the share comes
    // from British topics instead, and the page says the library runs low.
    let (_, r) = c.post("/api/today/topics", json!({ "lang": "en", "exclude": ["trains 第 1 課"] })).await;
    let kinds: Vec<&str> = r["cards"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "library").count(), 1, "{r}");
    assert!(!r["cards"].as_array().unwrap().iter().any(|x| x["title"] == card["title"] || x["title"] == "trains 第 1 課"), "not again: {r}");
    assert_eq!(r["library_short"], true);
    let prompts = card_calls(&calls);
    assert!(prompts.iter().any(|p| p.contains("英國的飲食") && p.contains("Interests: (none yet)")), "a call for British topics: {prompts:?}");
    assert!(r["cards"].as_array().unwrap().len() >= 3, "{r}");
}

/// 派本週的課 (`source: mix`): the library's share over the whole run, not
/// per day; with too few texts and nothing left to write, the news fills in
/// and the run says so.
#[tokio::test]
async fn weekly_mix_follows_the_library_share() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("JELearner-private");
    let (c, _calls) = setup(app_dir.path(), &ws_path).await;
    write_library(&ws_path, &[("whisky", "飲食"), ("trains", "交通")], 3);
    let (_, me) = c.get("/api/status").await;
    let admin = me["user"]["id"].as_str().unwrap().to_string();
    let wait = || async {
        let mut p = Value::Null;
        for _ in 0..400 {
            p = c.get("/api/family/prepare").await.1["progress"].clone();
            if p["running"] == false {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        p
    };
    let index = |id: &str| -> Vec<Value> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(ws_path.join(format!("profiles/{id}/ipad/index.json"))).unwrap()).unwrap();
        v["days"].as_object().unwrap().values().flat_map(|d| d.as_array().unwrap().clone()).collect()
    };
    // 2 days × 2 = 4 lessons: round(4 × 0.7) = 3 from the library, 1 other.
    c.post("/api/family/prepare", json!({ "members": [admin], "lang": "en", "days": 2, "choices": 2, "source": "mix" })).await;
    let p = wait().await;
    assert_eq!((p["done"].as_u64(), p["errors"].as_array().map(|e| e.len())), (Some(4), Some(0)), "{p}");
    let all = index(&admin);
    assert_eq!(all.iter().filter(|e| e["kind"] == "library").count(), 3, "{all:?}");
    // The next run takes the other three: all six written texts are used.
    c.post("/api/family/prepare", json!({ "members": [admin], "lang": "en", "days": 2, "choices": 2, "source": "mix" })).await;
    let p = wait().await;
    assert_eq!(p["done"], 4, "{p}");
    let all = index(&admin);
    assert_eq!(all.iter().filter(|e| e["kind"] == "library").count(), 6, "{all:?}");
    // Nothing left and nothing to write: the news fills in, said once.
    c.post("/api/family/prepare", json!({ "members": [admin], "lang": "en", "days": 1, "choices": 2, "source": "mix" })).await;
    let p = wait().await;
    assert_eq!(p["done"], 2, "{p}");
    assert!(p["errors"].as_array().unwrap().iter().any(|e| e.as_str().unwrap().contains("用新聞補上")), "{p}");
}
