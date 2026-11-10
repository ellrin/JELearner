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
const st = Object.assign({ picked: {}, done: {}, queue: [], convs: {}, steps: {} }, store.get("jl.state", {}));
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
let syncAgain = false;
async function sync() {
  // Something new while syncing (two results at once): one more round after this one.
  if (syncing) { syncAgain = true; return; }
  if (!cfg) return;
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
    // Everyone's points, worked out on the admin's computer (the buddies strip).
    try { store.set("jl.buddies", await readJson("ipad/buddies.json")); } catch { /* not written yet */ }
    // Today's and the coming days' lessons, kept here for offline use.
    for (const [d, list] of Object.entries(index.days ?? {})) {
      if (d < today()) continue;
      for (const e of list) {
        if (!store.get(`jl.pkg:${e.file}`)) {
          try { store.set(`jl.pkg:${e.file}`, await readJson(e.file)); } catch (err) { console.warn(e.file, err); }
        }
        // Its pictures too, for offline (shrunk, two weeks).
        for (const pic of store.get(`jl.pkg:${e.file}`)?.pictures ?? []) pictureBlob(pic.url).catch(() => {});
      }
    }
    syncState(`已同步 ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
    syncError = null;
  } catch (e) {
    syncError = e.message;
    syncState(navigator.onLine ? "沒有同步成功" : "離線中", navigator.onLine);
    if (navigator.onLine) console.warn(e);
    throw e;
  } finally {
    syncing = false;
    if (syncAgain) { syncAgain = false; setTimeout(() => sync().catch(() => {}), 0); }
  }
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
function listen(lang, onText, { max = 15, silence = 0, onLoud } = {}) {
  return new Promise((resolve, reject) => {
    if (!Rec) return reject(new Error("這個瀏覽器不能聽寫，請用打字的"));
    synth?.cancel();
    const r = new Rec();
    currentRec = r;
    r.lang = lang === "ja" ? "ja-JP" : "en-US";
    // With a silence stop the recognizer keeps listening; we end it after the pause.
    r.interimResults = true; r.continuous = !!silence;
    let final = "", interim = "", err = null, last = 0;
    const quiet = silence ? setInterval(() => { if (last && Date.now() - last > silence * 1000) { try { r.stop(); } catch { /* ended */ } } }, 250) : null;
    r.onresult = (e) => {
      if (!last) onLoud?.();
      last = Date.now();
      final = ""; interim = "";
      for (const res of e.results) { if (res.isFinal) final += res[0].transcript; else interim += res[0].transcript; }
      onText?.(final + interim);
    };
    r.onerror = (e) => { err = e.error; };
    r.onend = () => (clearInterval(quiet), err && err !== "no-speech" && !final && !interim ? reject(new Error(err === "not-allowed" ? "麥克風沒有開放，到 設定 › Safari › 麥克風 允許" : "沒聽清楚，再說一次，或用打字的")) : resolve(final || interim));
    try { r.start(); } catch (e) { reject(e); }
    setTimeout(() => { try { r.stop(); } catch { /* ended */ } }, max * 1000);
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
  if (!/^#setup=/.test(location.hash)) return false;
  if (!takeSetup(location.hash)) toast("設定連結讀不懂", true);
  history.replaceState(null, "", location.pathname);
  return true;
}
/** Settings from a setup link or a scanned QR code (`…#setup=<base64url JSON>`); true when taken. */
function takeSetup(text) {
  const m = String(text ?? "").match(/#setup=([A-Za-z0-9_-]+)/);
  if (!m) return false;
  try {
    const json = new TextDecoder().decode(Uint8Array.from(atob(m[1].replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)));
    const c = JSON.parse(json);
    if (!(c.repo && c.token && c.member)) return false;
    cfg = c; store.set("jl.cfg", cfg);
    return true;
  } catch { return false; }
}

// Scan the QR code on the admin's computer with this iPad's camera: the
// settings go straight into this app (on the home screen it has its own
// storage, which a link opened in Safari does not reach).
let jsqrLoading = null;
function loadJsQR() {
  jsqrLoading ??= new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = "vendor/jsQR.js";
    s.onload = () => resolve(window.jsQR);
    s.onerror = () => { jsqrLoading = null; reject(new Error("掃描功能下載不了，連上網路再試")); };
    document.head.append(s);
  });
  return jsqrLoading;
}
/** The text of a QR code in the frame, or null. */
function decodeQR(image) {
  const r = window.jsQR?.(image.data, image.width, image.height, { inversionAttempts: "dontInvert" });
  return r?.data || null;
}
async function scanSetup() {
  if (!navigator.mediaDevices?.getUserMedia) { toast("這個瀏覽器不能用相機，請用設定連結", true); return; }
  let stream;
  try {
    await loadJsQR();
    stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" }, width: { ideal: 1280 } }, audio: false });
  } catch (e) {
    toast(e.name === "NotAllowedError" ? "相機沒有打開：請允許使用相機，再按一次" : e.message || "相機打不開", true);
    return;
  }
  const overlay = el("div", "scan");
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-label", "掃描 QR code");
  const video = el("video");
  video.playsInline = true; video.muted = true; video.autoplay = true;
  video.srcObject = stream;
  const hint = el("p", "scan-hint", "對準電腦上的 QR code");
  hint.setAttribute("role", "status");
  let stopped = false;
  const stop = () => { stopped = true; stream.getTracks().forEach((t) => t.stop()); overlay.remove(); };
  const cancel = button("取消", "", stop);
  overlay.append(video, hint, cancel);
  document.body.append(overlay);
  cancel.focus();
  const cv = document.createElement("canvas");
  const g = cv.getContext("2d", { willReadFrequently: true });
  const tick = async () => {
    if (stopped) return;
    if (video.readyState >= 2 && video.videoWidth) {
      // The middle square, at most 800 px: enough for a dense code, quick to read.
      const side = Math.min(video.videoWidth, video.videoHeight), size = Math.min(800, side);
      cv.width = cv.height = size;
      g.drawImage(video, (video.videoWidth - side) / 2, (video.videoHeight - side) / 2, side, side, 0, 0, size, size);
      const text = decodeQR(g.getImageData(0, 0, size, size));
      if (text) {
        if (takeSetup(text)) {
          stop();
          toast("設定好了");
          stack.length = 0;
          show("今天", drawHome);
          sync().then(() => show("今天", drawHome, false)).catch(() => show("今天", drawHome, false));
          return;
        }
        hint.textContent = "這不是 JELearner 的設定 QR code";
      }
    }
    setTimeout(tick, 150);
  };
  tick();
}
function drawSettings(box) {
  box.append(el("h2", null, cfg ? "設定" : "第一次使用"));
  if (!cfg) {
    box.append(el("p", "muted", "請家人幫忙：掃描家人電腦上的 QR code 就設定好了。"));
    const top = el("div", "row");
    top.append(button("掃描電腦上的 QR code", "main", scanSetup));
    box.append(top);
    const manual = el("details", "manual");
    manual.append(el("summary", null, "手動填寫"));
    box.append(manual);
    box = manual;
  }
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
    // A new key (the old one ran out): scan the computer's QR code again.
    row.append(button("重新掃描 QR code", "", scanSetup));
  }
  box.append(row);
  if (st.queue.length) box.append(el("p", "note", `有 ${st.queue.length} 筆作答還沒送出，連上網路會自動送。`));
  box.append(el("p", "note", "鑰匙只存在這台 iPad 的瀏覽器裡。"));
}

// ── study buddies: everyone's points, from the admin's computer ──
const fmtPts = (p) => String(Math.round((p ?? 0) * 10) / 10);
function buddyStrip() {
  const data = store.get("jl.buddies");
  const list = data?.buddies ?? [];
  if (list.length < 2) return null;
  const fresh = data.as_of === today();
  const now = new Date();
  // The numbers are as of `as_of`: shift the seven days to end today.
  const g = Math.round((new Date(today()) - new Date(data.as_of)) / 86400000);
  const gap = Number.isFinite(g) ? Math.min(7, Math.max(0, g)) : 7;
  const box = el("section", "buddies");
  box.setAttribute("aria-label", "學習戰友");
  for (const b of list) {
    const days = [...(b.days ?? []).slice(gap), ...new Array(Math.min(gap, 7)).fill(0)].slice(-7);
    let mine = fresh ? b.today : 0;
    // Her own steps today that the computer has not counted yet (about a point each).
    if (b.me) { mine = Math.max(mine, st.steps?.[today()] ?? 0); days[6] = Math.max(days[6] ?? 0, mine); }
    // The streak as the seven days show it (the file's longer count when all seven are lit).
    let run = 0, i = days[6] > 0 ? 6 : 5;
    while (i >= 0 && days[i] > 0) { run += 1; i -= 1; }
    const streak = i < 0 ? Math.max(run, b.streak) : run;
    const card = el("div", `buddy${b.me ? " me" : ""}${mine >= b.goal ? " reached" : ""}`);
    const av = el("span", "av", ([...b.name][0] ?? "").toUpperCase());
    av.style.background = b.color;
    const body = el("div", "bb");
    const top = el("div", "bt");
    top.append(el("b", null, b.me ? `${b.name}（你）` : b.name), el("span", "lv", Object.values(b.levels ?? {}).join("・")));
    const nums = el("div", "bn");
    nums.append(el("b", null, fmtPts(mine)), document.createTextNode(` / ${fmtPts(b.goal)} 今日　`), el("b", null, String(streak)), document.createTextNode(" 天連續"));
    const dots = el("div", "bd");
    days.forEach((p, i) => {
      const dt = new Date(now); dt.setDate(now.getDate() - (6 - i));
      dots.append(el("i", `${p > 0 ? "on" : ""}${i === 6 ? " today" : ""}`, "日一二三四五六"[dt.getDay()]));
    });
    dots.setAttribute("role", "img");
    dots.setAttribute("aria-label", `近 7 天有 ${days.filter((p) => p > 0).length} 天練習`);
    body.append(top, nums, dots);
    card.append(av, body);
    box.append(card);
  }
  return box;
}

// ── today: choose one of the prepared lessons ──────────
function drawHome(box) {
  const index = store.get("jl.index");
  const d = today();
  const strip = index ? buddyStrip() : null;
  if (strip) box.append(strip);
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
  if (!d.includes(u.index)) {
    d.push(u.index);
    // Steps finished today, shown on the buddies strip until the computer has counted them.
    st.steps = { [today()]: (st.steps?.[today()] ?? 0) + 1 };
  }
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
    // The lesson taught out loud (plan §19.4.2), when the computer packed its script.
    if (pkg.guided?.script) gRow.prepend(button("老師帶著上", "main", () => show("老師帶著上", (b) => drawGuided(b, pkg, u))));
    g.append(gRow);
    box.append(g);
    const t = el("div", "card");
    // Paragraph by paragraph, each picture after its paragraph.
    const tx = el("div", "text paras");
    String(data.text ?? "").split(/\n/).map((x) => x.trim()).filter(Boolean).forEach((p, i) => {
      tx.append(rich("p", null, p));
      for (const pic of (pkg.pictures ?? []).filter((x) => x.paragraph === i)) tx.append(pictureFigure(pic));
    });
    t.append(tx);
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

// ── pictures between the paragraphs (plan §19.4.1) ─────
// Addresses and credits come with the lesson; the pictures themselves come
// from Wikimedia Commons, shrunk here (≤ 800 px, JPEG) and kept in Cache
// Storage for two weeks (localStorage is far too small for them).
const PIC_CACHE = "jl-pictures";
const PIC_KEEP = 14 * 24 * 3600 * 1000;
function shrink(blob) {
  return new Promise((resolve, reject) => {
    const u = URL.createObjectURL(blob);
    const im = new Image();
    im.onload = () => {
      const k = Math.min(1, 800 / im.naturalWidth);
      const cv = document.createElement("canvas");
      cv.width = Math.round(im.naturalWidth * k); cv.height = Math.round(im.naturalHeight * k);
      const g = cv.getContext("2d");
      g.fillStyle = "#fff"; g.fillRect(0, 0, cv.width, cv.height);
      g.drawImage(im, 0, 0, cv.width, cv.height);
      URL.revokeObjectURL(u);
      cv.toBlob((b) => (b ? resolve(b) : reject(new Error("圖片處理不了"))), "image/jpeg", 0.72);
    };
    im.onerror = () => { URL.revokeObjectURL(u); reject(new Error("圖片讀不了")); };
    im.src = u;
  });
}
async function pictureBlob(url) {
  const c = await caches.open(PIC_CACHE);
  const hit = await c.match(url);
  if (hit) return hit.blob();
  const r = await fetch(url, { mode: "cors" });
  if (!r.ok) throw new Error(`picture ${r.status}`);
  const small = await shrink(await r.blob());
  await c.put(url, new Response(small, { headers: { "Content-Type": "image/jpeg", "X-Saved": String(Date.now()) } }));
  return small;
}
async function prunePictures() {
  try {
    const c = await caches.open(PIC_CACHE);
    for (const req of await c.keys()) {
      const r = await c.match(req);
      if (Date.now() - (Number(r?.headers.get("X-Saved")) || 0) > PIC_KEEP) await c.delete(req);
    }
  } catch { /* no Cache Storage: nothing kept */ }
  // Pictures of the first version lived in localStorage.
  try { Object.keys(localStorage).filter((k) => k.startsWith("jl.pic:")).forEach((k) => localStorage.removeItem(k)); } catch { /* nothing */ }
}
/** A picture with its credit; it leaves no gap when it cannot be shown. */
function pictureFigure(pic) {
  const fig = el("figure", "pic");
  const img = el("img");
  img.alt = pic.caption ?? "";
  const cap = el("figcaption", "note");
  const a = el("a", null, `${pic.author}・${pic.licence}`);
  a.href = pic.page; a.target = "_blank"; a.rel = "noopener";
  cap.append("圖片：", a);
  fig.append(img, cap);
  pictureBlob(pic.url).then((b) => { img.src = URL.createObjectURL(b); }).catch(() => fig.remove());
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

// ── 老師帶著上: the lesson taught out loud (plan §19.4.2) ──
// The same flow as on the computer: the script written there, the teacher
// speaking, the learner tapping to talk (20 s at most, 2.5 s of silence ends
// it) or typing; live replies from Claude with the packed instructions.
const DONT_KNOW_RE = /\b(i\s*don'?t\s*know|no\s*idea|not\s*sure|i\s*forgot|i\s*have\s*no\s*idea)\b|わかりません|わからない|分かりません|分からない|知らない|しらない|不知道|不會|忘了/i;
const dontKnow = (said) => !String(said ?? "").trim() || (DONT_KNOW_RE.test(said) && String(said).trim().split(/\s+/).length <= 5 && String(said).length <= 30);
function saidLetter(said) {
  const t = String(said ?? "").normalize("NFKC").trim().toLowerCase().replace(/[^\p{L}\p{N}\s]/gu, "").replace(/^(option|answer|letter|the answer is|it'?s|its)\s+/, "").trim();
  return { a: "A", ay: "A", eh: "A", b: "B", bee: "B", be: "B", c: "C", see: "C", sea: "C", d: "D", dee: "D" }[t] ?? null;
}
const OUTPUT_KINDS = ["translation", "sentence-making", "cloze", "paraphrase"];
function usesTerm(said, term, lang) {
  const s = String(said ?? "").toLowerCase();
  const t = plain(term).toLowerCase().replace(/[〜~…().,!?]/g, " ").trim();
  if (!t) return false;
  if (lang === "ja") return t.split(/\s+/).every((p) => s.includes(p));
  const parts = t.split(/[\s/+]+/).filter((w) => w.length >= 3 && !["the", "and", "for", "with"].includes(w));
  return (parts.length ? parts : [t]).every((w) => new RegExp(`\\b${w.slice(0, Math.max(3, w.length - 2)).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`, "i").test(s));
}
/** The live reply's user message: the same text the computer builds (script::reply_user). */
function replyUser(a) {
  let s = `Moment: ${a.kind}\nTeacher said: ${a.question.trim()}\n`;
  if (a.kind === "sentence") s += `Target: ${a.target.trim()}\nExample: ${a.example.trim()}\n`;
  return s + `Learner said: ${a.said.trim()}`;
}
function drawGuided(box, pkg, u) {
  const lang = pkg.lang;
  const script = pkg.guided.script;
  const data = pkg.data ?? {};
  const units = pkg.plan.units;
  const out = units.find((x) => OUTPUT_KINDS.includes(x.kind) && !doneOf(keyOf(pkg)).includes(x.index));
  const minutes = units.filter((x) => x === u || x === out).reduce((n, x) => n + (x.minutes ?? 0), 0) || 15;
  const head = el("div", "g-head");
  const stages = el("ol", "g-stages");
  ["閒聊", "今天", "課文", "問答"].forEach((t) => stages.append(el("li", null, t)));
  const clock = el("span", "g-clock");
  head.append(stages, clock);
  const teacher = el("div", "card g-teacher");
  const tText = el("div", "t");
  teacher.append(el("span", "who", "老師"), tText);
  const me = el("div", "g-me");
  const heard = el("div", "heard");
  const ctl = el("div", "row g-ctl");
  me.append(heard, ctl);
  const dock = el("div", "g-dock");
  const card = el("div", "card");
  const tx = el("div", "text paras");
  const paras = [];
  String(data.text ?? "").split(/\n/).map((x) => x.trim()).filter(Boolean).forEach((p, i) => {
    const pe = rich("p", null, p);
    tx.append(pe);
    paras.push(pe);
    for (const pic of (pkg.pictures ?? []).filter((x) => x.paragraph === i)) tx.append(pictureFigure(pic));
  });
  card.append(tx);
  dock.append(head, teacher, me);
  box.append(dock, card);
  document.documentElement.style.setProperty("--top-h", `${$(".top").offsetHeight}px`);
  const ro = window.ResizeObserver ? new ResizeObserver(() => box.style.setProperty("--dock-h", `${dock.offsetHeight}px`)) : null;
  ro?.observe(dock);
  let stopped = false;
  // Leaving this view (‹) ends the lesson: nothing keeps speaking or listening.
  const gone = () => {
    if (head.isConnected && !stopped) return false;
    stopped = true;
    clearInterval(tick); ro?.disconnect();
    synth?.cancel();
    try { currentRec?.abort(); } catch { /* none */ }
    return true;
  };
  const started = Date.now();
  const tick = setInterval(() => {
    if (gone()) return;
    const left = Math.max(0, minutes * 60 - (Date.now() - started) / 1000);
    clock.textContent = left > 0 ? `剩 ${Math.ceil(left / 60)} 分` : "超過時間了";
  }, 1000);
  const stage = (i) => [...stages.children].forEach((li, k) => { li.classList.toggle("on", k === i); li.classList.toggle("done", k < i); });
  let progress = 0;
  const behind = () => (Date.now() - started) / 1000 > minutes * 60 * (0.15 + 0.65 * progress) + 45;
  const say = async (text) => {
    if (gone() || !text) return;
    tText.innerHTML = ruby(text);
    teacher.classList.add("speaking");
    await speak(text, lang);
    teacher.classList.remove("speaking");
  };
  const READY = "\u0000ready";
  const hear = ({ ready = false, placeholder = "" } = {}) => new Promise((resolve) => {
    if (gone()) return resolve(null);
    ctl.innerHTML = ""; heard.textContent = "";
    let settled = false;
    const settle = (v) => {
      if (settled) return;
      settled = true;
      try { currentRec?.stop(); } catch { /* ended */ }
      ctl.innerHTML = "";
      if (v) heard.textContent = v === READY ? "Ready!" : v;
      resolve(v);
    };
    if (Rec) {
      const mic = button(ready ? "🎤 說一聲" : "🎤 按一下說話", "main", async () => {
        if (mic.classList.contains("on")) { try { currentRec?.stop(); } catch { /* ended */ } return; }
        mic.classList.add("on"); mic.textContent = "■ 說完了";
        try {
          const t = await listen(lang, (x) => (heard.textContent = x), { max: 20, silence: 2.5, onLoud: ready ? () => settle(READY) : null });
          settle(t ?? "");
        } catch (e) { toast(e.message, true); mic.classList.remove("on"); mic.textContent = "🎤 再試一次"; }
      });
      ctl.append(mic);
    }
    if (ready) ctl.append(button("Ready!", Rec ? "" : "main", () => settle(READY)));
    else {
      const i = el("input"); i.type = "text"; i.placeholder = placeholder || (Rec ? "或用打的" : "用打的回答");
      if (lang === "ja") i.lang = "ja";
      i.onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); settle(i.value.trim()); } };
      ctl.append(i, button("送出", "", () => settle(i.value.trim())));
    }
    ctl.append(button("跳過", "", () => settle(null)));
  });
  const pick = (l) => l[Math.floor(Math.random() * l.length)] ?? "";
  const live = async (ask) => {
    const r = pkg.guided.reply;
    if (!r?.system || !cfg.apiKey || !navigator.onLine) return null;
    try {
      const res = await Promise.race([claude(r.system, [{ role: "user", content: replyUser(ask) }], r.schema, r.max_tokens ?? 300), new Promise((_, no) => setTimeout(() => no(new Error("slow")), 10000))]);
      return res?.reply ? res : null;
    } catch { return null; }
  };
  const RIGHT = lang === "ja" ? ["そのとおり！", "正解です！", "いいですね！"] : ["That's right!", "Exactly!", "Well done!"];
  const EXAMPLE = lang === "ja" ? "たとえば、" : "For example: ";
  const HINT = lang === "ja" ? "もう一度、本文を見てみましょう。" : "Have another look at the text.";
  const items = [], answers = [];
  const run = async () => {
    stage(0);
    for (const q of [script.small_talk.opening, script.small_talk.follow_up]) {
      await say(q);
      const said = await hear();
      if (said === null) continue;
      const r = said ? await live({ kind: "small_talk", question: q, said }) : null;
      await say(r?.reply ?? pick(script.small_talk.praise));
    }
    await say(script.small_talk.to_lesson);
    stage(1);
    for (const l of script.intro.lines) await say(l);
    await say(script.intro.ready);
    await hear({ ready: true });
    stage(2);
    for (const [i, p] of script.paragraphs.entries()) {
      if (stopped) break;
      progress = i / Math.max(1, script.paragraphs.length);
      paras.forEach((x, k) => x.classList.toggle("g-now", k === i));
      paras[i]?.scrollIntoView({ block: "start", behavior: "smooth" });
      await say(p.read);
      if ((await hear()) !== null) await say(p.after_read);
      for (const ex of p.explain) {
        if (ex.optional && behind()) continue;
        await say(ex.line);
      }
      if (p.sentence?.target) {
        const judge = async (said) => {
          if (!said) return { ok: false, reply: null };
          const r = await live({ kind: "sentence", question: p.sentence.prompt, target: p.sentence.target, example: p.sentence.example, said });
          return r ? { ok: !!r.ok, reply: r.reply } : { ok: usesTerm(said, p.sentence.target, lang), reply: null };
        };
        await say(p.sentence.prompt);
        let said = await hear({ placeholder: p.sentence.target });
        let j = said !== null ? await judge(said) : { ok: false, reply: null };
        if (said !== null && !j.ok) {
          await say(j.reply ?? p.sentence.retry);
          const again = await hear({ placeholder: p.sentence.target });
          if (again) { said = again; j = await judge(said); } else j = { ok: false, reply: null };
        }
        if (j.ok) await say(j.reply ?? p.sentence.praise);
        else if (said !== null) await say(`${EXAMPLE}${p.sentence.example}`);
        if (said !== null) items.push({ prompt: p.sentence.prompt, answer: said ?? "", model: p.sentence.example, correct: j.ok });
      }
    }
    paras.forEach((x) => x.classList.remove("g-now"));
    progress = 1;
    stage(3);
    const qs = data.questions ?? [];
    // The script's lines only when they pair one to one with the lesson's questions.
    const paired = script.quiz.length === qs.length;
    const quiz = (qs.length ? qs : script.quiz).map((lq, k) => {
      const sq = qs.length ? (paired ? script.quiz[k] : null) : lq;
      const opts = lq.kind === "choice" && lq.options?.length ? lq.options : null;
      const ask = opts ? `${plain(lq.question)} ${opts.map((x, i) => `${"ABCD"[i]}: ${plain(x)}.`).join(" ")}` : sq?.question ?? lq.question;
      return { question: ask, answer: lq.answer ?? sq?.answer ?? "", keywords: opts ? [] : sq?.keywords ?? [], hint: sq?.hint || HINT, reveal: sq?.reveal || lq.answer || "", options: opts };
    });
    for (const [k, q] of quiz.entries()) {
      if (stopped) break;
      await say(q.question);
      let said = await hear();
      if (said === null) continue;
      if (dontKnow(said)) { await say(q.hint); said = await hear(); if (said === null) continue; }
      const keys = q.keywords.filter((x) => x.trim());
      const byWords = keys.length ? keys.filter((x) => usesTerm(said, x, lang)).length >= Math.max(1, Math.ceil(keys.length / 2)) : usesTerm(said, q.answer, lang);
      const letter = q.options ? "ABCD"[q.options.indexOf(q.answer)] : null;
      const ok = !dontKnow(said) && (byWords || (!!letter && saidLetter(said) === letter));
      await say(ok ? pick(RIGHT) : q.reveal);
      if (k < qs.length) answers.push({ index: k, answer: said, correct: ok });
    }
    await say(script.goodbye);
    clearInterval(tick);
    // The lesson's own units, sent with the next sync (points on the computer).
    const qn = qs.length;
    if (answers.length >= qn && !doneOf(keyOf(pkg)).includes(u.index)) finishUnit(pkg, u, { answers });
    if (out?.kind === "sentence-making" && items.length) finishUnit(pkg, out, { items });
    endCard(answers.length >= qn ? answers.filter((a) => a.correct).length : null, qn);
  };
  /** The end: how the quiz went, and the next step (or another go when it was not passed). */
  const endCard = (right, qn, problem) => {
    clearInterval(tick); ro?.disconnect();
    stopped = true;
    dock.remove();
    const c = el("div", "card g-end");
    const passed = right != null && (qn === 0 || right / qn >= 0.7);
    c.append(el("b", null, problem ? "這段卡住了" : right == null ? "問答沒有全部答完，課文這一步還沒算" : passed ? `這段上完了！問答答對 ${right} / ${qn} 題` : `差一點：答對 ${right} / ${qn} 題，再上一次就好`));
    if (problem) c.append(el("p", "note", problem));
    const row = el("div", "row");
    const back = () => { stack.pop(); const v = stack[stack.length - 1]; show(v.title, v.draw, false); };
    if (passed) row.append(...[...nextRow(pkg, u).childNodes]);
    else row.append(button("再上一次", "main", () => show("老師帶著上", (b) => drawGuided(b, pkg, u), false)));
    row.append(button("回到這一課", "", back));
    c.append(row);
    box.prepend(c);
    c.scrollIntoView({ block: "start" });
  };
  // Anything going wrong ends with a way out, never a stuck screen.
  run().catch((e) => { console.warn(e); if (head.isConnected) endCard(null, 0, e.message || "等一下再試一次"); });
}

// ── start ───────────────────────────────────────────────
readSetupLink();
if (!cfg) show("設定", drawSettings);
else {
  show("今天", drawHome);
  sync().then(() => { if (stack.length === 1 && stack[0].draw === drawHome) show("今天", drawHome, false); }).catch(() => {});
}
prunePictures();
window.addEventListener("online", () => sync().catch(() => {}));
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") sync().catch(() => {}); });
if ("serviceWorker" in navigator) navigator.serviceWorker.register("sw.js").catch(() => {});
