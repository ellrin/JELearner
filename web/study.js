// Stage A (docs/plan.md §8–§10, §19.1): the today page as a task list, the
// 20-turn conversation page, result cards with next steps, the progress page,
// drag-and-drop import and the profile card. Little text; the next step is
// shown by the one primary button and by components that light up.

const UNIT_LABEL = { conversation: "對話", quiz: "教材測驗", review: "複習", reading: "閱讀", listening: "聆聽時間", translation: "翻譯", "sentence-making": "造句", cloze: "填空", paraphrase: "換句話說" };
const SKILL_LABEL = { listening: "聽", reading: "讀", speaking: "說", writing: "寫", vocab: "單字文法" };

function primaryButton(label, onclick, small = false) {
  const b = el("button", `primary${small ? " small" : ""}`, label);
  b.onclick = onclick;
  return b;
}

const fmtPts = (p) => (Math.round((p ?? 0) * 10) / 10).toString();
const pct = (x) => `${Math.round((x ?? 0) * 100)}%`;

// ── shared components ──────────────────────────────────

/** `.level-ring`: the level in the middle, the ring fills towards the next step. */
function levelRing(size, label, frac, sub, ready) {
  const box = el("div", `level-ring${ready ? " ready" : ""}`);
  box.style.width = box.style.height = `${size}px`;
  box.style.setProperty("--ring-font", `${Math.round(size / 5.2)}px`);
  const w = Math.max(3, Math.round(size / 14));
  const r = (size - w) / 2;
  const c = 2 * Math.PI * r;
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", `0 0 ${size} ${size}`);
  // The ring runs from --accent to --accent-2 (stop colours in styles.css).
  const gid = `lr-grad-${++levelRing.n}`;
  const defs = document.createElementNS(ns, "defs");
  const grad = document.createElementNS(ns, "linearGradient");
  grad.id = gid;
  grad.setAttribute("x1", "1"); grad.setAttribute("y1", "0.5"); grad.setAttribute("x2", "0"); grad.setAttribute("y2", "0.5");
  for (const [off, cls] of [["0", "g1"], ["1", "g2"]]) {
    const st = document.createElementNS(ns, "stop");
    st.setAttribute("offset", off); st.setAttribute("class", cls);
    grad.append(st);
  }
  defs.append(grad);
  svg.append(defs);
  for (const cls of ["track", "fill"]) {
    const ci = document.createElementNS(ns, "circle");
    ci.setAttribute("class", cls);
    ci.setAttribute("cx", size / 2); ci.setAttribute("cy", size / 2); ci.setAttribute("r", r);
    ci.setAttribute("stroke-width", w);
    if (cls === "fill") {
      ci.style.stroke = `url(#${gid})`;
      ci.setAttribute("stroke-dasharray", c);
      ci.setAttribute("stroke-dashoffset", c);
      requestAnimationFrame(() => requestAnimationFrame(() => ci.setAttribute("stroke-dashoffset", c * (1 - Math.max(0, Math.min(1, frac))))));
    }
    svg.append(ci);
  }
  const lab = el("div", "label");
  const inner = el("div");
  inner.append(el("b", null, label));
  if (sub) inner.append(el("small", null, sub));
  lab.append(inner);
  box.append(svg, lab);
  return box;
}
levelRing.n = 0;

/** `.pts-bar`: a filled bar (0–1). */
function ptsBar(frac, cls = "") {
  const b = el("div", `pts-bar ${cls}`);
  const i = el("i");
  b.append(i);
  requestAnimationFrame(() => requestAnimationFrame(() => (i.style.width = `${Math.max(0, Math.min(1, frac)) * 100}%`)));
  return b;
}

/** `.skill-bars`: horizontal bars, one row per entry `{ key, label, value, max, text }`. */
function skillBars(rows) {
  const box = el("div", "skill-bars");
  for (const r of rows) {
    const row = el("div", `row s-${r.key}`);
    row.title = `${r.label}：${r.text ?? fmtPts(r.value)}`;
    const bar = el("div", "bar");
    const i = el("i");
    bar.append(i);
    row.append(el("span", "k", r.label), bar, el("span", "v", r.text ?? fmtPts(r.value)));
    requestAnimationFrame(() => requestAnimationFrame(() => (i.style.width = `${r.max > 0 ? Math.min(1, r.value / r.max) * 100 : 0}%`)));
    box.append(row);
  }
  return box;
}

/**
 * `.stars`: one star per milestone of the step (plan §9.1), the reached ones
 * filled. `compact` = one star and "3/15" (the today page).
 */
function starRow(m, compact = false) {
  const box = el("div", `stars${compact ? " compact" : ""}`);
  box.setAttribute("role", "img");
  box.setAttribute("aria-label", `${m.reached} / ${m.total} 顆星`);
  box.title = `每 ${fmtPts(m.milestone_points)} 分一顆星：${m.reached} / ${m.total}`;
  if (compact) {
    const s = el("span", m.reached > 0 ? "on" : "");
    s.append(icon("star"));
    box.append(s, el("b", null, `${m.reached}/${m.total}`));
    return box;
  }
  for (let i = 0; i < m.total; i++) {
    const s = el("span", i < m.reached ? "on" : "");
    s.append(icon("star"));
    box.append(s);
  }
  return box;
}

/**
 * A near goal from the recent pace and the star size: the stars this week
 * at the recent pace, the weeks to the next star (up to 4), or else the
 * points this week that reach the next star within a month.
 */
function nearStarGoal(m) {
  const pace = m.pace_per_week ?? 0, next = m.to_next_milestone;
  if (next == null || pace <= 0) return "";
  if (pace >= next) return `這週再拿 ${1 + Math.floor((pace - next) / (m.milestone_points || pace))} 顆星`;
  const weeks = Math.ceil(next / pace);
  if (weeks <= 4) return `照最近的速度，約 ${weeks} 週拿下一顆星`;
  // Very slow lately: a weekly goal that reaches the next star in a month.
  return `這週拿 ${fmtPts(Math.ceil(next / 4))} 分，一個月內拿下一顆星`;
}

/** The star a new summary reached over the previous one (same language and step), or 0. */
function starGain(prev, sum) {
  if (!prev?.milestones || !sum?.milestones || prev.lang !== sum.lang || prev.level !== sum.level) return 0;
  return sum.milestones.reached > prev.milestones.reached ? sum.milestones.reached : 0;
}

/** A user's picture (or coloured initial) on a `.dot` / `.big-dot`. */
function setAvatar(node, u) {
  if (!node) return;
  if (u?.avatar) {
    node.style.background = "";
    node.style.backgroundImage = `url("${u.avatar}")`;
    node.classList.add("has-img");
  } else {
    node.style.backgroundImage = "";
    node.style.background = u?.color ?? "var(--faint)";
    node.classList.remove("has-img");
  }
}

/** The pass line and the bonus line (config/progress.toml, via the summary). */
const passRules = () => state.summary?.rules ?? { pass_score: 0.7, bonus_score: 0.95, bonus: 0.5 };

/**
 * A score that did not pass: a bar with a tick at the pass line and how
 * many more right answers it takes (`total` items), encouraging, no "+0".
 */
function scoreMeter(score, total, mistakes) {
  const pass = passRules().pass_score;
  const box = el("div", "meter");
  const bar = el("div", "meter-bar");
  bar.setAttribute("role", "img");
  bar.setAttribute("aria-label", `${pct(score)}，過關 ${pct(pass)}`);
  const fill = el("i");
  const tick = el("b", "tick");
  tick.style.left = `${pass * 100}%`;
  tick.title = `過關 ${pct(pass)}`;
  bar.append(fill, tick);
  requestAnimationFrame(() => requestAnimationFrame(() => (fill.style.width = `${Math.max(0, Math.min(1, score)) * 100}%`)));
  box.append(bar);
  const lines = [];
  if (total) {
    const need = Math.max(1, Math.ceil(pass * total - 1e-9) - Math.round(score * total));
    lines.push(`${Math.round(score * total)} / ${total} 題・再 ${need} 題就過關`);
  } else lines.push(`${pct(score)}・差一點就過關`);
  box.append(el("div", "meter-t", lines.join("")));
  if (mistakes) {
    const m = el("div", "meter-m");
    m.append(icon("alert"), document.createTextNode("錯的題目已收進錯題本"));
    box.append(m);
  }
  return box;
}

/**
 * The card shown when something is finished: +points (with a small label
 * when the bonus came), a word of praise, a level-up banner, the early
 * difficulty question, a new star of the step, one or two next steps. A score that did not pass
 * shows a meter with the pass line instead of a big "+0".
 */
function resultCard({ points = 0, title, score, total, mistakes, bonusLabel, levelUp, already, difficulty, notebook, star = 0, next = [] }) {
  const card = el("div", `result${points > 0 || already ? "" : " missed"}`);
  card.setAttribute("role", "status");
  if (points > 0) {
    const plus = el("div", "plus", `+${fmtPts(points)}`);
    card.append(plus);
    if (points > 1) card.append(el("span", "bonus", bonusLabel ?? (score != null && score >= 0.999 ? "全對加分" : "高分加分")));
  } else if (already) card.append(el("div", "plus", "✓"));
  card.append(el("h3", null, title));
  if (points > 0 || already) {
    const meta = [];
    if (score != null) meta.push(pct(score));
    if (already) meta.push("已得過分");
    if (meta.length) card.append(el("div", "score", meta.join("・")));
    if (mistakes) card.append(el("div", "score", "錯的題目已收進錯題本"));
  } else if (score != null && total) card.append(scoreMeter(score, total, mistakes));
  if (levelUp) {
    const lv = el("div", "levelup");
    lv.append(icon("star"), document.createTextNode(`${levelUp.from} → ${levelUp.to}`));
    card.append(lv);
  } else if (star > 0) {
    // A milestone of the step was reached by this unit.
    const st = el("div", "levelup star-up");
    st.append(icon("star"), document.createTextNode(`拿到第 ${star} 顆星！`));
    card.append(st);
  }
  if (difficulty) {
    const d = el("div", "diff");
    d.append(el("div", "q", "這課的難度？"));
    const seg = el("div", "seg");
    for (const [v, label] of [["too_easy", "太簡單"], ["right", "剛好"], ["too_hard", "太難"]]) {
      const b = el("button", null, label);
      b.dataset.v = v;
      b.onclick = async () => {
        $$("button", seg).forEach((x) => (x.disabled = true));
        b.setAttribute("aria-pressed", "true");
        try {
          const r = await api("/api/difficulty/feedback", { lang: difficulty.lang, feedback: v });
          d.append(el("div", "q", r.change && r.change.from !== r.change.to ? `教材難度 → ${r.difficulty}` : "好，維持這個難度"));
          refreshStatus();
        } catch (e) { flash(e.message, true); }
      };
      seg.append(b);
    }
    d.append(seg);
    card.append(d);
  }
  const row = el("div", "next");
  next.filter(Boolean).forEach((n) => {
    const b = el("button", n.primary ? "primary" : "ghost", n.label);
    b.onclick = n.onclick;
    row.append(b);
  });
  // The review notebook page of what was just finished.
  if (notebook?.stem) {
    const b = ghostButton("notebook", "筆記本", () => go("notebook", { nbPage: notebook }));
    b.classList.add("nb-link");
    row.append(b);
  }
  if (row.children.length) card.append(row);
  setTimeout(() => card.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "center" }), 60);
  return card;
}

// ── today's data, shared ───────────────────────────────
// The profile card, the today page and the secretary all read /api/today;
// one request serves them when they ask at the same moment.
let todayCache = null;
function fetchToday(fresh = false) {
  const key = `${state.status?.user?.id ?? ""}/${state.lang}`;
  if (!fresh && todayCache && todayCache.key === key && Date.now() - todayCache.at < 4000) return todayCache.p;
  const p = api(`/api/today?lang=${state.lang}`);
  todayCache = { key, at: Date.now(), p };
  p.catch(() => { if (todayCache?.p === p) todayCache = null; });
  return p;
}

/** A task's name for buttons and the secretary (e.g. 中翻英, 訂正錯題). */
function taskName(task) {
  return task.id === "mistakes" ? "訂正錯題" : unitTitle(task.kind);
}

/**
 * What to do after something is finished: today's next task as the main
 * button (its name), or back to today when everything is done.
 */
async function nextTodayStep() {
  try {
    const t = await fetchToday(true);
    // The server decides what is done and next (a quick review step is done
    // once nothing is due and there were reviews today), like renderToday.
    const task = t.next != null ? t.tasks[t.next] : null;
    // Like the today card: a step that needs Claude asks to connect it first.
    if (task?.needs_engine && !state.status?.engine_ready) return { label: "連接 Claude", primary: true, onclick: () => openSettings(3) };
    if (task) return { label: `下一步：${taskName(task)}`, primary: true, onclick: () => (task.id === "mistakes" ? go("mistakes") : openStep(task)) };
  } catch { /* fall back to the today page */ }
  return { label: "回到今日", primary: true, onclick: () => go("today") };
}

// ── profile card ───────────────────────────────────────

let meBusy = false;
async function refreshMe() {
  const s = state.status;
  const u = s?.user;
  $("#meCard").hidden = !u;
  if (!u) return;
  $("#meName").textContent = u.name;
  const img = $("#meImg");
  img.hidden = !u.avatar;
  if (u.avatar && img.getAttribute("src") !== u.avatar) img.src = u.avatar;
  $("#meInitial").hidden = !!u.avatar;
  $("#meInitial").textContent = u.name.slice(0, 1).toUpperCase();
  $("#meAvatar").style.background = u.avatar ? "var(--surface-hover)" : u.color;
  if (meBusy) return;
  meBusy = true;
  try {
    const t = await fetchToday();
    paintMe(t.summary);
  } catch { /* the card keeps its last numbers */ } finally { meBusy = false; }
}

/** Paint the profile card; returns the star `sum` reached over the last summary (0 if none). */
function paintMe(sum) {
  if (!sum) return 0;
  const star = starGain(state.summary, sum);
  state.summary = sum;
  const lv = $("#meLevel");
  lv.innerHTML = "";
  lv.append(levelRing(30, "", sum.points_since / (sum.step_points || 1), null, sum.eligible), el("span", null, `${sum.level}`));
  lv.title = sum.next ? `下一階 ${sum.next}：${fmtPts(sum.points_since)} / ${fmtPts(sum.step_points)}` : "";
  $("#meToday").textContent = fmtPts(sum.today);
  $("#meTotal").textContent = fmtPts(sum.total);
  $("#meStreak").textContent = String(sum.streak);
  return star;
}

// Upload: crop to a square, resize to 256 px and compress in the browser.
$("#meAvatar").onclick = () => $("#meAvatarFile").click();
$("#meAvatarFile").onchange = async (e) => {
  const f = e.target.files?.[0];
  e.target.value = "";
  if (!f) return;
  if (!f.type.startsWith("image/")) { flash("請選一張圖片", true); return; }
  try {
    const blob = await squareImage(f, 256, 0.82);
    if (blob.size > 2 * 1024 * 1024) throw new Error("圖片太大");
    const r = await fetch("/api/user/avatar", { method: "POST", body: blob, headers: { "Content-Type": blob.type } });
    const d = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(d.error ?? `${r.status}`);
    flash("頭像已更新");
    await refreshStatus();
  } catch (err) { flash(err.message, true); }
};

function squareImage(file, size, quality) {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const im = new Image();
    im.onload = () => {
      const side = Math.min(im.naturalWidth, im.naturalHeight);
      const cv = document.createElement("canvas");
      cv.width = cv.height = Math.min(size, side);
      const ctx = cv.getContext("2d");
      ctx.drawImage(im, (im.naturalWidth - side) / 2, (im.naturalHeight - side) / 2, side, side, 0, 0, cv.width, cv.height);
      URL.revokeObjectURL(url);
      cv.toBlob((b) => (b ? resolve(b) : reject(new Error("無法處理圖片"))), "image/jpeg", quality);
    };
    im.onerror = () => { URL.revokeObjectURL(url); reject(new Error("讀不到這張圖片")); };
    im.src = url;
  });
}

// ── today: the task list ───────────────────────────────

async function renderToday(page) {
  viewbar("今日", null);
  if (!state.status?.user) { page.append(el("div", "empty", "請先選擇使用者")); return; }
  const t = await fetchToday();
  const sum = t.summary;
  paintMe(sum);

  // The big ring counts today's points towards the daily goal; the level
  // is a thin bar beside it. Reaching the goal turns the ring -3 with a check.
  const reached = sum.goal > 0 && sum.today >= sum.goal;
  const head = el("div", `today-head${reached ? " reached" : ""}`);
  const ring = levelRing(132, fmtPts(sum.today), sum.today / (sum.goal || 1), `/ ${fmtPts(sum.goal)} 分`, reached);
  ring.classList.add("goal-ring");
  ring.title = `今日積分 ${fmtPts(sum.today)} / ${fmtPts(sum.goal)}`;
  if (reached) { const ck = el("span", "goal-check"); ck.append(icon("check")); ring.append(ck); }
  const mid = el("div", "pts");
  mid.append(el("div", "goal-line", reached ? "今天的目標達成了！" : `還差 ${fmtPts(Math.max(0, sum.goal - sum.today))} 分`));
  const lv = el("div", "lv-row");
  lv.append(el("b", null, sum.level), el("span", null, sum.next ? `→ ${sum.next}` : ""));
  const lvBar = ptsBar(sum.points_since / (sum.step_points || 1), `thin${sum.eligible ? " ok" : ""}`);
  lvBar.title = sum.next ? `下一階 ${sum.next}：${fmtPts(sum.points_since)} / ${fmtPts(sum.step_points)} 分` : "";
  mid.append(lv, lvBar);
  if (sum.next && sum.milestones) mid.append(starRow(sum.milestones, true));
  const streak = el("div", "streak");
  streak.title = "連續天數";
  streak.append(icon("flame"), el("b", null, String(sum.streak)), el("span", null, "天"));
  head.append(ring, mid, streak);
  page.append(head);

  const ready = !!state.status?.engine_ready;
  // One group per lesson of today (and the daily lesson not created yet).
  const groups = [];
  const keyOf = (task) => (task.daily ? "daily" : task.course ? `${task.course}#${task.number}` : "other");
  t.tasks.forEach((task, i) => {
    const k = keyOf(task);
    let g = groups.find((x) => x.key === k);
    if (!g) { g = { key: k, tasks: [] }; groups.push(g); }
    g.tasks.push([task, i]);
  });
  for (const g of groups) {
    const box = el("div", "today-lesson");
    if (g.key !== "other") {
      const les = g.key === "daily" ? null : t.lessons.find((l) => `${l.course}#${l.number}` === g.key);
      const hd = el("div", "tl-head");
      const kind = g.key === "daily" ? "每日課程" : les?.kind_label ?? "";
      hd.append(el("span", `tag ${g.key === "daily" || les?.kind === "daily" ? "acc2" : "acc"}`, kind));
      const title = g.key === "daily" ? (t.daily?.topic?.name ?? "今日課程") : les?.kind === "daily" ? les.title : lessonLabel(les?.number, les?.title);
      hd.append(elr("b", null, title, state.lang));
      if (les && les.kind !== "daily") hd.append(el("span", "sub", les.course_name));
      // Steps of the lesson (the ring above counts points).
      const doneN = g.tasks.filter(([x]) => x.done).length;
      const mins = g.key === "daily" ? t.daily?.minutes : les?.minutes;
      const meta = el("span", "n", `${doneN} / ${g.tasks.length} 步${mins ? `・${mins} 分鐘` : ""}`);
      hd.append(meta);
      box.append(hd);
    }
    const list = el("div", "tasks");
    g.tasks.forEach(([task, i]) => list.append(taskCard(task, i === t.next, ready)));
    box.append(list);
    page.append(box);
  }
  if (t.next == null) {
    const done = el("div", "all-done");
    const m = el("div", "done-mark"); const sp = el("span"); sp.append(icon("check")); m.append(sp);
    done.append(m, el("h3", null, "今天的課完成了"));
    const row = el("div", "actions-row center");
    row.append(ghostButton("layers", "課程", () => go("courses")), ghostButton("chat", "再一段對話", () => go("conversation", { conv: null })));
    done.append(row);
    page.append(done);
  }
}

/** 第 3 課・Title, without repeating 第 3 課 when the title already starts with it. */
function lessonLabel(n, title) {
  const t = String(title ?? "").replace(/^第\s*\d+\s*課\s*[：:・]\s*/, "");
  return t ? `第 ${n} 課・${t}` : `第 ${n} 課`;
}

const UNIT_TITLE = { review: "複習卡片", reading: "課文與閱讀", translation: "翻譯", "sentence-making": "造句", cloze: "填空", paraphrase: "換句話說", conversation: "AI 對話", quiz: "測驗", listening: "聽力", dictation: "聽寫", shadowing: "跟讀" };
const UNIT_ICON = { review: "cards", reading: "book", translation: "pen", "sentence-making": "pen", cloze: "pen", paraphrase: "pen", conversation: "chat", quiz: "check" };
const unitTitle = (k, lang = state.lang) => (k === "translation" ? (lang === "ja" ? "中翻日" : "中翻英") : UNIT_TITLE[k] ?? k);

/** Open a step of a course lesson (creating today's daily lesson first if needed). */
async function openStep(task) {
  if (task.kind === "review" && task.lesson_id == null && !state.status?.engine_ready) {
    go("courses", { quickReview: true, fromToday: true, inDaily: true, courseCreate: null });
    return;
  }
  if (task.daily) {
    try {
      const d = await api("/api/daily", { lang: state.lang });
      go("courses", { course: d.id, courseLesson: d.next ?? 1, step: task.unit, fromToday: true, courseCreate: null });
    } catch (e) { flash(e.message, true); }
    return;
  }
  go("courses", { course: task.course, courseLesson: task.number, step: task.unit, fromToday: true, courseCreate: null });
}

function taskCard(task, isNext, engineReady) {
  const card = el("div", `task${task.done ? " done" : ""}${isNext ? " next" : ""}`);
  card.dataset.task = task.kind ?? task.id;
  const ic = el("div", "ic");
  const body = el("div");
  let title, sub, action, label = "開始", reward = null;
  if (task.id === "unit") {
    ic.append(icon(UNIT_ICON[task.kind] ?? "book"));
    title = unitTitle(task.kind);
    // A review step without a lesson and without Claude is a quick review:
    // its point comes per full round of cards (progress.toml review_round).
    const quick = task.kind === "review" && task.lesson_id == null && !engineReady;
    if (task.kind === "review") {
      const parts = [];
      if (task.due) parts.push(`${task.due} 張到期`);
      if (quick && task.round) parts.push(`本輪 ${task.round.done} / ${task.round.size} 張`);
      sub = parts.join("・");
    } else if (task.kind === "conversation") sub = `${task.items} 回合`;
    else sub = `${task.items} 題`;
    // Each step passed = 1 point (more with a high score).
    reward = quick && task.round ? `滿 ${task.round.size} 張 +1・約 ${task.minutes} 分鐘` : `+1・約 ${task.minutes} 分鐘`;
    action = () => openStep(task);
    if (task.unit > 0 && !task.done) label = "繼續";
  } else {
    ic.append(icon("alert"));
    title = "訂正錯題";
    sub = `${task.open} 題`;
    action = () => go("mistakes");
  }
  if (task.needs_engine && !engineReady && !task.done) {
    action = () => openSettings(3);
    label = "連接 Claude";
  }
  const s = el("div", "s", sub);
  if (reward && !task.done) s.append(el("span", "rw", reward));
  body.append(el("div", "t", title), s);
  card.append(ic, body);
  if (task.done) {
    const m = el("span", "mark");
    if (task.points != null) m.append(el("b", null, `+${fmtPts(task.points)}`));
    m.append(icon("check"));
    card.append(m);
  } else if (isNext) {
    const b = primaryButton(label, (e) => { e.stopPropagation(); action(); });
    b.classList.add("go");
    card.append(b);
  }
  card.onclick = action;
  return card;
}

// ── lessons: quiz result and reading time ──────────────

async function quizAnswered(quiz, index, answer, correct) {
  quiz.answers[index] = { index, answer: answer ?? "", correct };
  if (quiz.done || Object.keys(quiz.answers).length < quiz.total) return;
  quiz.done = true;
  if (quiz.onDone) return quiz.onDone(Object.values(quiz.answers));
  try {
    const r = await api(`/api/lessons/${quiz.id}/finish`, { answers: Object.values(quiz.answers) });
    const star = paintMe(r.summary);
    // Each lesson scores once, so a miss is not "try again": the wrong
    // answers are in the mistakes book, and correcting them is the next step.
    const wrong = Object.values(quiz.answers).some((a) => a.correct === false);
    const next = [];
    if (!r.passed && !r.already && wrong) next.push({ label: "訂正錯題", primary: true, onclick: () => go("mistakes") });
    else next.push(await nextTodayStep());
    next.push({ label: "練習對話", onclick: () => go("conversation", { conv: null }) });
    const card = resultCard({
      points: r.points,
      title: r.already ? "又完成一次" : r.passed ? "測驗完成" : "差一點",
      score: r.score,
      total: quiz.total,
      mistakes: wrong,
      already: r.already,
      levelUp: r.level_up,
      star,
      difficulty: r.ask_difficulty && !r.already ? { lang: quiz.lang } : null,
      next,
      notebook: r.notebook ? { lang: quiz.lang, stem: r.notebook } : null,
    });
    quiz.after.after(card);
    refreshStatus().then(greet);
  } catch (e) { flash(e.message, true); }
}

// Passive reading: counted only while a lesson is on screen and the learner
// is active (no input for 2 minutes = idle). Nothing can be logged by hand.
let lastActive = Date.now();
["mousemove", "keydown", "scroll", "touchstart", "wheel"].forEach((ev) => document.addEventListener(ev, () => (lastActive = Date.now()), { passive: true, capture: true }));
let readingTimer = null;
function startReading() {
  if (readingTimer) return;
  readingTimer = setInterval(async () => {
    const reading = ((state.view === "lessons" && state.lessonId) || (state.view === "courses" && state.courseLesson)) && document.visibilityState === "visible" && Date.now() - lastActive < 120000;
    if (!reading || !state.status?.user) return;
    try {
      const r = await api("/api/passive", { lang: state.lang, kind: "reading", seconds: 30 });
      if (r.awarded) { flash(`閱讀時間 +${r.awarded}`); refreshStatus(); }
    } catch { /* not important enough to interrupt */ }
  }, 30000);
}

// ── conversation ───────────────────────────────────────

let convTimer = null;
let skipTopics = [];

async function renderConversation(page) {
  clearInterval(convTimer);
  viewbar(`對話・${langLabel(state.lang)}`, null);
  if (!state.status?.user) { page.append(el("div", "empty", "請先選擇使用者")); return; }
  const list = await api(`/api/conversations?lang=${state.lang}`);
  if (!state.conv) {
    const open = list.find((c) => c.status !== "done");
    if (open) state.conv = { id: open.id };
  }
  if (state.conv?.id && !state.conv.conversation) {
    try { state.conv = await api(`/api/conversations/${state.conv.id}?user=${encodeURIComponent(state.status.user.id)}`); } catch { state.conv = null; }
  }
  if (state.conv && state.conv.conversation.lang !== state.lang) state.conv = null;
  if (!state.conv) return drawConvStart(page, list);
  drawConv(page);
}

async function drawConvStart(page, list) {
  const box = el("div", "conv-start");
  page.append(box);
  let next;
  try {
    next = await api(`/api/conversations/next?lang=${state.lang}${skipTopics.length ? `&skip=${encodeURIComponent(skipTopics.join(","))}` : ""}`);
  } catch (e) { box.append(el("div", "note err", e.message)); return; }
  const topic = el("div", "topic");
  topic.append(el("b", null, next.topic.name), el("span", `tag${next.topic.kind === "interest" ? " acc" : ""}`, next.topic.kind === "interest" ? "興趣" : "通識"));
  const another = iconButton("refresh", "換一個主題", () => { skipTopics.push(next.topic.id); render(); });
  topic.append(another);
  const facts = el("div", "facts");
  const f1 = el("span"); f1.append(icon("chat"), document.createTextNode(`${next.turns} 回合`));
  const f2 = el("span"); f2.append(icon("clock"), document.createTextNode(`每回合 ${next.seconds_per_turn} 秒`));
  const f3 = el("span"); f3.append(icon("target"), document.createTextNode(next.level));
  facts.append(f1, f2, f3);
  const row = el("div", "row");
  const startBtn = primaryButton("開始對話", () => busy(startBtn, async () => {
    startBtn.textContent = "準備中…";
    try {
      state.conv = await api("/api/conversations", { lang: state.lang, topic_id: next.topic.id });
      skipTopics = [];
      render();
    } catch (e) { flash(e.message, true); startBtn.textContent = "開始對話"; }
  }));
  if (!state.status?.engine_ready) {
    startBtn.textContent = "連接 Claude";
    startBtn.onclick = () => openSettings(3);
  }
  const help = el("button", "help", "?");
  help.dataset.tip = state.lang === "ja"
    ? "用簡單的日文聊天，可以混中文。AI 會用日文把你的意思重講一遍（↺），不會說你錯。下面有句型開頭可以點。完成全部回合就得分。"
    : "AI 會把你的意思用更自然的說法重講一遍（↺），再問下一個問題，不會說你錯。試著用上目標字，用到時會亮起。完成全部回合就得分。";
  row.append(startBtn, help);
  box.append(topic, facts, row);

  const done = list.filter((c) => c.status === "done");
  if (done.length) {
    const card = el("div", "card");
    card.append(el("h3", null, "完成的對話"));
    const ul = el("ul", "list");
    for (const c of done.slice(0, 20)) {
      const li = el("li", "item click");
      li.append(el("div", "t", c.topic));
      const meta = el("div", "meta");
      meta.append(el("span", null, fmtDate(c.started_at)), el("span", null, `${c.turns_done} 回合`));
      if (c.score != null) meta.append(el("span", null, pct(c.score)));
      li.append(meta);
      const side = el("div", "side");
      side.append(el("span", `tag${c.points > 0 ? " acc" : ""}`, `+${fmtPts(c.points)}`));
      li.append(side);
      li.onclick = () => { state.conv = { id: c.id }; render(); };
      ul.append(li);
    }
    card.append(ul);
    page.append(card);
  }
}

function drawConv(page, newlyUsed = []) {
  clearInterval(convTimer);
  page.innerHTML = "";
  const v = state.conv;
  const c = v.conversation;
  const ja = c.lang === "ja";
  // Only a finished conversation can be left for another; an open one waits.
  const back = c.course
    ? ghostButton("back", "這一課", () => backToLesson(c.course))
    : v.status === "done" ? ghostButton("back", "主題", () => { state.conv = null; render(); }) : null;
  viewbar(`對話・${langLabel(c.lang)}`, null, ...(back ? [back] : []));

  // Head: topic, dot progress (last 3 highlighted), target words.
  const head = el("div", "conv-head");
  const top = el("div", "ch-top");
  // In a course lesson the tag names the course (not 興趣／通識).
  const courseName = c.course ? (state.cl?.course?.id === c.course.course ? state.cl.course.name : "課程") : null;
  const tag = c.course
    ? el("span", "tag acc2", c.course.lesson ? `${courseName}・第 ${c.course.lesson} 課` : courseName)
    : el("span", `tag${c.topic.kind === "interest" ? " acc" : ""}`, c.topic.kind === "interest" ? "興趣" : "通識");
  top.append(el("b", null, c.topic.name), tag, el("span", "n", `${v.turns_done} / ${v.turns_limit}`));
  const dots = el("div", "dots");
  dots.setAttribute("aria-label", `${v.turns_done} / ${v.turns_limit} 回合`);
  for (let i = 0; i < v.turns_limit; i++) {
    const d = el("i");
    if (i < v.turns_done) d.className = "on";
    else if (i === v.turns_done && v.status === "active") d.className = "now";
    if (i >= v.turns_limit - 3) d.classList.add("end");
    dots.append(d);
  }
  head.append(top, dots);
  if (c.targets.length) {
    const tg = el("div", "targets");
    for (const t of c.targets) {
      const used = v.used.includes(t.term);
      const chip = el("span", `target${used ? " used" : ""}${newlyUsed.includes(t.term) ? " new" : ""}${ja ? " ja" : ""}`, t.term);
      chip.title = [t.reading, t.meaning].filter(Boolean).join("・");
      tg.append(chip);
    }
    head.append(tg);
  }
  page.append(head);

  // Transcript.
  const tr = el("div", "transcript");
  for (const turn of c.turns) tr.append(bubble(turn, ja));
  page.append(tr);

  if (v.status === "active") page.append(composer(v, tr, ja));
  else drawConvEnd(page, v);
  requestAnimationFrame(() => { $("#pageArea").scrollTop = $("#pageArea").scrollHeight; });
}

function bubble(turn, ja) {
  const b = el("div", `bubble ${turn.role === "ai" ? "from-ai" : "from-me"}${ja ? " ja" : ""}`);
  if (turn.role === "ai" && turn.recast) {
    const r = el("div", "recast");
    const mark = el("span", "r", "↺");
    mark.title = "換個說法";
    r.append(mark, el("span", null, turn.recast));
    b.append(r);
  }
  b.append(document.createTextNode(turn.text));
  if (turn.role === "ai") b.append(Speech.speakerButton([turn.recast, turn.text].filter(Boolean).join(" "), ja ? "ja" : "en"));
  if (turn.role === "me" && turn.used?.length) b.append(el("span", "used", `✓ ${turn.used.join("、")}`));
  return b;
}

function composer(v, tr, ja) {
  const box = el("div", "composer");
  const last = [...v.conversation.turns].reverse().find((t) => t.role === "ai");
  const ta = el("textarea");
  ta.rows = 2;
  ta.maxLength = v.max_chars;
  ta.placeholder = ja ? "日本語で（中文也可以）" : "Type your answer";
  if (ja) ta.lang = "ja";
  if (last?.starters?.length) {
    const st = el("div", "starters");
    for (const s of last.starters) {
      const chip = el("button", "chip-btn ja", s);
      chip.onclick = () => { ta.value = s.replace(/[〜…]+$/u, ""); ta.focus(); onInput(); };
      st.append(chip);
    }
    box.append(st);
  }
  const count = el("span", "count", `0/${v.max_chars}`);
  // Countdown ring: a pace guide for typing; the turn can be sent any time.
  const timer = el("div", "timer");
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 40 40");
  const C = 2 * Math.PI * 17;
  let fill;
  for (const cls of ["track", "fill"]) {
    const ci = document.createElementNS(ns, "circle");
    ci.setAttribute("class", cls); ci.setAttribute("cx", 20); ci.setAttribute("cy", 20); ci.setAttribute("r", 17);
    if (cls === "fill") { ci.setAttribute("stroke-dasharray", C); ci.setAttribute("stroke-dashoffset", 0); fill = ci; }
    svg.append(ci);
  }
  const secsLabel = el("span", null, String(v.seconds_per_turn));
  timer.append(svg, secsLabel);
  timer.title = "建議作答時間";
  const send = el("button", "send");
  send.setAttribute("aria-label", "送出");
  send.append(icon("send"));
  send.disabled = true;
  const wrap = el("div", "box");
  // Speak instead of typing (recognition on): the text box fills, send stays manual.
  let before = "";
  const mic = Speech.micButton(ja ? "ja" : "en", (t) => { ta.value = `${before}${before && !ja ? " " : ""}${t}`.slice(0, v.max_chars); onInput(); });
  mic.addEventListener("click", () => { if (!mic.classList.contains("listening")) before = ta.value.trim(); }, true);
  wrap.append(ta, timer, mic, send);
  const hint = el("div", "c-hint");
  box.append(wrap, el("div", "actions-row"), hint);
  $(".actions-row", box).append(count);

  const total = v.seconds_per_turn;
  let left = total;
  convTimer = setInterval(() => {
    // While the mic records or the server transcribes, its own countdown is
    // the one that matters: the turn ring waits, dimmed (one number at a time).
    const speaking = mic.classList.contains("listening") || mic.classList.contains("busy");
    timer.classList.toggle("paused", speaking);
    if (speaking) return;
    left -= 1;
    if (left <= 0) { clearInterval(convTimer); timer.classList.add("over"); secsLabel.textContent = "0"; fill.setAttribute("stroke-dashoffset", C); return; }
    secsLabel.textContent = String(left);
    fill.setAttribute("stroke-dashoffset", C * (1 - left / total));
  }, 1000);

  function onInput() {
    // Grow with the text (a spoken answer can be long), up to the CSS max-height.
    ta.style.height = "auto";
    ta.style.height = `${ta.scrollHeight}px`;
    count.textContent = `${ta.value.length}/${v.max_chars}`;
    send.disabled = !ta.value.trim();
    hint.textContent = "";
  }
  ta.oninput = onInput;
  ta.onkeydown = (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); if (!send.disabled) doSend(); }
  };
  send.onclick = doSend;

  async function doSend() {
    const text = ta.value.trim();
    if (!text) return;
    send.disabled = true; ta.disabled = true;
    clearInterval(convTimer);
    const mine = bubble({ role: "me", text }, ja);
    const typing = el("div", "bubble from-ai typing");
    typing.append(el("i"), el("i"), el("i"));
    tr.append(mine, typing);
    $("#pageArea").scrollTop = $("#pageArea").scrollHeight;
    const usedBefore = v.used;
    try {
      const r = await api(`/api/conversations/${v.id}/turns?user=${encodeURIComponent(state.status?.user?.id ?? "")}`, { text });
      state.conv = r;
      drawConv($("#page"), r.used.filter((u) => !usedBefore.includes(u)));
      // Auto-read the AI's answer (Settings → 語音).
      const ai = [...r.conversation.turns].reverse().find((t) => t.role === "ai");
      if (ai && Speech.prefs().autoRead) Speech.speak([ai.recast, ai.text].filter(Boolean).join(" "), ja ? "ja" : "en", $$(".bubble.from-ai .spk").pop());
      if (r.locked) finishConversation();
    } catch (e) {
      mine.remove(); typing.remove();
      ta.disabled = false; send.disabled = false;
      hint.textContent = e.message;
      ta.focus();
    }
  }
  setTimeout(() => ta.focus(), 50);
  return box;
}

function drawConvEnd(page, v) {
  if (v.status === "locked") {
    const wait = el("div", "conv-done wait");
    wait.id = "convWait";
    wait.append(el("span", "spin"), document.createTextNode("評估中…"));
    page.append(wait);
    return;
  }
  const c = v.conversation;
  const res = v.result ?? { points: c.points, score: c.score, passed: c.passed, review: c.review, used: v.used };
  const nb = res.notebook ? { lang: c.lang, stem: res.notebook } : null;
  if (v.justFinished) {
    // One card: the points, the praise and the next step (no separate bar).
    const course = res.course;
    const daily = c.course && state.cl?.course?.id === c.course.course && state.cl.course.kind === "daily";
    const next = [];
    if (c.course && (course?.lesson_done || daily)) next.push(v.nextToday ?? { label: "回到今日", primary: true, onclick: () => go("today") });
    else if (c.course) next.push({ label: "回到這一課", primary: true, onclick: () => backToLesson(c.course) });
    else if (v.nextToday && v.nextToday.label !== "回到今日") next.push(v.nextToday, { label: "再一段", onclick: () => { state.conv = null; render(); } });
    else next.push({ label: "再一段", primary: true, onclick: () => { state.conv = null; render(); } }, { label: "回到今日", onclick: () => go("today") });
    if (c.course && course?.next?.lesson && course.next.lesson !== c.course.lesson && !daily) next.push({ label: "下一課", onclick: () => go("courses", { course: c.course.course, courseLesson: course.next.lesson, step: null }) });
    page.append(resultCard({
      points: res.points,
      title: course?.lesson_done ? "這一課完成了！" : res.passed ? (res.excellent ? "說得真好！" : "完成這段對話") : "完成了，下次再多說一點",
      score: res.passed ? res.score : null,
      bonusLabel: "表現優秀加分",
      levelUp: res.level_up,
      star: v.star,
      notebook: nb,
      next,
    }));
  } else {
    const done = el("div", "conv-done");
    done.append(el("b", null, "完成"));
    if (c.points != null) done.append(el("span", "plus", `+${fmtPts(c.points)}`));
    page.append(done);
  }
  if (c.review) page.append(feedbackCard(c, v.used));
  if (!v.justFinished) {
    const row = el("div", "actions-row");
    if (c.course) row.append(primaryButton("回到這一課", () => backToLesson(c.course), true));
    else row.append(primaryButton("再一段", () => { state.conv = null; render(); }, true));
    page.append(row);
  }
}

function backToLesson(link) {
  state.conv = null;
  go("courses", { course: link.course, courseLesson: link.lesson, step: null });
}

async function finishConversation() {
  try {
    const r = await api(`/api/conversations/${state.conv.id}/finish?user=${encodeURIComponent(state.status?.user?.id ?? "")}`, {});
    r.justFinished = true;
    if (r.summary) r.star = paintMe(r.summary);
    r.nextToday = await nextTodayStep();
    state.conv = r;
    drawConv($("#page"));
    refreshStatus().then(greet);
  } catch (e) {
    const w = $("#convWait");
    if (w) {
      w.textContent = e.message;
      w.append(el("br"), ghostButton("refresh", "再試一次", () => { w.textContent = "評估中…"; finishConversation(); }));
    }
  }
}

function feedbackCard(c, used) {
  const r = c.review;
  const ja = c.lang === "ja";
  const card = el("div", "feedback");
  card.append(el("div", null, r.summary));
  card.append(skillBars([
    { key: "fluency", label: "流暢度", value: r.fluency, max: 5, text: `${r.fluency}/5` },
    { key: "targets", label: "目標字", value: r.target_use, max: 5, text: `${r.target_use}/5` },
    { key: "grammar", label: "文法", value: r.grammar, max: 5, text: `${r.grammar}/5` },
  ]));
  if (r.better?.length) {
    const sec = el("div", "better");
    sec.append(el("h4", null, "可以這樣說"));
    const ul = el("ul");
    for (const b of r.better) {
      const li = el("li", ja ? "ja" : null);
      li.append(el("span", "o", b.original), document.createTextNode(" → "), el("span", "s", b.suggestion));
      if (b.why) li.append(el("div", "w", b.why));
      ul.append(li);
    }
    sec.append(ul);
    card.append(sec);
  }
  if (c.targets.length) {
    const sec = el("div");
    sec.append(el("h4", null, "目標字"));
    const tg = el("div", "targets");
    for (const t of c.targets) tg.append(el("span", `target${used.includes(t.term) ? " used" : ""}`, t.term));
    sec.append(tg);
    card.append(sec);
  }
  if (r.suggested_words?.length) {
    const sec = el("div", "words");
    sec.append(el("h4", null, "加入生字"));
    for (const w of r.suggested_words) {
      const row = el("div", "w");
      row.append(el("b", ja ? "ja" : null, w.term), el("span", "m", [w.reading, w.meaning].filter(Boolean).join("・")));
      const add = iconButton("plus", "加入生字", async () => {
        try {
          await api("/api/words", { lang: c.lang, term: w.term, reading: w.reading, meaning: w.meaning });
          add.replaceWith(Object.assign(el("span", "tag ok", "已加入")));
          refreshStatus();
        } catch (e) { flash(e.message, true); }
      });
      row.append(add);
      sec.append(row);
    }
    card.append(sec);
  }
  if (r.keywords?.length) {
    const sec = el("div");
    sec.append(el("h4", null, "關鍵字"));
    const ch = el("div", "topic-chips");
    r.keywords.forEach((k) => ch.append(el("span", "on", k)));
    sec.append(ch);
    card.append(sec);
  }
  return card;
}

// ── progress ───────────────────────────────────────────

async function renderProgress(page) {
  viewbar(`進度・${langLabel(state.lang)}`, null);
  if (!state.status?.user) { page.append(el("div", "empty", "請先選擇使用者")); return; }
  const p = await api(`/api/progress?lang=${state.lang}`);
  const sum = p.summary;
  const a = p.assessment;
  paintMe(sum);

  const head = el("div", "prog-head");
  head.append(levelRing(128, sum.level, a.points_since / (a.step_points || 1), `教材 ${sum.difficulty}`, a.eligible));
  const right = el("div");
  right.append(el("div", "next", a.next ? `下一階 ${a.next}` : "最高等級"));
  // Milestones: a star every few points, and the weeks at the recent pace.
  const m = sum.milestones;
  if (a.next && m) {
    const ms = el("div", "milestones");
    const stars = el("div", "ms-stars");
    stars.append(starRow(m));
    ms.append(stars);
    const lines = el("div", "ms-lines");
    if (m.to_next_milestone != null) lines.append(el("span", null, `再 ${fmtPts(m.to_next_milestone)} 分拿下一顆星`));
    if (m.weeks_to_next_step == null) lines.append(el("span", "muted", "多學幾天就能估算"));
    else if (m.weeks_to_next_step > 12) {
      // A long way off: a near goal instead (plan §9.1), the long estimate behind ?.
      lines.append(el("span", "muted", nearStarGoal(m)));
      const help = el("button", "help", "?");
      help.dataset.tip = `照最近的速度（每週約 ${fmtPts(m.pace_per_week)} 分），約 ${m.weeks_to_next_step} 週升到 ${a.next}。`;
      stars.append(help);
    } else if (m.weeks_to_next_step > 0) lines.append(el("span", "muted", `照最近的速度，約 ${m.weeks_to_next_step} 週升到 ${a.next}`));
    ms.append(lines);
    right.append(ms);
  }
  const conds = el("ul", "conds");
  const r = p.rules;
  for (const c of a.conditions) {
    const li = el("li", `${c.ok ? "ok" : ""}${c.id === "major_check" ? " lock" : ""}`);
    const mark = el("span", "c");
    mark.append(icon(c.id === "major_check" ? "lock" : "check"));
    let t, v, frac;
    if (c.id === "points") { t = "積分"; v = `${fmtPts(c.value)} / ${fmtPts(c.need)}`; frac = c.value / c.need; }
    else if (c.id === "balance") { t = "輸出（說＋寫）"; v = `${pct(c.value)} / ${pct(c.need)}`; frac = c.value / c.need; }
    else if (c.id === "recent") { t = `最近 ${r.recent_units} 個單元`; v = `${pct(c.value)} / ${pct(c.need)}`; frac = c.value / c.need; }
    else { t = "大級檢核"; v = "之後開放"; frac = 0; }
    li.append(mark, el("span", "t", t), el("span", "v", v));
    if (c.id !== "major_check") li.append(ptsBar(frac, c.ok ? "ok thin" : "thin"));
    conds.append(li);
  }
  const help = el("button", "help", "?");
  help.dataset.tip = "等級只升不降。三個條件都達到就升一階：本階積分、輸出（說＋寫）的比例、最近單元的平均分數。跨到下一個大級要再通過大級檢核（之後的階段）。教材難度是另一件事，會依你的回饋與正確率自動調整。";
  right.append(conds, help);
  head.append(right);
  page.append(head);

  const g = el("div", "grid2");
  const skills = el("div", "card");
  const sh = el("h3", null, "技能");
  sh.append(el("span", "c", `共 ${fmtPts(sum.total_lang)} 分`));
  skills.append(sh);
  const max = Math.max(1, ...["listening", "reading", "speaking", "writing", "vocab"].map((k) => p.skills[k]));
  skills.append(skillBars(["listening", "reading", "speaking", "writing", "vocab"].map((k) => ({ key: k, label: SKILL_LABEL[k], value: p.skills[k], max }))));
  g.append(skills);

  const days = el("div", "card");
  days.append(el("h3", null, "最近 14 天"));
  const dmax = Math.max(sum.goal, ...p.days.map((d) => d[1]));
  const bars = el("div", "days");
  const axis = el("div", "days-axis");
  p.days.forEach(([day, pts], i) => {
    const d = el("div", `d${pts > 0 ? "" : " zero"}`);
    d.title = `${day.slice(5)}：${fmtPts(pts)} 分`;
    const bar = el("i");
    bar.style.height = `${(pts / dmax) * 100}%`;
    d.append(bar);
    bars.append(d);
    axis.append(el("span", null, i % 7 === 0 || i === 13 ? day.slice(5).replace("-", "/") : ""));
  });
  days.append(bars, axis);
  g.append(days);
  page.append(g);

  // Two different things, kept apart: the learner's interests
  // (interests.csv) and the topics their conversations were about.
  const mine = (state.status?.user?.interests ?? []).filter((i) => !i.lang || i.lang === state.lang);
  if (mine.length) {
    const ic = el("div", "card");
    ic.append(el("h3", null, "我的興趣"));
    const chips = el("div", "topic-chips");
    for (const i of mine) chips.append(el("span", "on", i.keyword));
    ic.append(chips);
    page.append(ic);
  }
  const topics = el("div", "card");
  const th = el("h3", null, "對話聊過的主題");
  const thelp = el("button", "help", "?");
  thelp.dataset.tip = "AI 對話用過的主題與次數（興趣主題與通識主題）。選題時會挑最久沒聊的。";
  th.append(el("span", "grow"), thelp);
  topics.append(th);
  const chips = el("div", "topic-chips");
  for (const t of [...p.interests, ...p.topics]) {
    const c = el("span", t.count ? "on" : "", t.name);
    if (t.count) c.append(el("em", null, String(t.count)));
    chips.append(c);
  }
  topics.append(chips);
  page.append(topics);

  if (p.recent.length) {
    const rec = el("div", "card");
    rec.append(el("h3", null, "最近得分"));
    const ul = el("ul", "list");
    for (const x of p.recent) {
      const li = el("li", "item");
      li.append(el("div", "t", UNIT_LABEL[x.unit] ?? x.unit));
      const meta = el("div", "meta");
      meta.append(el("span", null, fmtDate(x.time)), el("span", null, SKILL_LABEL[x.skill] ?? x.skill));
      if (x.score != null) meta.append(el("span", null, pct(x.score)));
      li.append(meta);
      const side = el("div", "side");
      side.append(el("span", `tag${x.points > 0 ? " acc" : ""}`, `+${fmtPts(x.points)}`));
      li.append(side);
      ul.append(li);
    }
    rec.append(ul);
    page.append(rec);
  }
}

// ── import: drop anywhere, or the dialog ───────────────

const dlgImport = $("#dlgImport");
function openImport() {
  if (!dlgImport.open) dlgImport.showModal();
}
$(".close", dlgImport).onclick = () => dlgImport.close();
$("#actImport").onclick = () => { $("#impResults").innerHTML = ""; openImport(); };
$("#impDrop").onclick = () => $("#impFiles").click();
$("#impDrop").onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); $("#impFiles").click(); } };
$("#impFiles").onchange = (e) => { importFiles([...e.target.files]); e.target.value = ""; };
$("#asideDrop").onclick = () => { $("#impResults").innerHTML = ""; openImport(); };
$("#asideDrop").onkeydown = (e) => { if (e.key === "Enter") $("#asideDrop").click(); };
// A word list template to start from (UTF-8 with BOM so spreadsheets read it).
$("#impTemplate").onclick = (e) => {
  e.preventDefault();
  const csv = "\ufeffterm,reading,meaning,example\nresilient,/rɪˈzɪliənt/,有韌性的,The team stayed resilient.\n締め切り,しめきり,截止日期,締め切りは金曜日です。\n";
  const a = el("a");
  a.href = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
  a.download = "vocab-template.csv";
  document.body.append(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
};

/** Empty states point straight at importing. */
function emptyImport(text) {
  const e = el("div", "empty cta", text);
  const d = el("div", "drop small");
  d.append(icon("upload"), el("span", null, "拖放或選擇檔案"));
  d.onclick = () => { $("#impResults").innerHTML = ""; openImport(); };
  e.append(d);
  return e;
}

const KIND_LABEL = { vocab: "單字", grammar: "文法", materials: "每週素材", text: "筆記", pdf: "PDF", image: "圖片" };
/** Kept as files in materials/ (read in a later stage), not imported into review. */
const isMaterialFile = (f) => /\.(md|markdown|txt|pdf|png|jpe?g|webp|gif)$/i.test(f.name) || f.type.startsWith("image/");
async function importFiles(files) {
  if (!files.length) return;
  openImport();
  const ul = $("#impResults");
  for (const f of files) {
    const li = el("li");
    const mark = el("span", null, "…");
    const what = el("span", null, f.name);
    const info = el("span", "f", "");
    li.append(mark, what, info);
    ul.prepend(li);
    if (isMaterialFile(f)) {
      try {
        // The same limits as the server: text 2 MB, pictures 10 MB, PDF 20 MB.
        const max = /\.pdf$/i.test(f.name) ? 20 : /\.(md|markdown|txt)$/i.test(f.name) ? 2 : 10;
        if (f.size > max * 1024 * 1024) throw new Error(`檔案太大（上限 ${max} MB）`);
        const r = await fetch(`/api/import/file?${new URLSearchParams({ name: f.name })}`, { method: "POST", body: f });
        const d = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(d.error ?? (r.status === 413 ? "檔案太大" : `${r.status}`));
        mark.className = "ok"; mark.textContent = "✓";
        li.dataset.kind = d.kind;
        what.textContent = `${KIND_LABEL[d.kind] ?? d.kind}・存進素材`;
        info.textContent = f.name;
        info.title = d.file;
      } catch (e) {
        mark.className = "bad"; mark.textContent = "✕";
        info.textContent = e.message; info.title = e.message;
      }
      continue;
    }
    try {
      const text = await f.text();
      const q = new URLSearchParams({ kind: "auto", name: f.name, lang: state.lang });
      const r = await apiText(`/api/import?${q}`, text);
      const rep = r.report;
      mark.className = "ok"; mark.textContent = "✓";
      li.dataset.kind = r.kind;
      what.textContent = `${KIND_LABEL[r.kind] ?? r.kind}　+${rep.words + rep.grammar}${rep.articles ? `・文章 ${rep.articles}` : ""}`;
      info.textContent = f.name;
      info.title = [r.file, ...rep.warnings].join("\n");
      $("#impOut").className = "note ok";
      $("#impOut").textContent = `✓ ${f.name}`;
    } catch (e) {
      mark.className = "bad"; mark.textContent = "✕";
      info.textContent = e.message;
      info.title = e.message;
    }
  }
  // Next steps for what came in (plan §0.1: the next step is a button).
  $("#impNext")?.remove();
  const okKinds = new Set($$("li", ul).filter((li) => $(".ok", li)).map((li) => li.dataset.kind));
  if (okKinds.size) {
    const row = el("div", "actions-row imp-next");
    row.id = "impNext";
    const lists = okKinds.has("vocab") || okKinds.has("grammar");
    const mats = [...okKinds].some((k) => !["vocab", "grammar"].includes(k));
    const close = (fn) => () => { dlgImport.close(); fn(); };
    if (lists) row.append(primaryButton("用這份清單建立課程", close(() => go("courses", { course: null, courseLesson: null, courseCreate: "fixed" })), true));
    else if (mats) row.append(primaryButton("用這份素材建立課程", close(() => go("courses", { course: null, courseLesson: null, courseCreate: "topic" })), true));
    if (mats) row.append(ghostButton("folder", "看素材", close(() => go("materials"))));
    if (okKinds.has("vocab")) row.append(ghostButton("cards", "看生字", close(() => go("words"))));
    ul.after(row);
  }
  await refreshStatus();
  render();
}

// Window-wide drag and drop.
(() => {
  const overlay = $("#dropOverlay");
  let depth = 0;
  const hasFiles = (e) => [...(e.dataTransfer?.types ?? [])].includes("Files");
  window.addEventListener("dragenter", (e) => { if (!hasFiles(e)) return; depth += 1; overlay.hidden = false; $("#impDrop").classList.add("over"); });
  window.addEventListener("dragleave", () => { depth = Math.max(0, depth - 1); if (!depth) { overlay.hidden = true; $("#impDrop").classList.remove("over"); } });
  window.addEventListener("dragover", (e) => { if (hasFiles(e)) e.preventDefault(); });
  window.addEventListener("drop", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0; overlay.hidden = true; $("#impDrop").classList.remove("over");
    $("#impResults").innerHTML = "";
    importFiles([...e.dataTransfer.files]);
  });
})();
