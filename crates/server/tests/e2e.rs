//! Offline end-to-end test: the real server over HTTP, with mock servers for
//! the news site (including robots.txt), the Claude API, Gmail and Google
//! Calendar. No network access and no API key needed.

use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use jl_server::{GoogleMode, Options, Paths, Settings};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

/// A news site: robots.txt forbids /private/, one feed, two articles.
fn news_site(base: Arc<Mutex<String>>) -> Router {
    let feed = move || {
        let b = base.lock().unwrap().clone();
        async move {
            let xml = format!(
                r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Mock Tech</title>
                <item><title>Chipmakers race to build AI accelerators</title><link>{b}/articles/chips.html</link>
                  <description>New factories are planned.</description><pubDate>Mon, 02 Nov 2026 08:00:00 GMT</pubDate></item>
                <item><title>Members only</title><link>{b}/private/secret.html</link><description>x</description></item>
                </channel></rss>"#
            );
            ([("content-type", "application/rss+xml")], xml)
        }
    };
    Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /private/\n" }))
        .route("/feed.xml", get(feed))
        .route(
            "/articles/chips.html",
            get(|| async {
                axum::response::Html(
                    r#"<html><head><meta property="og:title" content="Chipmakers race to build AI accelerators"></head><body>
                    <nav><p>Home</p></nav><article>
                    <p>Several chipmakers announced plans on Monday to build new factories for AI accelerators.</p>
                    <p>Analysts say demand for specialised chips will keep rising over the next decade.</p>
                    </article></body></html>"#,
                )
            }),
        )
        .route("/private/secret.html", get(|| async { "should never be fetched" }))
}

fn lesson_json() -> Value {
    json!({
        "title": "Chipmakers race to build AI accelerators",
        "summary": "多家晶片製造商宣布興建新的 AI 加速器工廠。",
        "vocabulary": [
            {"term":"accelerator","reading":"/əkˈseləreɪtər/","part_of_speech":"n.","meaning":"加速器","example":"The lab bought a new accelerator.","example_translation":"實驗室買了新的加速器。"},
            {"term":"demand","reading":"/dɪˈmænd/","part_of_speech":"n.","meaning":"需求","example":"Demand for batteries is growing.","example_translation":"電池需求正在成長。"}
        ],
        "grammar": [
            {"pattern":"keep + V-ing","meaning":"持續…","explanation":"表示動作或狀態持續。","example":"Prices keep rising.","example_translation":"價格持續上漲。"}
        ],
        "questions": [
            {"kind":"choice","question":"What did the chipmakers announce?","options":["New factories","Layoffs","A merger"],"answer":"New factories","explanation":"第一段提到新工廠。"},
            {"kind":"short","question":"Use 'keep + V-ing' in a sentence.","options":[],"answer":"Demand keeps rising.","explanation":"keep 後接動名詞。"}
        ]
    })
}

/// A conversation turn: recast the learner's last message, ask again.
fn turn_reply(b: &Value) -> Value {
    let msgs = b["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "user", "the transcript starts with a user message");
    for w in msgs.windows(2) {
        assert_ne!(w[0]["role"], w[1]["role"], "turns alternate");
    }
    let last = msgs.last().unwrap()["content"].as_str().unwrap();
    let ja = b["system"].as_str().unwrap().contains("Japanese student");
    let opening = msgs.len() == 1;
    json!({
        "recast": if opening { String::new() } else { format!("So you mean: {last}") },
        "question": if ja { "なにが好きですか。" } else { "What else do you enjoy about it?" },
        "used_targets": [],
        "starters": if ja { json!(["わたしは〜", "〜が好きです"]) } else { json!([]) },
    })
}

fn review_json() -> Value {
    json!({
        "fluency": 4, "target_use": 4, "grammar": 5, "overall": "excellent",
        "summary": "你說了很多完整的句子。",
        "better": [{"original": "I very like it", "suggestion": "I really like it.", "why": "very 不修飾動詞"}],
        "suggested_words": [{"term": "enthusiastic", "reading": "/ɪnˌθuːziˈæstɪk/", "meaning": "熱衷的"}],
        "keywords": ["club", "music"]
    })
}

/// The Claude Messages API, checking what a real request must look like.
fn mock_claude(calls: Arc<Mutex<Vec<Value>>>) -> Router {
    Router::new().route(
        "/v1/messages",
        post(move |h: HeaderMap, Json(b): Json<Value>| {
            let calls = calls.clone();
            async move {
                if h.get("x-api-key").map(|v| v.to_str().unwrap()) != Some("sk-ant-test-key-0000") {
                    return (StatusCode::UNAUTHORIZED, Json(json!({"type":"error","error":{"type":"authentication_error","message":"bad key"}})))
                        .into_response();
                }
                assert_eq!(h["anthropic-version"], "2026-10-21");
                // Refusal fallbacks exist for Sonnet and Opus, not for Haiku (real-time answers).
                if b["model"].as_str().unwrap_or("").starts_with("claude-haiku") {
                    assert!(h.get("anthropic-beta").is_none() && b.get("fallbacks").is_none(), "no fallbacks on Haiku");
                } else {
                    assert_eq!(h["anthropic-beta"], "server-side-fallback-2026-07-01");
                }
                // Conversation turns and their evaluation are real time (Haiku); the rest makes lessons (Sonnet).
                let realtime = b["output_config"]["format"]["schema"]["properties"]["recast"].is_object()
                    || b["output_config"]["format"]["schema"]["properties"]["fluency"].is_object();
                assert_eq!(b["model"], if realtime { "claude-haiku-5-5" } else { "claude-sonnet-5-5" }, "model per task");
                assert!(b.get("temperature").is_none());
                calls.lock().unwrap().push(b.clone());
                let props = &b["output_config"]["format"]["schema"]["properties"];
                let text = if props["recast"].is_object() {
                    turn_reply(&b).to_string()
                } else if props["fluency"].is_object() {
                    review_json().to_string()
                } else if b["output_config"]["format"]["type"] == "json_schema" {
                    lesson_json().to_string()
                } else {
                    "OK".into()
                };
                Json(json!({
                    "id": "msg_test", "type": "message", "role": "assistant", "model": b["model"],
                    "content": [{"type": "text", "text": text}],
                    "stop_reason": "end_turn", "usage": {"input_tokens": 10, "output_tokens": 10}
                }))
                .into_response()
            }
        }),
    )
}

/// Gmail send + Calendar insert/list, recording what arrives.
fn mock_google(mail: Arc<Mutex<Vec<Value>>>, events: Arc<Mutex<Vec<Value>>>) -> Router {
    let ev2 = events.clone();
    Router::new()
        .route(
            "/gmail/v1/users/me/messages/send",
            post(move |h: HeaderMap, Json(b): Json<Value>| {
                let mail = mail.clone();
                async move {
                    assert_eq!(h["authorization"], "Bearer google-test-token");
                    let mut m = mail.lock().unwrap();
                    m.push(b);
                    Json(json!({"id": format!("gm{}", m.len())}))
                }
            }),
        )
        .route(
            "/calendar/v3/calendars/primary/events",
            post(move |Json(b): Json<Value>| {
                let events = events.clone();
                async move {
                    let mut e = events.lock().unwrap();
                    e.push(b);
                    Json(json!({"id": format!("ev{}", e.len())}))
                }
            })
            .merge(get(move |Query(_q): Query<HashMap<String, String>>| {
                let e = ev2.clone();
                async move { Json(json!({"items": e.lock().unwrap().clone()})) }
            })),
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
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
    }
    async fn post_text(&self, path: &str, body: &str) -> (StatusCode, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).body(body.to_string()).send().await.unwrap();
        (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn start(settings: Settings) -> Client {
    let app = jl_server::router(settings, Options { provider: None }).await;
    Client { base: serve(app).await, http: reqwest::Client::new() }
}

fn settings(app_dir: &Path, claude: &str, google: &str) -> Settings {
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.to_path_buf());
    s.claude.base_url = claude.to_string();
    s.claude_api_key = Some("sk-ant-test-key-0000".into());
    s.google.mode = GoogleMode::Api;
    s.google.gmail_base = google.to_string();
    s.google.calendar_base = google.to_string();
    s.google.token = Some("google-test-token".into());
    s
}

#[tokio::test]
async fn full_text_pipeline_offline() {
    // Mock services.
    let site_base = Arc::new(Mutex::new(String::new()));
    let site = serve(news_site(site_base.clone())).await;
    *site_base.lock().unwrap() = site.clone();
    let claude_calls = Arc::new(Mutex::new(vec![]));
    let claude = serve(mock_claude(claude_calls.clone())).await;
    let (mail, events) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(vec![])));
    let google = serve(mock_google(mail.clone(), events.clone())).await;

    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let c = start(settings(app_dir.path(), &claude, &google)).await;

    // First run: demo workspace, then connect the private workspace.
    let (st, s) = c.get("/api/status").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(s["demo"], true);
    assert_eq!(s["engine_ready"], true);
    let (st, r) = c.post("/api/setup/workspace", json!({"path": ws_path, "init": true})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["users"], 0, "a new workspace has no users yet");
    let (st, e) = c.get("/api/words?lang=en").await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "nothing before a user is chosen: {e}");
    // The first user: languages and self-assessed levels.
    let (st, r) = c
        .post("/api/users", json!({"name": "Ellrin", "languages": ["en", "ja"], "majors": {"en": "B1", "ja": "N5"}, "interests": ["AI chips"]}))
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["user"]["id"], "ellrin");
    assert_eq!(r["user"]["levels"]["en"]["level"], "B1-1");
    assert_eq!(r["user"]["levels"]["ja"]["steps"], 6, "N5 has six steps");
    assert!(r["import"]["words"].as_u64().unwrap() >= 4, "template vocab imported: {r}");
    assert!(ws_path.join("profiles/ellrin/interests.csv").exists());
    let cfg = std::fs::read_to_string(app_dir.path().join("config.toml")).unwrap();
    assert!(cfg.contains("JELearner-private"), "workspace remembered outside the repo");
    assert!(cfg.contains("active_users"), "this computer remembers the user");
    let (_, s) = c.get("/api/status").await;
    assert_eq!(s["admin"], true, "the first user is the admin");
    assert_eq!(s["demo"], false);
    let db = s["db"].as_str().unwrap();
    assert!(db.starts_with(app_dir.path().to_str().unwrap()), "database lives in the app folder, not the workspace");
    assert!(!db.starts_with(ws_path.to_str().unwrap()));

    // Point the crawler at the mock site.
    std::fs::write(
        ws_path.join("sources.toml"),
        format!("[[source]]\nid = \"mock\"\nname = \"Mock Tech\"\nlang = \"en\"\nfeed = \"{site}/feed.xml\"\n"),
    )
    .unwrap();
    let (st, news) = c.get("/api/news?lang=en&refresh=true").await;
    assert_eq!(st, StatusCode::OK, "{news}");
    assert_eq!(news["errors"], json!([]), "{news}");
    let arts = news["articles"].as_array().unwrap();
    let chips = arts.iter().find(|a| a["url"].as_str().unwrap().ends_with("/articles/chips.html")).unwrap();
    let private = arts.iter().find(|a| a["url"].as_str().unwrap().contains("/private/")).unwrap();

    // robots.txt is respected.
    let (st, err) = c.post("/api/lessons", json!({"article_id": private["id"]})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("robots.txt"), "{err}");

    // News -> lesson.
    let (st, lesson) = c.post("/api/lessons", json!({"article_id": chips["id"], "level": "B2"})).await;
    assert_eq!(st, StatusCode::OK, "{lesson}");
    assert_eq!(lesson["added"]["words"], 2);
    let path = ws_path.join(lesson["path"].as_str().unwrap());
    let md = std::fs::read_to_string(&path).unwrap();
    assert!(md.contains("# Chipmakers race to build AI accelerators"));
    assert!(md.contains("| accelerator |"));
    assert!(md.contains("keep + V-ing"));
    assert!(md.contains("jelearner:lesson"), "data embedded for rebuilding");
    {
        let calls = claude_calls.lock().unwrap();
        let req = &calls[0];
        assert_eq!(req["model"], "claude-sonnet-5-5", "lessons: the catalog's generation model, not hard-coded");
        assert!(req["messages"][0]["content"].as_str().unwrap().contains("Several chipmakers announced plans"));
        assert!(req["system"].as_str().unwrap().contains("B2"));
        assert!(req["messages"][0]["content"].as_str().unwrap().contains("resilient"), "known words passed");
    }
    let id = lesson["id"].as_i64().unwrap();
    let (_, words) = c.get("/api/words?lang=en&q=accel").await;
    assert_eq!(words.as_array().unwrap().len(), 1);

    // Quiz: one wrong choice, one self-marked short answer.
    let (st, res) = c
        .post(
            &format!("/api/lessons/{id}/answers"),
            json!({"answers": [{"index": 0, "answer": "Layoffs"}, {"index": 1, "answer": "Demand keeps rising"}]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{res}");
    assert_eq!(res["results"][0]["correct"], false);
    assert_eq!(res["results"][1]["correct"], true, "short answer matches loosely");
    let (_, ms) = c.get("/api/mistakes?lang=en").await;
    assert_eq!(ms.as_array().unwrap().len(), 1);
    assert_eq!(ms[0]["correct_answer"], "New factories");

    // Paste text instead of a URL (Japanese).
    let (st, l2) = c.post("/api/lessons", json!({"lang": "ja", "title": "メモ", "text": "政府は半導体工場を支援する。"})).await;
    assert_eq!(st, StatusCode::OK, "{l2}");
    assert!(l2["path"].as_str().unwrap().starts_with("profiles/ellrin/lessons/ja/"), "{l2}");

    // Own vocabulary: CSV upload and a word typed in by hand.
    let (st, imp) = c
        .post_text("/api/import?kind=vocab&name=toeic.csv&lang=en", "word,meaning\nquarterly,每季的\nrevenue,營收\n")
        .await;
    assert_eq!(st, StatusCode::OK, "{imp}");
    assert_eq!(imp["report"]["words"], 2);
    assert!(ws_path.join(imp["file"].as_str().unwrap()).exists(), "uploaded file kept in the workspace");
    let (st, bad) = c.post_text("/api/import?kind=vocab&name=x.csv", "a,b\n1,2\n").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
    let (st, _) = c.post("/api/words", json!({"lang": "en", "term": "mitigate", "meaning": "減輕"})).await;
    assert_eq!(st, StatusCode::OK);
    let mine = std::fs::read_to_string(ws_path.join("profiles/ellrin/vocab/my-words-en.csv")).unwrap();
    assert!(mine.contains("mitigate,,減輕"));

    // Weekly material JSON.
    let material = json!({"week": "2026-W46", "title": "第46週", "language": "ja", "items": [
        {"kind": "word", "term": "需要", "reading": "じゅよう", "meaning": "需求"},
        {"kind": "grammar", "pattern": "〜につれて", "meaning": "隨著"}]});
    let (st, imp) = c.post_text("/api/import?kind=materials&name=week46.json", &material.to_string()).await;
    assert_eq!(st, StatusCode::OK, "{imp}");
    assert!(imp["file"].as_str().unwrap().starts_with("profiles/ellrin/materials/2026-W46/"));
    let (_, mats) = c.get("/api/materials").await;
    assert_eq!(mats.as_array().unwrap().len(), 2, "template sample + uploaded");

    // Review: grade every due English card once.
    let (_, due) = c.get("/api/review?lang=en&limit=100").await;
    let due = due.as_array().unwrap().clone();
    assert!(due.len() >= 6);
    for d in &due {
        let grade = if d["front"] == "demand" { 1 } else { 4 };
        let (st, _) = c.post("/api/review", json!({"kind": d["item_kind"], "id": d["item_id"], "grade": grade})).await;
        assert_eq!(st, StatusCode::OK);
    }
    let (_, due) = c.get("/api/review?lang=en").await;
    assert!(due.as_array().unwrap().is_empty());

    // Records export for the private repo.
    let (st, rec) = c.post("/api/records/export", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert!(rec["progress"].as_u64().unwrap() >= 6);
    assert!(std::fs::read_to_string(ws_path.join("profiles/ellrin/records/mistakes.csv")).unwrap().contains("New factories"));

    // Google (mock server with the real REST shapes).
    let (st, e) = c.post("/api/google/weekly-report", json!({})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "no address yet: {e}");
    let (st, _) = c.post_text("/api/status", "").await;
    assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED);
    let r = c.http.put(format!("{}/api/user", c.base)).json(&json!({"report_email": "me@example.com"})).send().await.unwrap();
    assert!(r.status().is_success());
    // The UI theme follows the user; only known ids are accepted.
    let r = c.http.put(format!("{}/api/user", c.base)).json(&json!({"theme": "sakuramatcha"})).send().await.unwrap();
    assert!(r.status().is_success());
    assert_eq!(c.get("/api/status").await.1["user"]["theme"], "sakuramatcha");
    assert!(std::fs::read_to_string(ws_path.join("profiles/ellrin/profile.toml")).unwrap().contains("theme = \"sakuramatcha\""));
    let r = c.http.put(format!("{}/api/user", c.base)).json(&json!({"theme": "orange"})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let (st, rep) = c.post("/api/google/weekly-report", json!({})).await;
    assert_eq!(st, StatusCode::OK, "{rep}");
    assert!(rep["email"]["body"].as_str().unwrap().contains("Ellrin"));
    assert_eq!(mail.lock().unwrap().len(), 1);
    let (st, sched) = c.post("/api/google/schedule-reviews", json!({"days": 7, "at_utc": "12:00"})).await;
    assert_eq!(st, StatusCode::OK, "{sched}");
    assert!(!sched["created"].as_array().unwrap().is_empty());
    let (st, book) = c.post("/api/google/book-lesson", json!({"start": "2026-11-10T12:00:00Z", "minutes": 30, "topic": "新聞討論", "lang": "ja"})).await;
    assert_eq!(st, StatusCode::OK, "{book}");
    assert!(book["email_id"].is_string(), "confirmation mail sent");
    assert_eq!(mail.lock().unwrap().len(), 2);
    assert!(events.lock().unwrap().iter().any(|e| e["summary"] == "日文課：新聞討論"));

    // Fetching a disallowed URL directly is refused too.
    let (st, _) = c.post("/api/articles", json!({"url": format!("{site}/private/secret.html"), "lang": "en"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Restart: same app folder, the database persisted.
    let c2 = start(settings(app_dir.path(), &claude, &google)).await;
    let (_, s2) = c2.get("/api/status").await;
    assert_eq!(s2["workspace"], json!(ws_path.display().to_string()));
    assert_eq!(s2["stats"]["en"]["lessons"], 1);
    assert_eq!(s2["stats"]["en"]["open_mistakes"], 1);

    // A second machine: fresh app folder, same (synced) workspace. Lessons
    // come back from the Markdown files, progress and mistakes from records/.
    let other = tempfile::tempdir().unwrap();
    let mut s3 = settings(other.path(), &claude, &google);
    s3.workspace = Some(ws_path.clone());
    let c3 = start(s3).await;
    let (_, s3) = c3.get("/api/status").await;
    assert_eq!(s3["stats"]["en"]["lessons"], 1, "{s3}");
    assert_eq!(s3["stats"]["ja"]["lessons"], 1);
    assert_eq!(s3["stats"]["en"]["open_mistakes"], 1);
    let (_, due3) = c3.get("/api/review?lang=en").await;
    assert!(due3.as_array().unwrap().is_empty(), "review schedule restored: {due3}");
}

#[tokio::test]
async fn engine_errors_are_reported() {
    let app_dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let claude = serve(mock_claude(calls)).await;
    let mut s = settings(app_dir.path(), &claude, "http://127.0.0.1:9");
    s.google.mode = GoogleMode::Mock;
    s.claude_api_key = Some("sk-ant-wrong".into());
    let c = start(s).await;
    let (st, e) = c.post("/api/engine/test", json!({})).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    assert!(e["error"].as_str().unwrap().contains("401"), "{e}");

    // Mock Google mode keeps a simulated outbox.
    let r = c.http.put(format!("{}/api/user", c.base)).json(&json!({"report_email": "me@example.com"})).send().await.unwrap();
    assert!(r.status().is_success());
    let (st, _) = c.post("/api/google/weekly-report", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let (_, out) = c.get("/api/google/outbox").await;
    assert_eq!(out["mail"].as_array().unwrap().len(), 1);
    let (st, e) = c.post("/api/settings/claude", json!({"effort": "turbo"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
}

#[tokio::test]
async fn serves_the_web_ui() {
    let app_dir = tempfile::tempdir().unwrap();
    let c = start(Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf())).await;
    let html = c.http.get(format!("{}/", c.base)).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("<title>JELearner</title>"));
    let (st, m) = c.get("/api/models").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(m["plan_mac32"]["fits"], true);
}

/// The Claude subscription path, with a fake `claude` CLI: lessons without an
/// API key, `auto` preferring the subscription, switching, and sign-in.
#[cfg(unix)]
#[tokio::test]
async fn subscription_engine_through_claude_code() {
    let site_base = Arc::new(Mutex::new(String::new()));
    let site = serve(news_site(site_base.clone())).await;
    *site_base.lock().unwrap() = site.clone();
    let claude_calls = Arc::new(Mutex::new(vec![]));
    let claude_api = serve(mock_claude(claude_calls.clone())).await;

    let cli_dir = tempfile::tempdir().unwrap();
    let cli = jl_core::claude_code::fake::write(cli_dir.path());
    std::fs::write(cli_dir.path().join("reply.json"), lesson_json().to_string()).unwrap();

    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    s.claude_bin = Some(cli.clone());
    s.claude.base_url = claude_api.clone();
    s.workspace = Some(ws_dir.path().to_path_buf());
    let c = start(s).await;
    let (st, _) = c.post("/api/setup/workspace", json!({"path": ws_dir.path(), "init": true})).await;
    assert_eq!(st, StatusCode::OK);
    // No user yet: nobody can sign in.
    let (st, _) = c.post("/api/claude-code/login", json!({})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = c.post("/api/users", json!({"name": "Ellrin", "languages": ["en"]})).await;
    assert_eq!(st, StatusCode::OK);

    // Installed but signed out, no key: nothing to generate with.
    let (_, cl) = c.get("/api/claude?fresh=true").await;
    assert_eq!(cl["subscription"]["installed"], true);
    assert_eq!(cl["subscription"]["ready"], false);
    assert_eq!(cl["engine"], "auto");
    assert!(cl["active_error"].as_str().unwrap().contains("登入"), "{cl}");

    // Sign in through Claude Code (the fake completes it at once).
    let (st, _) = c.post("/api/claude-code/login", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let mut ready = false;
    for _ in 0..50 {
        let (_, cl) = c.get("/api/claude?fresh=true").await;
        if cl["subscription"]["ready"] == true {
            ready = true;
            assert_eq!(cl["active"], "subscription");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ready, "login detected");
    let (_, status) = c.get("/api/status").await;
    assert_eq!(status["engine_label"], "Claude 訂閱");

    // A lesson from pasted text, through the subscription.
    let (st, l) = c.post("/api/lessons", json!({"lang": "en", "title": "Chips", "text": "Several chipmakers announced new factories."})).await;
    assert_eq!(st, StatusCode::OK, "{l}");
    assert_eq!(l["engine"], "claude-subscription:claude-sonnet-5-5");
    let args = std::fs::read_to_string(cli_dir.path().join("last-args")).unwrap();
    assert!(args.contains("--json-schema") && args.contains("--safe-mode"), "{args}");
    assert!(std::fs::read_to_string(cli_dir.path().join("last-stdin")).unwrap().contains("Several chipmakers"));
    assert!(claude_calls.lock().unwrap().is_empty(), "the API was not used");
    let ellrin_dir = std::fs::read_to_string(cli_dir.path().join("last-config-dir")).unwrap();
    assert!(ellrin_dir.ends_with("/ellrin"), "Claude Code ran with Ellrin's own sign-in folder: {ellrin_dir}");

    // A second user on the same computer does not get the first user's Claude.
    let (st, r) = c.post("/api/users", json!({"name": "daidai", "languages": ["ja"], "majors": {"ja": "N5"}})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["user"]["id"], "daidai");
    let (_, cl) = c.get("/api/claude?fresh=true").await;
    assert_eq!(cl["subscription"]["ready"], false, "the second user is not signed in by the first user's login: {cl}");
    assert!(cl["active"].is_null());
    let (st, _) = c.post("/api/lessons", json!({"lang": "ja", "text": "テスト"})).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "no fallback to someone else's Claude");
    let (_, words) = c.get("/api/words?lang=en&q=accel").await;
    assert!(words.as_array().unwrap().is_empty(), "the second user has their own database");
    let (_, st2) = c.get("/api/status").await;
    assert_eq!(st2["users"].as_array().unwrap().len(), 2);
    assert_eq!(st2["admin"], false, "only the first user is admin");
    // Back to Ellrin: signed in again, her words are there.
    let (st, _) = c.post("/api/users/select", json!({"id": "ellrin"})).await;
    assert_eq!(st, StatusCode::OK);
    let (_, cl) = c.get("/api/claude").await;
    assert_eq!(cl["active"], "subscription");
    let (_, words) = c.get("/api/words?lang=en&q=accel").await;
    assert_eq!(words.as_array().unwrap().len(), 1);

    // Add an API key: auto still prefers the subscription; switch to API explicitly.
    let (st, _) = c.post("/api/settings/claude", json!({"key": "sk-ant-test-key-0000"})).await;
    assert_eq!(st, StatusCode::OK);
    let (_, cl) = c.get("/api/claude").await;
    assert_eq!(cl["active"], "subscription");
    let (st, _) = c.post("/api/settings/engine", json!({"kind": "api"})).await;
    assert_eq!(st, StatusCode::OK);
    let (st, t) = c.post("/api/engine/test", json!({})).await;
    assert_eq!(st, StatusCode::OK, "{t}");
    assert_eq!(t["via"], "Claude API");
    assert_eq!(claude_calls.lock().unwrap().len(), 1);
    // Each path can be tested on its own regardless of the choice.
    let (st, t) = c.post("/api/engine/test", json!({"engine": "subscription"})).await;
    assert_eq!(st, StatusCode::OK, "{t}");
    assert_eq!(t["reply"], "OK");
    assert!(std::fs::read_to_string(app_dir.path().join("config.toml")).unwrap().contains("kind = \"api\""));
    let (st, _) = c.post("/api/settings/engine", json!({"kind": "turbo"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

/// A first-version (single-user) workspace in a git repository is moved into
/// profiles/<id>/ with `git mv`, nothing is lost, and the owner keeps the
/// first version's database.
#[tokio::test]
async fn migrates_a_first_version_workspace() {
    let ws_dir = tempfile::tempdir().unwrap();
    let ws = ws_dir.path();
    std::fs::write(ws.join("workspace.toml"), "id = \"ws-v1\"\nlanguages = [\"en\", \"ja\"]\n[levels]\nen = \"B2\"\nja = \"N5\"\n").unwrap();
    std::fs::create_dir_all(ws.join("vocab")).unwrap();
    std::fs::write(ws.join("vocab/mine-en.csv"), "term,meaning\nmitigate,減輕\n").unwrap();
    std::fs::create_dir_all(ws.join("records")).unwrap();
    std::fs::write(ws.join("records/mistakes.csv"), "lang,question,user_answer,correct_answer,explanation,created_at,resolved_at\nen,Q?,a,b,,2026-11-01T00:00:00Z,\n").unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git").args(args).current_dir(ws).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@example.com", "add", "-A"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "v1"]);

    let app_dir = tempfile::tempdir().unwrap();
    // The first version's database for this workspace.
    let legacy = app_dir.path().join("data/ws-v1/learner.sqlite");
    jl_core::db::Db::open(&legacy).await.unwrap();
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    s.workspace = Some(ws.to_path_buf());
    let c = start(s).await;

    let (_, st) = c.get("/api/status").await;
    assert_eq!(st["layout"], "v1");
    let (code, e) = c.get("/api/words?lang=en").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(e["error"].as_str().unwrap().contains("多使用者"), "{e}");

    let (code, r) = c.post("/api/workspace/migrate", json!({"name": "Ellrin"})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["migration"]["profile"], "ellrin");
    assert_eq!(r["user"]["levels"]["en"]["level"], "B2-1", "first-version levels carried over");
    assert_eq!(std::fs::read_to_string(ws.join("profiles/ellrin/vocab/mine-en.csv")).unwrap(), "term,meaning\nmitigate,減輕\n");
    let porcelain = git(&["status", "--porcelain"]);
    assert!(porcelain.contains("R  vocab/mine-en.csv -> profiles/ellrin/vocab/mine-en.csv"), "moved with git mv:\n{porcelain}");
    assert!(app_dir.path().join("data/ws-v1/ellrin/learner.sqlite").exists(), "the owner keeps the old database");
    assert!(!legacy.exists());

    let (_, words) = c.get("/api/words?lang=en&q=mitigate").await;
    assert_eq!(words.as_array().unwrap().len(), 1);
    let (_, ms) = c.get("/api/mistakes?lang=en").await;
    assert_eq!(ms.as_array().unwrap().len(), 1, "records restored");
    let (_, st) = c.get("/api/status").await;
    assert_eq!(st["layout"], "v2");
    let (code, _) = c.post("/api/workspace/migrate", json!({"name": "Ellrin"})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "only once");
}

/// Workspace locations that would put personal files in the wrong place are
/// refused, and nothing is created.
#[tokio::test]
async fn refuses_unsafe_workspace_locations() {
    let app_dir = tempfile::tempdir().unwrap();
    let c = start(Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf())).await;
    let repo = Paths::from_repo().web.parent().unwrap().to_path_buf();
    let inside = repo.join("my-private-workspace");
    for (route, body) in [
        ("/api/setup/new", json!({"path": "", "remote": ""})),
        ("/api/setup/new", json!({"path": "   ", "remote": ""})),
        ("/api/setup/new", json!({"path": "relative/folder", "remote": ""})),
        ("/api/setup/new", json!({"path": "/tmp/../tmp/x", "remote": ""})),
        ("/api/setup/new", json!({"path": inside, "remote": ""})),
        ("/api/setup/workspace", json!({"path": "", "init": true})),
        ("/api/setup/workspace", json!({"path": inside, "init": true})),
    ] {
        let (st, e) = c.post(route, body.clone()).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{route} {body}: {e}");
        assert!(e["error"].is_string());
    }
    assert!(!inside.exists(), "nothing created inside the program folder");
    assert!(!std::path::Path::new("relative").exists());
    let cfg = std::fs::read_to_string(app_dir.path().join("config.toml")).unwrap_or_default();
    assert!(!cfg.contains("workspace_path"), "no workspace remembered: {cfg}");
}


async fn put(c: &Client, path: &str, body: Value) -> (StatusCode, Value) {
    let r = c.http.put(format!("{}{path}", c.base)).json(&body).send().await.unwrap();
    (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json().await.unwrap_or(Value::Null))
}

/// A new workspace with Ellrin (English B1 and Japanese N5) and the mock
/// Claude API.
async fn stage_a_setup(app_dir: &Path, ws_path: &Path) -> (Client, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(vec![]));
    let claude = serve(mock_claude(calls.clone())).await;
    let mut st = settings(app_dir, &claude, "http://127.0.0.1:9");
    st.google.mode = GoogleMode::Mock;
    let c = start(st).await;
    let (code, r) = c.post("/api/setup/workspace", json!({"path": ws_path, "init": true})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let (code, r) = c.post("/api/users", json!({"name": "Ellrin", "languages": ["en", "ja"], "majors": {"en": "B1", "ja": "N5"}, "interests": ["music"]})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    (c, calls)
}

fn points_csv(ws_path: &Path) -> String {
    let dir = ws_path.join("profiles/ellrin/records/points");
    let mut out = String::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        out.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    out
}

/// The typed conversation from start to lock: topic and target words, a
/// too-short turn refused, the whole transcript sent every turn, the turn
/// limit, the lock, the evaluation, the point, the Markdown record and the
/// points CSV.
#[tokio::test]
async fn typed_conversation_until_locked() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, calls) = stage_a_setup(app_dir.path(), &ws_path).await;
    let (code, _) = put(&c, "/api/user", json!({"turns": 5})).await;
    assert_eq!(code, StatusCode::OK);

    let (code, next) = c.get("/api/conversations/next?lang=en").await;
    assert_eq!(code, StatusCode::OK, "{next}");
    assert_eq!(next["turns"], 5);
    assert_eq!(next["topic"]["kind"], "interest", "the first topic is from the interests (ratio 0.6)");
    let (_, other) = c.get(&format!("/api/conversations/next?lang=en&skip={}", next["topic"]["id"].as_str().unwrap())).await;
    assert_ne!(other["topic"]["id"], next["topic"]["id"], "another one");

    let (code, conv) = c.post("/api/conversations", json!({"lang": "en"})).await;
    assert_eq!(code, StatusCode::OK, "{conv}");
    let id = conv["id"].as_i64().unwrap();
    assert_eq!(conv["turns_done"], 0);
    let targets: Vec<String> = conv["conversation"]["targets"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap().to_string()).collect();
    assert!(!targets.is_empty(), "target words come from the learner's vocabulary");
    assert_eq!(conv["conversation"]["turns"][0]["role"], "ai", "the AI opens");

    let (code, e) = c.post(&format!("/api/conversations/{id}/turns"), json!({"text": "ok"})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "one word is not a turn: {e}");
    let long = "word ".repeat(80);
    let (code, _) = c.post(&format!("/api/conversations/{id}/turns"), json!({"text": long})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "typed turns are limited in length");

    let mut last = Value::Null;
    for i in 1..=5 {
        let text = if i == 2 { format!("Yesterday I used the word {} in a club meeting.", targets[0]) } else { format!("This is my answer number {i} about music.") };
        let (code, r) = c.post(&format!("/api/conversations/{id}/turns"), json!({"text": text})).await;
        assert_eq!(code, StatusCode::OK, "turn {i}: {r}");
        assert_eq!(r["turns_done"], i);
        assert_eq!(r["locked"], i == 5);
        if i == 2 {
            assert_eq!(r["used_now"], json!([targets[0]]), "the target word lights up");
        }
        let ai = r["conversation"]["turns"].as_array().unwrap().last().unwrap().clone();
        assert!(ai["recast"].as_str().unwrap().contains(&format!("number {i}")) || i == 2, "recast of what was said: {ai}");
        last = r;
    }
    assert_eq!(last["used"], json!([targets[0]]));
    let (code, e) = c.post(&format!("/api/conversations/{id}/turns"), json!({"text": "One more thing to say here."})).await;
    assert_eq!(code, StatusCode::CONFLICT, "locked after the last turn: {e}");
    {
        let calls = calls.lock().unwrap();
        let turn_calls: Vec<&Value> = calls.iter().filter(|b| b["output_config"]["format"]["schema"]["properties"]["recast"].is_object()).collect();
        assert_eq!(turn_calls.len(), 6, "opening + 5 turns");
        let final_call = turn_calls.last().unwrap();
        assert_eq!(final_call["messages"].as_array().unwrap().len(), 11, "cue + opening + 4 × (turn, reply) + last turn: the whole transcript");
        assert!(final_call["system"].as_str().unwrap().contains("last turn"));
        assert!(final_call["system"].as_str().unwrap().contains(&targets[0]));
        assert_eq!(final_call["model"], "claude-haiku-5-5", "conversation turns: the real-time model");
    }

    let (code, done) = c.post(&format!("/api/conversations/{id}/finish"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["status"], "done");
    assert_eq!(done["result"]["passed"], true);
    assert_eq!(done["result"]["points"], 1.5, "excellent = 1 + 0.5");
    assert_eq!(done["result"]["review"]["better"][0]["suggestion"], "I really like it.");
    assert_eq!(done["summary"]["today"], 1.5);
    let path = ws_path.join(done["path"].as_str().unwrap());
    assert!(done["path"].as_str().unwrap().starts_with("profiles/ellrin/conversations/en/"));
    let md = std::fs::read_to_string(&path).unwrap();
    assert!(md.contains("↺ So you mean: This is my answer number 1"), "{md}");
    assert!(md.contains("I really like it."));
    assert!(md.contains("jelearner:conversation"), "data embedded for rebuilding");
    let csv = points_csv(&ws_path);
    assert!(csv.starts_with("time,lang,unit,skill,points,score,detail\n"), "{csv}");
    assert!(csv.contains(",en,conversation,writing,1.5,0.87,conversations/en/"), "{csv}");

    // Finishing again changes nothing.
    let (code, again) = c.post(&format!("/api/conversations/{id}/finish"), json!({})).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(again["result"]["points"], 1.5);
    assert_eq!(points_csv(&ws_path).matches(",conversation,").count(), 1);

    // Today: no course yet, so the steps of today's daily lesson (a free
    // conversation is extra practice, not one of its steps).
    let (_, today) = c.get("/api/today?lang=en").await;
    assert!(today["daily"]["topic"]["name"].is_string(), "{today}");
    let kinds: Vec<&str> = today["tasks"].as_array().unwrap().iter().filter_map(|t| t["kind"].as_str()).collect();
    assert_eq!(kinds.first(), Some(&"reading"), "the text first: {today}");
    assert!(kinds.contains(&"conversation") && kinds.contains(&"reading"), "{today}");
    assert_eq!(today["next"], 0);
    assert_eq!(today["summary"]["streak"], 1);

    // Japanese: low-pressure prompt with sentence starters.
    let (code, ja) = c.post("/api/conversations", json!({"lang": "ja"})).await;
    assert_eq!(code, StatusCode::OK, "{ja}");
    assert_eq!(ja["conversation"]["turns"][0]["starters"], json!(["わたしは〜", "〜が好きです"]));
    let (code, _) = c.post(&format!("/api/conversations/{}/turns", ja["id"]), json!({"text": "はい"})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, r) = c.post(&format!("/api/conversations/{}/turns", ja["id"]), json!({"text": "音楽が好きです、特に jazz"})).await;
    assert_eq!(code, StatusCode::OK, "{r}");

    // A fresh database rebuilds the conversation from its file.
    let other = tempfile::tempdir().unwrap();
    let mut s2 = Settings::for_tests(Paths::from_repo(), other.path().to_path_buf());
    s2.workspace = Some(ws_path.clone());
    let c2 = start(s2).await;
    let (_, list) = c2.get("/api/conversations?lang=en").await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["status"], "done");
    let (_, today2) = c2.get("/api/today?lang=en").await;
    assert_eq!(today2["summary"]["total"], 1.5, "points restored from records/points");
}

/// A conversation belongs to the user who started it: after switching to
/// daidai, ellrin's conversation id cannot be read, continued or finished,
/// not even when daidai has a conversation with the same id.
#[tokio::test]
async fn conversation_stays_with_its_user_after_a_switch() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = stage_a_setup(app_dir.path(), &ws_path).await;
    let (code, conv) = c.post("/api/conversations", json!({"lang": "en"})).await;
    assert_eq!(code, StatusCode::OK, "{conv}");
    let id = conv["id"].as_i64().unwrap();
    let (code, r) = c.post(&format!("/api/conversations/{id}/turns?user=ellrin"), json!({"text": "This is my first answer about music."})).await;
    assert_eq!(code, StatusCode::OK, "{r}");

    // Switch: creating daidai makes her the active user.
    let (code, r) = c.post("/api/users", json!({"name": "daidai", "languages": ["ja"], "majors": {"ja": "N5"}})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["user"]["id"], "daidai");
    let turn = json!({"text": "This is my second answer about music."});
    let (code, _) = c.get(&format!("/api/conversations/{id}")).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "not in daidai's database");
    let (code, _) = c.post(&format!("/api/conversations/{id}/turns"), turn.clone()).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = c.post(&format!("/api/conversations/{id}/finish"), json!({})).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = c.post(&format!("/api/conversations/{id}/turns?user=ellrin"), turn.clone()).await;
    assert_eq!(code, StatusCode::CONFLICT, "the page still thinks ellrin is active");

    // daidai's own first conversation gets the same id in her database.
    let (code, dconv) = c.post("/api/conversations", json!({"lang": "ja"})).await;
    assert_eq!(code, StatusCode::OK, "{dconv}");
    assert_eq!(dconv["id"].as_i64().unwrap(), id, "ids are per user");
    let (code, _) = c.post(&format!("/api/conversations/{id}/turns?user=ellrin"), json!({"text": "音楽が好きです、特に jazz"})).await;
    assert_eq!(code, StatusCode::CONFLICT, "ellrin's page cannot write into daidai's conversation");
    let (code, _) = c.post(&format!("/api/conversations/{id}/finish?user=ellrin"), json!({})).await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (_, d) = c.get(&format!("/api/conversations/{id}?user=daidai")).await;
    assert_eq!(d["turns_done"], 0, "daidai's conversation is untouched");

    // Back to ellrin: her conversation goes on where it was.
    let (code, _) = c.post("/api/users/select", json!({"id": "ellrin"})).await;
    assert_eq!(code, StatusCode::OK);
    let (code, r) = c.post(&format!("/api/conversations/{id}/turns?user=ellrin"), turn).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["turns_done"], 2);
}

/// Points come back on a fresh machine from records/points; a level step
/// needs points, balance and recent scores; lessons, reviews and passive
/// reading earn points; early difficulty feedback moves the difficulty.
#[tokio::test]
async fn points_levels_and_difficulty() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = stage_a_setup(app_dir.path(), &ws_path).await;
    // Workspace rules: short rounds for the test.
    std::fs::write(ws_path.join("progress.toml"), "[points]\nreview_round = 3\npassive_minutes = 2\n").unwrap();

    // Records written on another computer, synced in.
    let dir = ws_path.join("profiles/ellrin/records/points");
    std::fs::write(
        dir.join("2026-11-01.csv"),
        "time,lang,unit,skill,points,score,detail\n\
         2026-11-01T09:00:00+08:00,en,conversation,writing,100,0.9,conversations/en/a.md\n\
         2026-11-01T10:00:00+08:00,en,conversation,speaking,100,0.9,conversations/en/b.md\n\
         2026-11-01T11:00:00+08:00,en,quiz,reading,100,0.8,lessons/en/a.md\n\
         2026-11-01T12:00:00+08:00,en,quiz,reading,100,0.8,lessons/en/b.md\n",
    )
    .unwrap();
    let (code, rep) = c.post("/api/workspace/reindex", json!({})).await;
    assert_eq!(code, StatusCode::OK, "{rep}");
    assert_eq!(rep["points"], 4);
    let (_, p) = c.get("/api/progress?lang=en").await;
    assert_eq!(p["summary"]["level"], "B1-1");
    assert_eq!(p["assessment"]["step_points"], 375.0);
    let cond = |p: &Value, id: &str| p["assessment"]["conditions"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()["ok"].clone();
    assert_eq!(cond(&p, "points"), true, "{p}");
    assert_eq!(cond(&p, "balance"), true);
    assert_eq!(cond(&p, "recent"), false, "only four scored units");
    assert_eq!(p["assessment"]["eligible"], false);
    let (_, a) = c.post("/api/progress/assess", json!({"lang": "en"})).await;
    assert!(a["level_up"].is_null());

    // A fifth unit (failed, 0 points, still counts as recent performance).
    std::fs::write(dir.join("2026-11-02.csv"), "time,lang,unit,skill,points,score,detail\n2026-11-02T09:00:00+08:00,en,quiz,reading,0,0.9,lessons/en/c.md\n").unwrap();
    c.post("/api/workspace/reindex", json!({})).await;
    let (_, p) = c.get("/api/progress?lang=en").await;
    assert_eq!(p["assessment"]["eligible"], true, "{p}");
    assert_eq!(p["skills"]["writing"], 100.0);
    let (code, a) = c.post("/api/progress/assess", json!({"lang": "en"})).await;
    assert_eq!(code, StatusCode::OK, "{a}");
    assert_eq!(a["level_up"], json!({"from": "B1-1", "to": "B1-2"}));
    assert_eq!(a["assessment"]["points_since"], 0.0, "points count again from the new step");
    let profile = std::fs::read_to_string(ws_path.join("profiles/ellrin/profile.toml")).unwrap();
    assert!(profile.contains("level = \"B1-2\"") && profile.contains("level_since"), "{profile}");
    let (_, st) = c.get("/api/status").await;
    assert_eq!(st["user"]["levels"]["en"]["level"], "B1-2");
    assert_eq!(st["user"]["levels"]["en"]["difficulty_label"], "B1-1", "the level does not move the content difficulty");

    // A lesson quiz: all right = 1 + bonus, once per lesson.
    let (code, lesson) = c.post("/api/lessons", json!({"lang": "en", "title": "Chips", "text": "Several chipmakers announced new factories."})).await;
    assert_eq!(code, StatusCode::OK, "{lesson}");
    let lid = lesson["id"].as_i64().unwrap();
    let answers = json!({"answers": [{"index": 0, "answer": "New factories"}, {"index": 1, "answer": "Prices keep rising.", "correct": true}]});
    let (code, fin) = c.post(&format!("/api/lessons/{lid}/finish"), answers.clone()).await;
    assert_eq!(code, StatusCode::OK, "{fin}");
    assert_eq!(fin["score"], 1.0);
    assert_eq!(fin["points"], 1.5);
    assert_eq!(fin["ask_difficulty"], true, "the first lessons ask about difficulty");
    let (_, again) = c.post(&format!("/api/lessons/{lid}/finish"), answers).await;
    assert_eq!(again["already"], true);
    assert_eq!(again["points"], 0.0);
    // Early feedback: too easy = +15.
    let (code, fb) = c.post("/api/difficulty/feedback", json!({"lang": "en", "feedback": "too_easy"})).await;
    assert_eq!(code, StatusCode::OK, "{fb}");
    assert_eq!(fb["change"]["from"], 200);
    assert_eq!(fb["change"]["to"], 215);
    assert_eq!(fb["early_feedback_left"], 4);
    // A failed quiz on a second lesson: 0 points, may be redone.
    let (_, l2) = c.post("/api/lessons", json!({"lang": "en", "title": "Chips 2", "text": "More factories."})).await;
    let (_, fail) = c.post(&format!("/api/lessons/{}/finish", l2["id"]), json!({"answers": [{"index": 0, "answer": "Layoffs"}]})).await;
    assert_eq!(fail["passed"], false);
    assert_eq!(fail["points"], 0.0);
    assert!(points_csv(&ws_path).contains(",en,quiz,reading,0,0,lessons/en/"), "failed attempts are recorded with 0 points");

    // Review: a round of 3 cards is a unit.
    let (_, due) = c.get("/api/review?lang=en&limit=3").await;
    let mut last = Value::Null;
    for d in due.as_array().unwrap() {
        let (code, r) = c.post("/api/review", json!({"kind": d["item_kind"], "id": d["item_id"], "grade": 4})).await;
        assert_eq!(code, StatusCode::OK, "{r}");
        last = r;
    }
    assert_eq!(last["award"]["points"], 1.5, "{last}");
    assert_eq!(last["award"]["cards"], 3);

    // Passive reading: 2 minutes (workspace rule) = 1 point; capped per call.
    let (_, r) = c.post("/api/passive", json!({"lang": "en", "kind": "reading", "seconds": 60})).await;
    assert_eq!(r["awarded"], 0);
    let (_, r) = c.post("/api/passive", json!({"lang": "en", "kind": "reading", "seconds": 9999})).await;
    assert_eq!(r["awarded"], 1, "{r}");
    let (code, _) = c.post("/api/passive", json!({"lang": "en", "kind": "manual", "seconds": 60})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "no manual logging");

    let (_, p) = c.get("/api/progress?lang=en").await;
    let total = p["summary"]["total_lang"].as_f64().unwrap();
    assert_eq!(total, 400.0 + 1.5 + 1.5 + 1.0);

    // A fresh machine: same workspace, empty app folder.
    let other = tempfile::tempdir().unwrap();
    let mut s2 = Settings::for_tests(Paths::from_repo(), other.path().to_path_buf());
    s2.workspace = Some(ws_path.clone());
    let c2 = start(s2).await;
    let (_, p2) = c2.get("/api/progress?lang=en").await;
    assert_eq!(p2["summary"]["total_lang"].as_f64().unwrap(), total, "points restored from records/points: {p2}");
    assert_eq!(p2["summary"]["level"], "B1-2");
    assert_eq!(p2["skills"]["speaking"], 100.0);
    assert_eq!(p2["summary"]["difficulty"], "B1-1", "215 is still B1-1");
    let (_, lessons2) = c2.get("/api/lessons?lang=en").await;
    let lid2 = lessons2.as_array().unwrap().iter().find(|l| l["path"] == lesson["path"]).unwrap()["id"].as_i64().unwrap();
    let (_, again) = c2.post(&format!("/api/lessons/{lid2}/finish"), json!({"answers": [{"index": 0, "answer": "New factories"}]})).await;
    assert_eq!(again["already"], true, "a lesson rewarded on another computer is not rewarded twice");

    // Drag and drop: the file type is detected.
    let (code, imp) = c.post_text("/api/import?kind=auto&name=n3.csv&lang=ja", "文型,意味\n〜わけだ,難怪\n").await;
    assert_eq!(code, StatusCode::OK, "{imp}");
    assert_eq!(imp["kind"], "grammar");
    let (code, _) = c.post_text("/api/import?kind=auto&name=x.csv", "a,b\n1,2\n").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

/// Stars within the step and the weeks estimate are in the summary of the
/// today and progress pages and of a finished unit (docs/plan.md §9.1).
#[tokio::test]
async fn milestones_and_weeks_estimate_in_payloads() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _calls) = stage_a_setup(app_dir.path(), &ws_path).await;

    // Nothing yet: no stars, no pace, no estimate.
    let (code, t) = c.get("/api/today?lang=en").await;
    assert_eq!(code, StatusCode::OK, "{t}");
    let m = &t["summary"]["milestones"];
    assert_eq!(m["reached"], 0, "{m}");
    assert_eq!(m["total"], 15);
    assert_eq!(m["milestone_points"], 25.0);
    assert_eq!(m["to_next_milestone"], 25.0);
    assert!(m["weeks_to_next_step"].is_null());

    // 42 points today (written to records/points/, as from another computer).
    let dir = ws_path.join("profiles/ellrin/records/points");
    let rec = jl_core::points::PointRecord::now(jl_core::Lang::En, "conversation", jl_core::points::Skill::Writing, 42.0, Some(0.9), "conversations/en/a.md");
    jl_core::points::append(&dir, &rec).unwrap();
    c.post("/api/workspace/reindex", json!({})).await;
    let (_, p) = c.get("/api/progress?lang=en").await;
    let m = &p["summary"]["milestones"];
    assert_eq!(m["reached"], 1, "{m}");
    assert_eq!(m["to_next_milestone"], 8.0);
    assert_eq!(m["recent_points"], 42.0);
    assert_eq!(m["pace_per_week"], 10.5, "42 points in 28 days");
    assert_eq!(m["weeks_to_next_step"], 32, "333 left at 10.5 a week");
    let (_, t) = c.get("/api/today?lang=en").await;
    assert_eq!(t["summary"]["milestones"], p["summary"]["milestones"]);

    // A finished unit carries the new summary (the result card compares stars).
    let (_, lesson) = c.post("/api/lessons", json!({"lang": "en", "title": "Chips", "text": "Several chipmakers announced new factories."})).await;
    let answers = json!({"answers": [{"index": 0, "answer": "New factories"}, {"index": 1, "answer": "Prices keep rising.", "correct": true}]});
    let (code, fin) = c.post(&format!("/api/lessons/{}/finish", lesson["id"]), answers).await;
    assert_eq!(code, StatusCode::OK, "{fin}");
    assert_eq!(fin["summary"]["milestones"]["points_into_step"], 43.5, "{fin}");
}

/// The user's picture: stored in the app folder (never the workspace),
/// served back, only images, at most 2 MB.
#[tokio::test]
async fn avatar_upload_and_limits() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _) = stage_a_setup(app_dir.path(), &ws_path).await;
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
    jpeg.extend(std::iter::repeat_n(7u8, 5000));
    let up = |body: Vec<u8>| {
        let c = &c;
        async move {
            let r = c.http.post(format!("{}/api/user/avatar", c.base)).body(body).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let (code, r) = up(jpeg.clone()).await;
    assert_eq!(code, 200, "{r}");
    let url = r["avatar"].as_str().unwrap().to_string();
    assert!(url.starts_with("/api/avatars/ellrin?v="));
    let got = c.http.get(format!("{}{url}", c.base)).send().await.unwrap();
    assert_eq!(got.headers()["content-type"], "image/jpeg");
    assert_eq!(got.bytes().await.unwrap().to_vec(), jpeg);
    let (_, s) = c.get("/api/status").await;
    let ws_id = s["user"]["avatar"].as_str().map(|_| ()).and(Some(()));
    assert!(ws_id.is_some(), "status carries the picture");
    assert!(s["users"][0]["avatar"].is_string(), "user cards too");
    // Stored next to the databases, not in the workspace repository.
    let stored: Vec<_> = walk(app_dir.path()).into_iter().filter(|p| p.ends_with("ellrin.jpg")).collect();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].to_string_lossy().contains("/avatars/ws-"));
    assert!(walk(&ws_path).iter().all(|p| !p.to_string_lossy().contains("ellrin.jpg")), "never in the workspace");

    let (code, e) = up(b"not an image at all".to_vec()).await;
    assert_eq!(code, 400, "{e}");
    let mut big = vec![0xFF, 0xD8, 0xFF, 0xE0];
    big.extend(std::iter::repeat_n(1u8, 2 * 1024 * 1024 + 10));
    let (code, e) = up(big).await;
    assert_eq!(code, 400, "over 2 MB: {e}");
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend([0u8; 100]);
    let (code, _) = up(png).await;
    assert_eq!(code, 200);
    let got = c.http.get(format!("{}/api/avatars/ellrin", c.base)).send().await.unwrap();
    assert_eq!(got.headers()["content-type"], "image/png", "a new picture replaces the old one");
    let r = c.http.delete(format!("{}/api/user/avatar", c.base)).send().await.unwrap();
    assert!(r.status().is_success());
    let got = c.http.get(format!("{}/api/avatars/ellrin", c.base)).send().await.unwrap();
    assert_eq!(got.status().as_u16(), 404);
    let got = c.http.get(format!("{}/api/avatars/..%2Fetc", c.base)).send().await.unwrap();
    assert_eq!(got.status().as_u16(), 404);
}

/// Materials kept as files: Markdown / text notes, PDFs and pictures go into
/// the profile's materials/ under a safe free name (never overwriting); the
/// type is checked by extension and content; too big or unknown is refused.
#[tokio::test]
async fn import_material_files() {
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws_path = ws_dir.path().join("JELearner-private");
    let (c, _) = stage_a_setup(app_dir.path(), &ws_path).await;
    let up = |name: &str, body: Vec<u8>| {
        let c = &c;
        let url = format!("{}/api/import/file?name={}", c.base, urlencode(name));
        async move {
            let r = c.http.post(url).body(body).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let (code, r) = up("week notes.md", "# 本週\n\n- fab\n".as_bytes().to_vec()).await;
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["kind"], "text");
    assert_eq!(r["file"], "profiles/ellrin/materials/week-notes.md");
    let (code, r2) = up("week notes.md", b"second".to_vec()).await;
    assert_eq!(code, 200, "{r2}");
    assert_eq!(r2["file"], "profiles/ellrin/materials/week-notes-2.md", "never overwrites");
    assert_eq!(std::fs::read_to_string(ws_path.join("profiles/ellrin/materials/week-notes.md")).unwrap(), "# 本週\n\n- fab\n");
    let (code, r) = up("article.txt", b"A short text.".to_vec()).await;
    assert_eq!((code, r["kind"].as_str()), (200, Some("text")));
    let mut pdf = b"%PDF-1.7\n".to_vec();
    pdf.extend([0u8; 200]);
    let (code, r) = up("../../Scan 01.PDF", pdf).await;
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["kind"], "pdf");
    assert_eq!(r["file"], "profiles/ellrin/materials/scan-01.pdf", "a safe name inside materials/");
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend([0u8; 100]);
    let (code, r) = up("whiteboard.png", png).await;
    assert_eq!((code, r["kind"].as_str()), (200, Some("image")));
    let (code, r) = up("photo.jpg", vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]).await;
    assert_eq!((code, r["kind"].as_str()), (200, Some("image")));
    // Wrong content for the extension, unknown types, binary text, too big.
    let (code, _) = up("fake.pdf", b"not a pdf".to_vec()).await;
    assert_eq!(code, 400);
    let (code, _) = up("run.exe", b"MZ....".to_vec()).await;
    assert_eq!(code, 400);
    let (code, _) = up("notes.txt", vec![0xFF, 0xFE, 0x00, 0x01]).await;
    assert_eq!(code, 400);
    let mut big = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    big.extend(std::iter::repeat_n(0u8, 10 * 1024 * 1024 + 1));
    let (code, _) = up("big.png", big).await;
    assert_eq!(code, 400);
    // Nothing went to review or outside materials/.
    let (_, words) = c.get("/api/words?lang=en&q=fab").await;
    assert_eq!(words.as_array().unwrap().len(), 0);
    let files: Vec<_> = walk(&ws_path.join("profiles/ellrin/materials")).into_iter().filter(|p| p.extension().is_some_and(|e| e != "json")).collect();
    assert_eq!(files.len(), 6, "{files:?}");
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
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

/// The Claude subscription (fake Claude Code CLI): one prompt per call, so
/// the whole transcript is written into the prompt every turn.
#[cfg(unix)]
#[tokio::test]
async fn conversation_through_claude_code_passes_the_transcript() {
    let cli_dir = tempfile::tempdir().unwrap();
    let cli = jl_core::claude_code::fake::write(cli_dir.path());
    let reply = |q: &str| json!({"recast": "So you like jazz.", "question": q, "used_targets": [], "starters": []}).to_string();
    std::fs::write(cli_dir.path().join("reply.json"), reply("What music do you like?")).unwrap();
    let app_dir = tempfile::tempdir().unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let mut s = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    s.claude_bin = Some(cli.clone());
    let c = start(s).await;
    c.post("/api/setup/workspace", json!({"path": ws_dir.path(), "init": true})).await;
    let (code, _) = c.post("/api/users", json!({"name": "Ellrin", "languages": ["en"]})).await;
    assert_eq!(code, StatusCode::OK);
    c.post("/api/claude-code/login", json!({})).await;
    for _ in 0..50 {
        let (_, cl) = c.get("/api/claude?fresh=true").await;
        if cl["subscription"]["ready"] == true {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (code, conv) = c.post("/api/conversations", json!({"lang": "en"})).await;
    assert_eq!(code, StatusCode::OK, "{conv}");
    assert!(conv["conversation"]["engine"].as_str().unwrap().starts_with("claude-subscription:"));
    std::fs::write(cli_dir.path().join("reply.json"), reply("Who is your favourite player?")).unwrap();
    let (code, r) = c.post(&format!("/api/conversations/{}/turns", conv["id"]), json!({"text": "I like jazz a lot, especially live."})).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    let stdin = std::fs::read_to_string(cli_dir.path().join("last-stdin")).unwrap();
    assert!(stdin.contains("<turn role=\"assistant\">\nWhat music do you like?"), "earlier AI turn in the prompt:\n{stdin}");
    assert!(stdin.contains("<turn role=\"user\">\nI like jazz a lot"), "{stdin}");
    let args = std::fs::read_to_string(cli_dir.path().join("last-args")).unwrap();
    assert!(args.contains("recast") && args.contains("--safe-mode"), "turn schema passed: {args}");
}

/// A WAV for the fake `jl-speech`: a 44-byte header, then the "speech" as text.
fn fake_wav(said: &str) -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    b.resize(44, 0);
    b.extend_from_slice(said.as_bytes());
    b
}

/// On-device recognition (decisions §14) through a fake `jl-speech`: status,
/// a recording per language, the model download, and the refusals.
#[cfg(unix)]
#[tokio::test]
async fn speech_recognition_through_the_helper() {
    let app_dir = tempfile::tempdir().unwrap();
    // Default test settings: no engine (the helper path does not exist).
    let c = start(Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf())).await;
    let (st, s) = c.get("/api/speech").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(s["engine"], Value::Null, "{s}");

    let fake_dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    settings.speech_bin = Some(jl_core::speech::fake::write(fake_dir.path()));
    let c = start(settings).await;
    let (_, s) = c.get("/api/speech").await;
    assert_eq!(s["engine"], "macos");
    assert_eq!(s["langs"][0], json!({"lang": "en", "locale": "en-US", "supported": true, "installed": true}));
    assert_eq!(s["langs"][1]["installed"], false);

    let send = |lang: &str, body: Vec<u8>| {
        let url = format!("{}/api/speech/transcribe?lang={lang}", c.base);
        let http = c.http.clone();
        async move {
            let r = http.post(url).header("content-type", "audio/wav").body(body).send().await.unwrap();
            (StatusCode::from_u16(r.status().as_u16()).unwrap(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let (st, t) = send("en", fake_wav("I am planning to buy three books")).await;
    assert_eq!(st, StatusCode::OK, "{t}");
    assert_eq!(t["text"], "I am planning to buy three books");
    assert_eq!(t["confidence"], 0.9);
    assert!(std::fs::read_to_string(fake_dir.path().join("last-args")).unwrap().starts_with("transcribe en-US "));

    // Japanese model missing → 409, the page offers the download.
    let (st, e) = send("ja", fake_wav("こんにちは")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    let (st, s) = c.post("/api/speech/install", json!({"lang": "ja"})).await;
    assert_eq!(st, StatusCode::OK, "{s}");
    assert_eq!(s["langs"][1]["installed"], true);
    let (st, t) = send("ja", fake_wav("こんにちは")).await;
    assert_eq!((st, t["text"].as_str()), (StatusCode::OK, Some("こんにちは")));

    // Refusals: no language, not a WAV, empty, too large.
    assert_eq!(send("", fake_wav("x")).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(send("en", b"not a wav at all".to_vec()).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(send("en", vec![]).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(send("en", vec![0; jl_server::speech::AUDIO_MAX_BYTES + 1]).await.0, StatusCode::PAYLOAD_TOO_LARGE);
}

/// Local models from the shared folder (decisions §15): which brains are
/// downloaded, choosing one, and the messages when something is missing.
/// Starting a real llama-server is covered by jl-core's ignored test.
#[tokio::test]
async fn local_models_from_the_shared_folder() {
    let app_dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut st = Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf());
    st.llama_external = false;
    let c = start(st.clone()).await;
    // No shared folder configured.
    let (code, e) = c.post("/api/settings/llama", json!({"model": "qwen3.5-4b-q8"})).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert!(e["error"].as_str().unwrap().contains("LOCAL_LLM_HOME"), "{e}");

    // A folder with Qwen 4B downloaded but no llama.cpp.
    let repo = home.path().join("gguf/unsloth/Qwen3.5-4B-GGUF");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("Qwen3.5-4B-Q8_0.gguf"), b"x").unwrap();
    st.llama_home = Some(home.path().to_path_buf());
    let c = start(st).await;
    let (_, m) = c.get("/api/models").await;
    let brain = |id: &str| m["brains"].as_array().unwrap().iter().find(|b| b["id"] == id).cloned().unwrap();
    assert_eq!(brain("qwen3.5-4b-q8")["installed"], true);
    assert_eq!(brain("qwen3.8-27b-q4")["installed"], false);
    assert_eq!(m["runtime"], false);
    assert_eq!(brain(m["catalog"]["defaults"]["brain"].as_str().unwrap())["chosen"], true);

    let (code, e) = c.post("/api/settings/llama", json!({"model": "no-such-model"})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    let (code, e) = c.post("/api/settings/llama", json!({"model": "apple-speech"})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "only brains run in llama-server: {e}");
    let (code, e) = c.post("/api/settings/llama", json!({"model": "qwen3.5-4b-q8"})).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{e}");
    assert!(e["error"].as_str().unwrap().contains("llama.cpp"), "{e}");
    // The choice is remembered for this computer even though it could not start yet.
    let cfg = std::fs::read_to_string(app_dir.path().join("config.toml")).unwrap();
    assert!(cfg.contains("model = \"qwen3.5-4b-q8\"") && cfg.contains("kind = \"llama-server\""), "{cfg}");
    let (_, m) = c.get("/api/models").await;
    assert_eq!(m["brains"].as_array().unwrap().iter().find(|b| b["chosen"] == true).unwrap()["id"], "qwen3.5-4b-q8");
}

/// Run git for test fixtures with an author set, so no global config is needed.
fn git_fixture(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// The setup wizard's checks and guards (plan §19.2.3) against local bare
/// repos standing in for GitHub: nested folders refused, an existing
/// workspace downloaded, a non-empty repo flagged for "new", a wrong URL.
#[tokio::test]
async fn setup_wizard_checks_and_guards() {
    let app_dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let c = start(Settings::for_tests(Paths::from_repo(), app_dir.path().to_path_buf())).await;
    let check = |plan: &str, url: &str, path: &Path| {
        let body = json!({"plan": plan, "url": url, "path": path});
        let c = &c;
        async move { c.post("/api/setup/check", body).await }
    };
    let by_id = |r: &Value, id: &str| r["checks"].as_array().unwrap().iter().find(|x| x["id"] == id).cloned().unwrap_or(Value::Null);

    // An existing workspace in a bare "remote".
    let seed = root.path().join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("workspace.toml"), "id = \"ws-test\"\nname = \"Test\"\nadmins = []\n").unwrap();
    git_fixture(&seed, &["init", "-q"]);
    git_fixture(&seed, &["add", "-A"]);
    git_fixture(&seed, &["commit", "-q", "-m", "seed"]);
    let full = root.path().join("full.git");
    git_fixture(root.path(), &["clone", "-q", "--bare", seed.to_str().unwrap(), full.to_str().unwrap()]);
    let empty = root.path().join("empty.git");
    git_fixture(root.path(), &["init", "-q", "--bare", empty.to_str().unwrap()]);
    let (full_url, empty_url) = (full.to_str().unwrap().to_string(), empty.to_str().unwrap().to_string());

    // Inside another repo (the author's nested clone): the check stops there, and so do the actions.
    let nested = seed.join("JELearner-private");
    let (st, r) = check("local", "", &nested).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["ready"], false);
    assert!(by_id(&r, "location")["msg"].as_str().unwrap().contains("另一個 Git repository"), "{r}");
    let (st, e) = c.post("/api/setup/new", json!({"path": nested})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    assert!(!nested.exists(), "nothing was created");
    let (st, _) = c.post("/api/setup/clone", json!({"url": full_url, "path": nested})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Used it before: the full repo downloads into the chosen folder.
    let target = root.path().join("mine");
    let (_, r) = check("clone", &full_url, &target).await;
    assert_eq!(by_id(&r, "reach")["ok"], true, "{r}");
    assert_eq!(by_id(&r, "location")["ok"], true, "{r}");
    assert_eq!(by_id(&r, "private")["ok"], true, "not GitHub: the user is asked to check: {r}");
    assert!(by_id(&r, "empty").is_null(), "a repo with content is what 'clone' wants: {r}");
    let (st, r) = c.post("/api/setup/clone", json!({"url": full_url, "path": target})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(target.join("workspace.toml").exists());
    // The same folder again: it is no longer free.
    let (_, r) = check("clone", &full_url, &target).await;
    assert_eq!(by_id(&r, "location")["ok"], false, "{r}");

    // New and synced: an empty repo is right, one with content is not.
    let fresh = root.path().join("fresh");
    let (_, r) = check("create", &empty_url, &fresh).await;
    assert_eq!(by_id(&r, "empty")["ok"], true, "{r}");
    let (_, r) = check("create", &full_url, &fresh).await;
    assert_eq!(by_id(&r, "empty")["ok"], false, "{r}");
    // "Used it before" with an empty repo points back to "first time".
    let (_, r) = check("clone", &empty_url, &root.path().join("x")).await;
    assert_eq!(by_id(&r, "empty")["ok"], false, "{r}");

    // A URL that does not answer.
    let (_, r) = check("clone", &root.path().join("nope.git").display().to_string(), &root.path().join("y")).await;
    assert_eq!(by_id(&r, "reach")["ok"], false, "{r}");
    assert_eq!(r["ready"], false);
    let (st, _) = c.post("/api/setup/check", json!({"plan": "nonsense"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Family mode (plan §19.2.4–19.2.5) with two "computers" sharing a bare
/// repo: the admin's workspace, then the assisted setup on the member's
/// computer (clone, the member, PIN), then the member's automatic sync that
/// commits only `profiles/<member>/` and replays it on top of the admin's work.
#[tokio::test]
async fn family_mode_two_computers() {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("family.git");
    git_fixture(root.path(), &["init", "-q", "--bare", bare.to_str().unwrap()]);
    let url = bare.to_str().unwrap().to_string();

    // The admin's computer: a workspace with the admin, pushed.
    let (app_a, ws_a) = (root.path().join("app-a"), root.path().join("ws-a"));
    let a = start(Settings::for_tests(Paths::from_repo(), app_a.clone())).await;
    let (st, r) = a.post("/api/setup/workspace", json!({"path": ws_a, "init": true})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, r) = a.post("/api/users", json!({"name": "Ellrin", "languages": ["en"], "majors": {"en": "B2"}})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    git_fixture(&ws_a, &["init", "-q"]);
    git_fixture(&ws_a, &["add", "-A"]);
    git_fixture(&ws_a, &["commit", "-q", "-m", "admin workspace"]);
    git_fixture(&ws_a, &["remote", "add", "origin", &url]);
    git_fixture(&ws_a, &["push", "-q", "-u", "origin", "main"]);

    // The member's computer, set up by the admin.
    let (app_b, ws_b) = (root.path().join("app-b"), root.path().join("ws-b"));
    let b = start(Settings::for_tests(Paths::from_repo(), app_b.clone())).await;
    let (_, k) = b.post("/api/family/key", json!({"url": url})).await;
    assert_eq!(k["public_key"], Value::Null, "not GitHub: no deploy key: {k}");
    let (_, t) = b.post("/api/family/test", json!({"url": url})).await;
    assert_eq!((t["ok"].as_bool(), t["empty"].as_bool()), (Some(true), Some(false)), "{t}");
    let (st, r) = b.post("/api/family/clone", json!({"url": url, "path": ws_b})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, r) = b.post("/api/users", json!({"name": "daidai", "languages": ["ja"], "majors": {"ja": "N5"}})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, e) = b.post("/api/family/finish", json!({"member": "daidai", "pin": "12"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "PIN too short: {e}");
    let (st, e) = b.post("/api/family/finish", json!({"member": "nobody", "pin": "2468"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    let (st, r) = b.post("/api/family/finish", json!({"member": "daidai", "pin": "2468"})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["synced"], true, "the new member went up while the admin was there: {r}");
    let (_, s) = b.get("/api/status").await;
    assert_eq!(s["family"], json!({"member": "daidai", "ready": true}));
    let cfg = std::fs::read_to_string(app_b.join("config.toml")).unwrap();
    assert!(cfg.contains("[family]") && cfg.contains("pin_hash") && !cfg.contains("2468"), "only the hash is kept: {cfg}");
    assert!(git_out(&bare, &["log", "--oneline", "-1"]).contains("daidai"), "the member is in the shared repo");
    assert_eq!(git_out(&bare, &["log", "-1", "--format=%an"]), "daidai", "authored as the member, no global Git author needed");

    // PIN
    assert_eq!(b.post("/api/family/unlock", json!({"pin": "0000"})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(b.post("/api/family/unlock", json!({"pin": "2468"})).await.0, StatusCode::OK);

    // The member's sync commits only profiles/daidai/; a shared file edit stays local.
    std::fs::write(ws_b.join("profiles/daidai/note.txt"), "my note\n").unwrap();
    std::fs::write(ws_b.join("topics.toml"), "# changed on the member's computer\n").unwrap();
    let (_, r) = b.post("/api/family/sync", json!({})).await;
    assert_eq!((r["ok"].as_bool(), r["pushed"].as_bool()), (Some(true), Some(true)), "{r}");
    let files = git_out(&bare, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.lines().all(|f| f.starts_with("profiles/daidai/")), "only the member's folder: {files}");
    assert!(git_out(&ws_b, &["status", "--porcelain"]).contains("topics.toml"), "shared edit not committed");

    // Both sides moved: the admin pushes, the member changes again; the sync replays on top.
    git_fixture(&ws_a, &["pull", "-q", "--rebase"]);
    std::fs::write(ws_a.join("profiles/ellrin/admin.txt"), "from the admin\n").unwrap();
    git_fixture(&ws_a, &["add", "-A"]);
    git_fixture(&ws_a, &["commit", "-q", "-m", "admin work"]);
    git_fixture(&ws_a, &["push", "-q"]);
    std::fs::write(ws_b.join("profiles/daidai/note.txt"), "my note, again\n").unwrap();
    let (_, r) = b.post("/api/family/sync", json!({})).await;
    assert_eq!((r["ok"].as_bool(), r["pulled"].as_bool(), r["pushed"].as_bool()), (Some(true), Some(true), Some(true)), "{r}");
    assert!(ws_b.join("profiles/ellrin/admin.txt").exists(), "the admin's work arrived");
    assert!(ws_b.join("topics.toml").exists() && std::fs::read_to_string(ws_b.join("topics.toml")).unwrap().contains("member's computer"), "the local shared edit was put back, not lost");
    let (_, r) = b.post("/api/family/sync", json!({})).await;
    assert_eq!((r["ok"].as_bool(), r["pushed"].as_bool()), (Some(true), Some(false)), "nothing new: {r}");

    // The remote is gone (unplugged, deleted…): a quiet failure, with a reason.
    git_fixture(&ws_b, &["remote", "set-url", "origin", root.path().join("gone.git").to_str().unwrap()]);
    let (st, r) = b.post("/api/family/sync", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["reason"].is_string() && r["detail"].is_string(), "{r}");

    // The admin turns family mode off (PIN first).
    assert_eq!(b.post("/api/family/leave", json!({"pin": "1111"})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(b.post("/api/family/leave", json!({"pin": "2468"})).await.0, StatusCode::OK);
    let (_, s) = b.get("/api/status").await;
    assert_eq!(s["family"], Value::Null);
    assert!(!std::fs::read_to_string(app_b.join("config.toml")).unwrap().contains("[family]"));
    // A computer that is not in family mode has no family sync.
    assert_eq!(b.post("/api/family/sync", json!({})).await.0, StatusCode::BAD_REQUEST);
}
