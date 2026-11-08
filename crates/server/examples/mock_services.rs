//! Offline stand-ins for the outside world, for trying the UI without an API
//! key or network:
//!
//! ```sh
//! cargo run -p jl-server --example mock_services        # prints the env vars
//! ```
//!
//! - a Claude Messages API that answers with a canned lesson (English or
//!   Japanese, depending on the article), conversation turns (a recast of the
//!   learner's message and the next question), the end evaluation, course
//!   outlines (topic and fixed) and course lessons that use the planned
//!   words — told apart by the JSON schema in the request;
//! - a news site with robots.txt (`/private/` disallowed), an English and a
//!   Japanese RSS feed and article pages.

use axum::extract::Path;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

/// 7980, or `JELEARNER_MOCK_PORT` when that one is taken (e.g. a second copy).
fn port() -> u16 {
    std::env::var("JELEARNER_MOCK_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7980)
}

fn base() -> String {
    format!("http://127.0.0.1:{}", port())
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

// ── courses (docs/courses.md) ──────────────────────────

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
const JA_POOL: &[(&str, &str, &str)] = &[
    ("天気", "てんき", "天氣"),
    ("電車", "でんしゃ", "電車"),
    ("駅", "えき", "車站"),
    ("友達", "ともだち", "朋友"),
    ("映画", "えいが", "電影"),
    ("料理", "りょうり", "料理"),
    ("学校", "がっこう", "學校"),
    ("音楽", "おんがく", "音樂"),
    ("週末", "しゅうまつ", "週末"),
    ("図書館", "としょかん", "圖書館"),
];

const JA_QS: [&str; 5] = ["{今日|きょう}の{天気|てんき}は？", "なにについて{話|はな}しましたか。", "どうでしたか。", "だれと{話|はな}しましたか。", "どこですか。"];
const EN_QS: [&str; 5] = ["What did the chipmakers announce?", "What is the main bottleneck?", "Who explained why it matters?", "What may be slow?", "What will shape the next decade?"];

/// A topic outline (5 lessons, new words in lessons 1–3 are kept by the app).
fn course_outline(user: &str, ja: bool) -> Value {
    let kw = user.lines().find_map(|l| l.strip_prefix("Keywords: ")).unwrap_or(if ja { "日本の生活" } else { "AI chips" }).to_string();
    let pool = if ja { JA_POOL } else { EN_POOL };
    let angles = ["background", "technical", "social", "personal life", "review"];
    let lessons: Vec<Value> = (0..5)
        .map(|i| {
            let w = |k: usize| { let t = pool[k % pool.len()]; json!({ "term": t.0, "reading": t.1, "meaning": t.2 }) };
            json!({
                "title": if ja { format!("{kw}（{}）", i + 1) } else { format!("{kw}: {}", ["What it is", "Inside the fab", "Chips and jobs", "My phone's chip", "Looking back"][i]) },
                "angle": angles[i], "purpose": "認識主題的核心單字", "search": [kw.clone()],
                "conversation_topic": if ja { "週末になにをしますか".to_string() } else { format!("What do you think about {kw}?") },
                "words": [w(3 * i), w(3 * i + 1), w(3 * i + 2)],
                "grammar": [if ja { json!({ "pattern": "〜たいです", "meaning": "想要…" }) } else { json!({ "pattern": "so ... that ...", "meaning": "如此…以致於" }) }]
            })
        })
        .collect();
    json!({ "title": if ja { kw.clone() } else { format!("{kw} This Week") }, "summary": if ja { "用簡單的日文認識主題" } else { "一週認識這個主題" }, "keywords": [kw], "enough_material": true, "lessons": lessons })
}

fn fixed_titles() -> Value {
    json!({ "lessons": (1..=80).map(|n| json!({ "number": n, "title": format!("Workday {n}"), "angle": "work", "purpose": "職場常用字", "conversation_topic": "Your workplace" })).collect::<Vec<_>>() })
}

/// A course lesson that uses every planned word (parsed from the prompt);
/// Japanese texts mark readings as {漢字|かんじ}.
fn course_lesson(user: &str, ja: bool) -> Value {
    let mut words: Vec<(String, String, String)> = vec![];
    let mut review: Vec<String> = vec![];
    let mut outputs: Vec<(String, usize)> = vec![];
    let mut questions = 3;
    let mut in_words = false;
    for line in user.lines() {
        if line.starts_with("New words:") { in_words = true; continue; }
        if in_words {
            match line.strip_prefix("- ") {
                Some("(none)") => {}
                Some(w) => {
                    let (head, meaning) = w.split_once(": ").unwrap_or((w, ""));
                    let (term, reading) = match head.split_once(" [") { Some((t, r)) => (t.to_string(), r.trim_end_matches(']').to_string()), None => (head.to_string(), String::new()) };
                    words.push((term, reading, meaning.to_string()));
                    continue;
                }
                None => in_words = false,
            }
        }
        if let Some(r) = line.strip_prefix("Review words (use them in the text too): ") { review = r.split(", ").map(String::from).collect(); }
        if let Some(q) = line.strip_prefix("- Reading questions: ") { questions = q.trim().parse().unwrap_or(3).clamp(1, 5); }
        for kind in ["translation", "sentence-making", "cloze", "paraphrase"] {
            if let Some(n) = line.strip_prefix(&format!("- {kind}: ")) { outputs.push((kind.to_string(), n.split(' ').next().unwrap_or("2").parse().unwrap_or(2))); }
        }
    }
    if words.is_empty() && review.is_empty() {
        let pool = if ja { JA_POOL } else { EN_POOL };
        words = pool.iter().take(4).map(|t| (t.0.to_string(), t.1.to_string(), t.2.to_string())).collect();
    }
    let reading_of = |t: &str| words.iter().find(|w| w.0 == t).map(|w| w.1.clone()).or_else(|| JA_POOL.iter().find(|p| p.0 == t).map(|p| p.1.to_string())).unwrap_or_default();
    let mark = |t: &str| if ja && !reading_of(t).is_empty() { format!("{{{t}|{}}}", reading_of(t)) } else { t.to_string() };
    let all: Vec<String> = words.iter().map(|w| w.0.clone()).chain(review.iter().cloned()).collect();
    let text = if ja {
        let mut t = String::from("{今日|きょう}は{晴|は}れです。");
        for w in &all { t.push_str(&format!("{}について{{話|はな}}しました。", mark(w))); }
        t.push_str("とても{楽|たの}しかったです。");
        t
    } else {
        let mut t = String::from("Several chipmakers announced new factories this week. ");
        for (i, w) in all.iter().enumerate() {
            t.push_str(&[format!("Analysts say the {w} question will shape the next decade. "), format!("Engineers explained why {w} matters for cheaper phones. "), format!("A government report linked {w} to new jobs in the region. ")][i % 3]);
        }
        t.push_str("Power supply is still the main bottleneck, so progress may be slow.");
        t
    };
    let vocabulary: Vec<Value> = words.iter().map(|(t, r, m)| json!({
        "term": t, "reading": r, "part_of_speech": if ja { "名詞" } else { "n." }, "meaning": m,
        "example": if ja { format!("{}はとても{{大切|たいせつ}}です。", mark(t)) } else { format!("The {t} is growing fast.") },
        "example_translation": if ja { format!("{m}非常重要。") } else { format!("{m}正快速成長。") }
    })).collect();
    let qs: Vec<Value> = (0..questions).map(|i| if ja {
        json!({ "kind": "choice", "question": JA_QS[i % 5], "options": ["{晴|は}れ", "{雨|あめ}", "{雪|ゆき}"], "answer": "{晴|は}れ", "explanation": "課文第一句。" })
    } else {
        json!({ "kind": "choice", "question": EN_QS[i % 5], "options": ["New factories", "Layoffs", "A merger"], "answer": "New factories", "explanation": "第一句提到新工廠。" })
    }).collect();
    let mut output: Vec<Value> = vec![];
    for (k, n) in &outputs { for i in 0..*n {
        let w = all.get(i % all.len().max(1)).cloned().unwrap_or_default();
        output.push(
        match (k.as_str(), ja) {
            ("translation", false) => json!({ "kind": k, "prompt": format!("政府提供{}給新工廠。", ["補助", "協助", "資金"][i % 3]), "answer": "The government gives a subsidy to the new factory.", "hint": w }),
            ("translation", true) => json!({ "kind": k, "prompt": "我週末和朋友去看電影。", "answer": "{週末|しゅうまつ}、{友達|ともだち}と{映画|えいが}を{見|み}に{行|い}きました。", "hint": w }),
            (_, false) => json!({ "kind": k, "prompt": format!("用「{w}」造一個句子"), "answer": format!("The new {w} will help many people."), "hint": w }),
            (_, true) => json!({ "kind": k, "prompt": format!("「{w}」を{{使|つか}}って{{文|ぶん}}を{{作|つく}}ってください。"), "answer": format!("{}がすきです。", mark(&w)), "hint": w }),
        });
    } }
    json!({
        "title": if ja { "{今日|きょう}の{話|はなし}" } else { "Inside the fab" },
        "summary": if ja { "用今天的單字聊日常生活。" } else { "晶片廠擴產，帶動補助與就業的討論。" },
        "text": text, "vocabulary": vocabulary,
        "grammar": [if ja {
            json!({ "pattern": "〜たいです", "meaning": "想要…", "explanation": "動詞ます形去掉ます＋たいです，表示自己的願望。", "example": "{映画|えいが}が{見|み}たいです。", "example_translation": "我想看電影。" })
        } else {
            json!({ "pattern": "so ... that ...", "meaning": "如此…以致於…", "explanation": "so 後接形容詞或副詞，that 子句說明結果。", "example": "The demand was so high that prices rose.", "example_translation": "需求高到價格上漲。" })
        }],
        "questions": qs, "output": output,
        "conversation": { "topic": if ja { "週末のこと" } else { "Should governments pay for chip factories?" }, "opening": if ja { "こんにちは！{週末|しゅうまつ}はなにをしましたか。" } else { "Hi! Do you think a subsidy for a chip factory is a good idea?" }, "target_words": all.iter().take(3).collect::<Vec<_>>() }
    })
}

/// Topic cards for 開始今天的課: the first listed headline as a news card,
/// then topics not shown yet ("換一批" sends the shown titles).
fn topic_cards(user: &str) -> Value {
    let ja = user.contains("Japanese");
    let shown: Vec<String> = user
        .lines()
        .find_map(|l| l.strip_prefix("Already shown today (do not repeat): "))
        .map(|l| l.split(" / ").map(|x| x.trim().to_string()).filter(|x| x != "(none)").collect())
        .unwrap_or_default();
    let first = user.lines().find_map(|l| l.strip_prefix("- [").and_then(|r| r.split(']').next()).and_then(|n| n.parse::<i64>().ok()));
    let pool: &[(&str, &str, &str)] = if ja {
        &[("新しい AI 晶片工廠", "最近的新聞，單字實用", "科技"), ("京都的公車與地鐵", "旅行時一定會用到", "交通"), ("便利商店的早餐", "生活日語的好題材", "飲食"), ("雨季的日常", "季節用語", "生活"),
          ("溫泉旅館的禮儀", "敬語練習", "旅行"), ("日本的垃圾分類", "生活必備", "生活"), ("拉麵的地方特色", "介紹食物的說法", "飲食"), ("新幹線的訂票", "買票的對話", "交通")]
    } else {
        &[("AI 晶片的新工廠", "今天的新聞，單字剛好在你的程度", "科技"), ("愛丁堡的電車", "旅行時用得到", "交通"), ("蘇格蘭早餐", "點餐常見的說法", "飲食"), ("雨天的城市生活", "天氣與通勤的用語", "生活"),
          ("博物館之夜", "描述展覽與歷史", "文化"), ("高地的健行路線", "方向與地形的單字", "地理"), ("超市裡的自有品牌", "比價與購物", "生活"), ("威士忌蒸餾廠", "參觀行程的對話", "飲食")]
    };
    let cards: Vec<Value> = pool
        .iter()
        .filter(|(t, _, _)| !shown.iter().any(|s| s == *t))
        .take(4)
        .enumerate()
        .map(|(i, (t, why, cat))| {
            let news = i == 0 && first.is_some() && shown.is_empty();
            json!({ "title": t, "why": why, "kind": if news { "news" } else if i % 2 == 0 { "interest" } else { "general" }, "article_id": if news { json!(first) } else { Value::Null }, "keywords": if ja { json!(["京都"]) } else { json!(["Edinburgh"]) }, "category": cat })
        })
        .collect();
    json!({ "cards": cards })
}

/// A focus theme's setting sheet.
fn focus_sheet(user: &str) -> Value {
    if user.contains("Japanese") {
        return json!({ "summary": "京都", "cities": ["京都"], "streets": ["四条通", "河原町通"], "shops": ["ローソン", "錦市場"], "transport": ["京都市バス", "阪急"],
            "foods": ["湯豆腐"], "drinks": ["抹茶"], "landmarks": ["清水寺"], "daily_life": ["ICOCA"], "news_keywords": ["京都"] });
    }
    json!({ "summary": "蘇格蘭與英國", "cities": ["Edinburgh", "Glasgow"], "streets": ["Princes Street", "Royal Mile"], "shops": ["Tesco", "Greggs"],
        "transport": ["ScotRail", "Edinburgh Trams"], "foods": ["haggis"], "drinks": ["whisky"], "landmarks": ["Edinburgh Castle"],
        "daily_life": ["pounds and pence"], "news_keywords": ["Edinburgh", "Holyrood"] })
}

async fn messages(Json(b): Json<Value>) -> Json<Value> {
    let user = b["messages"][0]["content"].as_str().unwrap_or("");
    let ja = user.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    let props = &b["output_config"]["format"]["schema"]["properties"];
    let (text, wait) = if props["cards"].is_object() {
        (topic_cards(user).to_string(), 900)
    } else if props["news_keywords"].is_object() {
        (focus_sheet(user).to_string(), 900)
    } else if props["enough_material"].is_object() {
        (course_outline(user, ja || user.contains("Japanese")).to_string(), 900)
    } else if props["lessons"]["items"]["properties"]["number"].is_object() {
        (fixed_titles().to_string(), 600)
    } else if props["text"].is_object() && props["conversation"].is_object() {
        let ja = b["system"].as_str().unwrap_or("").contains("Japanese teacher");
        (course_lesson(user, ja).to_string(), 1000)
    } else if props["recast"].is_object() {
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
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port())).await.expect("mock port free (set JELEARNER_MOCK_PORT for another)");
    println!("Mock services on {}", base());
    println!("Run the app against them:");
    println!("  JELEARNER_ENGINE=api JELEARNER_CLAUDE_BASE_URL={} ANTHROPIC_API_KEY=sk-ant-mock JELEARNER_NO_KEYCHAIN=1 cargo run -p jl-server", base());
    println!("and put these feeds in your workspace's sources.toml:");
    println!("  {}/feeds/en.xml   {}/feeds/ja.xml", base(), base());
    axum::serve(l, app).await.unwrap();
}
