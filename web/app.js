// JELearner UI.
// Top bar: language (EN / 日本語), import, sync, settings.
// Left: views. Centre: the current view (lessons render on a paper sheet,
// like PaperComposer's page). Right: the study secretary (avatar placeholder
// until the digital-human stage), lesson generation and import.

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
const store = {
  get(k, d) { try { return JSON.parse(localStorage.getItem(k)) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch {} },
};

const state = {
  lang: store.get("jl.lang", "en"),
  view: ["secretary"].includes(store.get("jl.view", "today")) ? "today" : store.get("jl.view", "today"),
  conv: null,         // the open conversation
  status: null,
  picked: null,       // article chosen on the news page
  lessonId: null,     // lesson open on the lessons page
  showMd: false,
  showResolved: false,
  query: "",
};

async function api(path, body, method, signal) {
  const opts = body !== undefined
    ? { method: method ?? "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
    : (method ? { method } : {});
  if (signal) opts.signal = signal;
  const r = await fetch(path, opts);
  const text = await r.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!r.ok) throw new Error(data?.error ?? (typeof data === "string" && data) ?? `${r.status}`);
  // Family mode: whatever the member just saved goes up a little later, on its own.
  if (opts?.method && opts.method !== "GET" && !path.startsWith("/api/family/")) scheduleFamilySync();
  return data;
}

async function apiText(path, text) {
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "text/plain; charset=utf-8" }, body: text });
  const data = await r.json().catch(() => null);
  if (!r.ok) throw new Error(data?.error ?? `${r.status}`);
  return data;
}

const fmtDate = (s) => (s ? new Date(s).toLocaleDateString("zh-TW", { month: "2-digit", day: "2-digit" }) : "");
const fmtDue = (s) => {
  if (!s) return "";
  const d = (new Date(s) - Date.now()) / 86400000;
  return d <= 0 ? "到期" : d < 1 ? "今天" : `${Math.ceil(d)} 天後`;
};
const isJa = () => state.lang === "ja";

// ── furigana ───────────────────────────────────────────
// Japanese lesson texts mark readings as {漢字|かんじ}; they show as <ruby>.
// The reading line can be hidden (remembered per user on this computer).
const escHtml = (t) => String(t ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const RUBY_RE = /\{([^{}|]+)\|([^{}]*)\}/g;
/** `{漢字|かんじ}` → `<ruby>漢字<rt>かんじ</rt></ruby>` (the rest escaped). */
function rubyHtml(text) { return escHtml(text).replace(RUBY_RE, "<ruby>$1<rt>$2</rt></ruby>"); }
/** `{漢字|かんじ}` → `漢字`. */
function plainRuby(text) { return String(text ?? "").replace(RUBY_RE, "$1"); }
/** Text into a node: Japanese with ruby and lang="ja", otherwise plain. */
function setText(node, text, lang) {
  if (lang === "ja") { node.innerHTML = rubyHtml(text); node.lang = "ja"; }
  else node.textContent = plainRuby(text);
  return node;
}
/** el() for lesson content in either language. */
const elr = (tag, cls, text, lang) => setText(el(tag, cls), text, lang);
const furiKey = () => `jl.furigana.${state.status?.user?.id ?? "_"}`;
const furiganaOn = () => store.get(furiKey(), true);
function applyFurigana() { document.documentElement.classList.toggle("hide-rt", !furiganaOn()); }
/** The ふりがな switch (Japanese pages only). */
function furiganaButton() {
  const b = el("button", "ghost furi", "ふりがな");
  b.lang = "ja";
  b.setAttribute("aria-pressed", String(furiganaOn()));
  b.title = "顯示或隱藏假名標音";
  b.onclick = () => { store.set(furiKey(), !furiganaOn()); applyFurigana(); b.setAttribute("aria-pressed", String(furiganaOn())); };
  return b;
}
const langLabel = (l) => ({ en: "英文", ja: "日文" }[l] ?? l);

// ── feedback ───────────────────────────────────────────
let flashTimer;
function flash(msg, err = false) {
  $(".tip.flash")?.remove();
  const t = el("div", `tip flash${err ? " err" : ""}`, msg);
  // An open modal dialog sits in the top layer: anything outside it is dimmed and covered.
  ($("dialog[open]") ?? document.body).append(t);
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => t.remove(), err ? 6000 : 3000);
}

let sayTimer;
function say(msg) {
  $("#say").textContent = msg;
  const a = $("#avatar");
  a.classList.add("talk");
  clearTimeout(sayTimer);
  sayTimer = setTimeout(() => a.classList.remove("talk"), Math.min(4000, 400 + msg.length * 45));
}

async function busy(btn, fn) {
  const old = btn.disabled;
  btn.disabled = true;
  btn.classList.add("busy");
  try { return await fn(); } finally { btn.disabled = old; btn.classList.remove("busy"); }
}

// ── theme ──────────────────────────────────────────────
// Twenty palettes in themes.css: ten dark and ten light, each with five
// single-hue themes and five combinations of three accents. Each theme is
// dark or light by itself. The same ids are in core's THEMES list.
const THEMES = [
  { id: "hacker", name: "駭客終端", dark: true, multi: false, sw: ["#121712", "#16c94d", "#39ff6a", "#a8ffbc"] },
  { id: "amber", name: "琥珀終端", dark: true, multi: false, sw: ["#1c1a16", "#ff8a1f", "#ffb000", "#ffd98a"] },
  { id: "ice", name: "冰晶藍", dark: true, multi: false, sw: ["#172028", "#2fb2e6", "#5ce1ff", "#b8f2ff"] },
  { id: "ultraviolet", name: "紫外線", dark: true, multi: false, sw: ["#1b1530", "#8c5cff", "#ae7aff", "#dcc8ff"] },
  { id: "redalert", name: "紅色警戒", dark: true, multi: false, sw: ["#1d1517", "#e8323f", "#ff4752", "#ffa3a8"] },
  { id: "neon", name: "霓虹夜市", dark: true, multi: true, sw: ["#161731", "#ff3fb4", "#2de2e6", "#ffd23f"] },
  { id: "shibuya", name: "澀谷雨夜", dark: true, multi: true, sw: ["#151c26", "#ff5fa2", "#4fd8ff", "#a98cff"] },
  { id: "hongkong", name: "香港霓虹", dark: true, multi: true, sw: ["#122220", "#ff5147", "#2ee6a6", "#ffc845"] },
  { id: "synthwave", name: "落日合成", dark: true, multi: true, sw: ["#1e1329", "#ff8a3d", "#ff4fa0", "#b691ff"] },
  { id: "aurora", name: "極光", dark: true, multi: true, sw: ["#121b2a", "#4cf5a8", "#46c8ff", "#d58cff"] },
  { id: "lavender", name: "薰衣草", dark: false, multi: false, sw: ["#ebe7f6", "#b7abdb", "#6f4fd0", "#5232ad"] },
  { id: "lemon", name: "檸檬塔", dark: false, multi: false, sw: ["#faf2d2", "#ffdf5e", "#f5c518", "#e9a200"] },
  { id: "mint", name: "薄荷", dark: false, multi: false, sw: ["#e2f3eb", "#9eccb5", "#0b7a5d", "#08604a"] },
  { id: "blueberry", name: "藍莓", dark: false, multi: false, sw: ["#e3e8f6", "#a6b3db", "#3a4bb0", "#2a3888"] },
  { id: "strawberry", name: "草莓", dark: false, multi: false, sw: ["#f9e4ea", "#d9a2b4", "#c8254f", "#9c1a3c"] },
  { id: "giftbox", name: "甜點禮盒", dark: false, multi: true, sw: ["#f8efe4", "#e0457b", "#3ec7a0", "#f5c518"] },
  { id: "peachlavender", name: "蜜桃薰衣草", dark: false, multi: true, sw: ["#fae7da", "#ff8a5c", "#9b7fe6", "#e86a9e"] },
  { id: "sakuramatcha", name: "櫻花抹茶", dark: false, multi: true, sw: ["#f5eaee", "#f08cae", "#8dba4a", "#8c4f42"] },
  { id: "blueberrylemon", name: "藍莓檸檬", dark: false, multi: true, sw: ["#e6ebf7", "#4a5fd0", "#f5c518", "#a462d4"] },
  { id: "caramelpistachio", name: "焦糖開心果", dark: false, multi: true, sw: ["#f2e7d6", "#d68b3a", "#93c572", "#cf6f84"] },
];
const DEFAULT_THEME = "giftbox";
// Ids from earlier versions → the nearest current theme.
const OLD_THEMES = { acid: "hacker", voltage: "ice", plum: "neon", toxic: "amber", pistachio: "mint", peach: "peachlavender", sakura: "sakuramatcha", caramel: "caramelpistachio", taro: "lavender" };
const themeOf = (id) => THEMES.find((t) => t.id === id);

// The browser's choice; ids from earlier versions (and the first version's
// orange/night/terminal + light/dark) map to the nearest theme once.
function savedTheme() {
  let t = store.get("jl.theme", null);
  if (themeOf(t)) return t;
  if (OLD_THEMES[t]) { store.set("jl.theme", OLD_THEMES[t]); return OLD_THEMES[t]; }
  if (t === "orange" || t === "night" || t === "terminal") {
    const light = store.get("jl.mode", matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark") === "light";
    t = { orange: light ? "peachlavender" : "amber", night: light ? "lavender" : "neon", terminal: light ? "mint" : "hacker" }[t];
    store.set("jl.theme", t);
    try { localStorage.removeItem("jl.mode"); } catch {}
    return t;
  }
  return DEFAULT_THEME;
}
// The user's own theme (saved in their profile) wins over the browser's.
const currentTheme = () => {
  const u = state.status?.user?.theme;
  return themeOf(u) ? u : themeOf(OLD_THEMES[u]) ? OLD_THEMES[u] : savedTheme();
};

function showTheme(id) {
  const t = themeOf(id) ?? themeOf(DEFAULT_THEME);
  document.documentElement.dataset.theme = t.id;
  document.documentElement.dataset.mode = t.dark ? "dark" : "light";
}
function applyTheme() {
  const t = currentTheme();
  showTheme(t);
  $$(".theme-card").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.theme === t)));
}
async function chooseTheme(id) {
  store.set("jl.theme", id);
  const u = state.status?.user;
  if (u) {
    u.theme = id;
    try { await api("/api/user", { theme: id }, "PUT"); } catch (e) { flash(e.message, true); }
  }
  applyTheme();
}
// Settings → 外觀: a swatch card per theme (light group, then dark; in each,
// single hue, then combinations); hover or focus previews it.
for (const t of THEMES) {
  const b = el("button", "theme-card");
  b.dataset.theme = t.id;
  b.setAttribute("aria-label", t.name);
  const sw = el("span", "sw");
  for (const c of t.sw) { const i = el("i"); i.style.background = c; sw.append(i); }
  b.append(sw, el("span", "nm", t.name));
  b.onmouseenter = b.onfocus = () => showTheme(t.id);
  b.onmouseleave = b.onblur = () => showTheme(currentTheme());
  b.onclick = () => chooseTheme(t.id);
  $(`#themes${t.dark ? "Dark" : "Light"}${t.multi ? "Multi" : "Mono"}`).append(b);
}

// ── help tips & collapsible blocks ─────────────────────
(() => {
  let tip;
  const hide = () => { tip?.remove(); tip = null; };
  document.addEventListener("mouseover", (e) => {
    const h = e.target.closest?.(".help[data-tip]");
    if (!h) return;
    hide();
    tip = el("div", "tip", h.dataset.tip);
    (h.closest("dialog") ?? document.body).append(tip);
    const r = h.getBoundingClientRect();
    tip.style.top = `${Math.min(r.bottom + 6, innerHeight - tip.offsetHeight - 8)}px`;
    tip.style.left = `${Math.max(8, Math.min(r.left - 8, innerWidth - tip.offsetWidth - 8))}px`;
  });
  document.addEventListener("mouseout", (e) => { if (e.target.closest?.(".help[data-tip]")) hide(); });
})();
$$(".block .toggle").forEach((b) => (b.onclick = () => {
  const blk = b.closest(".block");
  const open = blk.dataset.open !== "true";
  blk.dataset.open = String(open);
  b.textContent = open ? "−" : "+";
  b.setAttribute("aria-expanded", String(open));
}));
$$(".block").forEach((blk) => {
  const t = $(".toggle", blk);
  t.textContent = blk.dataset.open === "true" ? "−" : "+";
  t.setAttribute("aria-expanded", String(blk.dataset.open === "true"));
});

function segment(root, value, onpick) {
  $$("button", root).forEach((b) => {
    b.setAttribute("aria-pressed", String(b.dataset.v === value));
    b.onclick = () => { $$("button", root).forEach((x) => x.setAttribute("aria-pressed", String(x === b))); onpick(b.dataset.v); };
  });
}

// ── status ─────────────────────────────────────────────
async function refreshStatus() {
  try {
    state.status = await api("/api/status");
  } catch (e) {
    flash(e.message, true);
    return;
  }
  const s = state.status;
  applyUser();
  applyFurigana();
  applyFamily();
  // Another user or language: the review queue and selections belong to them.
  const who = `${s.user?.id ?? ""}/${state.lang}`;
  if (state.who !== who) {
    state.who = who;
    resetUserState();
  }
  refreshMe();
  const st = s.stats?.[state.lang] ?? {};
  const set = (id, v, hot) => { const n = $(id); n.textContent = v ? String(v) : ""; n.classList.toggle("hot", !!hot); };
  set("#cDue", st.due, st.due > 0);
  set("#cLessons", st.lessons);
  set("#cWords", st.words);
  set("#cGrammar", st.grammar);
  set("#cMistakes", st.open_mistakes, st.open_mistakes > 0);
  $("#demoChip").hidden = !s.demo;
  paintFoot(s);
  // Level choices for the generator: the steps around the learner's
  // current difficulty (one major level below and above).
  const lv = $("#genLevel");
  lv.innerHTML = "";
  const cur = s.user?.levels?.[state.lang];
  const majors = (s.level_options?.[state.lang] ?? []);
  const at = Math.max(0, majors.findIndex((m) => m.major === cur?.major));
  for (const m of majors.slice(Math.max(0, at - 1), at + 2)) {
    for (let i = 1; i <= m.steps; i++) {
      const label = `${m.major}-${i}`;
      const op = el("option", null, label === cur?.difficulty_label ? `${label}（目前）` : label);
      op.value = label;
      if (label === cur?.difficulty_label) op.selected = true;
      lv.append(op);
    }
  }

  // Status bar: only whether Claude works (details in its tooltip and settings).
  const sb = $("#statusbar");
  sb.innerHTML = "";
  const dot = el("span", `dot ${s.engine_ready ? "ok" : "warn"}`);
  const eng = el("button", "eng", null);
  // Which engine is in use: a local model is not Claude.
  const local = /^llama-server/.test(s.engine ?? "");
  // Conversations on their own engine: both names, e.g. 「Claude・對話用本機模型」.
  const rt = s.realtime_engine ? (s.realtime_engine === "llama-server" ? "本機模型" : "Claude") : null;
  const both = rt && rt !== (local ? "本機模型" : "Claude") ? `・對話用${rt}` : "";
  eng.append(dot, document.createTextNode(s.family?.ready ? (s.engine_ready ? "AI 老師準備好了" : "AI 老師還沒準備好") : `${local ? "本機模型" : "Claude"}${both}${s.engine_ready && (s.realtime_ready ?? true) ? " 已連線" : " 未連線"}`));
  eng.title = s.engine_ready ? `${s.engine_label}・${s.engine}` : (s.engine_error ?? "設定 → Claude");
  eng.onclick = () => openSettings(local ? 4 : 3);
  sb.append(eng, el("span", "grow"));
  if (s.import?.warnings?.length) {
    const w = el("span", null, `匯入警告 ${s.import.warnings.length}`);
    w.title = s.import.warnings.join("\n");
    w.style.color = "var(--warn)";
    sb.append(w);
  }
  if (s.error) flash(s.error, true);
}

/** The left nav's foot: the workspace's name and whether it is synced (path in the tooltip). */
let footGen = 0;
async function paintFoot(s) {
  const f = $("#wsFoot");
  f.innerHTML = "";
  if (!s.workspace) return;
  const name = s.demo ? "示範工作區" : s.workspace_name || (s.workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "私人工作區");
  const ic = el("span", "sync");
  ic.append(icon("cloud"));
  f.append(ic, el("span", "nm", name));
  f.title = s.workspace;
  if (s.demo || s.family?.ready) return; // family mode: no sync state, ever
  const gen = ++footGen;
  try {
    const g = await api("/api/git/status");
    if (gen !== footGen) return;
    const state_ = !g.repo ? "local" : g.changed?.length || g.ahead ? "dirty" : "ok";
    ic.className = `sync ${state_}`;
    f.title = `${s.workspace}\n${{ local: "只在這台電腦（不是 Git repository）", dirty: `${g.changed?.length ?? 0} 個檔案尚未同步`, ok: "已同步" }[state_]}`;
  } catch { /* the name is enough */ }
}

/**
 * The secretary's line: today's next task first (「繼續今天的課：中翻英」),
 * then reviews and mistakes, then a news suggestion.
 */
async function greet() {
  const s = state.status;
  if (!s) return;
  const st = s.stats?.[state.lang] ?? {};
  if (!s.user) return say("請先選擇使用者，或新增一位。");
  const name = s.demo ? "" : `${s.user.name}，`;
  if (s.demo) return say(`${name}歡迎！目前用的是示範資料。到「設定 → 工作區」連接你的私人 repo（例如 JELearner-private），生字和錯題才會存到你自己的地方。`);
  if (!s.engine_ready && s.family?.ready) return say(`${name}Claude 還沒連上，請家人幫你看一下。複習和錯題本可以先用。`);
  if (!s.engine_ready) return say(`${name}你的 Claude 還沒連上。到「設定 → Claude」用瀏覽器登入你自己的 Claude 帳號（用訂閱額度），或貼上 API key。複習和錯題本可以先用。`);
  try {
    const t = await fetchToday();
    const task = t.next != null ? t.tasks[t.next] : null;
    const reached = t.summary && t.summary.goal > 0 && t.summary.today >= t.summary.goal;
    // Today's lesson not made yet: point at 開始今天的課 (stage F3).
    const notMade = task?.daily && t.daily;
    const nextLine = notMade ? "按「開始今天的課」，挑一個今天想學的主題。" : task ? (task.id === "mistakes" ? `還有 ${task.open} 題錯題，訂正一下吧。` : `${reached ? "想多練可以繼續" : "繼續今天的課"}：${taskName(task)}。`) : "";
    if (reached) return say(`${name}今天的目標達成了！${nextLine || "想多練可以再來一段對話。"}`);
    const nudge = await buddyNudge(t.summary?.today ?? 0);
    if (task) return say(`${name}${nudge}${nextLine}`);
  } catch { /* fall back to the numbers below */ }
  const parts = [];
  if (st.due) parts.push(`${st.due} 張${langLabel(state.lang)}卡片要複習`);
  if (st.open_mistakes) parts.push(`${st.open_mistakes} 題錯題還沒訂正`);
  say(parts.length ? `${name}今天有 ${parts.join("、")}。` : `${name}${langLabel(state.lang)}目前沒有待辦。去「新聞」挑一篇做成教材吧。`);
}

// ── language & views ───────────────────────────────────
function setLang(v) {
  state.lang = v; store.set("jl.lang", v);
  state.picked = null; state.lessonId = null;
  Object.assign(state, { course: null, courseLesson: null, courseCreate: null, step: null, nbPage: null, conv: null, fromToday: false, inDaily: false });
  updatePicked();
  refreshStatus().then(() => { greet(); render(); });
}
segment($("#lang"), state.lang, setLang);

// ── the user ───────────────────────────────────────────
// Top bar: who is studying, and only the languages they chose.
function applyUser() {
  const s = state.status;
  const u = s?.user;
  setAvatar($("#userDot"), u);
  $("#userName").textContent = u ? u.name : (s?.layout === "v1" ? "需要轉換工作區" : "選擇使用者");
  const langs = u?.languages ?? ["en", "ja"];
  if (!langs.includes(state.lang)) { state.lang = langs[0]; store.set("jl.lang", state.lang); }
  $$("#lang button").forEach((b) => { b.hidden = !langs.includes(b.dataset.v); b.setAttribute("aria-pressed", String(b.dataset.v === state.lang)); });
  $("#lang").hidden = langs.length < 2;
  applyTheme();
}

const userMenu = $("#userMenu");
$("#actUser").onclick = (e) => {
  e.stopPropagation();
  if (familyLocked()) return; // a family member's computer: one person, nothing to switch
  if (!userMenu.hidden) { userMenu.hidden = true; return; }
  const s = state.status;
  userMenu.innerHTML = "";
  if (s?.layout === "v1") {
    const b = el("button", null, "轉成多使用者結構…");
    b.onclick = () => { userMenu.hidden = true; startWizard("migrate"); };
    userMenu.append(b);
  } else {
    for (const u of s?.users ?? []) {
      const b = el("button");
      const dot = el("span", "dot"); setAvatar(dot, u);
      b.append(dot, document.createTextNode(u.name));
      if (u.admin) b.append(el("span", "tag", "管理員"));
      if (u.id === s.user?.id) b.setAttribute("aria-current", "true");
      b.onclick = () => { userMenu.hidden = true; switchUser(u.id); };
      userMenu.append(b);
    }
    userMenu.append(el("div", "sep"));
    const add = el("button", null, "新增使用者…");
    add.onclick = () => { userMenu.hidden = true; startWizard("add"); };
    userMenu.append(add);
    if (s?.user) {
      const me = el("button", null, "我的設定…");
      me.onclick = () => { userMenu.hidden = true; openSettings(1); };
      userMenu.append(me);
    }
  }
  const r = $("#actUser").getBoundingClientRect();
  userMenu.style.left = `${r.left}px`;
  userMenu.style.top = `${r.bottom + 4}px`;
  userMenu.hidden = false;
};
document.addEventListener("click", (e) => { if (!userMenu.hidden && !userMenu.contains(e.target)) userMenu.hidden = true; });
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (!userMenu.hidden) { userMenu.hidden = true; $("#actUser").focus(); }
  document.body.classList.remove("aside-open", "nav-open");
});

// Narrow windows: the right panel and the left nav become drawers.
$("#actAside").onclick = (e) => { e.stopPropagation(); document.body.classList.toggle("aside-open"); document.body.classList.remove("nav-open"); };
$("#actNav").onclick = (e) => { e.stopPropagation(); document.body.classList.toggle("nav-open"); document.body.classList.remove("aside-open"); };
document.addEventListener("click", (e) => {
  if (e.target.closest(".right, .left, #actAside, #actNav, dialog")) return;
  document.body.classList.remove("aside-open", "nav-open");
});

/**
 * Forget everything that belongs to the previous user (or language): the open
 * conversation, the review queue, the picked article, the open lesson and
 * the today page mode. Called on every user switch and new user, and by
 * refreshStatus whenever the user or language changes.
 */
function resetUserState() {
  state.conv = null;
  state.picked = null;
  state.lessonId = null;
  state.showMd = false;
  state.query = "";
  state.summary = null;
  Object.assign(state, { course: null, courseLesson: null, courseCreate: null, step: null, nbPage: null, fromToday: false, inDaily: false, cl: null });
  if (typeof todayCache !== "undefined") todayCache = null;
  if (typeof convTimer !== "undefined") clearInterval(convTimer);
  if (typeof skipTopics !== "undefined") skipTopics = [];
  updatePicked();
  const out = $("#genOut"); out.textContent = ""; out.title = "";
}

async function switchUser(id) {
  try {
    await api("/api/users/select", { id });
    resetUserState();
    await refreshStatus();
    greet();
    render();
  } catch (e) { flash(e.message, true); }
}

// Language pickers (wizard and settings): a checkbox per language and the
// self-assessed major level with a one-line description.
function langPicker(box, user, showProgress) {
  box.innerHTML = "";
  const rows = {};
  for (const code of ["en", "ja"]) {
    const row = el("div", "lang-row");
    const on = el("input"); on.type = "checkbox"; on.checked = user ? (user.languages ?? []).includes(code) : code === "en";
    const name = el("label", "lang-name"); name.append(on, document.createTextNode(code === "en" ? "英文（CEFR）" : "日文（JLPT）"));
    const sel = el("select");
    const hint = el("div", "note");
    const cur = user?.levels?.[code];
    for (const m of state.status?.level_options?.[code] ?? []) {
      const o = el("option", null, `${m.major}　${m.hint}`); o.value = m.major;
      if (cur ? m.major === cur.major : m.major === (code === "en" ? "B1" : "N5")) o.selected = true;
      sel.append(o);
    }
    if (showProgress && cur) hint.textContent = `等級 ${cur.level}・教材難度 ${cur.difficulty_label}（${cur.steps} 小階制）`;
    sel.disabled = !on.checked;
    on.onchange = () => (sel.disabled = !on.checked);
    row.append(name, sel, hint);
    box.append(row);
    rows[code] = { on, sel };
  }
  return () => {
    const languages = Object.keys(rows).filter((c) => rows[c].on.checked);
    const majors = Object.fromEntries(languages.map((c) => [c, rows[c].sel.value]));
    return { languages, majors };
  };
}

// The nav opens a view at its start (list, not the last opened item).
$$("#views button").forEach((b) => (b.onclick = () => { go(b.dataset.view, { course: null, courseLesson: null, courseCreate: null, step: null, nbPage: null, lessonId: null, conv: state.view === "conversation" ? state.conv : null }); }));

/** Open a view (nav, task cards, next-step buttons). */
function go(view, extra = {}) {
  state.view = view; store.set("jl.view", view);
  state.query = "";
  // A lesson opened from today (or a daily lesson) keeps 今日 lit in the nav.
  state.fromToday = false; state.inDaily = false; state.quickReview = false;
  Object.assign(state, extra);
  document.body.classList.remove("nav-open");
  render();
}

// The view bar's height (it wraps on narrow screens): sticky bars below it use it.
(() => {
  const vb = document.querySelector("#viewbar");
  if (!vb || !window.ResizeObserver) return;
  new ResizeObserver(() => document.documentElement.style.setProperty("--vb-h", `${vb.offsetHeight}px`)).observe(vb);
})();

function viewbar(title, count, ...extra) {
  const vb = $("#viewbar");
  vb.innerHTML = "";
  const h = el("h1", null, title);
  if (count != null) h.append(el("span", "c", String(count)));
  vb.append(h, el("span", "grow"), ...extra);
}

function searchBox(placeholder, onchange) {
  const i = el("input");
  i.type = "search"; i.placeholder = placeholder; i.value = state.query;
  let t;
  i.oninput = () => { clearTimeout(t); t = setTimeout(() => { state.query = i.value; onchange(); }, 250); };
  return i;
}

function ghostButton(iconName, label, onclick) {
  const b = el("button", "ghost");
  b.append(icon(iconName), document.createTextNode(label));
  b.onclick = onclick;
  return b;
}

// Each render draws into its own detached container; only the latest call
// puts it on the page, so overlapping calls never draw a page twice.
let renderGen = 0;
// A page that waits a long time (a lesson the model is writing) gives its
// request this signal: leaving the page lets go of the connection, so the
// browser's few connections never all wait and the next page can load.
let renderCtl = null;
/** Light the nav item of the current view; a daily lesson belongs to 今日. */
function markNav() {
  const cur = state.view === "courses" && (state.courseLesson || state.quickReview) && state.inDaily ? "today" : state.view;
  $$("#views button").forEach((b) => (b.getAttribute("data-view") === cur ? b.setAttribute("aria-current", "page") : b.removeAttribute("aria-current")));
}

/** A small ⋯ button whose tooltip holds technical details (file path, engine). */
function infoButton(lines) {
  const b = el("button", "help info", "⋯");
  b.type = "button";
  b.setAttribute("aria-label", "詳細資訊");
  b.dataset.tip = lines.filter(Boolean).join("\n");
  return b;
}

async function render() {
  const gen = ++renderGen;
  renderCtl?.abort();
  renderCtl = new AbortController();
  state.signal = renderCtl.signal;
  markNav();
  Speech.stop();
  Speech.stopListening();
  const body = el("div", "page-body");
  // A page that will wait shows its waiting state now (it is drawn off-screen otherwise).
  state.paintNow = () => { if (gen === renderGen) $("#page").replaceChildren(body); };
  try {
    await ({ today: renderToday, courses: renderCourses, conversation: renderConversation, news: renderNews, lessons: renderLessons, words: renderWords, grammar: renderGrammar,
      mistakes: renderMistakes, notebook: renderNotebook, materials: renderMaterials, progress: renderProgress }[state.view] ?? renderToday)(body);
  } catch (e) {
    body.append(el("div", "empty", e.message));
  }
  if (gen !== renderGen) return;
  $("#page").replaceChildren(body);
  Speech.refreshButtons(body);
  $("#pageArea").scrollTop = 0;
}

// ── news ───────────────────────────────────────────────
async function renderNews(page, refresh = false) {
  const btn = ghostButton("refresh", "抓最新新聞", async () => {
    if (state.newsBusy) return;
    await busy(btn, async () => { page.innerHTML = ""; await renderNews(page, true); });
  });
  viewbar(`新聞・${langLabel(state.lang)}`, null, btn);
  let wait = null;
  btn.disabled = refresh;
  state.newsBusy = refresh;
  if (refresh) {
    wait = el("div", "lesson-wait");
    const sk = el("div", "skeleton");
    for (let i = 0; i < 4; i++) sk.append(el("i"));
    const n = el("div", "note", "正在抓最新新聞…");
    n.setAttribute("role", "status");
    wait.append(sk, n);
    page.append(wait);
  }
  let data;
  try { data = await api(`/api/news?lang=${state.lang}${refresh ? "&refresh=true" : ""}`); } finally { wait?.remove(); btn.disabled = false; state.newsBusy = false; }
  if (refresh) {
    if (data.errors?.length) flash(data.errors.join("；"), true);
    else flash(`取得 ${data.fetched} 則`);
  }
  if (!data.articles.length) {
    const e = el("div", "empty cta", "還沒有新聞");
    e.append(primaryButton("抓最新新聞", () => btn.click()));
    page.append(e);
    return;
  }
  const ul = el("ul", "list");
  for (const a of data.articles) {
    const li = el("li", `item${a.blocked ? " blocked" : " click"}${state.picked?.id === a.id ? " on" : ""}`);
    const t = el("div", `t${a.lang === "ja" ? " ja" : ""}`, a.title || a.url);
    if (a.lang === "ja") t.lang = "ja";
    if (a.blocked) {
      const tag = el("span", "tag", "無法擷取");
      tag.title = "這個網站的 robots.txt 不允許擷取，無法做成教材";
      t.append(" ", tag);
      li.setAttribute("aria-disabled", "true");
    }
    li.append(t);
    if (a.summary) li.append(el("div", `sub${a.lang === "ja" ? " ja" : ""}`, a.summary.slice(0, 220)));
    const meta = el("div", "meta");
    meta.append(el("span", null, a.source), el("span", null, fmtDate(a.published_at ?? a.fetched_at)));
    li.append(meta);
    if (!a.blocked) {
      // Two ways to use it: a lesson now, or a topic course around it.
      const acts = el("div", "news-acts");
      const mk = el("button", "ghost small", "做成教材");
      mk.onclick = (e) => {
        e.stopPropagation();
        state.picked = a; updatePicked(); setGenMode("news");
        $$(".item.on", ul).forEach((x) => x.classList.remove("on")); li.classList.add("on");
        mk.disabled = true; mk.textContent = "整理中…";
        flash("整理中…（約 30 秒到 2 分鐘）");
        $("#genRun").click();
      };
      const tc = el("button", "ghost small", "做成主題課程");
      tc.onclick = (e) => { e.stopPropagation(); go("courses", { courseCreate: "topic", course: null, courseLesson: null, topicPrefill: a.url.startsWith("http") ? a.url : a.title }); };
      acts.append(mk, tc);
      li.append(acts);
    }
    const side = el("div", "side");
    const open = el("a", "ico");
    open.href = a.url.startsWith("http") ? a.url : "#";
    open.target = "_blank"; open.rel = "noopener noreferrer";
    open.setAttribute("aria-label", "開啟原文");
    open.append(icon("external"));
    open.onclick = (e) => e.stopPropagation();
    if (a.url.startsWith("http")) side.append(open);
    li.append(side);
    li.onclick = () => {
      if (a.blocked) return;
      state.picked = a;
      $$(".item.on", ul).forEach((x) => x.classList.remove("on"));
      li.classList.add("on");
      setGenMode("news");
      updatePicked();
      say(`選好了「${a.title.slice(0, 40)}」。按「做成教材」，我會整理單字、文法和問答。`);
    };
    ul.append(li);
  }
  page.append(ul);
}

// ── lessons ────────────────────────────────────────────
function lessonItem(l) {
  const li = el("li", "item click");
  li.append(el("div", `t${l.lang === "ja" ? " ja" : ""}`, l.title));
  const meta = el("div", "meta");
  meta.append(el("span", null, l.level), el("span", null, fmtDate(l.created_at)));
  li.append(meta);
  li.title = l.path;
  li.onclick = () => { state.view = "lessons"; state.lessonId = l.id; render(); };
  return li;
}

async function renderLessons(page) {
  if (state.lessonId) return renderLesson(page, state.lessonId);
  const list = await api(`/api/lessons?lang=${state.lang}`);
  viewbar(`教材・${langLabel(state.lang)}`, list.length);
  if (!list.length) {
    const e = el("div", "empty cta", "還沒有教材");
    e.append(primaryButton("從新聞做一份", () => go("news")));
    page.append(e);
    return;
  }
  const ul = el("ul", "list");
  for (const l of list) ul.append(lessonItem(l));
  page.append(ul);
}

async function renderLesson(page, id) {
  const { lesson, row } = await api(`/api/lessons/${id}`);
  const back = ghostButton("back", "教材列表", () => { state.lessonId = null; render(); });
  const md = ghostButton("code", state.showMd ? "排版" : "Markdown", () => { state.showMd = !state.showMd; render(); });
  const extra = [back, md];
  if (row.lang === "ja") extra.unshift(furiganaButton());
  if (row.course) extra.unshift(ghostButton("layers", "課程", () => go("courses", { course: row.course, courseLesson: row.course_lesson, step: null })));
  viewbar(plainRuby(row.title), null, ...extra);
  const paper = el("div", "paper");
  paper.lang = row.lang;
  page.append(paper);
  if (state.showMd) {
    paper.append(el("pre", "md", row.markdown.replace(/\n*<!-- jelearner:lesson [\s\S]*$/, "\n")));
    return;
  }
  // When every question has a verdict, the quiz is finished: result card.
  const quiz = { id, lang: row.lang, total: lesson.questions.length, answers: {}, done: false, after: paper };
  lessonPaper(paper, lesson, row, { quiz });
  startReading();
}

/**
 * A lesson on the paper sheet: title, meta, summary, today's text (course
 * lessons), words, grammar and questions. Japanese shows {漢字|かんじ} as ruby.
 * `parts` limits what is drawn (e.g. ["text", "questions"]).
 */
function lessonPaper(paper, lesson, row, { quiz, parts, highlight } = {}) {
  const lang = row.lang;
  const want = (p) => !parts || parts.includes(p);
  if (want("head")) {
    paper.append(elr("h1", null, lesson.title, lang));
    const meta = el("div", "meta");
    meta.append(el("span", null, `${langLabel(lang)}・${row.level}`), el("span", null, fmtDate(row.created_at)));
    if (row.source_url) { const a = el("a", null, "原文"); a.href = row.source_url; a.target = "_blank"; a.rel = "noopener noreferrer"; meta.append(a); }
    // Engine and file path are details: behind the ⋯ button.
    meta.append(infoButton([row.path, row.engine ? `產生：${row.engine}` : ""]));
    paper.append(meta);
    if (lesson.summary) paper.append(el("div", "summary", lesson.summary));
  }
  const h2 = (t, n) => { const h = el("h2", null, t); if (n != null) h.append(el("span", "c", String(n))); return h; };
  if (want("text") && lesson.text) {
    paper.append(h2("今日課文"));
    const tx = el("div", "lesson-text");
    for (const para of lesson.text.split(/\n{2,}|\n/).filter((x) => x.trim())) {
      const pEl = elr("p", null, para, lang);
      if (highlight?.length) markWords(pEl, highlight, lang);
      // Read aloud; shadowing when recognition is on (speech.js).
      const tools = el("span", "say-tools");
      tools.append(Speech.speakerButton(para, lang), Speech.shadowButton(para, lang, pEl));
      pEl.append(tools);
      tx.append(pEl);
    }
    paper.append(tx);
  }
  if (want("words") && lesson.vocabulary?.length) {
    paper.append(h2("單字", lesson.vocabulary.length));
    const tb = el("table", "vocab");
    tb.innerHTML = "<thead><tr><th>單字</th><th>讀音</th><th>意思與例句</th></tr></thead>";
    const body = el("tbody");
    for (const v of lesson.vocabulary) {
      const tr = el("tr");
      const term = el("td", "term");
      term.textContent = plainRuby(v.term);
      if (lang === "ja") term.lang = "ja";
      term.append(Speech.speakerButton(v.term, lang));
      tr.append(term, el("td", "reading", v.reading));
      const td = el("td");
      td.append(el("div", null, `${v.part_of_speech ? `〔${v.part_of_speech}〕` : ""}${v.meaning}`));
      const ex = elr("div", "ex", v.example, lang);
      if (v.example) ex.append(Speech.speakerButton(v.example, lang));
      ex.append(el("div", "tr", v.example_translation));
      td.append(ex);
      tr.append(td);
      body.append(tr);
    }
    tb.append(body);
    paper.append(tb);
  }
  if (want("grammar") && lesson.grammar?.length) {
    paper.append(h2("文法", lesson.grammar.length));
    lesson.grammar.forEach((g, i) => {
      const d = el("div", "gram");
      const h = elr("h3", null, `${i + 1}. ${g.pattern}`, lang);
      h.append(el("span", "m", g.meaning));
      d.append(h, el("p", null, g.explanation));
      const ex = elr("p", "ex", g.example, lang);
      if (g.example) ex.append(Speech.speakerButton(g.example, lang));
      ex.append(el("div", "tr", g.example_translation));
      d.append(ex);
      paper.append(d);
    });
  }
  if (want("questions") && lesson.questions?.length) {
    paper.append(h2("問答", lesson.questions.length));
    const lessonId = quiz?.id;
    lesson.questions.forEach((q, i) => paper.append(questionCard(lessonId, q, i, quiz, lang)));
  }
}

/** Underline the lesson's target words in a paragraph (English: word starts, so inflections count). */
function markWords(node, terms, lang) {
  const list = [...new Set(terms.map((t) => plainRuby(t).trim()).filter((t) => t.length > 1))].sort((a, b) => b.length - a.length);
  if (!list.length) return;
  const esc = (t) => escHtml(t).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = lang === "ja"
    ? new RegExp(`(${list.map(esc).join("|")})`, "g")
    : new RegExp(`\\b(${list.map((t) => esc(t.length >= 5 ? t.slice(0, -1) : t)).join("|")})(\\w*)`, "gi");
  // Only text outside tags and outside readings.
  node.innerHTML = node.innerHTML.split(/(<rt>[^<]*<\/rt>|<[^>]+>)/).map((part) => (part.startsWith("<") ? part : part.replace(re, (m) => `<mark class="tw">${m}</mark>`))).join("");
}

function questionCard(lessonId, q, index, quiz, lang = state.lang) {
  const box = el("div", "q");
  const qt = elr("div", "qt", `${index + 1}. ${q.question}`, lang);
  box.append(qt);
  // The question is in the language studied; its Chinese is one tap away.
  if (q.question_translation) {
    const tr = el("div", "q-tr", q.question_translation);
    tr.hidden = true;
    const hint = el("button", "q-hint", "中");
    hint.setAttribute("aria-label", "看中文");
    hint.setAttribute("aria-expanded", "false");
    hint.onclick = () => {
      tr.hidden = !tr.hidden;
      hint.classList.toggle("on", !tr.hidden);
      hint.setAttribute("aria-expanded", String(!tr.hidden));
    };
    qt.append(hint);
    box.append(tr);
  }
  const fb = el("div", "fb");
  const showFeedback = (correct, answer) => {
    if (quiz && correct !== null && correct !== undefined) quizAnswered(quiz, index, answer, correct);
    fb.innerHTML = "";
    const v = el("b", null, correct === true ? "答對 " : correct === false ? "答錯（已記入錯題本） " : "參考答案 ");
    fb.append(v, elr("span", null, q.answer, lang), document.createTextNode(`　${q.explanation}`));
  };
  const send = async (answer, correct) => {
    const res = await api(`/api/lessons/${lessonId}/answers`, { answers: [{ index, answer, correct }] });
    return res.results[0];
  };
  if (q.kind === "choice") {
    const opts = el("div", "opts");
    q.options.forEach((o, k) => {
      const b = elr("button", "opt", `${"ABCDEFG"[k]}. ${o}`, lang);
      b.dataset.opt = o;
      b.onclick = async () => {
        try {
          const r = await send(o, null);
          $$(".opt", opts).forEach((x) => (x.disabled = true));
          b.classList.add(r.correct ? "right" : "wrong");
          if (!r.correct) $$(".opt", opts).find((x) => x.dataset.opt === q.answer)?.classList.add("right");
          showFeedback(r.correct, o);
          if (!r.correct) refreshStatus();
        } catch (e) { flash(e.message, true); }
      };
      opts.append(b);
    });
    box.append(opts);
  } else {
    const ta = el("textarea");
    ta.rows = 2;
    const row = el("div", "row");
    const check = el("button", "ghost", "對答案");
    check.onclick = async () => {
      try {
        const r = await send(ta.value, null);
        showFeedback(r.correct, ta.value);
        if (r.correct === null) {
          // The learner judges a free answer against the model answer.
          const ok = el("button", "ghost ok", "✓ 對了");
          const no = el("button", "ghost no", "✗ 再加油");
          ok.onclick = () => { showFeedback(true, ta.value); ok.remove(); no.remove(); };
          no.onclick = async () => { await send(ta.value, false); showFeedback(false, ta.value); ok.remove(); no.remove(); refreshStatus(); };
          row.append(ok, no);
          ok.focus({ preventScroll: true });
        }
        check.remove();
      } catch (e) { flash(e.message, true); }
    };
    row.append(check);
    box.append(ta, row);
  }
  box.append(fb);
  return box;
}

// ── words & grammar ────────────────────────────────────
async function renderWords(page) {
  const list = await api(`/api/words?lang=${state.lang}&q=${encodeURIComponent(state.query)}`);
  viewbar(`生字・${langLabel(state.lang)}`, list.length, searchBox("搜尋單字、意思、標籤", () => render()));
  const t = el("table", "grid words");
  t.innerHTML = "<thead><tr><th>單字</th><th>讀音</th><th>意思</th><th>例句</th><th>複習</th><th>來源</th><th></th></tr></thead>";
  const tb = el("tbody");
  tb.append(addRow(["term", "reading", "meaning", "example"], ["新單字", isJa() ? "よみ" : "/ipa/", "意思", "例句"], async (v) => {
    await api("/api/words", { lang: state.lang, ...v });
    flash(`已加入，並寫入 vocab/my-words-${state.lang}.csv`);
    await refreshStatus(); render();
  }, 7));
  for (const w of list) {
    const tr = el("tr");
    const ja = w.lang === "ja" ? " ja" : "";
    const term = el("td", `term${ja}`, w.term);
    term.append(Speech.speakerButton(w.term, w.lang));
    tr.append(term, el("td", "mono", w.reading), el("td", null, w.meaning), el("td", ja.trim(), w.example),
      el("td", "mono", w.reps ? `${fmtDue(w.due_at)}・${w.reps} 次` : fmtDue(w.due_at)), sourceCell(w.source));
    const td = el("td");
    td.append(delButton(async () => {
      const r = await api(`/api/words/${w.id}`, undefined, "DELETE");
      flash(r.removed_from ? `已刪除（也從 ${r.removed_from} 移除）` : `已從資料庫刪除；若要永久刪除，請編輯 ${r.source}`);
      await refreshStatus(); render();
    }));
    tr.append(td);
    tb.append(tr);
  }
  t.append(tb);
  page.append(t);
}

async function renderGrammar(page) {
  const list = await api(`/api/grammar?lang=${state.lang}&q=${encodeURIComponent(state.query)}`);
  viewbar(`文法・${langLabel(state.lang)}`, list.length, searchBox("搜尋文法、說明", () => render()));
  const t = el("table", "grid");
  t.innerHTML = "<thead><tr><th>文法</th><th>意思</th><th>說明</th><th>例句</th><th>程度</th><th>複習</th><th></th></tr></thead>";
  const tb = el("tbody");
  tb.append(addRow(["pattern", "meaning", "explanation", "example", "level"], ["新文法", "意思", "說明", "例句", isJa() ? "N3" : "B2"], async (v) => {
    await api("/api/grammar", { lang: state.lang, ...v });
    flash(`已加入，並寫入 grammar/my-grammar-${state.lang}.csv`);
    await refreshStatus(); render();
  }, 7));
  for (const g of list) {
    const tr = el("tr");
    const ja = g.lang === "ja" ? " ja" : "";
    tr.append(el("td", `term${ja}`, g.pattern), el("td", null, g.meaning), el("td", null, g.explanation), el("td", ja.trim(), g.example),
      el("td", "mono", g.level), el("td", "mono", fmtDue(g.due_at)));
    const td = el("td");
    td.append(delButton(async () => {
      const r = await api(`/api/grammar/${g.id}`, undefined, "DELETE");
      flash(r.removed_from ? `已刪除（也從 ${r.removed_from} 移除）` : `已從資料庫刪除；若要永久刪除，請編輯 ${r.source}`);
      await refreshStatus(); render();
    }));
    tr.append(td);
    tb.append(tr);
  }
  t.append(tb);
  page.append(t);
}

/** 來源: 課 for a lesson's word, the list's name for an imported one; the path in the tooltip. */
function sourceCell(source) {
  const s = String(source ?? "");
  const file = s.split("/").pop().replace(/\.(csv|md|json)$/i, "");
  const td = el("td", "src", /\/lessons\//.test(s) ? "課" : /\/materials\//.test(s) ? "素材" : file);
  td.title = s;
  return td;
}

function addRow(fields, placeholders, onsave, cols) {
  const tr = el("tr", "add");
  const inputs = {};
  fields.forEach((f, i) => {
    const td = el("td");
    const inp = el("input");
    inp.placeholder = placeholders[i];
    inputs[f] = inp;
    td.append(inp);
    tr.append(td);
  });
  for (let i = fields.length; i < cols - 1; i++) tr.append(el("td"));
  const td = el("td");
  const b = el("button", "ico");
  b.setAttribute("aria-label", "加入");
  b.append(icon("plus"));
  const save = async () => {
    const v = Object.fromEntries(Object.entries(inputs).map(([k, i]) => [k, i.value.trim()]));
    if (!v[fields[0]]) return inputs[fields[0]].focus();
    try { await onsave(v); } catch (e) { flash(e.message, true); }
  };
  b.onclick = save;
  Object.values(inputs).forEach((i) => (i.onkeydown = (e) => { if (e.key === "Enter") save(); }));
  td.append(b);
  tr.append(td);
  return tr;
}

function delButton(fn) {
  const b = el("button", "ico del");
  b.setAttribute("aria-label", "刪除");
  b.append(icon("trash"));
  b.onclick = async () => { try { await fn(); } catch (e) { flash(e.message, true); } };
  return b;
}

// ── mistakes ───────────────────────────────────────────
async function renderMistakes(page) {
  const list = await api(`/api/mistakes?lang=${state.lang}${state.showResolved ? "&all=true" : ""}`);
  const toggle = ghostButton("eye", state.showResolved ? "隱藏已訂正" : "顯示已訂正", () => { state.showResolved = !state.showResolved; render(); });
  viewbar(`錯題本・${langLabel(state.lang)}`, list.length, toggle);
  if (!list.length) {
    const e = el("div", "empty cta", state.showResolved ? "還沒有錯題" : "錯題都訂正了");
    e.append(primaryButton("回到今日", () => go("today")));
    page.append(e);
    return;
  }
  const ul = el("ul", "list");
  for (const m of list) {
    const li = el("li", "item");
    li.append(el("div", `t${isJa() ? " ja" : ""}`, m.question));
    const sub = el("div", "sub");
    sub.innerHTML = "";
    sub.append(el("span", "bad", `你的答案：${m.user_answer || "（空白）"}`), el("br"), el("span", "ok", `正確答案：${m.correct_answer}`));
    if (m.explanation) sub.append(el("br"), document.createTextNode(m.explanation));
    li.append(sub);
    const meta = el("div", "meta");
    meta.append(el("span", null, fmtDate(m.created_at)));
    if (m.resolved_at) meta.append(el("span", "tag ok", "已訂正"));
    li.append(meta);
    const side = el("div", "side");
    if (m.lesson_id) {
      const go = el("button", "ico"); go.setAttribute("aria-label", "回到教材"); go.append(icon("book"));
      go.onclick = () => { state.view = "lessons"; state.lessonId = m.lesson_id; render(); };
      side.append(go);
    }
    if (!m.resolved_at) {
      const ok = el("button", "ghost small"); ok.append(icon("check"), document.createTextNode("懂了"));
      ok.title = "標成已訂正";
      ok.onclick = async () => { await api(`/api/mistakes/${m.id}/resolve`, {}); todayCache = null; await refreshStatus(); greet(); render(); };
      side.append(ok);
    }
    li.append(side);
    ul.append(li);
  }
  page.append(ul);
}

// ── materials ──────────────────────────────────────────
async function renderMaterials(page) {
  const list = await api("/api/materials");
  const up = ghostButton("upload", "匯入", () => openImport());
  viewbar("每週素材", list.length, up);
  const fmt = el("details", "fmt card");
  fmt.append(el("summary", null, "JSON 格式"));
  fmt.append(el("pre", null, JSON.stringify({
    week: "2026-W45", title: "本週素材", language: "ja",
    items: [
      { kind: "word", term: "規制", reading: "きせい", meaning: "管制" },
      { kind: "grammar", pattern: "〜をめぐって", meaning: "圍繞著…", example: "…" },
      { kind: "article", title: "短文", url: "", text: "本文…" },
      { kind: "note", text: "自己的備註" },
    ],
  }, null, 2)));
  page.append(fmt);
  if (!list.length) { page.append(emptyImport("還沒有素材")); return; }
  const ul = el("ul", "list");
  for (const m of list) {
    const li = el("li", "item");
    li.append(el("div", "t", `${m.week}　${m.title || ""}`));
    const meta = el("div", "meta");
    meta.append(el("span", null, langLabel(m.lang)), el("span", null, `${m.items} 項`), el("span", null, m.path));
    li.append(meta);
    ul.append(li);
  }
  page.append(ul);
  page.append(el("div", "note", "素材裡的文章會出現在「新聞」頁（來源標為 materials），可以直接做成教材。"));
}

// ── mail & calendar (settings → 信件與行事曆) ─────────────
async function renderMail(page) {
  page.innerHTML = "";
  const render = () => renderMail(page);
  const g = await api("/api/google");
  const mock = g.mail === "mock";
  const info = el("div", "card");
  info.append(el("h3", null, mock ? "目前是模擬模式" : "已連線 Google"));
  info.append(el("div", "note", mock
    ? "Gmail 與 Google 日曆還沒串接（OAuth 在下一階段），信件和行程會放在下面的「模擬寄件匣」，格式與真正寄出的一樣。"
    : "信件從你的 Gmail 寄出，行程寫入你的 Google 日曆。"));
  info.append(el("div", "note", `週報信箱：${g.report_email ?? "未設定（設定 → 使用者）"}`));
  page.append(info);

  const rep = el("div", "card");
  rep.append(el("h3", null, "學習週報"));
  rep.append(el("div", "note", "過去 7 天：新單字、複習答對率、錯題與下週建議。"));
  const repBtn = el("button", "primary small", "寄出週報");
  repBtn.onclick = () => busy(repBtn, async () => {
    try { const r = await api("/api/google/weekly-report", {}); flash(`已寄出（${r.mode === "mock" ? "模擬" : "Gmail"}）`); render(); }
    catch (e) { flash(e.message, true); }
  });
  rep.append(repBtn);
  page.append(rep);

  const sch = el("div", "card");
  sch.append(el("h3", null, "把複習排進行事曆"));
  const fg = el("div", "form-grid");
  const days = el("input"); days.type = "number"; days.value = 7; days.min = 1; days.max = 31;
  const at = el("input"); at.type = "time"; at.value = "12:00";
  fg.append(el("span", null, "未來幾天"), days, el("span", null, "時間（UTC）"), at);
  const schBtn = el("button", "primary small", "排入行事曆");
  schBtn.onclick = () => busy(schBtn, async () => {
    try {
      const r = await api("/api/google/schedule-reviews", { days: Number(days.value), at_utc: at.value });
      flash(r.created.length ? `已排 ${r.created.length} 個複習時段` : "這段時間沒有到期的卡片");
      render();
    } catch (e) { flash(e.message, true); }
  });
  sch.append(fg, schBtn);
  page.append(sch);

  const bk = el("div", "card");
  bk.append(el("h3", null, "排一堂課（並寄確認信）"));
  const fg2 = el("div", "form-grid");
  const topic = el("input"); topic.placeholder = "例如：NHK 新聞討論";
  const start = el("input"); start.type = "datetime-local";
  const d = new Date(Date.now() + 86400000); d.setMinutes(0, 0, 0);
  start.value = new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  const mins = el("input"); mins.type = "number"; mins.value = 30; mins.min = 10; mins.max = 240;
  fg2.append(el("span", null, "主題"), topic, el("span", null, "開始"), start, el("span", null, "分鐘"), mins);
  const bkBtn = el("button", "primary small", "排課");
  bkBtn.onclick = () => busy(bkBtn, async () => {
    try {
      const r = await api("/api/google/book-lesson", { topic: topic.value, start: new Date(start.value).toISOString(), minutes: Number(mins.value), lang: state.lang });
      flash(r.email_skipped ?? "已排課並寄出確認信");
      render();
    } catch (e) { flash(e.message, true); }
  });
  bk.append(fg2, bkBtn);
  page.append(bk);

  if (mock) {
    const out = await api("/api/google/outbox");
    const ob = el("div", "card");
    const h = el("h3", null, "模擬寄件匣");
    h.append(el("span", "c", `${out.mail.length} 封・${out.events.length} 個行程`));
    ob.append(h);
    for (const m of [...out.mail].reverse()) {
      const it = el("div", "item");
      it.append(el("div", "t", m.email.subject));
      const meta = el("div", "meta"); meta.append(el("span", null, `寄給 ${m.email.to}`), el("span", null, new Date(m.at).toLocaleString("zh-TW")));
      it.append(meta, el("div", "mail", m.email.body));
      ob.append(it);
    }
    for (const e of out.events) {
      const it = el("div", "item");
      it.append(el("div", "t", e.event.summary));
      const meta = el("div", "meta"); meta.append(el("span", null, new Date(e.event.start).toLocaleString("zh-TW")), el("span", null, `→ ${new Date(e.event.end).toLocaleTimeString("zh-TW")}`));
      it.append(meta);
      ob.append(it);
    }
    if (!out.mail.length && !out.events.length) ob.append(el("div", "empty", "還沒有寄出任何東西。"));
    page.append(ob);
  }
}

// ── right panel: generate a lesson ─────────────────────
let genMode = "news";
function setGenMode(m) {
  genMode = m;
  segment($("#genMode"), m, setGenMode);
  $$(".gen-src").forEach((n) => (n.hidden = n.dataset.mode !== m));
}
setGenMode("news");

function updatePicked() {
  const p = $("#genPicked");
  p.classList.toggle("on", !!state.picked);
  p.textContent = state.picked ? state.picked.title : "到「新聞」頁選一篇";
}

$("#genRun").onclick = async () => {
  const btn = $("#genRun");
  // The block starts folded (開始今天的課 is the main way in); its result shows in it.
  const blk = btn.closest(".block");
  if (blk.dataset.open !== "true") { blk.dataset.open = "true"; $(".toggle", blk).textContent = "−"; $(".toggle", blk).setAttribute("aria-expanded", "true"); }
  const body = { lang: state.lang, level: $("#genLevel").value };
  if (genMode === "news") {
    if (!state.picked) { flash("先到「新聞」頁選一篇", true); return; }
    body.article_id = state.picked.id;
  } else if (genMode === "url") {
    body.url = $("#genUrl").value.trim();
    if (!body.url) { $("#genUrl").focus(); return; }
  } else {
    body.text = $("#genText").value;
    body.title = $("#genTitle").value;
    if (!body.text.trim()) { $("#genText").focus(); return; }
  }
  const out = $("#genOut");
  out.className = "note";
  out.textContent = "整理中…（約 30 秒到 2 分鐘）";
  say("我在讀這篇文章，挑出值得學的單字和文法…");
  btn.disabled = true;
  try {
    const r = await api("/api/lessons", body);
    $(".tip.flash")?.remove();
    out.className = "note ok";
    out.textContent = `✓ 已加入 ${r.added.words} 個字・${r.added.grammar} 個文法`;
    out.title = r.path;
    say(`教材好了：「${r.lesson.title}」。單字已加入複習，問答答錯的會自動進錯題本。`);
    state.view = "lessons"; state.lessonId = r.id; state.showMd = false;
    await refreshStatus();
    render();
  } catch (e) {
    out.className = "note err";
    out.textContent = e.message;
    flash(e.message, true);
    say("這篇沒有做成功，原因寫在「產生教材」按鈕下方。");
  } finally {
    btn.disabled = false;
  }
};

// ── sync ───────────────────────────────────────────────
$("#actSync").onclick = () => busy($("#actSync"), async () => {
  try {
    await api("/api/records/export", {});
    const r = await api("/api/git/sync", {});
    flash(r.committed ? `已同步：${r.committed}${r.pushed ? "（已推送）" : ""}` : r.pushed ? "已推送" : "沒有需要同步的修改");
    todayCache = null;
    await refreshStatus();
    if (state.view === "today") render();
  } catch (e) { flash(e.message, true); }
});

// ── settings ───────────────────────────────────────────
const dlg = $("#dlgSettings");
$("#actSettings").onclick = () => openSettings(0);
$("#demoChip").onclick = () => openSettings(0);
$(".close", dlg).onclick = () => dlg.close();

function openSettings(step) {
  // Family mode: Claude is the admin's to fix; other locked admin tabs open 學習語言 instead.
  if (familyLocked() && step === 3) { flash("AI 老師還沒準備好，請家人幫忙看一下"); return; }
  if (familyLocked() && ADMIN_TABS.includes(step)) step = 2;
  applyAdminGate();
  dlg.showModal();
  showStep(step);
}

/**
 * Dialog tabs that scroll on narrow screens (title and × stay put): bring
 * the selected tab into view and fade the edge where more tabs are hidden.
 */
function tabsInView(box) {
  const sel = $('button[aria-selected="true"]', box);
  if (sel) {
    const l = sel.offsetLeft - box.offsetLeft, r = l + sel.offsetWidth;
    if (l < box.scrollLeft) box.scrollLeft = l - 12;
    else if (r > box.scrollLeft + box.clientWidth) box.scrollLeft = r - box.clientWidth + 12;
  }
  tabsFade(box);
}
function tabsFade(box) {
  box.classList.toggle("more-r", box.scrollLeft + box.clientWidth < box.scrollWidth - 2);
  box.classList.toggle("more-l", box.scrollLeft > 2);
}
for (const id of ["#setTabs", "#stTabs"]) {
  const box = $(id);
  box.addEventListener("scroll", () => tabsFade(box), { passive: true });
  addEventListener("resize", () => tabsFade(box));
}

function showStep(step) {
  $$("#setTabs button").forEach((b) => b.setAttribute("aria-selected", String(Number(b.dataset.step) === step)));
  tabsInView($("#setTabs"));
  $$(".wizard .step", dlg).forEach((s) => (s.hidden = Number(s.dataset.step) !== step));
  [loadWorkspaceStep, loadUserStep, loadLangsStep, loadClaudeStep, loadModelsStep, () => renderMail($("#setMail")).catch((e) => ($("#setMail").textContent = e.message)), () => {}, () => renderSpeechSettings($("#setSpeech"))][step]();
}
$$("#setTabs button").forEach((b) => (b.onclick = () => showStep(Number(b.dataset.step))));

let wsMode = "open";
function setWsMode(m) {
  wsMode = m;
  segment($("#setWsMode"), m, setWsMode);
  $$("#dlgSettings .step[data-step='0'] .form[data-mode]").forEach((f) => (f.hidden = f.dataset.mode !== m));
}
setWsMode("new");

async function loadWorkspaceStep() {
  const s = state.status;
  const now = $("#setWsNow");
  now.innerHTML = "";
  if (s?.workspace) {
    now.append(document.createTextNode(s.demo ? "目前：示範工作區 " : "目前的私人工作區："), el("code", null, s.workspace), el("br"),
      document.createTextNode("資料庫（不在 repo 裡）："), el("code", null, s.db ?? ""));
  } else {
    now.textContent = s?.error ?? "尚未選擇工作區";
  }
  const g = $("#setGit");
  g.innerHTML = "";
  try {
    const st = await api("/api/git/status");
    const line = (cls, mark, text) => { const li = el("li"); li.append(el("span", cls, mark), el("span", null, text)); g.append(li); };
    if (!st.repo) line("mid", "•", "不是 Git repository（只存在這台電腦）");
    else {
      line("ok", "✓", `Git：${st.branch ?? ""}${st.remote ? `　${st.remote}` : "　（沒有遠端）"}`);
      line(st.changed.length ? "mid" : "ok", st.changed.length ? "•" : "✓", st.changed.length ? `${st.changed.length} 個檔案尚未同步` : "沒有未同步的修改");
      if (st.ahead || st.behind) line("mid", "•", `領先 ${st.ahead}、落後 ${st.behind} 個 commit`);
    }
  } catch (e) { g.append(el("li", null, e.message)); }
}

const wsOut = (msg, err) => { const o = $("#setWsOut"); o.className = `note${err ? " err" : " ok"}`; o.textContent = msg; };
async function afterWorkspace(r) {
  const imp = r.import ? `：讀入單字 ${r.import.words}、文法 ${r.import.grammar}、教材 ${r.import.lessons}` : "";
  wsOut(`已開啟 ${r.workspace}${imp}${r.git ? `；${r.git}` : ""}${r.git_error ? `；Git：${r.git_error}` : ""}`, !!r.git_error);
  await refreshStatus();
  loadWorkspaceStep();
  greet();
  render();
  // A new or first-version workspace needs a user (or a migration) next.
  if (!state.status?.user) { dlg.close(); checkStart(); }
}
$("#setOpen").onclick = () => busy($("#setOpen"), async () => {
  try { await afterWorkspace(await api("/api/setup/workspace", { path: $("#setOpenPath").value, init: $("#setOpenInit").checked })); } catch (e) { wsOut(e.message, true); }
});
$("#setNew").onclick = () => busy($("#setNew"), async () => {
  try { await afterWorkspace(await api("/api/setup/new", { path: $("#setNewPath").value, remote: $("#setNewRemote").value })); } catch (e) { wsOut(e.message, true); }
});
$("#setClone").onclick = () => busy($("#setClone"), async () => {
  try { await afterWorkspace(await api("/api/setup/clone", { url: $("#setCloneUrl").value, parent: $("#setCloneParent").value || "~/" })); } catch (e) { wsOut(e.message, true); }
});
$("#setGhNew").onclick = () => api("/api/open/github-new-repo", {}).catch((e) => flash(e.message, true));
$("#setSync").onclick = () => busy($("#setSync"), async () => {
  try { const r = await api("/api/git/sync", {}); wsOut(r.committed ? `已 commit：${r.committed}${r.pushed ? "，已推送" : ""}` : "沒有需要同步的修改"); loadWorkspaceStep(); }
  catch (e) { wsOut(e.message, true); }
});
$("#setExport").onclick = () => busy($("#setExport"), async () => {
  try { const r = await api("/api/records/export", {}); wsOut(`已寫入 ${r.dir}/：複習紀錄 ${r.progress} 筆、錯題 ${r.mistakes} 筆`); loadWorkspaceStep(); }
  catch (e) { wsOut(e.message, true); }
});
$("#setReindex").onclick = () => busy($("#setReindex"), async () => {
  try { const r = await api("/api/workspace/reindex", {}); wsOut(`單字 ${r.words}、文法 ${r.grammar}、素材 ${r.materials}、教材 ${r.lessons}${r.warnings.length ? `；警告：${r.warnings.join("；")}` : ""}`, r.warnings.length > 0); await refreshStatus(); render(); }
  catch (e) { wsOut(e.message, true); }
});

// 02 使用者
function loadUserStep() {
  const s = state.status;
  const u = s?.user;
  $("#setName").value = u?.name ?? "";
  $("#setColor").value = u?.color ?? "#ff6b35";
  $("#setEmail").value = u?.report_email ?? "";
  const ul = $("#setMembers");
  ul.innerHTML = "";
  const iAmAdmin = !!(s?.users ?? []).find((x) => x.id === u?.id)?.admin;
  for (const m of s?.users ?? []) {
    const li = el("li");
    const dot = el("span", "dot"); dot.style.background = m.color;
    li.append(dot, el("span", "n", m.name), el("span", "tag", m.kind === "owner" ? "擁有者" : "家庭成員"));
    if (m.admin) li.append(el("span", "tag acc", "管理員"));
    if (m.id === u?.id) li.append(el("span", "tag ok", "目前"));
    // The admin prepares lessons for the iPads (plan F5): a member's, or their own.
    if (iAmAdmin) { const p = el("button", "ghost", "備課"); p.setAttribute("aria-label", `替 ${m.name} 備課`); p.onclick = () => openPrep(m); li.append(p); }
    if (m.id !== u?.id) {
      const b = el("button", "ghost", "切換"); b.onclick = async () => { await switchUser(m.id); loadUserStep(); }; li.append(b);
    }
    ul.append(li);
  }
  // A preparation still running (the settings were closed meanwhile) shows its progress.
  if (iAmAdmin) api("/api/family/prepare").then((r) => { if (r.progress?.running) openPrep(s.users.find((x) => x.id === r.progress.member) ?? { id: r.progress.member, name: r.progress.member }, true); }).catch(() => {});
}

// 替家人備課 (plan F5 stage 2): a few days of lessons made here with this
// computer's Claude; the next sync takes them to the member's iPad. Their
// results from the iPad come back with the sync and are applied here.
let prepTimer = null;
function openPrep(m, watching = false) {
  const box = $("#setPrep");
  box.hidden = false;
  box.innerHTML = "";
  box.dataset.member = m.id;
  const head = el("div", "seg-row");
  const mine = m.id === state.status?.user?.id;
  head.append(el("b", null, mine ? "替自己備課（給我的 iPad）" : `替 ${m.name} 備課（給 iPad）`));
  const help = el("button", "help", "?");
  help.setAttribute("aria-label", "說明：替家人備課");
  help.dataset.tip = `用這台電腦的 Claude 做好接下來幾天的課，每天幾個主題可以在 iPad 上挑。課存在各自的資料夾，按同步就會送到 iPad；在 iPad 上的作答也是同步時收回來、在這裡算分。備過的主題不會再出現。一起備的人拿到同樣的主題，課文、單字數與題目依各自的程度。`;
  head.append(help);
  const close = el("button", "ghost", "收起");
  const backTo = () => [...$$("#setMembers button")].find((b) => b.getAttribute("aria-label") === `替 ${m.name} 備課`);
  close.onclick = () => { clearInterval(prepTimer); prepTimer = null; box.hidden = true; backTo()?.focus(); };
  head.append(el("span", "grow"), close);
  box.append(head);
  const form = el("div", "prep-form");
  const sel = (label, opts, v) => {
    const l = el("label");
    const s = el("select");
    s.setAttribute("aria-label", label);
    for (const [val, text] of opts) { const o = el("option", null, text); o.value = val; if (val === v) o.selected = true; s.append(o); }
    l.append(el("span", null, label), s);
    form.append(l);
    return s;
  };
  // Who gets these lessons: this member, and anyone else ticked (the same
  // topics, each at their own level).
  const others = (state.status?.users ?? []).filter((x) => x.id !== m.id);
  const also = [];
  if (others.length) {
    const who = el("div", "prep-who");
    who.append(el("span", null, "一起備給"));
    for (const o of others) {
      const l = el("label", "check");
      const c = el("input"); c.type = "checkbox"; c.value = o.id;
      c.checked = store.get("jl.prepAlso", []).includes(o.id);
      l.append(c, el("span", null, o.id === state.status?.user?.id ? `${o.name}（我的 iPad）` : o.name));
      who.append(l);
      also.push(c);
    }
    box.append(who);
  }
  const picked = () => [m.id, ...also.filter((c) => c.checked).map((c) => c.value)];
  const nameOf = (id) => (state.status?.users ?? []).find((x) => x.id === id)?.name ?? id;
  const lang = sel("語言", [["en", "英文"], ["ja", "日文"]], state.lang);
  const days = sel("天數", [1, 2, 3, 5, 7].map((d) => [String(d), `${d} 天`]), "3");
  const choices = sel("每天可選", [1, 2, 3, 4].map((c) => [String(c), `${c} 個主題`]), "3");
  // The content: the topic library of that language (default), or the day's news.
  const sourceLabel = el("label");
  const source = el("select");
  source.setAttribute("aria-label", "內容");
  sourceLabel.append(el("span", null, "內容"), source);
  form.append(sourceLabel);
  let libs = [];
  const fillSource = () => {
    const lib = libs.find((c) => c.lang === lang.value && c.written > 0);
    source.innerHTML = "";
    // The mix is what 派本週的課 uses: try it small here (1 day, 1 person).
    if (lib) {
      const mix = el("option", null, `主題庫七成＋新聞（和「派本週的課」一樣）`); mix.value = "mix"; source.append(mix);
      const o = el("option", null, `只用主題庫・${lib.name}（${lib.written} 堂）`); o.value = "library"; source.append(o);
    }
    const n = el("option", null, "每日新聞"); n.value = "news"; source.append(n);
  };
  lang.addEventListener("change", fillSource);
  fillSource();
  api("/api/library").then((r) => { libs = r.collections ?? []; fillSource(); }).catch(() => {});
  box.append(form);
  const row = el("div", "actions-row");
  const go = primaryButton("開始備課", null, true);
  const collect = ghostButton("download", "收回作答", null);
  row.append(go, collect);
  box.append(row);
  const out = el("div", "prep-out");
  box.append(out);
  box.append(ipadSetup(m));
  let startedHere = false;
  let lastKey = "";
  /** One line, plus a button that does the next thing when there is one. */
  const say = (cls, text, action) => {
    const d = el("div", `note ${cls}`, text);
    if (action) { const b = ghostButton(action.icon, action.label, action.onclick); b.classList.add("small"); d.append(" ", b); }
    return d;
  };
  const toClaude = { icon: "gear", label: "去設定 Claude", onclick: () => openSettings(3) };
  const errLine = (e) => {
    const f = friendly(e);
    const d = say("err", f, f !== e ? toClaude : null);
    if (f !== e) d.title = e;
    return d;
  };
  const show = (p) => {
    // Re-draw only when something changed (the status line is read out once).
    const key = JSON.stringify(p ?? null);
    if (key === lastKey) return;
    lastKey = key;
    out.innerHTML = "";
    go.disabled = false;
    go.textContent = "開始備課";
    if (!p) return;
    const inRun = (p.members?.length ? p.members : [p.member]).includes(m.id);
    if (p.running && !inRun) {
      out.append(say("", `正在替 ${nameOf(p.member)} 備課，做完才能開始下一位。`));
      go.disabled = true;
      return;
    }
    if (p.running) {
      go.disabled = true;
      go.textContent = "備課中…";
      const bar = el("progress");
      bar.max = Math.max(1, p.total);
      if (p.done > 0) bar.value = p.done; // empty: the moving bar while topics are chosen
      bar.setAttribute("aria-label", "備課進度");
      const whom = (p.members?.length ?? 0) > 1 ? `${nameOf(p.member)}・` : "";
      out.append(bar, el("div", "note", `${p.done} / ${p.total} 堂・${whom}${p.current ? `正在做「${p.current}」` : "正在挑主題"}`));
      for (const e of p.errors ?? []) out.append(errLine(e));
      return;
    }
    if (!inRun) return;
    const prefix = startedHere ? "" : "上次：";
    const status = el("div");
    status.setAttribute("role", "status");
    if (p.done) {
      status.append(say("ok", `${prefix}做好 ${p.done} 堂。按同步就會送到 iPad。`, { icon: "cloud", label: "同步", onclick: () => $("#actSync").click() }));
      if (p.done < p.total) status.append(say("", `比預定的 ${p.total} 堂少：想不出更多沒備過的主題。可以設定重點主題，或過幾天再備。`));
    } else if (!(p.errors ?? []).length) {
      status.append(say("", `${prefix}這次沒有做出課：想不出沒備過的主題。可以設定重點主題，或過幾天再備。`));
    }
    out.append(status);
    if (p.made?.length) out.append(el("div", "note", p.made.join("、")));
    for (const e of p.errors ?? []) out.append(/新的主題|換一批/.test(e) ? say("", "想不出更多沒備過的主題。可以設定重點主題，或過幾天再備。") : errLine(e));
  };
  const poll = async () => {
    // The settings were closed: stop asking.
    if (!$("#dlgSettings").open || box.hidden) { clearInterval(prepTimer); prepTimer = null; return; }
    try {
      const r = await api("/api/family/prepare");
      show(r.progress);
      if (!r.progress?.running) { clearInterval(prepTimer); prepTimer = null; }
    } catch { /* try again on the next tick */ }
  };
  const watch = () => { clearInterval(prepTimer); prepTimer = setInterval(poll, 2000); poll(); };
  go.onclick = () => busy(go, async () => {
    if (!state.status?.engine_ready) {
      lastKey = "";
      out.innerHTML = "";
      out.append(say("err", "這台電腦的 Claude 還沒連上，備課要用它。", toClaude));
      return;
    }
    try {
      startedHere = true;
      store.set("jl.prepAlso", also.filter((c) => c.checked).map((c) => c.value));
      await api("/api/family/prepare", { members: picked(), lang: lang.value, days: Number(days.value), choices: Number(choices.value), source: source.value });
      watch();
    } catch (e) { lastKey = ""; out.innerHTML = ""; out.append(errLine(e.message)); }
  });
  collect.onclick = () => busy(collect, async () => {
    lastKey = "";
    try {
      const r = await api("/api/family/inbox", {});
      const a = (r.applied ?? []).find((x) => x.member === m.id);
      out.innerHTML = "";
      const st = el("div");
      st.setAttribute("role", "status");
      st.append(say("ok", a?.files ? `收回 ${a.files} 份作答：${a.units} 個單元、${a.conversations} 段對話，共 ${Math.round(a.points * 10) / 10} 分。` : "目前沒有新的作答。先按同步，把 iPad 送來的作答拉下來。"));
      todayCache = null; refreshStatus();
      out.append(st);
      for (const e of a?.errors ?? []) out.append(errLine(e));
    } catch (e) { out.innerHTML = ""; out.append(errLine(e.message)); }
  });
  if (watching) watch();
  else poll();
  lang.focus();
}
/**
 * The first time on a member's iPad: one link that carries the settings
 * (repository, member, the two keys), made here in the browser and copied —
 * nothing is stored or sent. Opened on the iPad, the app sets itself up.
 */
function ipadSetup(m) {
  const d = el("details", "other-way ipad-setup");
  d.append(el("summary", null, `第一次設定 ${m.name} 的 iPad`));
  const form = el("div", "form");
  const field = (label, type, ph, value = "") => {
    const l = el("label");
    const i = el("input");
    i.type = type; i.placeholder = ph; i.value = value; i.autocomplete = type === "password" ? "new-password" : "off";
    l.append(el("span", null, label), i);
    form.append(l);
    return i;
  };
  const app = field("iPad App 網址", "url", "https://你的帳號.github.io/JELearner/app/", store.get("jl.ipadApp", ""));
  const repo = field("資料 repo", "text", "帳號/JELearner-private");
  const token = field("GitHub 鑰匙", "password", "github_pat_…（只開這個 repo，Contents 可讀寫）");
  const key = field("Claude API key", "password", "sk-ant-…（對話用，建議設每月上限）");
  api("/api/git/status").then((g) => {
    const mm = /github\.com[:/]([^/]+\/[^/]+?)(\.git)?$/.exec(g.remote ?? "");
    if (mm && !repo.value) repo.value = mm[1];
  }).catch(() => {});
  const row = el("div", "actions-row");
  const note = el("span", "note");
  note.setAttribute("role", "status");
  /** The link from the form, or null with a word about what is missing. */
  const link = () => {
    note.className = "note err";
    const base = app.value.trim(), r = repo.value.trim().replace(/^https:\/\/github\.com\//, "").replace(/\.git$/, "");
    if (!/^https?:\/\//.test(base)) { note.textContent = "先填 iPad App 的網址"; app.focus(); return null; }
    if (!/^[\w.-]+\/[\w.-]+$/.test(r)) { note.textContent = "repo 寫成「帳號/名稱」"; repo.focus(); return null; }
    if (!token.value.trim()) { note.textContent = "還需要 GitHub 鑰匙"; token.focus(); return null; }
    store.set("jl.ipadApp", base);
    const json = JSON.stringify({ repo: r, member: m.id, token: token.value.trim(), apiKey: key.value.trim() });
    const b64 = btoa(String.fromCharCode(...new TextEncoder().encode(json))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    note.className = "note";
    note.textContent = "";
    return `${base.replace(/#.*$/, "")}#setup=${b64}`;
  };
  // The QR code: scanned from inside the iPad app (第一次使用 › 掃描), so the
  // keys go straight into the app on the home screen. Always black on white.
  const qrBox = el("div", "qr-box");
  qrBox.hidden = true;
  const show = primaryButton("顯示 QR code", () => {
    if (!qrBox.hidden) { qrBox.hidden = true; qrBox.innerHTML = ""; show.textContent = "顯示 QR code"; return; }
    const url = link();
    if (!url) return;
    try {
      const qr = qrcode(0, "L");
      qr.addData(url);
      qr.make();
      const n = qr.getModuleCount(), quiet = 4, px = Math.max(3, Math.floor(720 / (n + quiet * 2)));
      const cv = el("canvas");
      cv.width = cv.height = (n + quiet * 2) * px;
      const g = cv.getContext("2d");
      g.fillStyle = "#fff"; g.fillRect(0, 0, cv.width, cv.height);
      g.fillStyle = "#000";
      for (let r2 = 0; r2 < n; r2++) for (let c2 = 0; c2 < n; c2++) if (qr.isDark(r2, c2)) g.fillRect((c2 + quiet) * px, (r2 + quiet) * px, px, px);
      cv.setAttribute("role", "img");
      cv.setAttribute("aria-label", `${m.name} 的 iPad 設定 QR code`);
      const ol = el("ol", "qr-steps");
      for (const t of [`在 ${m.name} 的 iPad 用 Safari 打開上面的網址`, "分享 → 加入主畫面，從主畫面打開", "按「掃描電腦上的 QR code」，對準這裡"]) ol.append(el("li", null, t));
      qrBox.append(cv, ol);
      qrBox.hidden = false;
      show.textContent = "收起 QR code";
      qrBox.scrollIntoView({ block: "nearest" });
    } catch { note.className = "note err"; note.textContent = "內容太長，做不出 QR code，請用複製連結"; }
  }, true);
  const copy = ghostButton("copy", "複製連結", async () => {
    const url = link();
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      note.textContent = `已複製。傳到 ${m.name} 的 iPad，用 Safari 打開。`;
    } catch { note.className = "note err"; note.textContent = "無法複製，請允許剪貼簿"; }
  });
  const help = el("button", "help", "?");
  help.setAttribute("aria-label", "說明：iPad 設定連結");
  help.dataset.tip = "QR code 和連結裡有兩把鑰匙：掃完就收起來，連結傳完把訊息刪掉。鑰匙只留在那台 iPad 裡，這台電腦不保存。";
  row.append(show, copy, help, note);
  d.append(form, row, qrBox);
  return d;
}

$("#setSaveUser").onclick = () => busy($("#setSaveUser"), async () => {
  try {
    await api("/api/user", { name: $("#setName").value, color: $("#setColor").value, report_email: $("#setEmail").value }, "PUT");
    flash("已儲存");
    await refreshStatus(); loadUserStep(); greet();
  } catch (e) { flash(e.message, true); }
});
$("#setAddUser").onclick = () => { dlg.close(); startWizard("add"); };

// 03 學習語言
let readSetLangs = () => ({});
function loadLangsStep() {
  const u = state.status?.user;
  readSetLangs = langPicker($("#setLangs"), u, true);
  $("#setDaily").value = u?.daily_minutes ?? 30;
  $("#setSecs").value = u?.conversation?.seconds_per_turn ?? 15;
  $("#setTurns").value = u?.conversation?.turns ?? 20;
  $("#setRatio").value = Math.round((u?.conversation?.interest_ratio ?? 0.6) * 100);
  $("#setRatio").oninput();
}
$("#setRatio").oninput = () => { const v = Number($("#setRatio").value); $("#setRatioOut").textContent = `興趣 ${v}%・通識 ${100 - v}%`; };
$("#setSaveLangs").onclick = () => busy($("#setSaveLangs"), async () => {
  try {
    const { languages, majors } = readSetLangs();
    await api("/api/user", {
      languages, majors,
      daily_minutes: Number($("#setDaily").value),
      seconds_per_turn: Number($("#setSecs").value),
      turns: Number($("#setTurns").value),
      interest_ratio: Number($("#setRatio").value) / 100,
    }, "PUT");
    flash("已儲存");
    await refreshStatus(); loadLangsStep(); render();
  } catch (e) { flash(e.message, true); }
});

let via = store.get("jl.via", null);
function setVia(v) {
  via = v;
  store.set("jl.via", v);
  segment($("#setVia"), v, setVia);
  $$("#dlgSettings .step[data-step='3'] .via").forEach((n) => (n.hidden = n.dataset.via !== v));
}

async function loadClaudeStep(fresh = false) {
  const c = await api(`/api/claude${fresh ? "?fresh=true" : ""}`);
  $("#setEngine").value = c.engine;
  $("#setEngineRt").value = state.status?.realtime_engine ?? "same";
  const act = $("#setActive");
  act.className = `now ${c.active ? "" : "warn"}`;
  // Not connected yet: this tab is where it happens, so no "go to Settings".
  act.textContent = c.active ? `目前使用：${c.active_label}` : String(c.active_error ?? "").startsWith("Claude 還沒連上") ? "Claude 還沒連上：用下面任一種方式連接" : c.active_error;
  if (!via) via = c.subscription.ready || !c.api.configured ? "subscription" : "api";
  setVia(via);

  // Subscription: installed → signed in → billing the subscription.
  const sub = c.subscription;
  const ul = $("#ccChecks");
  ul.innerHTML = "";
  const line = (ok, text) => { const li = el("li"); li.append(el("span", ok === true ? "ok" : ok === false ? "bad" : "mid", ok === true ? "✓" : ok === false ? "✕" : "•"), el("span", null, text)); ul.append(li); };
  line(sub.installed, sub.installed ? `已安裝 Claude Code${sub.version ? ` ${sub.version}` : ""}` : "尚未安裝 Claude Code");
  if (sub.installed) {
    line(sub.logged_in ? (sub.account ? true : null) : false, sub.logged_in ? (sub.account ? `已登入：${sub.account}` : "已登入，但無法確認是哪個帳號；若不是你的帳號，請按「用瀏覽器登入」") : "尚未登入");
    if (sub.logged_in) line(sub.ready ? true : null, sub.ready ? "使用 Claude 訂閱額度" : `登入方式是 ${sub.auth_method ?? "未知"}，不是 Claude 訂閱；請用「用瀏覽器登入」改用訂閱帳號`);
  }
  $("#ccInstall").hidden = sub.installed;
  $("#ccCmd").textContent = c.install;
  $("#ccLogin").hidden = !sub.installed || (sub.ready && !!sub.account);

  // API key.
  const editing = !c.api.configured;
  $("#setKeyShow").hidden = editing;
  $("#setKeyEnter").hidden = !editing;
  $("#setKeySave").hidden = !editing;
  $("#setKeyMasked").textContent = c.api.configured ? `${c.api.masked}　（${{ env: "環境變數", keychain: "系統金鑰圈", file: "設定資料夾", settings: "設定" }[c.api.source] ?? c.api.source}）` : "";

  // Short name and what it is good at first (the id is long); the default of each choice is marked.
  const fill = (sel, current, def) => {
    sel.innerHTML = "";
    for (const m of c.models) {
      const o = el("option", null, `${m.summary || m.id}${m.id === def ? "（預設）" : ""}`);
      o.value = m.id; o.title = m.id;
      if (m.id === current) o.selected = true;
      sel.append(o);
    }
    if (current && !c.models.some((m) => m.id === current)) { const o = el("option", null, current); o.value = current; o.selected = true; sel.prepend(o); }
  };
  fill($("#setModel"), c.model, c.defaults?.model);
  fill($("#setModelRt"), c.realtime_model, c.defaults?.realtime);
  $("#setEffort").value = c.effort ?? "";
  return c;
}
$("#setEngineRt").onchange = async () => {
  try { await api("/api/settings/engine", { realtime: $("#setEngineRt").value }); await refreshStatus(); await loadClaudeStep(); }
  catch (e) { flash(e.message, true); }
};
$("#setEngine").onchange = async () => {
  try { await api("/api/settings/engine", { kind: $("#setEngine").value }); await loadClaudeStep(); await refreshStatus(); greet(); }
  catch (e) { flash(e.message, true); }
};
$("#ccRecheck").onclick = () => busy($("#ccRecheck"), async () => { await loadClaudeStep(true); await refreshStatus(); });
$("#ccCopy").onclick = () => navigator.clipboard?.writeText($("#ccCmd").textContent).then(() => flash("已複製"));
$("#ccSetup").onclick = () => api("/api/open/claude-code-setup", {}).catch((e) => flash(e.message, true));
$("#ccLogin").onclick = async () => {
  try {
    await api("/api/claude-code/login", {});
    flash("已開啟瀏覽器，請在 Anthropic 的頁面登入你的 Claude 帳號");
    // Poll until the login lands (up to 5 minutes, or the dialog closes).
    for (let i = 0; i < 100 && dlg.open; i++) {
      await new Promise((r) => setTimeout(r, 3000));
      const c = await loadClaudeStep(true);
      if (c.subscription.ready) { flash("已登入，之後會使用你的 Claude 訂閱"); await refreshStatus(); greet(); break; }
    }
  } catch (e) { flash(e.message, true); }
};
$("#setKeyEdit").onclick = () => { $("#setKeyShow").hidden = true; $("#setKeyEnter").hidden = false; $("#setKeySave").hidden = false; $("#setKey").focus(); };
$("#setKeyRemove").onclick = async () => { await api("/api/settings/claude", { key: "" }); await refreshStatus(); loadClaudeStep(); };
$("#setGetKey").onclick = () => api("/api/open/anthropic-keys", {}).catch((e) => flash(e.message, true));
$("#setKeySave").onclick = async () => {
  const k = $("#setKey").value.trim();
  if (!k) return;
  try {
    const r = await api("/api/settings/claude", { key: k });
    $("#setKey").value = "";
    flash(r.stored_in === "keychain" ? "已存入系統金鑰圈" : "已存入設定資料夾（credentials.toml）");
    await refreshStatus(); loadClaudeStep(); greet();
  } catch (e) { flash(e.message, true); }
};
const saveClaude = (body, then) => api("/api/settings/claude", body).then(() => { flash("已儲存"); then?.(); }).catch((e) => flash(e.message, true));
$("#setModel").onchange = () => saveClaude({ model: $("#setModel").value }, refreshStatus);
$("#setModelRt").onchange = () => saveClaude({ realtime_model: $("#setModelRt").value });
$("#setEffort").onchange = () => saveClaude({ effort: $("#setEffort").value });
// Tests the method shown (subscription or API key), whatever is chosen above.
$("#setTest").onclick = () => busy($("#setTest"), async () => {
  const out = $("#setTestOut");
  out.className = "note"; out.textContent = "測試中…";
  try { const r = await api("/api/engine/test", { engine: via }); out.className = "note ok"; out.textContent = `${r.via}：回覆「${r.reply}」（${r.ms} ms）`; }
  catch (e) { out.className = "note err"; out.textContent = e.message; }
});

async function loadModelsStep() {
  const m = await api("/api/models");
  const sys = $("#setSys");
  sys.innerHTML = "";
  sys.append(document.createTextNode(`這台電腦：記憶體 ${m.memory_gb} GB。`), el("br"));
  if (m.external) sys.append(document.createTextNode(`本機模型連到 ${m.external}（config.toml 的 [llama] url）。`));
  else if (!m.home) sys.append(document.createTextNode("還沒有設定模型的共用資料夾：設定環境變數 LOCAL_LLM_HOME，或在 config.toml 的 [llama] 寫 home。"));
  else sys.append(document.createTextNode(m.runtime ? "模型與 llama.cpp 都從共用資料夾讀取。" : "共用資料夾裡還沒有 llama.cpp：在 PaperComposer 的「AI 與同步」裝一次即可。"));
  const ul = $("#setModels");
  ul.innerHTML = "";
  const role = { brain: "大腦", ears: "耳朵", mouth: "嘴巴" };
  const brains = Object.fromEntries((m.brains ?? []).map((b) => [b.id, b]));
  const usingLocal = m.engine === "llama-server";
  for (const x of m.catalog.models) {
    const b = brains[x.id];
    const tooBig = x.role === "brain" && x.min_memory_gb > m.memory_gb + 0.5;
    const li = el("li", `model${tooBig ? " dim" : ""}`);
    const mh = el("div", "mh");
    mh.append(el("b", null, `${x.name} ${x.variant}`), el("span", "sum", x.summary));
    const tags = el("span", "tags");
    tags.append(el("span", "tag", role[x.role] ?? x.role));
    if (b?.running) tags.append(el("span", "tag ok", "使用中"));
    else if (b?.chosen && usingLocal) tags.append(el("span", "tag acc", "已選擇"));
    if (b && !b.installed && !m.external) {
      tags.append(el("span", "tag", "未下載"));
      const help = el("button", "help", "?");
      help.dataset.tip = `在 PaperComposer 下載，或：hf download ${x.repo} --include "*${x.file_suffix}" --local-dir "${m.home ?? "$LOCAL_LLM_HOME"}/gguf/${x.repo}"`;
      tags.append(help);
    }
    mh.append(tags);
    const meta = el("div", "meta");
    const items = x.role === "brain"
      ? [`約 ${x.approx_gb} GB`, `至少 ${x.min_memory_gb} GB 記憶體`, x.license]
      : [x.runtime === "macos" ? "macOS 內建，不佔模型記憶體" : x.runtime];
    items.forEach((t) => meta.append(el("span", null, t)));
    const row = el("div", "use-row");
    row.append(meta);
    // A downloaded brain that is not running: one click starts it (and makes it the engine).
    if (b && (b.installed || m.external) && !b.running && !(m.external && b.chosen && usingLocal)) {
      const label = b.chosen && usingLocal ? "啟動" : "使用這個";
      const use = el("button", "ghost small", label);
      use.onclick = async () => {
        use.disabled = true;
        use.textContent = "啟動中…";
        try {
          const r = await api("/api/settings/llama", { model: x.id });
          flash(`已改用 ${x.name}（${Math.round(r.ms / 100) / 10} 秒）`);
          await refreshStatus();
        } catch (e) { flash(e.message, true); }
        loadModelsStep();
      };
      row.append(use);
    }
    li.append(mh, row);
    ul.append(li);
  }
}

// ── start ──────────────────────────────────────────────
applyTheme();
paintIcons();
updatePicked();
refreshStatus().then(() => {
  greet();
  render();
  checkStart();
  if (state.status?.family?.ready) familySync(); // bring in the admin's work on open
  else quietPull();
});

// Not family mode: bring in what the others did (their points, an iPad's
// answers) on open and now and then. Fast-forward only; nothing is said
// when there is no remote or no network. Pushing stays with the sync button.
async function quietPull() {
  const s = state.status;
  if (!s?.workspace || s.demo || s.family?.ready || (s.users ?? []).length < 2) return;
  try {
    const r = await api("/api/git/pull", {});
    if (!r.pulled) return;
    if ((s.users ?? []).find((u) => u.id === s.user?.id)?.admin) await api("/api/family/inbox", {}).catch(() => {});
    todayCache = null;
    await refreshStatus();
    greet();
    if (state.view === "today") render();
  } catch { /* offline, or only on this computer */ }
}
setInterval(() => { if (document.visibilityState === "visible") quietPull(); }, 10 * 60 * 1000);

// ── family mode (plan §19.2.5) ──────────────────────────
// The member never sees Git: sync runs on open, ~15 s after a change, every
// 10 minutes and when the window is hidden or closed. Offline is retried
// quietly; only a lasting problem shows one sentence, with the details for
// the admin folded away. Admin settings open with the PIN.
const fam = { timer: null, unlocked: false, pin: "" };
const familyOn = () => !!state.status?.family?.ready;

function applyFamily() {
  const on = familyOn();
  document.body.classList.toggle("family", on);
  $("#actSync").hidden = on;
  // 12: one person on this computer: the name is not a menu.
  const chip = $("#actUser");
  chip.tabIndex = on ? -1 : 0;
  if (on) { chip.removeAttribute("aria-haspopup"); chip.setAttribute("aria-label", "使用者"); }
  else { chip.setAttribute("aria-haspopup", "true"); chip.setAttribute("aria-label", "切換使用者"); }
  if (!on) {
    fam.unlocked = false;
    const banner = $("#famBanner");
    if (banner) banner.remove();
  }
  applyAdminGate();
}

function scheduleFamilySync(ms = 15000) {
  if (!familyOn()) return;
  clearTimeout(fam.timer);
  fam.timer = setTimeout(familySync, ms);
}

async function familySync() {
  if (!familyOn()) return;
  clearTimeout(fam.timer);
  let r;
  try { r = await api("/api/family/sync", {}); } catch (e) { r = { ok: false, reason: "other", detail: e.message }; }
  if (r.ok) {
    const banner = $("#famBanner");
    if (banner) banner.remove();
    if (r.pulled) { await refreshStatus(); render(); }
    return;
  }
  if (r.reason === "offline") return; // next time; nothing to say
  showFamilyBanner(r);
}

const FAMILY_REASON = { auth: "金鑰無法使用（可能在 GitHub 被移除）", conflict: "和管理員的修改衝突，已保留在這台電腦", other: "同步失敗" };
function showFamilyBanner(r) {
  let b = $("#famBanner");
  if (!b) { b = el("div", "fam-banner"); b.id = "famBanner"; b.setAttribute("role", "status"); document.body.append(b); }
  b.innerHTML = "";
  const d = el("details");
  d.append(el("summary", null, "給幫忙的人看"), el("pre", null, `${FAMILY_REASON[r.reason] ?? r.reason}\n${r.detail ?? ""}`));
  b.append(el("span", null, "資料暫時沒有存到家人那邊，請家人幫忙看一下。學習可以照常進行。"), d);
}

setInterval(() => { if (familyOn()) familySync(); }, 10 * 60 * 1000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden" && familyOn()) navigator.sendBeacon?.("/api/family/sync");
});

// Admin tabs (workspace, Claude, local models, mail) wait for the PIN.
const ADMIN_TABS = [0, 3, 4, 5];
const familyLocked = () => familyOn() && !fam.unlocked;
function applyAdminGate() {
  const locked = familyLocked();
  document.body.classList.toggle("fam-locked", locked); // .admin-only parts of open tabs, too
  $$("#setTabs button").forEach((b) => { if (ADMIN_TABS.includes(Number(b.dataset.step))) b.hidden = locked; });
  $("#setAdmin").hidden = !familyOn();
  $("#setAdminOpen").textContent = locked ? "管理員設定" : "關閉家人模式";
  $("#setAdmin .pin-row").hidden = true;
}
$("#setAdminOpen").onclick = async () => {
  if (familyLocked()) {
    const row = $("#setAdmin .pin-row");
    row.hidden = !row.hidden;
    if (!row.hidden) setTimeout(() => $("#setAdminPin").focus(), 0);
    return;
  }
  // Unlocked: the admin can turn family mode off on this computer.
  if (!confirm("關閉這台電腦的家人模式？之後同步與設定會像管理員的電腦一樣。")) return;
  try { await api("/api/family/leave", { pin: fam.pin }); flash("已關閉家人模式"); await refreshStatus(); render(); }
  catch (e) { flash(e.message, true); }
};
$("#setAdminGo").onclick = async () => {
  const pin = $("#setAdminPin").value.trim();
  try {
    await api("/api/family/unlock", { pin });
    fam.unlocked = true; fam.pin = pin;
    $("#setAdminPin").value = "";
    applyAdminGate();
    flash("已打開管理員設定");
    $("#setTabs button[data-step='0']")?.focus();
  } catch (e) { flash(e.message, true); $("#setAdminPin").select(); }
};
$("#setAdminPin").onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); $("#setAdminGo").click(); } };
