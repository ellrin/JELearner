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
                assert_eq!(h["anthropic-beta"], "server-side-fallback-2026-07-01");
                assert!(b.get("temperature").is_none());
                calls.lock().unwrap().push(b.clone());
                let text = if b["output_config"]["format"]["type"] == "json_schema" { lesson_json().to_string() } else { "OK".into() };
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
        assert_eq!(req["model"], "claude-opus-5-5", "model from the catalog, not hard-coded");
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
    assert_eq!(l["engine"], "claude-subscription:claude-opus-5-5");
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
