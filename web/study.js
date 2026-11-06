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

/**
 * The card shown when something is finished: +points, a word of praise,
 * a level-up banner, the early difficulty question, one or two next steps.
 */
function resultCard({ points = 0, title, score, levelUp, already, difficulty, notebook, next = [] }) {
  const card = el("div", "result");
  card.setAttribute("role", "status");
  const plus = el("div", `plus${points > 0 ? "" : " zero"}`, points > 0 ? `+${fmtPts(points)}` : already ? "✓" : "+0");
  card.append(plus, el("h3", null, title));
  const meta = [];
  if (score != null) meta.push(pct(score));
  if (already) meta.push("已得過分");
  if (meta.length) card.append(el("div", "score", meta.join("・")));
  if (levelUp) {
    const lv = el("div", "levelup");
    lv.append(icon("star"), document.createTextNode(`${levelUp.from} → ${levelUp.to}`));
    card.append(lv);
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
    const t = await api(`/api/today?lang=${state.lang}`);
    paintMe(t.summary);
  } catch { /* the card keeps its last numbers */ } finally { meBusy = false; }
}

function paintMe(sum) {
  if (!sum) return;
  state.summary = sum;
  const lv = $("#meLevel");
  lv.innerHTML = "";
  lv.append(levelRing(30, "", sum.points_since / (sum.step_points || 1), null, sum.eligible), el("span", null, `${sum.level}`));
  lv.title = sum.next ? `下一階 ${sum.next}：${fmtPts(sum.points_since)} / ${fmtPts(sum.step_points)}` : "";
  $("#meToday").textContent = fmtPts(sum.today);
  $("#meTotal").textContent = fmtPts(sum.total);
  $("#meStreak").textContent = String(sum.streak);
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
  if (state.todayMode === "review") return renderReview(page);
  viewbar("今日", null);
  if (!state.status?.user) { page.append(el("div", "empty", "請先選擇使用者")); return; }
  const t = await api(`/api/today?lang=${state.lang}`);
  const sum = t.summary;
  paintMe(sum);

  const head = el("div", "today-head");
  head.append(levelRing(96, sum.level, sum.points_since / (sum.step_points || 1), sum.next ? `→ ${sum.next}` : null, sum.eligible));
  const pts = el("div", "pts");
  const big = el("div", "big", fmtPts(sum.today));
  big.append(el("small", null, ` / ${fmtPts(sum.goal)}`));
  pts.append(el("div", "k", "今日積分"), big, ptsBar(sum.today / (sum.goal || 1), sum.today >= sum.goal ? "ok" : ""));
  const streak = el("div", "streak");
  streak.title = "連續天數";
  streak.append(icon("flame"), el("b", null, String(sum.streak)), el("span", null, "天"));
  head.append(pts, streak);
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
    const first = g.tasks[0][0];
    if (g.key !== "other") {
      const les = g.key === "daily" ? null : t.lessons.find((l) => `${l.course}#${l.number}` === g.key);
      const hd = el("div", "tl-head");
      const kind = g.key === "daily" ? "每日課程" : les?.kind_label ?? "";
      hd.append(el("span", `tag ${g.key === "daily" || les?.kind === "daily" ? "acc2" : "acc"}`, kind));
      const title = g.key === "daily" ? (t.daily?.topic?.name ?? "今日課程") : les?.kind === "daily" ? les.title : `第 ${les?.number} 課・${les?.title ?? ""}`;
      hd.append(elr("b", null, title, state.lang));
      if (les && les.kind !== "daily") hd.append(el("span", "sub", les.course_name));
      const doneN = g.tasks.filter(([x]) => x.done).length;
      const mins = g.key === "daily" ? t.daily?.minutes : les?.minutes;
      const meta = el("span", "n", `${doneN} / ${g.tasks.length}${mins ? `・${mins} 分` : ""}`);
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

const UNIT_TITLE = { review: "複習卡片", reading: "課文與閱讀", translation: "翻譯", "sentence-making": "造句", cloze: "填空", paraphrase: "換句話說", conversation: "AI 對話", quiz: "測驗", listening: "聽力", dictation: "聽寫", shadowing: "跟讀" };
const UNIT_ICON = { review: "cards", reading: "book", translation: "pen", "sentence-making": "pen", cloze: "pen", paraphrase: "pen", conversation: "chat", quiz: "check" };
const unitTitle = (k, lang = state.lang) => (k === "translation" ? (lang === "ja" ? "中翻日" : "中翻英") : UNIT_TITLE[k] ?? k);

/** Open a step of a course lesson (creating today's daily lesson first if needed). */
async function openStep(task) {
  if (task.daily) {
    try {
      const d = await api("/api/daily", { lang: state.lang });
      go("courses", { course: d.id, courseLesson: d.next ?? 1, step: task.unit });
    } catch (e) { flash(e.message, true); }
    return;
  }
  go("courses", { course: task.course, courseLesson: task.number, step: task.unit });
}

function taskCard(task, isNext, engineReady) {
  const card = el("div", `task${task.done ? " done" : ""}${isNext ? " next" : ""}`);
  card.dataset.task = task.kind ?? task.id;
  const ic = el("div", "ic");
  const body = el("div");
  let title, sub, action, label = "開始";
  if (task.id === "unit") {
    ic.append(icon(UNIT_ICON[task.kind] ?? "book"));
    title = unitTitle(task.kind);
    const bits = [];
    if (task.kind === "review") bits.push(task.due ? `${task.due} 張到期` : `${task.items} 張`);
    else if (task.kind === "reading") bits.push(`${task.items} 題`);
    else if (task.kind === "conversation") bits.push(`${task.items} 回合`);
    else bits.push(`${task.items} 題`);
    bits.push(`${task.minutes} 分`);
    sub = task.done ? "完成" : bits.join("・");
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
  body.append(el("div", "t", title), el("div", "s", sub));
  card.append(ic, body);
  if (task.done) {
    const m = el("span", "mark"); m.append(icon("check")); card.append(m);
  } else if (isNext) {
    const b = primaryButton(label, (e) => { e.stopPropagation(); action(); });
    b.classList.add("go");
    card.append(b);
  } else {
    card.append(el("span"));
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
    const card = resultCard({
      points: r.points,
      title: r.already ? "又完成一次" : r.passed ? "測驗完成" : "再試一次就好",
      score: r.score,
      already: r.already,
      levelUp: r.level_up,
      difficulty: r.ask_difficulty && !r.already ? { lang: quiz.lang } : null,
      next: [
        { label: "回到今日", primary: true, onclick: () => go("today") },
        { label: "練習對話", onclick: () => go("conversation", { conv: null }) },
      ],
      notebook: r.notebook ? { lang: quiz.lang, stem: r.notebook } : null,
    });
    quiz.after.after(card);
    paintMe(r.summary);
    refreshStatus();
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
  top.append(el("b", null, c.topic.name), el("span", `tag${c.topic.kind === "interest" ? " acc" : ""}`, c.topic.kind === "interest" ? "興趣" : "通識"), el("span", "n", `${v.turns_done} / ${v.turns_limit}`));
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
  wrap.append(ta, timer, send);
  const hint = el("div", "c-hint");
  box.append(wrap, el("div", "actions-row"), hint);
  $(".actions-row", box).append(count);

  const total = v.seconds_per_turn;
  let left = total;
  convTimer = setInterval(() => {
    left -= 1;
    if (left <= 0) { clearInterval(convTimer); timer.classList.add("over"); secsLabel.textContent = "0"; fill.setAttribute("stroke-dashoffset", C); return; }
    secsLabel.textContent = String(left);
    fill.setAttribute("stroke-dashoffset", C * (1 - left / total));
  }, 1000);

  function onInput() {
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
  const lock = el("div", "locked");
  lock.append(icon("lock"), document.createTextNode("完成"));
  const p = v.conversation.points;
  lock.append(el("span", "plus", v.status === "done" ? `+${fmtPts(p)}` : "+1"));
  page.append(lock);
  if (v.status === "locked") {
    const wait = el("div", "empty", "評估中…");
    wait.id = "convWait";
    page.append(wait);
    return;
  }
  const c = v.conversation;
  const res = v.result ?? { points: c.points, score: c.score, passed: c.passed, review: c.review, used: v.used };
  const nb = res.notebook ? { lang: c.lang, stem: res.notebook } : null;
  if (v.justFinished) {
    const course = res.course;
    page.append(resultCard({
      points: res.points,
      title: course?.lesson_done ? "這一課完成了！" : res.passed ? (res.excellent ? "說得真好！" : "完成這段對話") : "完成了，下次再多說一點",
      score: res.score,
      levelUp: res.level_up,
      notebook: nb,
      next: c.course
        ? [
          course?.next?.lesson && course.next.lesson !== c.course.lesson
            ? { label: `下一課`, primary: true, onclick: () => go("courses", { course: c.course.course, courseLesson: course.next.lesson, step: null }) }
            : { label: "回到這一課", primary: true, onclick: () => backToLesson(c.course) },
          { label: "回到今日", onclick: () => go("today") },
        ]
        : [
          { label: "再一段", primary: true, onclick: () => { state.conv = null; render(); } },
          { label: "回到今日", onclick: () => go("today") },
        ],
    }));
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
    state.conv = r;
    drawConv($("#page"));
    if (r.summary) paintMe(r.summary);
    refreshStatus();
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

  const topics = el("div", "card");
  topics.append(el("h3", null, "主題"));
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

/** Empty states point straight at importing. */
function emptyImport(text) {
  const e = el("div", "empty cta", text);
  const d = el("div", "drop small");
  d.append(icon("upload"), el("span", null, "拖放或選擇檔案"));
  d.onclick = () => { $("#impResults").innerHTML = ""; openImport(); };
  e.append(d);
  return e;
}

const KIND_LABEL = { vocab: "單字", grammar: "文法", materials: "每週素材" };
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
    try {
      const text = await f.text();
      const q = new URLSearchParams({ kind: "auto", name: f.name, lang: state.lang });
      const r = await apiText(`/api/import?${q}`, text);
      const rep = r.report;
      mark.className = "ok"; mark.textContent = "✓";
      what.textContent = `${KIND_LABEL[r.kind] ?? r.kind}　+${rep.words + rep.grammar}${rep.articles ? `・文章 ${rep.articles}` : ""}`;
      info.textContent = r.file;
      info.title = rep.warnings.join("\n");
      $("#impOut").className = "note ok";
      $("#impOut").textContent = `✓ ${f.name}`;
    } catch (e) {
      mark.className = "bad"; mark.textContent = "✕";
      info.textContent = e.message;
      info.title = e.message;
    }
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
