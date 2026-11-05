//! Offline stand-ins for the outside world, for trying the UI without an API
//! key or network:
//!
//! ```sh
//! cargo run -p jl-server --example mock_services        # prints the env vars
//! ```
//!
//! - a Claude Messages API that answers with a canned lesson (English or
//!   Japanese, depending on the article), conversation turns (a recast of the
//!   learner's message and the next question) and the end evaluation — told
//!   apart by the JSON schema in the request;
//! - a news site with robots.txt (`/private/` disallowed), an English and a
//!   Japanese RSS feed and article pages.

use axum::extract::Path;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

const PORT: u16 = 7980;

fn base() -> String {
    format!("http://127.0.0.1:{PORT}")
}

fn english_lesson() -> Value {
    json!({
        "title": "Chipmakers race to build AI accelerators",
        "summary": "多家晶片製造商宣布在亞洲與美國興建新工廠，生產 AI 加速器；分析師預期未來十年需求持續成長，但電力與人才不足可能拖慢進度。",
        "vocabulary": [
            {"term":"accelerator","reading":"/əkˈseləreɪtər/","part_of_speech":"n.","meaning":"加速器（專用運算晶片）","example":"The startup designs low-power accelerators for phones.","example_translation":"這家新創公司為手機設計低功耗加速器。"},
            {"term":"capacity","reading":"/kəˈpæsəti/","part_of_speech":"n.","meaning":"產能；容量","example":"The plant will double its capacity next year.","example_translation":"這座工廠明年將把產能加倍。"},
            {"term":"bottleneck","reading":"/ˈbɑːtlnek/","part_of_speech":"n.","meaning":"瓶頸","example":"Power supply has become the main bottleneck.","example_translation":"電力供應已成為主要瓶頸。"},
            {"term":"ramp up","reading":"/ræmp ʌp/","part_of_speech":"phr. v.","meaning":"逐步提高（產量等）","example":"They plan to ramp up production by spring.","example_translation":"他們計畫在春天前逐步提高產量。"}
        ],
        "grammar": [
            {"pattern":"keep + V-ing","meaning":"持續…","explanation":"keep 後接動名詞，表示動作或狀態一直持續，常用於趨勢描述。","example":"Demand for chips keeps rising.","example_translation":"晶片需求持續上升。"},
            {"pattern":"be expected to + V","meaning":"預期會…","explanation":"新聞常用的被動句型，避免指明是誰預期。","example":"The factory is expected to open in 2028.","example_translation":"這座工廠預計於 2028 年啟用。"}
        ],
        "questions": [
            {"kind":"choice","question":"What is the main bottleneck mentioned in the article?","options":["Power supply","Software","Shipping"],"answer":"Power supply","explanation":"文章最後一段提到電力不足。"},
            {"kind":"choice","question":"「ramp up」最接近的意思是？","options":["逐步提高","突然停止","重新設計"],"answer":"逐步提高","explanation":"ramp 是斜坡，ramp up 指逐步增加。"},
            {"kind":"short","question":"Use \"be expected to\" to describe something about your work.","options":[],"answer":"The new system is expected to launch next month.","explanation":"be expected to 後接原形動詞。"}
        ]
    })
}

fn japanese_lesson() -> Value {
    json!({
        "title": "政府、半導体工場の新設を支援へ",
        "summary": "日本政府宣布補助國內新的半導體工廠，希望確保供應鏈穩定；但地方上也出現對用水與人才不足的擔憂。",
        "vocabulary": [
            {"term":"支援","reading":"しえん","part_of_speech":"名詞・サ変","meaning":"支援；援助","example":"被災地を支援するボランティアが集まった。","example_translation":"支援災區的志工聚集了起來。"},
            {"term":"供給網","reading":"きょうきゅうもう","part_of_speech":"名詞","meaning":"供應鏈","example":"供給網の見直しが進んでいる。","example_translation":"正在重新檢討供應鏈。"},
            {"term":"懸念","reading":"けねん","part_of_speech":"名詞・サ変","meaning":"擔憂；疑慮","example":"水不足への懸念が広がっている。","example_translation":"對缺水的擔憂正在擴大。"},
            {"term":"確保","reading":"かくほ","part_of_speech":"名詞・サ変","meaning":"確保","example":"人材の確保が課題だ。","example_translation":"確保人才是一大課題。"}
        ],
        "grammar": [
            {"pattern":"〜に伴って","meaning":"隨著…","explanation":"前接名詞或「動詞辭書形＋の」，表示一方變化時另一方也跟著變化，常見於新聞。","example":"工場の建設に伴って、周辺の道路も整備される。","example_translation":"隨著工廠興建，周邊道路也會整修。"},
            {"pattern":"〜を巡って","meaning":"圍繞著…（爭議）","explanation":"前接名詞，表示議論或對立的焦點。","example":"補助金を巡って議論が続いている。","example_translation":"圍繞補助金的爭論持續中。"}
        ],
        "questions": [
            {"kind":"choice","question":"「懸念」の読み方は？","options":["けねん","けんねん","かんねん"],"answer":"けねん","explanation":"「懸」在此讀「け」。"},
            {"kind":"choice","question":"政府は何を支援しますか。","options":["半導体工場の新設","農業の機械化","観光客の誘致"],"answer":"半導体工場の新設","explanation":"第一段。"},
            {"kind":"short","question":"「〜に伴って」を使って文を作ってください。","options":[],"answer":"技術の進歩に伴って、生活が便利になった。","explanation":"名詞＋に伴って。"}
        ]
    })
}

const EN_QUESTIONS: &[&str] = &[
    "Hi! I'm Sam, an exchange student. What do you usually do after class?",
    "That sounds fun. How did you get into it?",
    "What do you like most about it?",
    "Is it popular among your friends too?",
    "If you could change one thing about it, what would it be?",
    "What would you recommend to someone who wants to try it?",
];
const JA_QUESTIONS: &[&str] = &["こんにちは！ゆきです。しゅみはなんですか。", "いいですね！いつもどこでしますか。", "だれとしますか。", "どうしてすきですか。"];

/// One AI turn: a recast of the learner's last message and the next question.
fn turn(b: &Value) -> Value {
    let msgs = b["messages"].as_array().cloned().unwrap_or_default();
    let ja = b["system"].as_str().unwrap_or("").contains("Japanese student");
    let last = msgs.last().and_then(|m| m["content"].as_str()).unwrap_or("").trim().to_string();
    let n = msgs.len() / 2;
    let is_last = b["system"].as_str().unwrap_or("").contains("This was the learner's last turn");
    let recast = if msgs.len() <= 1 {
        String::new()
    } else if ja {
        format!("「{}」ですね。", last.trim_end_matches(['。', '.']))
    } else {
        let l = last.trim_end_matches('.');
        format!("So, if I understand you, {}{}.", l.chars().next().map(|c| c.to_lowercase().to_string()).unwrap_or_default(), l.chars().skip(1).collect::<String>())
    };
    let question = if is_last {
        if ja { "たのしかったです。またはなしましょう！".to_string() } else { "It was really nice talking with you — good luck with everything!".to_string() }
    } else if ja {
        JA_QUESTIONS[n % JA_QUESTIONS.len()].to_string()
    } else {
        EN_QUESTIONS[n % EN_QUESTIONS.len()].to_string()
    };
    let starters = if ja && !is_last { json!(["わたしは〜", "〜がすきです。なぜなら…", "よく〜します"]) } else { json!([]) };
    json!({ "recast": recast, "question": question, "used_targets": [], "starters": starters })
}

fn review(ja: bool) -> Value {
    if ja {
        json!({
            "fluency": 3, "target_use": 3, "grammar": 4, "overall": "good",
            "summary": "每一回合都有用日文回答，很棒！",
            "better": [{"original": "音楽すき", "suggestion": "音楽が好きです。", "why": "加上助詞「が」和「です」更自然"}],
            "suggested_words": [{"term": "趣味", "reading": "しゅみ", "meaning": "興趣"}],
            "keywords": ["音楽", "趣味"]
        })
    } else {
        json!({
            "fluency": 4, "target_use": 4, "grammar": 5, "overall": "excellent",
            "summary": "你用完整的句子回答，而且自然地用到了目標字。",
            "better": [
                {"original": "I very like it", "suggestion": "I really like it.", "why": "very 不修飾動詞"},
                {"original": "It make me relax", "suggestion": "It helps me relax.", "why": "主詞第三人稱單數"}
            ],
            "suggested_words": [
                {"term": "unwind", "reading": "/ʌnˈwaɪnd/", "meaning": "放鬆"},
                {"term": "get into", "reading": "/ɡet ˈɪntuː/", "meaning": "開始喜歡上"}
            ],
            "keywords": ["music", "club", "weekend"]
        })
    }
}

async fn messages(Json(b): Json<Value>) -> Json<Value> {
    let user = b["messages"][0]["content"].as_str().unwrap_or("");
    let ja = user.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    let props = &b["output_config"]["format"]["schema"]["properties"];
    let (text, wait) = if props["recast"].is_object() {
        (turn(&b).to_string(), 500)
    } else if props["fluency"].is_object() {
        let ja = b["system"].as_str().unwrap_or("").contains("in Japanese");
        (review(ja).to_string(), 900)
    } else if b["output_config"]["format"].is_object() {
        (if ja { japanese_lesson() } else { english_lesson() }.to_string(), 800)
    } else {
        ("OK".to_string(), 200)
    };
    // Simulate thinking time.
    tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
    Json(json!({
        "id": "msg_mock", "type": "message", "role": "assistant", "model": b["model"],
        "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": 0, "output_tokens": 0}
    }))
}

fn rss(items: &[(&str, &str, &str)]) -> String {
    let body: String = items
        .iter()
        .map(|(t, path, d)| format!("<item><title>{t}</title><link>{}{path}</link><description>{d}</description><pubDate>Mon, 02 Nov 2026 08:00:00 GMT</pubDate></item>", base()))
        .collect();
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0"><channel><title>Mock</title>{body}</channel></rss>"#)
}

async fn article(Path(name): Path<String>) -> Html<String> {
    let (title, paras): (&str, &[&str]) = match name.as_str() {
        "chips.html" => (
            "Chipmakers race to build AI accelerators",
            &[
                "Several chipmakers announced plans on Monday to build new factories for AI accelerators in Asia and the United States.",
                "Analysts say demand for specialised chips will keep rising over the next decade as companies ramp up their data centres.",
                "However, power supply has become the main bottleneck, and some projects are expected to be delayed.",
            ],
        ),
        "robots.html" => (
            "Robots learn to fold laundry",
            &["Researchers unveiled a robot that learned to fold laundry from thousands of videos.", "The team says the method could help in hospitals and hotels."],
        ),
        "hanndoutai.html" => (
            "政府、半導体工場の新設を支援へ",
            &["政府は国内に新しい半導体工場を建設する企業を支援する方針を発表した。", "供給網の安定を確保する狙いがあるが、地元では水不足や人材確保への懸念も出ている。"],
        ),
        _ => ("Not found", &[]),
    };
    let ps: String = paras.iter().map(|p| format!("<p>{p}</p>")).collect();
    Html(format!(r#"<html><head><meta property="og:title" content="{title}"></head><body><nav><p>Home</p></nav><article><h1>{title}</h1>{ps}</article></body></html>"#))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/v1/messages", post(messages))
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /private/\n" }))
        .route(
            "/feeds/en.xml",
            get(|| async {
                rss(&[
                    ("Chipmakers race to build AI accelerators", "/articles/chips.html", "New factories are planned in Asia and the US."),
                    ("Robots learn to fold laundry", "/articles/robots.html", "A robot learned from videos."),
                    ("Subscribers only: market outlook", "/private/outlook.html", "Blocked by robots.txt."),
                ])
            }),
        )
        .route("/feeds/ja.xml", get(|| async { rss(&[("政府、半導体工場の新設を支援へ", "/articles/hanndoutai.html", "供給網の安定を確保する狙い。")]) }))
        .route("/articles/{name}", get(article));
    let l = tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await.expect("port 7980 free");
    println!("Mock services on {}", base());
    println!("Run the app against them:");
    println!("  JELEARNER_ENGINE=api JELEARNER_CLAUDE_BASE_URL={} ANTHROPIC_API_KEY=sk-ant-mock JELEARNER_NO_KEYCHAIN=1 cargo run -p jl-server", base());
    println!("and put these feeds in your workspace's sources.toml:");
    println!("  {}/feeds/en.xml   {}/feeds/ja.xml", base(), base());
    axum::serve(l, app).await.unwrap();
}
