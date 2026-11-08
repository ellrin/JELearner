// JELearner for the iPad (docs/plan.md stage F5, step 3).
//
// Lessons are prepared on the admin's computer and kept in the shared private
// repository; this page reads them with the GitHub API, holds the AI
// conversation with the Claude API (the real-time model, Haiku by default),
// and writes the raw results back into the learner's own folder
// (`profiles/<id>/ipad/inbox/`). Points, level and difficulty are worked out
// on the admin's computer with the app's own rules.
//
// Nothing here is personal: the repository, token, key and learner live only
// in this browser's storage. No build step.

import Anthropic from "https://cdn.jsdelivr.net/npm/@anthropic-ai/sdk@1.6.0/+esm";

const $ = (s) => document.querySelector(s);
const main = $("#main");
const VERSION = 1;

// ── storage (this iPad only) ────────────────────────────
const store = {
  get(k, d = null) { try { const v = localStorage.getItem(k); return v == null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { toast("這台 iPad 的儲存空間滿了", true); } },
  del(k) { try { localStorage.removeItem(k); } catch { /* nothing */ } },
};
let cfg = store.get("jl.cfg");
const st = Object.assign({ picked: {}, done: {}, queue: [], convs: {} }, store.get("jl.state", {}));
const save = () => store.set("jl.state", st);

// ── small helpers ───────────────────────────────────────
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
const esc = (t) => String(t ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
/** `{漢字|かんじ}` → ruby; the rest escaped. */
const ruby = (t) => esc(t).replace(/\{([^{}|]+)\|([^{}]*)\}/g, "<ruby>$1<rt>$2</rt></ruby>");
const plain = (t) => String(t ?? "").replace(/\{([^{}|]+)\|[^{}]*\}/g, "$1");
function rich(tag, cls, text) { const e = el(tag, cls); e.innerHTML = ruby(text); return e; }
function button(label, cls, onclick) { const b = el("button", `btn ${cls ?? ""}`, label); b.onclick = onclick; return b; }
let toastTimer;
function toast(msg, bad = false) {
  const t = $("#toast");
  t.textContent = msg; t.className = `toast${bad ? " bad" : ""}`; t.hidden = false;
  t.onclick = () => (t.hidden = true);
  clearTimeout(toastTimer); toastTimer = setTimeout(() => (t.hidden = true), bad ? 7000 : 3500);
}
const today = () => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
const UNIT = { review: "單字與文法", reading: "課文", quiz: "測驗", translation: "翻譯", "sentence-making": "造句", cloze: "填空", paraphrase: "換句話說", conversation: "AI 對話" };

// ── views and navigation ────────────────────────────────
const stack = [];
function show(title, draw, push = true) {
  if (push) stack.push({ title, draw });
  $("#title").textContent = title;
  $("#back").hidden = stack.length <= 1;
  $("#openSettings").hidden = !cfg;
  main.innerHTML = "";
  window.speechSynthesis?.cancel();
  draw(main);
  window.scrollTo(0, 0);
}
$("#back").onclick = () => { if (stack.length > 1) { stack.pop(); const v = stack[stack.length - 1]; show(v.title, v.draw, false); } };
$("#openSettings").onclick = () => show("設定", drawSettings);

// ── GitHub: the shared private repository ───────────────
const base = () => `profiles/${cfg.member}/`;
async function gh(path, opts = {}) {
  const r = await fetch(`https://api.github.com/repos/${cfg.repo}${path}`, {
    ...opts,
    headers: { Accept: "application/vnd.github+json", Authorization: `Bearer ${cfg.token}`, "X-GitHub-Api-Version": "2022-11-28", ...(opts.headers ?? {}) },
  });
  if (!r.ok) console.warn("GitHub", r.status, path);
  if (r.status === 401) throw new Error("GitHub 不認得這把鑰匙了，請家人幫忙重新設定");
  if (r.status === 403 || r.status === 404) throw Object.assign(new Error("找不到課程資料，請家人檢查設定（repo 或使用者 id）"), { status: r.status });
  if (!r.ok) throw new Error("GitHub 暫時連不上，等一下再試");
  return r;
}
async function readJson(rel) {
  const r = await gh(`/contents/${encodeURI(base() + rel)}`, { headers: { Accept: "application/vnd.github.raw+json" } });
  return r.json();
}
const b64 = (s) => btoa(String.fromCharCode(...new TextEncoder().encode(s)));
async function putNew(rel, text, message) {
  await gh(`/contents/${encodeURI(base() + rel)}`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ message, content: b64(text) }) });
}

// ── sync: results up, lessons down ──────────────────────
let syncing = false;
let syncError = null;
function syncState(text, bad = false) { const s = $("#syncState"); s.textContent = text; s.className = `sync${bad ? " bad" : ""}`; }
async function sync() {
  if (!cfg || syncing) return;
  syncing = true;
  syncState("同步中…");
  try {
    if (st.queue.length) {
      const id = `${new Date().toISOString().replace(/[:.]/g, "")}-${Math.random().toString(36).slice(2, 8)}`;
      const results = [...st.queue];
      const body = JSON.stringify({ version: VERSION, id, member: cfg.member, results }, null, 1);
      await putNew(`ipad/inbox/${id}.json`, body, `iPad results (${results.length})`);
      st.queue.splice(0, results.length);
      save();
    }
    const index = await readJson("ipad/index.json");
    store.set("jl.index", index);
    // Today's and the coming days' lessons, kept here for offline use.
    for (const [d, list] of Object.entries(index.days ?? {})) {
      if (d < today()) continue;
      for (const e of list) {
        if (!store.get(`jl.pkg:${e.file}`)) {
          try { store.set(`jl.pkg:${e.file}`, await readJson(e.file)); } catch (err) { console.warn(e.file, err); }
        }
      }
    }
    syncState(`已同步 ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
    syncError = null;
  } catch (e) {
    syncError = e.message;
    syncState(navigator.onLine ? "沒有同步成功" : "離線中", navigator.onLine);
    if (navigator.onLine) console.warn(e);
    throw e;
  } finally { syncing = false; }
}
/** A result for the admin's computer; sent with the next sync. */
function queue(result) {
  st.queue.push(result);
  save();
  sync().catch(() => {});
}

// ── speech: the iPad's own voices and recognition ───────
const synth = window.speechSynthesis;
function pickVoice(lang) {
  const vs = (synth?.getVoices() ?? []).filter((v) => v.lang.toLowerCase().startsWith(lang));
  const rank = (v) => (/premium|enhanced|加強|高品質/i.test(v.name) ? 0 : 1) + (v.localService ? 0 : 2);
  return vs.sort((a, b) => rank(a) - rank(b))[0] ?? null;
}
function speak(text, lang) {
  return new Promise((resolve) => {
    if (!synth) return resolve();
    synth.cancel();
    const u = new SpeechSynthesisUtterance(plain(text));
    const v = pickVoice(lang);
    if (v) u.voice = v;
    u.lang = v?.lang ?? (lang === "ja" ? "ja-JP" : "en-US");
    u.rate = lang === "ja" ? 0.95 : 1;
    u.onend = u.onerror = () => resolve();
    synth.speak(u);
  });
}
const Rec = window.SpeechRecognition || window.webkitSpeechRecognition;
let currentRec = null;
/** Tap to talk: a new recognizer each time (the iPad audio session). */
function listen(lang, onText) {
  return new Promise((resolve, reject) => {
    if (!Rec) return reject(new Error("這個瀏覽器不能聽寫，請用打字的"));
    synth?.cancel();
    const r = new Rec();
    currentRec = r;
    r.lang = lang === "ja" ? "ja-JP" : "en-US";
    r.interimResults = true; r.continuous = false;
    let final = "", interim = "", err = null;
    r.onresult = (e) => {
      final = ""; interim = "";
      for (const res of e.results) { if (res.isFinal) final += res[0].transcript; else interim += res[0].transcript; }
      onText?.(final + interim);
    };
    r.onerror = (e) => { err = e.error; };
    r.onend = () => (err && err !== "no-speech" && !final && !interim ? reject(new Error(err === "not-allowed" ? "麥克風沒有開放，到 設定 › Safari › 麥克風 允許" : "沒聽清楚，再說一次，或用打字的")) : resolve(final || interim));
    try { r.start(); } catch (e) { reject(e); }
    setTimeout(() => { try { r.stop(); } catch { /* ended */ } }, 15000);
  });
}
function micButton(lang, target) {
  const b = el("button", "mic", "🎤");
  const idle = () => { b.classList.remove("on"); b.textContent = "🎤"; b.setAttribute("aria-label", "按一下開始說"); };
  idle();
  b.onclick = async () => {
    if (b.classList.contains("on")) { try { currentRec?.stop(); } catch { /* ended */ } return; }
    b.classList.add("on"); b.textContent = "■"; b.setAttribute("aria-label", "正在聽，再按一下停");
    try { const t = await listen(lang, (x) => { target.value = x; target.dispatchEvent(new Event("input")); }); idle(); target.value = t; target.dispatchEvent(new Event("input")); }
    catch (e) { toast(e.message, true); }
    finally { idle(); }
  };
  return b;
}
/** A text box that grows with what is said or typed (up to about four lines). */
function grow(ta) {
  ta.addEventListener("input", () => { ta.style.height = "auto"; ta.style.height = `${Math.min(ta.scrollHeight, 160)}px`; });
  return ta;
}

// ── Claude: the AI conversation ─────────────────────────
async function claude(system, messages, schema, maxTokens) {
  if (!cfg.apiKey) throw new Error("還沒有設定 AI 老師的鑰匙，請家人幫忙設定");
  const index = store.get("jl.index", {});
  const client = new Anthropic({ apiKey: cfg.apiKey, dangerouslyAllowBrowser: true });
  let res;
  try {
    res = await client.messages.create({
      model: index.realtime_model || "claude-haiku-5-5",
      max_tokens: maxTokens,
      output_config: { effort: index.realtime_effort || "low", format: { type: "json_schema", schema } },
      system,
      messages,
    });
  } catch (e) {
    if (e instanceof Anthropic.AuthenticationError) throw new Error("AI 老師的鑰匙不對，請家人幫忙重新設定");
    if (e instanceof Anthropic.RateLimitError) throw new Error("AI 老師忙不過來，等一下再試");
    if (e instanceof Anthropic.APIConnectionError) throw new Error("連不上 AI 老師，檢查一下網路");
    console.warn("Claude", e.status, e.message);
    throw new Error("AI 老師暫時沒有回答，等一下再試");
  }
  if (res.stop_reason === "refusal") throw new Error("AI 老師沒有回答這句，換個說法再試一次");
  const text = res.content.filter((b) => b.type === "text").map((b) => b.text).join("");
  return JSON.parse(text);
}

// ── setup ───────────────────────────────────────────────
// A setup link from the admin: …/app/#setup=<base64url JSON> (stays in this browser).
function readSetupLink() {
  const m = location.hash.match(/^#setup=([A-Za-z0-9_-]+)/);
  if (!m) return false;
  try {
    const json = new TextDecoder().decode(Uint8Array.from(atob(m[1].replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)));
    const c = JSON.parse(json);
    if (c.repo && c.token && c.member) { cfg = c; store.set("jl.cfg", cfg); }
  } catch { toast("設定連結讀不懂", true); }
  history.replaceState(null, "", location.pathname);
  return true;
}
function drawSettings(box) {
  box.append(el("h2", null, cfg ? "設定" : "第一次使用"));
  if (!cfg) box.append(el("p", "muted", "請家人幫忙：從家人的電腦傳來的設定連結打開，或在這裡填好。"));
  const f = (label, type, v, ph) => { const l = el("label", "f"); const i = el("input"); i.type = type; i.value = v ?? ""; i.placeholder = ph ?? ""; i.autocomplete = "off"; i.autocapitalize = "off"; l.append(el("span", null, label), i); box.append(l); return i; };
  const repo = f("資料 repo（帳號/名稱）", "text", cfg?.repo, "ellrin/JELearner-private");
  const member = f("你的使用者 id", "text", cfg?.member, "daidai");
  const token = f("GitHub 鑰匙", "password", cfg?.token, "github_pat_…");
  const key = f("AI 老師的鑰匙（Claude API key）", "password", cfg?.apiKey, "sk-ant-…");
  const row = el("div", "row");
  row.append(button("儲存並同步", "main", async () => {
    cfg = { repo: repo.value.trim().replace(/^https:\/\/github.com\//, "").replace(/\.git$/, ""), member: member.value.trim(), token: token.value.trim(), apiKey: key.value.trim() };
    if (!/^[\w.-]+\/[\w.-]+$/.test(cfg.repo) || !cfg.member || !cfg.token) { toast("repo、使用者 id 與 GitHub 鑰匙都要填", true); return; }
    store.set("jl.cfg", cfg);
    try { await sync(); toast("設定好了"); stack.length = 0; show("今天", drawHome); } catch (e) { toast(e.message, true); }
  }));
  if (cfg) {
    row.append(button("立即同步", "", async () => { try { await sync(); toast("同步好了"); } catch (e) { toast(e.message, true); } }));
  }
  box.append(row);
  if (st.queue.length) box.append(el("p", "note", `有 ${st.queue.length} 筆作答還沒送出，連上網路會自動送。`));
  box.append(el("p", "note", "鑰匙只存在這台 iPad 的瀏覽器裡。"));
}

// ── today: choose one of the prepared lessons ──────────
function drawHome(box) {
  const index = store.get("jl.index");
  const d = today();
  const list = index?.days?.[d] ?? [];
  if (!index) {
    if (!syncError) { box.append(el("p", "muted", "正在拿今天的課…")); return; }
    box.append(el("h2", null, "還拿不到今天的課"), el("p", "err", syncError));
    const row = el("div", "row");
    row.append(button("再試一次", "main", async () => { try { await sync(); } catch { /* shown below */ } show("今天", drawHome, false); }));
    box.append(row);
    return;
  }
  const picked = st.picked[d];
  if (picked) {
    const e = list.find((x) => x.file === picked);
    const pkg = store.get(`jl.pkg:${picked}`);
    if (pkg) {
      box.append(el("h2", null, "今天的課"));
      box.append(lessonCard(pkg, e ?? pkg.card, picked));
      const other = list.filter((x) => x.file !== picked);
      if (other.length && allDone(pkg)) {
        box.append(el("p", "note", "今天的課上完了，還想多學一點："));
        other.forEach((x) => box.append(choiceButton(x)));
      }
      return;
    }
  }
  if (!list.length) {
    box.append(el("h2", null, "今天還沒有課"));
    box.append(el("p", "muted", "家人備好課、同步之後就會出現。"));
    const next = Object.keys(index.days ?? {}).filter((k) => k > d).sort()[0];
    if (next) box.append(el("p", "note", `下一天有課的是 ${next}。`));
    return;
  }
  box.append(el("h2", null, "今天想學什麼？"));
  list.forEach((x) => box.append(choiceButton(x)));
}
function choiceButton(x) {
  const b = el("button", "choice");
  const kind = x.kind === "library" ? x.category ?? "" : { news: "新聞", interest: "興趣", general: "通識" }[x.kind] ?? "";
  if (kind) b.append(el("span", `tag${x.kind === "news" ? " acc" : ""}`, kind));
  b.append(el("b", null, x.title));
  if (x.why) b.append(el("span", "why", x.why));
  b.onclick = () => {
    if (!store.get(`jl.pkg:${x.file}`)) { toast("這堂課還沒下載，連上網路再試", true); return; }
    st.picked[today()] = x.file; save();
    openLesson(x.file);
  };
  return b;
}
const doneOf = (file) => (st.done[file] ??= []);
const keyOf = (pkg) => `${pkg.course}#${pkg.lesson}`;
const allDone = (pkg) => pkg.plan.units.every((u) => doneOf(keyOf(pkg)).includes(u.index));
function lessonCard(pkg, e, file) {
  const c = el("button", "choice");
  const d = doneOf(keyOf(pkg)).length;
  c.append(el("b", null, e?.title ?? pkg.plan.title), el("span", "why", `${d} / ${pkg.plan.units.length} 步${allDone(pkg) ? "・完成了" : ""}`));
  c.onclick = () => openLesson(file);
  return c;
}

// ── one lesson: its steps ───────────────────────────────
function openLesson(file) {
  const pkg = store.get(`jl.pkg:${file}`);
  if (!pkg) { toast("這堂課還沒下載", true); return; }
  show(pkg.card?.title ?? pkg.plan.title, (box) => {
    if (pkg.plan.purpose) box.append(el("p", "muted", pkg.plan.purpose));
    const next = pkg.plan.units.find((x) => !doneOf(keyOf(pkg)).includes(x.index));
    const top = el("div", "row");
    if (next) top.append(button(`${doneOf(keyOf(pkg)).length ? "繼續" : "開始"}：${UNIT[next.kind] ?? next.kind}`, "main", () => openUnit(pkg, next)));
    else top.append(el("span", "tag ok", "這堂課完成了"));
    box.append(top);
    const ul = el("ul", "units");
    for (const u of pkg.plan.units) {
      const li = el("li");
      const done = doneOf(keyOf(pkg)).includes(u.index);
      const b = el("button", done ? "done" : u === next ? "next" : "");
      const mk = el("span", "mark", done ? "✓" : String(u.index + 1));
      b.append(mk, el("span", null, UNIT[u.kind] ?? u.kind), el("span", "min", `${u.minutes} 分`));
      b.onclick = () => openUnit(pkg, u);
      li.append(b);
      ul.append(li);
    }
    box.append(ul);
  });
}
function finishUnit(pkg, u, result) {
  queue({ type: "unit", course: pkg.course, lesson: pkg.lesson, unit: u.index, ...result });
  const d = doneOf(keyOf(pkg));
  if (!d.includes(u.index)) d.push(u.index);
  save();
}
function nextRow(pkg, u) {
  const row = el("div", "row");
  const next = pkg.plan.units.find((x) => x.index > u.index && !doneOf(keyOf(pkg)).includes(x.index));
  if (next) row.append(button(`下一步：${UNIT[next.kind] ?? next.kind}`, "main", () => { stack.pop(); openUnit(pkg, next); }));
  else row.append(button("這堂課完成了", "main", () => { stack.length = 0; show("今天", drawHome); }));
  return row;
}
function openUnit(pkg, u) {
  const draw = { review: drawReview, reading: drawReading, quiz: drawReading, conversation: drawConversation }[u.kind] ?? drawOutput;
  show(UNIT[u.kind] ?? u.kind, (box) => draw(box, pkg, u));
}

// review: flash cards from the lesson's due cards
function drawReview(box, pkg, u) {
  // This lesson's words and grammar first, then the cards.
  const words = pkg.data?.vocabulary ?? [];
  const grammar = pkg.data?.grammar ?? [];
  if (words.length || grammar.length) {
    const c = el("div", "card");
    if (words.length) {
      c.append(el("h2", null, "單字"));
      for (const w of words) {
        const row = el("div", "q");
        const t = rich("b", null, w.term);
        const say = button("🔊", "", () => speak(w.term, pkg.lang)); say.setAttribute("aria-label", `唸 ${plain(w.term)}`); say.style.minHeight = "44px"; say.style.marginLeft = "8px";
        row.append(t, say, el("div", "why", `${w.reading ? `${w.reading}・` : ""}${w.meaning}`));
        if (w.example) { const ex = rich("div", null, w.example); ex.append(el("div", "why", w.example_translation ?? "")); row.append(ex); }
        c.append(row);
      }
    }
    if (grammar.length) {
      c.append(el("h2", null, "文法"));
      for (const g of grammar) {
        const row = el("div", "q");
        row.append(rich("b", null, g.pattern), el("div", "why", `${g.meaning}　${g.explanation ?? ""}`));
        if (g.example) { const ex = rich("div", null, g.example); ex.append(el("div", "why", g.example_translation ?? "")); row.append(ex); }
        c.append(row);
      }
    }
    box.append(c);
  }
  const cardsBox = el("div");
  box.append(cardsBox);
  box = cardsBox;
  const cards = (pkg.review_cards ?? []).slice(0, Math.max(1, u.items || 20));
  if (!cards.length) { box.append(el("p", "muted", "今天沒有要複習的卡片。")); finishUnit(pkg, u, { cards: 1, score: 1 }); box.append(nextRow(pkg, u)); return; }
  let i = 0, good = 0;
  const lang = pkg.lang;
  const step = () => {
    box.innerHTML = "";
    if (i >= cards.length) {
      const score = good / cards.length;
      finishUnit(pkg, u, { cards: cards.length, score });
      box.append(el("h2", null, `複習完了：記得 ${good} / ${cards.length}`), nextRow(pkg, u));
      return;
    }
    const c = cards[i];
    const f = el("div", "card flash");
    f.append(rich("div", "front", c.front));
    if (c.reading) f.append(el("div", "reading", c.reading));
    const back = rich("div", "back", c.back); back.hidden = true;
    f.append(back);
    box.append(el("p", "note", `${i + 1} / ${cards.length}`), f);
    const row = el("div", "row");
    const say = button("🔊 唸", "", () => speak(c.front, lang));
    const flip = button("看答案", "main", () => {
      back.hidden = false; flip.remove();
      row.append(button("記得", "ok", () => { good++; i++; step(); }), button("忘了", "no", () => { i++; step(); }));
    });
    row.append(say, flip);
    box.append(row);
  };
  step();
}

// reading and quiz: today's text and the questions
function drawReading(box, pkg, u) {
  const data = pkg.data ?? {};
  const lang = pkg.lang;
  if (u.kind === "reading" && data.text) {
    // Sentence by sentence: listen, then say it (before the questions).
    const g = el("div", "card");
    g.append(el("p", null, Rec ? "先聽一句、跟著唸一句，唸完整篇再回答下面的問題。" : "先一句一句聽，跟著唸出來，再回答下面的問題。"));
    const gRow = el("div", "row");
    gRow.append(button(Rec ? "逐句跟讀" : "逐句聽", "main", () => {
      const panel = shadowPanel(data.text, lang, () => {
        panel.replaceWith(g);
        box.querySelector(".card.q")?.scrollIntoView({ block: "start", behavior: "smooth" });
      });
      g.replaceWith(panel);
    }));
    g.append(gRow);
    box.append(g);
    const t = el("div", "card");
    if (pkg.picture?.path) t.append(pictureFigure(pkg.picture));
    t.append(rich("div", "text", data.text));
    const row = el("div", "row");
    row.append(button("🔊 唸課文", "", () => speak(data.text, lang)), button("停", "", () => synth?.cancel()));
    t.append(row);
    box.append(t);
  }
  const qs = data.questions ?? [];
  if (!qs.length) { finishUnit(pkg, u, { answers: [] }); box.append(nextRow(pkg, u)); return; }
  const answers = [];
  qs.forEach((q, k) => {
    const d = el("div", "card q");
    d.append(rich("div", null, `${k + 1}. ${q.question}`));
    if (q.question_translation) { const h = button("中", "", () => { h.replaceWith(el("div", "why", q.question_translation)); }); h.style.minHeight = "44px"; d.append(h); }
    const why = el("div", "why");
    const settle = (correct, given) => {
      answers[k] = { index: k, answer: given, correct };
      why.textContent = `${correct ? "✓ 對了" : `✗ 答案：${plain(q.answer)}`}${q.explanation ? `　${q.explanation}` : ""}`;
      if (answers.filter(Boolean).length === qs.length) {
        const n = answers.filter((a) => a.correct).length;
        finishUnit(pkg, u, { answers });
        const h = el("h2", null, `答對 ${n} / ${qs.length}`);
        box.append(h, nextRow(pkg, u));
        h.scrollIntoView({ block: "center", behavior: "smooth" });
      }
    };
    if (q.kind === "choice" && q.options?.length) {
      const opts = el("div", "opts");
      q.options.forEach((o) => {
        const b = el("button"); b.innerHTML = ruby(o);
        b.onclick = () => {
          if (answers[k]) return;
          const ok = o.trim() === q.answer.trim();
          b.classList.add(ok ? "right" : "wrong");
          settle(ok, o);
        };
        opts.append(b);
      });
      d.append(opts);
    } else {
      const ta = grow(el("textarea")); ta.rows = 2; if (lang === "ja") ta.lang = "ja";
      const row = el("div", "row");
      row.append(micButton(lang, ta), button("對答案", "", () => {
        if (answers[k]) return;
        const given = ta.value.trim();
        const norm = (s) => plain(s).toLowerCase().replace(/[\s。、．，,.!?！？「」『』"']/g, "");
        if (given && norm(given) === norm(q.answer)) { settle(true, given); return; }
        why.textContent = `參考答案：${plain(q.answer)}`;
        row.append(button("✓ 對了", "ok", () => settle(true, given)), button("✗ 再加油", "no", () => settle(false, given)));
      }));
      d.append(ta, row);
    }
    d.append(why);
    box.append(d);
  });
}

/** A library lesson's picture (from the repository; kept here for offline use) with its credit. */
function pictureFigure(pic) {
  const fig = el("figure", "pic");
  const img = el("img");
  img.alt = "";
  const cap = el("figcaption", "note");
  const a = el("a", null, `${pic.author}・${pic.licence}`);
  a.href = pic.page; a.target = "_blank"; a.rel = "noopener";
  cap.append("圖片：", a);
  fig.append(img, cap);
  const key = `jl.pic:${pic.path}`;
  const cached = store.get(key);
  if (cached) img.src = cached;
  else {
    gh(`/contents/${encodeURI(base() + pic.path)}`, { headers: { Accept: "application/vnd.github.raw+json" } })
      .then((r) => r.blob())
      .then((b) => new Promise((ok) => { const fr = new FileReader(); fr.onload = () => ok(fr.result); fr.readAsDataURL(b); }))
      .then((url) => { img.src = url; store.set(key, url); })
      .catch(() => fig.remove());
  }
  return fig;
}

/** Sentences of a text (English: . ! ? ; Japanese: 。！？). */
function sentencesOf(text, lang) {
  const t = plain(text).replace(/\s*\n+\s*/g, " ").trim();
  const parts = lang === "ja" ? t.split(/(?<=[。！？!?])/u) : t.split(/(?<=[.!?])\s+(?=[A-Z"'“‘(])/u);
  return parts.map((x) => x.trim()).filter((x) => x.replace(/[\s.,!?。、！？「」"'“”‘’()-]/g, "").length > 0);
}
/** How much of the sentence was said: matched words (characters for Japanese), in percent. */
function matchScore(target, said, lang) {
  const norm = (x) => x.toLowerCase().replace(/[.,!?。、！？「」"'“”‘’()]/g, " ");
  if (lang === "ja") {
    const a = [...norm(target).replace(/\s/g, "")], b = norm(said).replace(/\s/g, "");
    return a.length ? Math.round((a.filter((ch) => b.includes(ch)).length / a.length) * 100) : 0;
  }
  const a = norm(target).split(/\s+/).filter(Boolean), b = new Set(norm(said).split(/\s+/).filter(Boolean));
  return a.length ? Math.round((a.filter((w) => b.has(w)).length / a.length) * 100) : 0;
}
function shadowPanel(text, lang, onDone) {
  const list = sentencesOf(text, lang);
  const box = el("div", "card");
  let i = 0;
  const draw = () => {
    box.innerHTML = "";
    const s = list[i];
    box.append(el("p", "note", `第 ${i + 1} / ${list.length} 句`), rich("div", "text", s));
    const heard = el("div", "heard");
    const result = el("div");
    result.setAttribute("aria-live", "polite");
    const row = el("div", "row");
    const listenBtn = button("🔊 聽這一句", "", () => speak(s, lang));
    row.append(listenBtn);
    if (Rec) {
      const ta = el("textarea"); ta.hidden = true;
      const mic = micButton(lang, ta);
      ta.addEventListener("input", () => {
        heard.textContent = ta.value;
        if (mic.classList.contains("on")) return;
        const sc = matchScore(s, ta.value, lang);
        result.innerHTML = "";
        result.append(el("p", sc >= 80 ? "" : "note", sc >= 80 ? `✓ ${sc}%，很好！` : `${sc}%，再聽一次、再唸一次看看`));
      });
      row.append(mic);
    }
    const nav = el("div", "row");
    if (i > 0) nav.append(button("上一句", "", () => { i--; draw(); }));
    nav.append(i < list.length - 1 ? button("下一句", "main", () => { i++; draw(); }) : button("唸完了，去答題", "main", () => { synth?.cancel(); onDone ? onDone() : box.remove(); }));
    box.append(row, heard, result, nav);
    speak(s, lang);
  };
  if (list.length) draw();
  return box;
}

// output: write (or say) it, then compare with the model answer
function drawOutput(box, pkg, u) {
  const lang = pkg.lang;
  const all = pkg.data?.output ?? [];
  const items = all.filter((x) => x.kind === u.kind).length ? all.filter((x) => x.kind === u.kind) : all;
  if (!items.length) { box.append(el("p", "muted", "這一課沒有這種練習。")); finishUnit(pkg, u, { items: [{ prompt: "", answer: "", model: "", correct: true }] }); box.append(nextRow(pkg, u)); return; }
  box.append(el("p", "note", "先自己寫或說，再看參考答案，誠實標記對或錯。"));
  const marks = [];
  items.forEach((it, k) => {
    const d = el("div", "card");
    d.append(rich("div", null, `${k + 1}. ${it.prompt}`));
    const ta = grow(el("textarea")); ta.rows = 2; if (lang === "ja") ta.lang = "ja";
    const row = el("div", "row");
    const fb = el("div");
    const mark = (ok) => {
      marks[k] = { prompt: plain(it.prompt), answer: ta.value.trim(), model: plain(it.answer), hint: it.hint ?? "", correct: ok };
      row.innerHTML = ""; row.append(el("span", `tag ${ok ? "ok" : ""}`, ok ? "對了" : "記進錯題本"));
      ta.disabled = true;
      if (marks.filter(Boolean).length === items.length) {
        finishUnit(pkg, u, { items: marks });
        const h = el("h2", null, "做完了");
        box.append(h, nextRow(pkg, u));
        h.scrollIntoView({ block: "center", behavior: "smooth" });
      }
    };
    row.append(micButton(lang, ta), button("對答案", "", () => {
      fb.innerHTML = "";
      const m = el("div", "model"); m.innerHTML = `參考答案　${ruby(it.answer)}${it.hint ? `<div class="note">${esc(plain(it.hint))}</div>` : ""}`;
      fb.append(m);
      row.innerHTML = "";
      row.append(button("🔊", "", () => speak(it.answer, lang)), button("✓ 對了", "ok", () => mark(true)), button("✗ 再加油", "no", () => mark(false)));
    }));
    d.append(ta, row, fb);
    box.append(d);
  });
}

// conversation: the prompts were rendered on the admin's computer
function drawConversation(box, pkg, u) {
  const c = pkg.conversation;
  const lang = pkg.lang;
  if (!c?.system) {
    box.append(el("p", "muted", "這堂課的對話沒有準備好，先跳過。"));
    const d = doneOf(keyOf(pkg)); if (!d.includes(u.index)) d.push(u.index); save();
    box.append(nextRow(pkg, u));
    return;
  }
  const key = keyOf(pkg);
  const conv = (st.convs[key] ??= { started_at: new Date().toISOString(), turns: [{ role: "ai", text: c.opening || "", recast: "" }] });
  save();
  const me = () => conv.turns.filter((t) => t.role === "me").length;
  box.append(el("p", "note", `主題：${plain(c.topic)}・試著用：${(c.targets ?? []).map((t) => plain(t.term)).join("、")}`));
  const chat = el("div", "chat");
  box.append(chat);
  const bubble = (t) => {
    const b = el("div", `bubble ${t.role === "me" ? "me" : ""}`);
    if (t.recast) { const r = el("span", "recast"); r.innerHTML = `↻ ${ruby(t.recast)}`; b.append(r); }
    b.append(rich("span", null, t.text));
    if (t.role === "ai") { const s = el("button", "btn again", "🔊"); s.setAttribute("aria-label", "再聽一次"); s.onclick = () => speak(`${t.recast ? `${t.recast}。` : ""}${t.text}`, lang); b.prepend(s); }
    chat.append(b);
    toBottom();
  };
  // The bar is fixed at the bottom: keep the newest message above it.
  const toBottom = () => { chat.style.marginBottom = `${(bar?.offsetHeight ?? 120) + 16}px`; window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "smooth" }); };
  let bar;
  bar = el("div", "say");
  const status = el("div", "status"); status.setAttribute("role", "status");
  bar.append(status);
  const inner = el("div", "inner");
  const ta = grow(el("textarea")); ta.rows = 1; ta.placeholder = lang === "ja" ? "日本語で話そう" : "Say it in English"; if (lang === "ja") ta.lang = "ja";
  const send = button("送出", "main", null);
  inner.append(micButton(lang, ta), ta, send);
  bar.append(inner);
  box.append(bar);
  conv.turns.forEach(bubble);
  const say = (text, bad = false) => { status.textContent = text; status.className = `status${bad ? " err" : ""}`; };
  const left = () => say(me() >= c.turns ? "" : `還有 ${c.turns - me()} 句`);
  left();
  const asMessages = () => {
    const m = [{ role: "user", content: c.opening_cue }];
    for (const t of conv.turns) m.push(t.role === "ai" ? { role: "assistant", content: t.recast ? `${t.recast}\n${t.text}` : t.text } : { role: "user", content: t.text });
    return m;
  };
  const finish = async () => {
    inner.remove();
    say("AI 老師正在看你的表現…");
    const lines = conv.turns.map((t) => (t.role === "ai" ? `[partner] ${(t.recast ? `${t.recast}\n${t.text}` : t.text).replace(/\n/g, " ")}` : `[learner] ${t.text}`)).join("\n");
    try {
      const review = await claude(c.review_system, [{ role: "user", content: `${c.review_user_head}${lines}\n${c.review_user_tail}` }], c.review_schema, c.review_max_tokens || 4000);
      queue({ type: "conversation", course: pkg.course, lesson: pkg.lesson, unit: u.index, started_at: conv.started_at, turns: conv.turns, review });
      const d = doneOf(key); if (!d.includes(u.index)) d.push(u.index);
      delete st.convs[key]; save();
      bar.remove();
      chat.style.marginBottom = "16px";
      const r = el("div", "card");
      r.append(el("h2", null, "對話完成！"));
      if (review.summary) r.append(el("p", null, review.summary));
      for (const b of review.better ?? []) { const p = el("p", "note"); p.innerHTML = `${esc(b.original)} → <b>${esc(b.suggestion)}</b>　${esc(b.why)}`; r.append(p); }
      box.append(r, nextRow(pkg, u));
      r.scrollIntoView({ block: "start", behavior: "smooth" });
    } catch (e) {
      say(e.message, true);
      if (!bar.querySelector(".retry")) { const again = button("再試一次", "main retry", finish); bar.append(again); }
    }
  };
  const turn = async () => {
    const text = ta.value.trim();
    if (!text) return;
    ta.value = ""; send.disabled = true;
    const t = { role: "me", text, recast: "" };
    conv.turns.push(t); save(); bubble(t);
    ta.style.height = "auto";
    say("AI 老師在想…");
    try {
      const last = me() >= c.turns;
      const reply = await claude(last ? c.system_last : c.system, asMessages(), c.turn_schema, c.turn_max_tokens || 2000);
      const a = { role: "ai", text: reply.question ?? "", recast: reply.recast ?? "" };
      conv.turns.push(a); save(); bubble(a);
      speak(`${a.recast ? `${a.recast}。` : ""}${a.text}`, lang);
      left();
      if (last) finish();
    } catch (e) {
      // Take the turn back so it can be sent again.
      conv.turns.pop(); save(); chat.lastChild?.remove(); ta.value = text;
      say(e.message, true);
    } finally { send.disabled = false; }
  };
  send.onclick = turn;
  if (me() >= c.turns) finish();
  else if (conv.turns.length === 1 && c.opening) speak(c.opening, lang);
}

// ── start ───────────────────────────────────────────────
readSetupLink();
if (!cfg) show("設定", drawSettings);
else {
  show("今天", drawHome);
  sync().then(() => { if (stack.length === 1 && stack[0].draw === drawHome) show("今天", drawHome, false); }).catch(() => {});
}
window.addEventListener("online", () => sync().catch(() => {}));
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") sync().catch(() => {}); });
if ("serviceWorker" in navigator) navigator.serviceWorker.register("sw.js").catch(() => {});
