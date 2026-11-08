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
    Router::new().route(
        "/v1/messages",
        post(move |Json(b): Json<Value>| {
            let calls = calls.clone();
            async move {
                calls.lock().unwrap().push(b.clone());
                let props = &b["output_config"]["format"]["schema"]["properties"];
                let user = b["messages"][0]["content"].as_str().unwrap_or("").to_string();
                let text = if props["cards"].is_object() {
                    topic_cards(&user)
                } else if props["news_keywords"].is_object() {
                    focus_sheet()
                } else if props["keywords"].is_object() && props["summary"].is_object() && props.as_object().map_or(0, |o| o.len()) == 2 {
                    json!({ "summary": "一篇關於晶片工廠的新聞", "keywords": ["半導體", "music", "AI 晶片"] })
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
    let calls = Arc::new(Mutex::new(vec![]));
    let claude = serve(mock_claude(calls.clone())).await;
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
    assert_eq!(kinds, vec!["review", "reading", "sentence-making", "conversation"]);

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

    // 1. Review: grade the cards as part of the course (no separate review round).
    for card in cards.iter().filter(|c| c["item_id"].is_i64()).take(3) {
        let (code, r) = c.post("/api/review", json!({ "kind": card["item_kind"], "id": card["item_id"], "grade": 4, "in_course": true })).await;
        assert_eq!(code, StatusCode::OK);
        assert!(r.get("award").is_none() && r.get("round").is_none(), "{r}");
    }
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/0"), json!({ "cards": 3, "score": 1.0 })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["passed"], true);
    assert_eq!(r["points"], 1.5, "all remembered: bonus");
    assert_eq!(r["next"]["unit"], 1);

    // 2. Reading: answer every question (choice answers checked by the server).
    let qn = opened["lesson"]["questions"].as_array().unwrap().len();
    for i in 0..qn {
        let (code, _) = c.post(&format!("/api/lessons/{lesson_id}/answers"), json!({ "answers": [{ "index": i, "answer": "Yes" }] })).await;
        assert_eq!(code, StatusCode::OK);
    }
    let answers: Vec<Value> = (0..qn).map(|i| json!({ "index": i, "answer": "Yes" })).collect();
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/1"), json!({ "answers": answers })).await;
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
    assert!(csv.contains(",en,reading,reading,1.5,1,lessons/en/") && csv.contains("-02.md#1"), "{csv}");
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
    let (code, r) = c.post(&format!("/api/courses/{id}/lessons/2/units/1"), json!({ "answers": [{ "index": 0, "answer": "No" }] })).await;
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
    assert_eq!(tasks.iter().map(|t| t["kind"].as_str().unwrap()).collect::<Vec<_>>().first(), Some(&"review"));
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
    let (code, r) = c.post(&format!("/api/courses/{daily_id}/lessons/1/units/0"), json!({ "cards": 3, "score": 1.0 })).await;
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
    assert_eq!(tasks[0]["done"], true, "{today}");
    assert_eq!(today["next"], 1);
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
    assert_eq!(today["next"], 0);
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
    assert_eq!(today["next"], 0);

    // All due cards graded, fewer than a round: done, without points.
    for d in &due[2..] {
        c.post("/api/review", json!({ "kind": d["item_kind"], "id": d["item_id"], "grade": 4 })).await;
    }
    let (_, today) = c.get("/api/today?lang=en").await;
    assert_eq!(today["due"], 0, "{today}");
    let rv = review_task(&today);
    assert_eq!(rv["done"], true, "{rv}");
    assert!(rv["points"].is_null(), "no round finished, no point: {rv}");
    assert_eq!(today["next"], 1);

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
    assert_eq!(c.post("/api/family/prepare", json!({"member": admin, "lang": "en"})).await.0, StatusCode::BAD_REQUEST, "not for oneself");
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
}
