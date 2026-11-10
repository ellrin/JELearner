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

// ── study buddies ──────────────────────────────────────
// The members of a shared workspace see each other's points (GET
// /api/buddies, from the files everyone syncs). A strip on the today page:
// today's ring, the last seven days, the streak; for the admin, how far a
// member's iPad lessons are prepared and the way to prepare more.

let buddyCache = null;
function fetchBuddies(fresh = false) {
  const key = state.status?.user?.id ?? "";
  if (!fresh && buddyCache && buddyCache.key === key && Date.now() - buddyCache.at < 15000) return buddyCache.p;
  const p = api("/api/buddies").then((r) => r, () => ({ buddies: [] }));
  buddyCache = { key, at: Date.now(), p };
  return p;
}

/** One friendly line for the secretary when a buddy is ahead today (or waiting). */
async function buddyNudge(myToday) {
  try {
    const { buddies } = await fetchBuddies();
    const others = buddies.filter((b) => !b.me).sort((a, b) => b.today - a.today);
    const top = others[0];
    if (!top) return "";
    if (top.today > myToday) return `${top.name} 今天已經拿了 ${fmtPts(top.today)} 分，換你了！`;
    if (myToday > 0 && top.today === 0 && top.streak > 0) return `你今天先開始了，${top.name} 還沒上課。`;
  } catch { /* no line */ }
  return "";
}

const WEEKDAY = ["日", "一", "二", "三", "四", "五", "六"];

function buddyStrip(data) {
  const list = data.buddies ?? [];
  const box = el("section", "buddies");
  box.setAttribute("aria-label", "學習戰友");
  const lead = Math.max(...list.map((b) => b.week));
  const leaders = list.filter((b) => b.week === lead && lead > 0);
  for (const b of list) {
    const reached = b.goal > 0 && b.today >= b.goal;
    const card = el("div", `buddy${b.me ? " me" : ""}${reached ? " reached" : ""}`);
    const av = el("span", "b-av");
    setAvatar(av, b);
    if (!b.avatar) av.textContent = ([...b.name][0] ?? "").toUpperCase();
    const ring = levelRing(52, "", b.today / (b.goal || 1), null, reached);
    ring.classList.add("b-ring");
    ring.append(av);
    ring.title = `今天 ${fmtPts(b.today)} / ${fmtPts(b.goal)} 分`;
    const body = el("div", "b-body");
    const top = el("div", "b-top");
    const nm = el("b", null, b.me ? `${b.name}（你）` : b.name);
    nm.title = b.name;
    top.append(nm);
    const lv = Object.entries(b.levels ?? {}).map(([, v]) => v).join("・");
    if (lv) top.append(el("span", "b-lv", lv));
    if (leaders.length === 1 && leaders[0] === b) { const t = el("span", "tag acc", "本週領先"); top.append(t); }
    const nums = el("div", "b-nums");
    const num = (v, k, cls) => { const n = el("span", cls); n.append(el("b", null, v), document.createTextNode(k)); return n; };
    const st = num(String(b.streak), " 天", "st");
    st.prepend(icon("flame"));
    st.title = "連續天數";
    nums.append(num(fmtPts(b.today), " 今日", "td"), num(fmtPts(b.week), " 本週"), st);
    // The last seven days, today last: a filled dot for a day with practice.
    const days = el("div", "b-days");
    const now = new Date();
    (b.days ?? []).forEach((p, i) => {
      const d = new Date(now); d.setDate(now.getDate() - (6 - i));
      const dot = el("i", `${p > 0 ? "on" : ""}${i === 6 ? " today" : ""}`, WEEKDAY[d.getDay()]);
      dot.title = `${d.getMonth() + 1}/${d.getDate()}：${fmtPts(p)} 分`;
      days.append(dot);
    });
    days.setAttribute("role", "img");
    days.setAttribute("aria-label", `近 7 天有 ${(b.days ?? []).filter((p) => p > 0).length} 天練習`);
    body.append(top, nums, days);
    card.append(ring, body);
    // The admin's weekly job: this member's iPad lessons, and when they run out.
    if (data.admin && (!b.me || b.prepared)) {
      const pr = b.prepared;
      const left = pr?.days ?? 0;
      const foot = el("div", `b-prep${left <= 1 ? " low" : ""}`);
      foot.append(el("span", null, !pr ? "還沒備課" : left === 0 ? "備好的課上完了" : `課備到 ${fmtDate(pr.until)}（${left} 天）`));
      const go1 = el("button", left <= 1 ? "primary small" : "ghost small", "備課");
      go1.setAttribute("aria-label", `替 ${b.name} 備課`);
      go1.onclick = () => openPrepFor(b.id);
      foot.append(go1);
      card.append(foot);
    }
    box.append(card);
  }
  return box;
}

/** 設定 › 使用者 with the prepare panel of that member open. */
function openPrepFor(id) {
  openSettings(1);
  const m = (state.status?.users ?? []).find((u) => u.id === id);
  if (m) { openPrep(m); $("#setPrep").scrollIntoView({ block: "nearest" }); }
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
  // Study buddies: only when the workspace has someone else. The strip
  // arrives on its own, so today's tasks never wait for it.
  if ((state.status?.users ?? []).length > 1) {
    const slot = el("div", "buddies-slot");
    page.append(slot);
    fetchBuddies(true).then((data) => { if ((data.buddies ?? []).length > 1) slot.replaceWith(buddyStrip(data)); else slot.remove(); });
  }

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
    // Today's lesson not made yet: one way in (開始今天的課), plan §19.2.6.
    if (g.key === "daily" && t.daily) { page.append(todayStart(ready, t.daily.minutes)); continue; }
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

// ── 開始今天的課 (stage F3) ─────────────────────────────
// One button → 3–5 topic cards (news, interests, general topics) → pick one
// and only today's lesson is made. 換一批 leaves out the cards already shown.
// The cards stay while the learner looks elsewhere (same user, language, day).

let todayPick = null;
const pickKey = () => `${state.status?.user?.id ?? ""}/${state.lang}/${new Date().toDateString()}`;
const CARD_KIND = { news: "新聞", interest: "興趣", general: "通識" };

function todayStart(ready, minutes) {
  const box = el("div", "today-start");
  if (todayPick?.key !== pickKey()) todayPick = { key: pickKey(), cards: null, shown: [], refreshed: false, busy: false, err: null, newsErr: false, focus: undefined };
  // `focusOn`: what gets the keyboard focus after a redraw (the box is redrawn).
  const draw = (focusOn) => {
    box.innerHTML = "";
    box.setAttribute("aria-busy", String(!!todayPick.busy));
    const head = el("div", "ts-head");
    head.append(el("h3", null, "今天想學什麼？"));
    const focusBox = el("div", "ts-focus");
    head.append(focusBox);
    box.append(head);
    drawFocus(focusBox, draw, focusOn);
    if (!ready) {
      box.append(el("p", "ts-note", "主題卡與課文由 AI 老師準備，先連接 Claude 或本機模型。"));
      const row = el("div", "actions-row");
      row.append(primaryButton("連接 AI 老師", () => openSettings(3)));
      box.append(row);
    } else if (!todayPick.cards && !todayPick.busy) {
      if (todayPick.err) {
        // One short line; the details are for whoever helps.
        const n = el("p", "ts-note bad", "AI 老師暫時連不上，等一下再試。");
        n.setAttribute("role", "alert");
        n.title = todayPick.err;
        box.append(n);
      }
      const row = el("div", "actions-row");
      const go1 = primaryButton(todayPick.err ? "再試一次" : "開始今天的課", () => loadCards(draw));
      row.append(go1);
      if (!todayPick.err && minutes) row.append(el("span", "ts-hint", `約 ${minutes} 分鐘`));
      box.append(row);
      if (focusOn === "main") go1.focus();
    } else if (todayPick.busy === "cards") {
      const n = el("p", "ts-note", todayPick.refreshed ? "正在挑主題…" : "正在看今天的新聞、挑主題…");
      n.setAttribute("role", "status");
      box.append(n);
      const grid = el("div", "ts-cards");
      for (let i = 0; i < 4; i++) { const sk = el("div", "ts-card skeleton"); sk.append(el("i"), el("i"), el("i")); grid.append(sk); }
      box.append(grid);
    } else {
      if (todayPick.newsErr) box.append(el("p", "ts-note", "新聞暫時抓不到，先從興趣與通識挑。"));
      const grid = el("div", "ts-cards");
      for (const c of todayPick.cards) grid.append(topicCard(c, draw));
      box.append(grid);
      const row = el("div", "actions-row");
      row.append(ghostButton("refresh", "都不要，換一批", () => loadCards(draw)));
      box.append(row);
      if (focusOn === "cards") grid.querySelector(".ts-card")?.focus();
    }
    box.append(otherWays());
  };
  draw();
  return box;
}

async function loadCards(draw) {
  const p = todayPick;
  p.busy = "cards"; p.err = null;
  draw();
  try {
    const r = await api("/api/today/topics", { lang: state.lang, exclude: p.shown, refresh: !p.refreshed });
    p.refreshed = true;
    p.newsErr = (r.news_errors ?? []).length > 0 && !r.cards.some((c) => c.kind === "news");
    p.cards = r.cards;
    p.shown.push(...r.cards.map((c) => c.title));
  } catch (e) {
    // 換一批 failed: the cards shown stay; the first batch failed: say why.
    if (p.cards) flash(e.message, true); else p.err = e.message;
  } finally { p.busy = false; }
  if (p === todayPick) draw(p.cards ? "cards" : "main");
}

function topicCard(c, draw) {
  const b = el("button", `ts-card kind-${c.kind}`);
  const top = el("div", "ts-top");
  top.append(el("span", `tag${c.kind === "news" ? " acc" : ""}`, CARD_KIND[c.kind] ?? c.kind));
  if (c.category) top.append(el("span", "ts-cat", c.category));
  b.append(top, el("b", null, c.title));
  if (c.why) b.append(el("span", "ts-why", c.why));
  if (c.article) b.append(el("span", "ts-src", `${c.article.source}・${c.article.title}`));
  const p = todayPick;
  if (p.busy === "start") {
    b.disabled = true;
    if (p.picked === c) {
      b.classList.add("picked");
      const n = el("span", "ts-busy", "正在準備今天的課…");
      n.setAttribute("role", "status");
      b.append(n);
    }
  }
  b.onclick = async () => {
    p.busy = "start"; p.picked = c;
    draw("picked");
    try {
      const r = await api("/api/today/start", { lang: state.lang, card: c });
      todayCache = null;
      todayPick = null;
      go("courses", { course: r.id, courseLesson: r.next ?? 1, step: null, fromToday: true, courseCreate: null });
    } catch (e) {
      p.busy = false; p.picked = null;
      flash(e.message, true);
      draw("cards");
    }
  };
  return b;
}

/** The focus theme: a chip with ✕, or a small way to set one. */
async function drawFocus(box, redraw, focusOn) {
  const p = todayPick;
  if (p.focus === undefined) {
    try { p.focus = (await api(`/api/focus?lang=${state.lang}`)).focus?.name ?? null; } catch { p.focus = null; }
    if (p !== todayPick) return;
  }
  box.innerHTML = "";
  if (p.focus) {
    const chip = el("span", "focus-chip");
    chip.title = "這陣子的主題卡與課文都會放在這個地方";
    chip.append(icon("target"), el("span", null, `重點：${p.focus}`));
    const x = el("button", "x");
    x.setAttribute("aria-label", "取消重點主題");
    x.append(icon("x"));
    x.onclick = async () => {
      try {
        await api(`/api/focus?lang=${state.lang}`, undefined, "DELETE");
        Object.assign(p, { focus: null, cards: null, shown: [] });
        redraw("focus");
      } catch (e) { flash(e.message, true); }
    };
    chip.append(x);
    box.append(chip);
    if (focusOn === "focus") x.focus();
    return;
  }
  if (p.focusEdit) {
    const f = el("form", "focus-set");
    const i = el("input");
    i.placeholder = state.lang === "ja" ? "例：京都旅行" : "例：蘇格蘭／英國旅遊";
    i.maxLength = 40;
    i.setAttribute("aria-label", "重點主題");
    i.onkeydown = (e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); p.focusEdit = false; redraw("focus"); } };
    const ok = el("button", "primary small", "設定");
    ok.type = "submit";
    const no = el("button", "ghost", "取消");
    no.type = "button";
    no.onclick = () => { p.focusEdit = false; redraw("focus"); };
    f.append(i, ok, no);
    f.onsubmit = async (e) => {
      e.preventDefault();
      if (!i.value.trim()) return i.focus();
      ok.disabled = i.disabled = true;
      ok.textContent = "整理在地資料…";
      try {
        const r = await api("/api/focus", { lang: state.lang, name: i.value.trim() });
        Object.assign(p, { focus: r.focus.name, focusEdit: false, cards: null, shown: [] });
        redraw("focus");
      } catch (err) {
        flash(err.message, true);
        ok.disabled = i.disabled = false;
        ok.textContent = "設定";
      }
    };
    box.append(f);
    setTimeout(() => i.focus(), 0);
    return;
  }
  if (!state.status?.engine_ready) return;
  const add = el("button", "link-btn", "＋ 重點主題");
  add.title = "例如一趟旅行：主題卡和課文都會換成那裡的街道、店家、交通和新聞";
  add.onclick = () => { p.focusEdit = true; redraw(); };
  box.append(add);
  if (focusOn === "focus") add.focus();
}

/** The other ways in, folded: an article or address, or a course from a list. */
function otherWays() {
  const d = el("details", "ts-other");
  d.append(el("summary", null, "其他方式"));
  const row = el("div", "ts-other-row");
  row.append(
    ghostButton("link", "貼文章或網址", () => go("courses", { course: null, courseLesson: null, courseCreate: "topic" })),
    ghostButton("layers", "用單字／文法清單排課", () => go("courses", { course: null, courseLesson: null, courseCreate: "fixed" })),
  );
  d.append(row);
  return d;
}

/** 第 3 課・Title, without repeating 第 3 課 when the title already starts with it. */
function lessonLabel(n, title) {
  const t = String(title ?? "").replace(/^第\s*\d+\s*課\s*[：:・]\s*/, "");
  return t ? `第 ${n} 課・${t}` : `第 ${n} 課`;
}

const UNIT_TITLE = { review: "單字與文法", reading: "課文", translation: "翻譯", "sentence-making": "造句", cloze: "填空", paraphrase: "換句話說", conversation: "AI 對話", quiz: "測驗", listening: "聽力", dictation: "聽寫", shadowing: "跟讀" };
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
      if (task.due && !task.done) parts.push(`${task.due} 張到期`);
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
    ? el("span", "tag acc2", c.course.lesson ? `第 ${c.course.lesson} 課` : "課程")
    : el("span", `tag${c.topic.kind === "interest" ? " acc" : ""}`, c.topic.kind === "interest" ? "興趣" : "通識");
  if (c.course) tag.title = courseName;
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
  if (!dlgImport.open) { dlgImport.showModal(); $("#impDrop").focus(); }
}
/** Start a fresh list, unless an outline is still waiting for 建立. */
function clearImport() {
  if (!$("#impResults .pv-list")) { $("#impResults").innerHTML = ""; $("#impInt")?.remove(); }
}
$(".close", dlgImport).onclick = () => dlgImport.close();
$("#actImport").onclick = () => { clearImport(); openImport(); };
$("#impDrop").onclick = () => $("#impFiles").click();
$("#impDrop").onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); $("#impFiles").click(); } };
$("#impFiles").onchange = (e) => { importFiles([...e.target.files]); e.target.value = ""; };
$("#asideDrop").onclick = () => { clearImport(); openImport(); };
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
  d.onclick = () => { clearImport(); openImport(); };
  e.append(d);
  return e;
}

const KIND_LABEL = { vocab: "單字", grammar: "文法", materials: "每週素材", text: "筆記", pdf: "PDF", image: "圖片" };
/** Kept as files in materials/ (read in a later stage), not imported into review. */
const isMaterialFile = (f) => /\.(md|markdown|txt|pdf|png|jpe?g|webp|gif)$/i.test(f.name) || f.type.startsWith("image/");
const isTextFile = (f) => /\.(md|markdown|txt)$/i.test(f.name);

/** About how long a text is: characters for Chinese / Japanese, words otherwise. */
function textSize(t) {
  const cjk = (t.match(/[぀-ヿ㐀-鿿]/g) ?? []).length;
  const n = cjk > t.length / 4 ? cjk : (t.match(/\S+/g) ?? []).length;
  return n >= 100 ? `約 ${Math.round(n / 100) * 100} 字` : `${n} 字`;
}
const fmtBytes = (b) => (b >= 1024 * 1024 ? `${(b / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);
/** `profiles/ellrin/materials/chips.md` → `materials/chips.md` (the full path stays in the title). */
const shortPath = (p) => String(p ?? "").split("/").slice(-2).join("/");
/** Engine and network errors as one short line; the details go in the title. */
const friendly = (msg) => (/Claude|連線|error sending|timed out|逾時|模型/.test(msg) ? "AI 老師暫時連不上，等一下再試。" : msg);

// Upload feedback (stage F4, plan §19.2.7): each file gets a 「收到了」 card
// that says what came in (its name, kind and size) and where it is kept, and
// the next step as a button (a course from it: the outline first, then
// confirm). Below all the cards, one question for the whole batch: the
// interests the model read from it, remembered only when the learner says so.
async function importFiles(files) {
  if (!files.length) return;
  openImport();
  const ul = $("#impResults");
  $("#impInt")?.remove();
  const readable = [];
  for (const f of files) {
    const li = el("li", "rcv");
    const wait = el("div", "rcv-head", `正在收 ${f.name}…`);
    li.append(wait);
    ul.prepend(li);
    let got = null;
    try {
      if (isMaterialFile(f)) {
        // The same limits as the server: text 2 MB, pictures 10 MB, PDF 20 MB.
        const max = /\.pdf$/i.test(f.name) ? 20 : isTextFile(f) ? 2 : 10;
        if (f.size > max * 1024 * 1024) throw new Error(`檔案太大（上限 ${max} MB）`);
        const text = isTextFile(f) ? await f.text() : null;
        const r = await fetch(`/api/import/file?${new URLSearchParams({ name: f.name })}`, { method: "POST", body: f });
        const d = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(d.error ?? (r.status === 413 ? "檔案太大" : `${r.status}`));
        const what = d.kind === "text" ? `文章，${textSize(text)}` : d.kind === "pdf" ? `PDF，${fmtBytes(f.size)}` : d.kind === "image" ? "圖片" : KIND_LABEL[d.kind] ?? d.kind;
        got = { kind: d.kind, file: d.file, what, where: "存進素材", text };
      } else {
        const text = await f.text();
        const q = new URLSearchParams({ kind: "auto", name: f.name, lang: state.lang });
        const r = await apiText(`/api/import?${q}`, text);
        const rep = r.report;
        const what = r.kind === "vocab" ? `單字表，${rep.words} 字` : r.kind === "grammar" ? `文法表，${rep.grammar} 條` : r.kind === "materials" ? `每週素材，文章 ${rep.articles} 篇` : KIND_LABEL[r.kind] ?? r.kind;
        const where = r.kind === "vocab" ? "已加進生字" : r.kind === "grammar" ? "已加進文法" : "存進素材";
        got = { kind: r.kind, file: r.file, what, where, warnings: rep.warnings };
      }
    } catch (e) {
      drawFailed(li, f, e.message);
      continue;
    }
    drawReceived(li, f, got);
    if (["text", "materials", "vocab", "grammar"].includes(got.kind)) readable.push(got.file);
  }
  // One question for the batch (the first 3 readable files).
  if (readable.length && state.status?.engine_ready) {
    const box = el("div", "rcv-int");
    box.id = "impInt";
    ul.after(box);
    suggestInterests(box, readable.slice(0, 3));
  }
  ul.querySelector("li")?.scrollIntoView({ block: "nearest" });
  await refreshStatus();
  render();
}

function drawFailed(li, f, msg) {
  li.className = "rcv bad";
  li.innerHTML = "";
  const h = el("div", "rcv-head");
  const m = el("span", "mark"); m.append(icon("x"));
  h.append(m, el("b", null, "沒收到"), el("span", "rcv-name", f.name));
  li.append(h, el("div", "rcv-msg", msg));
  // A table the app could not read: the template is the next step.
  if (/\.(csv|tsv)$/i.test(f.name)) {
    const row = el("div", "actions-row");
    row.append(ghostButton("download", "下載單字表範本", () => $("#impTemplate").click()));
    li.append(row);
  }
}

function drawReceived(li, f, got) {
  li.innerHTML = "";
  li.dataset.kind = got.kind;
  const h = el("div", "rcv-head");
  const m = el("span", "mark ok"); m.append(icon("check"));
  h.append(m, el("b", null, `收到了：${got.what}`), el("span", "rcv-name", f.name));
  const where = el("div", "rcv-where", `${got.where}・${shortPath(got.file)}`);
  where.title = [got.file, ...(got.warnings ?? [])].join("\n");
  li.append(h, where);
  if (got.warnings?.length) li.append(el("div", "rcv-msg", `有 ${got.warnings.length} 行略過（游標停在上一行看原因）`));

  const close = (fn) => () => { dlgImport.close(); fn(); };
  const row = el("div", "actions-row");
  const canCourse = ["vocab", "grammar"].includes(got.kind) || (got.kind === "text" && got.text?.trim());
  const preview = el("div", "rcv-preview");
  if (canCourse) row.append(primaryButton("做成課程", () => (state.status?.engine_ready ? previewCourse(preview, row, got, f) : openSettings(3)), true));
  if (got.kind === "vocab") row.append(ghostButton("cards", "看生字", close(() => go("words"))));
  else if (got.kind === "grammar") row.append(ghostButton("tree", "看文法", close(() => go("grammar"))));
  else row.append(ghostButton("folder", "看素材", close(() => go("materials"))));
  li.append(row, preview);
}

async function suggestInterests(box, files) {
  const wait = el("span", "rcv-wait", "正在看這些內容跟什麼主題有關…");
  wait.setAttribute("role", "status");
  box.append(wait);
  const results = await Promise.all(files.map((file) => api("/api/interests/suggest", { lang: state.lang, file }).catch(() => null)));
  const keep = [];
  for (const r of results) for (const k of r?.keywords ?? []) if (!keep.some((x) => x.toLowerCase() === k.toLowerCase())) keep.push(k);
  const summaries = results.filter((r) => r?.summary).map((r) => r.summary);
  box.innerHTML = "";
  if (!keep.length) { box.remove(); return; }
  const ask = el("p", null, `${files.length === 1 && summaries[0] ? `${summaries[0]}。` : ""}看起來你對這些主題有興趣，要記住嗎？`);
  ask.setAttribute("role", "status");
  box.append(ask);
  const chips = el("div", "int-chips");
  const add = el("input");
  add.placeholder = "＋ 自己加";
  add.setAttribute("aria-label", "自己加一個主題");
  add.maxLength = 30;
  const yes = primaryButton("記住", null, true);
  const no = ghostButton("x", "不用", () => { box.remove(); ($("#impResults .primary") ?? $("#impDrop")).focus(); });
  const clean = (v) => v.trim().replace(/[,"\n]/g, "");
  const push = (v) => { if (v && !keep.some((k) => k.toLowerCase() === v.toLowerCase())) keep.push(v); };
  const draw = () => {
    chips.innerHTML = "";
    keep.forEach((k, i) => {
      const c = el("span", "int-chip", k);
      const x = el("button", "x");
      x.setAttribute("aria-label", `拿掉 ${k}`);
      x.append(icon("x"));
      x.onclick = () => { keep.splice(i, 1); draw(); (chips.querySelector(".x") ?? add).focus(); };
      c.append(x);
      chips.append(c);
    });
    chips.append(add);
    yes.disabled = !keep.length && !add.value.trim();
  };
  add.oninput = () => { yes.disabled = !keep.length && !add.value.trim(); };
  add.onkeydown = (e) => {
    if (e.key === "Enter" && add.value.trim()) {
      e.preventDefault();
      push(clean(add.value));
      add.value = "";
      draw(); add.focus();
    } else if (e.key === "Escape" && add.value) {
      // Clear the box, not close the dialog.
      e.preventDefault(); e.stopPropagation();
      add.value = "";
      yes.disabled = !keep.length;
    }
  };
  yes.onclick = () => busy(yes, async () => {
    push(clean(add.value));
    let r;
    try { r = await api("/api/interests", { keywords: keep }); } catch (e) { flash(e.message, true); return; }
    box.innerHTML = "";
    const done = el("p", "rcv-done");
    done.tabIndex = -1;
    done.setAttribute("role", "status");
    done.append(icon("check"), document.createTextNode(r.added ? `記住了 ${r.added} 個主題，之後挑新聞和主題卡會用到。` : "這些主題本來就記得了。"));
    box.append(done);
    done.focus();
    refreshStatus();
  });
  const row = el("div", "actions-row");
  row.append(yes, no);
  box.append(chips, row);
  draw();
}

/** A course from the upload: the outline is shown first; saved only on 建立. */
async function previewCourse(box, row, got, f) {
  const mk = row.querySelector(".primary");
  mk.disabled = true;
  box.innerHTML = "";
  box.setAttribute("aria-busy", "true");
  const wait = el("span", "rcv-wait", "正在規劃課程大綱…");
  wait.tabIndex = -1;
  wait.setAttribute("role", "status");
  box.append(wait, planningSkeleton());
  wait.focus();
  const body = got.kind === "text"
    ? { kind: "topic", lang: state.lang, keywords: [], sources: [{ text: got.text, title: f.name.replace(/\.[^.]+$/, "") }], preview: true }
    : { kind: "fixed", lang: state.lang, name: "", [got.kind === "grammar" ? "grammar" : "vocab"]: [got.file], file_name: f.name, describe: true, preview: true };
  let pv;
  try { pv = await api("/api/courses", body); } catch (e) {
    box.innerHTML = "";
    box.removeAttribute("aria-busy");
    const err = el("div", "rcv-msg bad", friendly(e.message));
    err.title = e.message;
    err.setAttribute("role", "alert");
    box.append(err);
    mk.disabled = false;
    mk.textContent = "再試一次";
    mk.focus();
    return;
  }
  box.innerHTML = "";
  box.removeAttribute("aria-busy");
  mk.textContent = "做成課程";
  const c = pv.preview;
  const head = el("div", "pv-head");
  head.append(el("b", null, c.name), el("span", "n", `${pv.total} 課`));
  box.append(head);
  if (c.summary) box.append(el("p", "pv-sum", c.summary));
  const ol = el("ol", "pv-list");
  // The outline file's own field names (`[[lesson]]`).
  for (const lp of c.lesson ?? c.lessons ?? []) {
    const li = el("li");
    li.append(el("span", "num", String(lp.number)), el("span", "t", lp.title || `第 ${lp.number} 課`));
    if (lp.purpose) li.append(el("span", "p", lp.purpose));
    ol.append(li);
  }
  box.append(ol);
  if (pv.warnings?.length) box.append(el("div", "rcv-msg", pv.warnings.join("；")));
  const r2 = el("div", "actions-row");
  const ok = primaryButton("就這樣，建立課程", () => busy(ok, async () => {
    let saved;
    try { saved = await api("/api/courses/confirm", { course: c }); } catch (e) { flash(friendly(e.message), true); return; }
    dlgImport.close();
    say(`「${saved.name}」建立好了：${saved.total} 課。從第 1 課開始吧！`);
    go("courses", { course: saved.id, courseLesson: null, courseCreate: null });
  }), true);
  const no = ghostButton("x", "不要", () => { box.innerHTML = ""; mk.disabled = false; mk.focus(); });
  r2.append(ok, no);
  box.append(r2);
  ok.focus();
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
    clearImport();
    importFiles([...e.dataTransfer.files]);
  });
})();
