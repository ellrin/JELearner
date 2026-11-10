// Stage B (docs/plan.md §11–§13, docs/courses.md): courses (list, quick
// create, outline), the course lesson page with its steps (review round →
// text + reading quiz → output → conversation) and result cards, and the
// review notebook (one page per lesson or conversation, notes and pictures).
// Little text; one primary button = the next step.

// Only a lesson in progress gets a word; planned and done lessons show it by
// their number circle and step dots.
const STATUS_LABEL = { generated: "進行中" };
const ROLE_LABEL = { background: "背景", article: "文章", review: "複習", regular: "", conversation: "對話", check: "檢核" };
const ANGLE_LABEL = { technical: "技術", social: "社會", "personal life": "生活", background: "背景", review: "複習", work: "職場" };

// ── list ───────────────────────────────────────────────

async function renderCourses(page) {
  if (!state.status?.user) { viewbar("課程", null); page.append(el("div", "empty", "請先選擇使用者")); return; }
  if (state.quickReview) return renderQuickReview(page);
  if (state.courseCreate) return renderCourseCreate(page);
  if (state.course && state.courseLesson) return renderCourseLesson(page);
  if (state.course) return renderCourseOutline(page);
  const data = await api(`/api/courses?lang=${state.lang}`);
  const list = data.courses;
  const add = primaryButton("新課程", () => { state.courseCreate = "pick"; render(); }, true);
  add.prepend(icon("plus"));
  viewbar(`課程・${langLabel(state.lang)}`, list.length || null, ...(list.length ? [add] : []));
  const weekly = await weeklyBar();
  if (weekly) page.append(weekly);
  if (!list.length) {
    page.append(createPicker());
    return;
  }
  // The course worked on most recently gets the one primary button.
  const recent = [...list].filter((c) => c.next != null).sort((a, b) => (b.activity > a.activity ? 1 : -1))[0];
  const grid = el("div", "course-grid");
  for (const c of list) grid.append(courseCard(c, c === recent));
  page.append(grid);
  if (data.skipped?.length) {
    const n = el("div", "note", `略過 ${data.skipped.length} 個無法讀取的檔案`);
    n.title = data.skipped.join("\n");
    page.append(n);
  }
}

// ── this week's lessons, for everyone, in one press ────
// The admin's weekly job: fresh news plus the library's unused lessons, a
// week of lessons for every member at their own level, then synced to the
// shared repository so the iPads find them.
const WEEK = { days: 7, choices: 2 };
let weeklyTimer = null;
let weeklyMine = false;
async function weeklyBar() {
  const st = state.status;
  const admin = !!(st?.users ?? []).find((x) => x.id === st?.user?.id)?.admin;
  if (!admin || st.demo || st.family?.ready || (st.users ?? []).length < 2) return null;
  // A redrawn page has a new bar: the old bar's timer stops here, and each
  // bar keeps its own (a shared one froze the new bar during a run).
  clearInterval(weeklyTimer); weeklyTimer = null;
  const data = await fetchBuddies();
  const members = (data.buddies ?? []).filter((b) => b.levels?.[state.lang]);
  if (members.length < 2) return null;
  const bar = el("section", "weekly");
  bar.setAttribute("aria-label", "本週的課");
  const info = el("div", "wk-info");
  info.append(el("b", null, "本週的課"));
  const who = el("div", "wk-who");
  const drawWho = (list) => {
    who.innerHTML = "";
    for (const b of list) {
      const left = b.prepared?.days ?? 0;
      const chip = el("span", `wk-m${left <= 1 ? " low" : ""}`);
      const dot = el("span", "dot"); setAvatar(dot, b);
      chip.append(dot, document.createTextNode(`${b.name}・${left ? `到 ${fmtDate(b.prepared.until)}` : "還沒有"}`));
      who.append(chip);
    }
  };
  drawWho(members);
  info.append(who);
  const go1 = primaryButton("派本週的課", null, true);
  const help = el("button", "help", "?");
  help.setAttribute("aria-label", "說明：派本週的課");
  help.dataset.tip = `抓最新新聞，加上主題庫還沒上過的課，替每個人依各自的程度做好 ${WEEK.days} 天的課（每天 ${WEEK.choices} 個主題可選），做完自動同步，iPad 打開就看得到。雲端的 Claude 每堂約一兩分鐘；本機模型每堂要好幾分鐘，一週的課可能要好幾個小時，第一次建議先在 設定 › 使用者 › 備課 選「主題庫七成＋新聞」試 1 天。`;
  const out = el("div", "wk-out");
  out.setAttribute("role", "status");
  bar.append(info, go1, help, out);
  const finish = async (p) => {
    go1.disabled = false; go1.textContent = "派本週的課";
    // What did not go as planned stays on screen, also after the sync line.
    const notes = prepNotes(p);
    const show = (line) => { out.innerHTML = ""; if (line) out.append(line); out.append(...notes.map((n) => n.cloneNode(true))); };
    if (!p.done) { show(notes.length ? null : el("div", "note", "沒有做出新的課：主題都備過了。")); return; }
    if (!weeklyMine) { show(el("div", "note ok", `上次做好 ${p.done} 堂。`)); return; }
    weeklyMine = false;
    show(el("div", "note", `做好 ${p.done} 堂，同步中…`));
    try {
      await api("/api/records/export", {});
      const r = await api("/api/git/sync", {});
      show(el("div", "note ok", r.pushed ? `做好 ${p.done} 堂，已經送出。` : `做好 ${p.done} 堂。這個工作區還沒有連到 repo，連上後按同步。`));
    } catch (e) {
      show(el("div", "note err", `做好 ${p.done} 堂，但同步沒有成功：${e.message}`));
    }
    buddyCache = null;
    paintFoot(state.status);
    fetchBuddies(true).then((d) => drawWho((d.buddies ?? []).filter((b) => b.levels?.[state.lang])));
  };
  let t = null;
  let shown = false;
  const startPolling = () => { if (t) return; t = setInterval(poll, 3000); weeklyTimer = t; };
  const stopPolling = () => { clearInterval(t); if (weeklyTimer === t) weeklyTimer = null; t = null; };
  const poll = async () => {
    if (!bar.isConnected) { stopPolling(); return; }
    try {
      const p = (await api("/api/family/prepare")).progress;
      if (!p) return;
      if (p.running) {
        go1.disabled = true; go1.textContent = "備課中…";
        out.innerHTML = "";
        const pr = el("progress"); pr.max = Math.max(1, p.total); if (p.done) pr.value = p.done;
        pr.setAttribute("aria-label", "備課進度");
        const name = members.find((b) => b.id === p.member)?.name ?? p.member;
        out.append(pr, el("div", "note", `${p.done} / ${p.total} 堂・${name}${p.current ? `・${p.current}` : ""}`));
        startPolling();
      } else if (t || weeklyMine || (!shown && p.planned && members.some((b) => (p.members ?? []).includes(b.id)))) {
        // Also the last run when the page is first shown (after a restart of the page).
        shown = true;
        stopPolling();
        await finish(p);
      }
    } catch { /* next tick */ }
  };
  go1.onclick = () => busy(go1, async () => {
    if (!state.status?.engine_ready) { openSettings(3); return; }
    try {
      // Fresh news first (robots.txt and the per-site delay apply); old news will do when it fails.
      go1.textContent = "抓新聞…";
      await api(`/api/news?lang=${state.lang}&refresh=true`).catch(() => {});
      await api("/api/family/prepare", { members: members.map((b) => b.id), lang: state.lang, days: WEEK.days, choices: WEEK.choices, source: "mix" });
      weeklyMine = true;
      startPolling();
      poll();
    } catch (e) { go1.textContent = "派本週的課"; out.innerHTML = ""; out.append(el("div", "note err", e.message)); }
  });
  // Coming back to the page: a run in progress shows itself (and is finished here).
  setTimeout(poll, 0);
  return bar;
}

/**
 * A run's problems in a few lines: repeated ones once, the library writing
 * failures as one line (details in its tooltip), the library shortfall as a
 * plain note (not an error), and how many lessons were not made.
 */
function prepNotes(p) {
  const errs = [...new Set(p.errors ?? [])];
  const out = [];
  const short = errs.filter((e) => /用新聞補上/.test(e));
  const writing = errs.filter((e) => /寫主題庫的課・/.test(e));
  const other = errs.filter((e) => !short.includes(e) && !writing.includes(e));
  for (const e of [...new Set(short.map((x) => x.replace(/^[^：]*：/, "")))]) out.push(el("div", "note", e));
  if (writing.length) {
    const n = el("div", "note err", `主題庫有 ${writing.length} 個主題沒寫成，用其他的課補上。`);
    n.title = writing.join("\n");
    out.push(n);
  }
  // Different raw errors can read the same once made friendly: once each.
  const plain = new Map();
  for (const e of other) plain.set(friendly(e), [...(plain.get(friendly(e)) ?? []), e]);
  [...plain].slice(0, 3).forEach(([text, raw]) => { const n = el("div", "note err", text); n.title = raw.join("\n"); out.push(n); });
  if (plain.size > 3) out.push(el("div", "note err", `還有 ${plain.size - 3} 個問題`));
  const planned = p.planned || p.total;
  if (planned && p.done < planned) out.push(el("div", "note", `比預定的 ${planned} 堂少了 ${planned - p.done} 堂。`));
  return out;
}

function kindTag(kind, label) {
  return el("span", `tag kind-${kind}`, label ?? (kind === "topic" ? "主題課程" : kind === "fixed" ? "固定課程" : kind === "daily" ? "每日課程" : kind));
}

function courseCard(c, primary) {
  const card = el("div", `course-card${c.status === "finished" ? " finished" : ""}${primary ? " next" : ""}`);
  const top = el("div", "cc-top");
  top.append(kindTag(c.kind, c.kind_label));
  if (c.status === "finished") top.append(el("span", "tag acc2", "完成"));
  card.append(top);
  card.append(el("h3", c.lang === "ja" ? "ja" : null, c.name));
  if (c.summary) card.append(el("div", "sum", c.summary));
  const prog = el("div", "cc-prog");
  prog.append(ptsBar(c.total ? c.done / c.total : 0, c.status === "finished" ? "ok" : ""), el("span", "n", `${c.done} / ${c.total}`));
  card.append(prog);
  const foot = el("div", "cc-foot");
  if (c.next != null) {
    foot.append(elr("span", "nx", lessonLabel(c.next, c.next_title), c.lang));
    const b = el("button", primary ? "primary small" : "ghost", c.done || c.next > 1 ? "繼續" : "開始");
    b.onclick = (e) => { e.stopPropagation(); go("courses", { course: c.id, courseLesson: c.next, step: null }); };
    foot.append(b);
  } else if (c.can_extend) {
    foot.append(el("span", "nx", `${c.words} 個字`));
    const b = el("button", "ghost", "延伸 5 課");
    b.onclick = (e) => { e.stopPropagation(); go("courses", { course: c.id }); };
    foot.append(b);
  }
  card.append(foot);
  card.onclick = () => go("courses", { course: c.id, courseLesson: null });
  return card;
}

// ── create ─────────────────────────────────────────────

/** Two big choices: a topic course or a fixed course. */
function createPicker() {
  const box = el("div", "create-pick");
  const tile = (kind, ic, title, sub) => {
    const b = el("button", `pick-tile kind-${kind}`);
    const i = el("span", "pi"); i.append(icon(ic));
    b.append(i, el("b", null, title), el("span", null, sub));
    b.onclick = () => { state.courseCreate = kind; render(); };
    return b;
  };
  box.append(tile("topic", "news", "主題課程", "關鍵字・網址・文章"), tile("fixed", "cards", "固定課程", "單字表・文法表"), tile("library", "book", "主題庫", "一堂講一件事"));
  return box;
}

async function renderCourseCreate(page) {
  const back = ghostButton("back", "課程", () => { state.courseCreate = null; render(); });
  if (state.courseCreate === "pick") {
    viewbar("新課程", null, back);
    page.append(createPicker());
    return;
  }
  if (state.courseCreate === "library") {
    viewbar("主題庫", null, ghostButton("back", "換一種", () => { state.courseCreate = "pick"; render(); }));
    const box = el("div", "create-box kind-library");
    page.append(box);
    await drawLibrary(box);
    return;
  }
  const topic = state.courseCreate === "topic";
  const toPick = ghostButton("back", "換一種", () => { state.courseCreate = "pick"; render(); });
  viewbar(topic ? "主題課程" : "固定課程", null, toPick);
  const box = el("div", `create-box kind-${state.courseCreate}`);
  page.append(box);
  if (topic) drawTopicCreate(box); else await drawFixedCreate(box);
}

/** What the box holds: URLs, a pasted text (long or several lines) or keywords. */
function parseTopicInput(text) {
  const urls = [], keywords = [];
  let pasted = "";
  const lines = text.split(/\n/).map((l) => l.trim()).filter(Boolean);
  const rest = [];
  for (const l of lines) {
    if (/^https?:\/\/\S+$/i.test(l)) urls.push(l); else rest.push(l);
  }
  const body = rest.join("\n");
  const sentences = (body.match(/[.!?。！？]/g) ?? []).length;
  if (body.length > 120 || sentences >= 2) pasted = body;
  else for (const k of body.split(/[,，、;\n]/)) if (k.trim()) keywords.push(k.trim());
  return { urls, keywords, pasted };
}

function drawTopicCreate(box) {
  const ta = el("textarea", "big-input");
  ta.rows = 4;
  ta.placeholder = state.lang === "ja" ? "例：日本の音楽、旅行\n或貼上網址、文章" : "例：AI chips, space travel\n或貼上網址、文章";
  // From a news card: its address is already in the box.
  if (state.topicPrefill) { ta.value = state.topicPrefill; state.topicPrefill = null; }
  const detected = el("div", "detected");
  const chips = el("div", "topic-chips suggest");
  const help = el("button", "help", "?");
  help.dataset.tip = "輸入關鍵字（逗號分開）、貼上網址（每行一個）或整段文章。網址會先檢查 robots.txt 再抓文字；你給的文章會成為第 2 課起的課文。預設 5 課：背景、三個角度的文章、複習＋對話。";
  const row = el("div", "actions-row");
  const create = primaryButton("建立課程", null);
  create.disabled = true;
  row.append(create, help);
  const out = el("div", "note");
  const update = () => {
    const p = parseTopicInput(ta.value);
    detected.innerHTML = "";
    const add = (ic, n, label) => { if (!n) return; const s = el("span", "det"); s.append(icon(ic), document.createTextNode(`${label} ${n}`)); detected.append(s); };
    add("target", p.keywords.length, "關鍵字");
    add("link", p.urls.length, "網址");
    add("book", p.pasted ? 1 : 0, "文章");
    create.disabled = !(p.keywords.length || p.urls.length || p.pasted);
  };
  ta.oninput = update;
  // The learner's own interests (interests.csv) as one-tap keywords.
  const ints = (state.status?.user?.interests ?? []).filter((i) => !i.lang || i.lang === state.lang).map((i) => i.keyword);
  for (const k of [...new Set(ints)].slice(0, 8)) {
    const c = el("span", null, k);
    c.onclick = () => { ta.value = ta.value.trim() ? `${ta.value.trim()}, ${k}` : k; update(); ta.focus(); };
    chips.append(c);
  }
  create.onclick = () => busy(create, async () => {
    const p = parseTopicInput(ta.value);
    const sources = p.urls.map((u) => ({ url: u }));
    if (p.pasted) sources.push({ text: p.pasted });
    if (!state.status?.engine_ready) { openSettings(3); return; }
    out.className = "note";
    out.textContent = "";
    const sk = planningSkeleton();
    box.append(sk);
    create.textContent = "規劃中…";
    try {
      const c = await api("/api/courses", { kind: "topic", lang: state.lang, keywords: p.keywords, sources });
      if (c.warnings?.length) flash(c.warnings.join("；"), true);
      say(`「${c.name}」規劃好了：${c.total} 課。從第 1 課開始吧！`);
      state.courseCreate = null;
      go("courses", { course: c.id, courseLesson: null });
    } catch (e) {
      out.className = "note err"; out.textContent = e.message;
      create.textContent = "建立課程";
      // A site that does not allow fetching: one tap to paste the text instead.
      if (/不允許自動抓取/.test(e.message)) {
        const paste = ghostButton("pen", "改貼文章", () => {
          ta.value = ta.value.split("\n").filter((l) => !/^https?:\/\//i.test(l.trim())).join("\n").trim();
          ta.placeholder = "把文章文字貼在這裡";
          out.textContent = ""; out.className = "note";
          update(); ta.focus();
        });
        out.append(document.createElement("br"), paste);
      }
    } finally { sk.remove(); }
  });
  box.append(ta, detected, chips, row, out);
  update();
  setTimeout(() => ta.focus(), 50);
}

async function drawFixedCreate(box) {
  const picked = new Set();
  let upload = null;
  const drop = el("div", "drop");
  drop.tabIndex = 0;
  drop.setAttribute("role", "button");
  drop.append(icon("upload"), el("b", null, "拖放單字表或文法表"), el("span", "note", "CSV・一行一個字也可以"));
  const file = el("input"); file.type = "file"; file.accept = ".csv,.tsv,.txt,text/csv,text/plain"; file.hidden = true;
  const chosen = el("div", "chosen");
  const lists = el("div", "list-chips");
  const name = el("input"); name.placeholder = "課程名稱（可留空）";
  const row = el("div", "actions-row");
  const create = primaryButton("建立課程", null);
  create.disabled = true;
  const help = el("button", "help", "?");
  help.dataset.tip = "依程度把單字分課（英文 B1 每課 10 個字），每第 5 課是複習課。上傳的清單存在 recipes/lists/（不覆蓋同名檔案），打開哪一課，那一課的字才進複習。上課時才產生課文：有含這些字的新聞就改寫，沒有就寫一篇短文。";
  row.append(create, help);
  const out = el("div", "note");
  const refresh = () => {
    create.disabled = !(picked.size || upload);
    chosen.innerHTML = "";
    if (upload) {
      const c = el("span", "chip-file");
      c.append(icon("cards"), document.createTextNode(upload.name));
      const x = iconButton("x", "移除", () => { upload = null; refresh(); });
      c.append(x);
      chosen.append(c);
    }
  };
  const take = async (f) => {
    if (!f) return;
    const text = await f.text();
    const kind = /(^|,|\t)\s*(pattern|文法)\s*(,|\t|$)/im.test(text.split("\n")[0] ?? "") ? "grammar" : "vocab";
    upload = { name: f.name, text, kind };
    // The same name the server gives the course when this is left empty.
    if (!name.value) name.placeholder = f.name.replace(/\.[^.]+$/, "").replace(/-(en|ja)$/i, "") || f.name;
    refresh();
  };
  drop.onclick = () => file.click();
  drop.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); file.click(); } };
  file.onchange = () => { take(file.files?.[0]); file.value = ""; };
  // Dropping on this zone takes the file for the course (not the import dialog).
  drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("over"); });
  drop.addEventListener("dragleave", () => drop.classList.remove("over"));
  drop.addEventListener("drop", (e) => { e.preventDefault(); e.stopPropagation(); drop.classList.remove("over"); $("#dropOverlay").hidden = true; take(e.dataTransfer.files?.[0]); });
  box.append(drop, file, chosen);
  try {
    const ls = await api(`/api/courses/lists?lang=${state.lang}`);
    if (ls.length) {
      lists.append(el("div", "k", "或用工作區裡的清單"));
      for (const l of ls) {
        const b = el("button", "chip-btn");
        b.append(icon(l.kind === "vocab" ? "cards" : "tree"), document.createTextNode(`${l.name}`), el("em", null, String(l.items)));
        b.setAttribute("aria-pressed", "false");
        b.onclick = () => {
          if (picked.has(l.path)) picked.delete(l.path); else picked.add(l.path);
          b.setAttribute("aria-pressed", String(picked.has(l.path)));
          refresh();
        };
        lists.append(b);
      }
      box.append(lists);
    }
  } catch { /* lists are optional */ }
  box.append(name, row, out);
  create.onclick = () => busy(create, async () => {
    const vocab = [...picked].filter((p) => !p.includes("/grammar/"));
    const grammar = [...picked].filter((p) => p.includes("/grammar/"));
    const body = { kind: "fixed", lang: state.lang, name: name.value.trim(), vocab, grammar, describe: !!state.status?.engine_ready };
    if (upload) { body[upload.kind === "grammar" ? "grammar_text" : "vocab_text"] = upload.text; body.file_name = upload.name; }
    const sk = planningSkeleton();
    box.append(sk);
    out.className = "note"; out.textContent = "";
    try {
      const c = await api("/api/courses", body);
      if (c.warnings?.length) flash(c.warnings.join("；"), true);
      say(`「${c.name}」排好了：${c.total} 課，${c.words_total} 個字。`);
      state.courseCreate = null;
      go("courses", { course: c.id, courseLesson: null });
    } catch (e) { out.className = "note err"; out.textContent = e.message; } finally { sk.remove(); }
  });
}

function planningSkeleton() {
  const sk = el("div", "skeleton");
  for (let i = 0; i < 4; i++) sk.append(el("i"));
  return sk;
}

// ── outline ────────────────────────────────────────────

async function renderCourseOutline(page) {
  const c = await api(`/api/courses/${encodeURIComponent(state.course)}`);
  const back = ghostButton("back", "課程", () => go("courses", { course: null }));
  // The name is on the card below (once).
  viewbar("", null, back);
  const head = el("div", "outline-head");
  const ring = levelRing(92, `${c.done}/${c.total}`, c.total ? c.done / c.total : 0, "課", c.status === "finished");
  const info = el("div", "oh-info");
  const tags = el("div", "oh-tags");
  tags.append(kindTag(c.kind, c.kind_label), el("span", "tag", c.level));
  for (const k of c.keywords ?? []) tags.append(el("span", "tag acc3", k));
  info.append(tags, el("h2", c.lang === "ja" ? "ja" : null, c.name));
  if (c.summary) info.append(el("div", "sum", c.summary));
  const act = el("div", "actions-row");
  if (c.next != null) {
    const started = c.lessons.some((l) => l.status !== "planned");
    act.append(primaryButton(`${started ? "繼續" : "開始"}第 ${c.next} 課`, () => go("courses", { course: c.id, courseLesson: c.next, step: null })));
  } else if (c.can_extend) {
    const ext = primaryButton("延伸 5 課", () => busy(ext, async () => {
      ext.textContent = "規劃中…";
      try { await api(`/api/courses/${c.id}/extend`, {}); flash("多了 5 課"); render(); }
      catch (e) { flash(e.message, true); ext.textContent = "延伸 5 課"; }
    }));
    act.append(ext, ghostButton("plus", "新主題", () => { state.course = null; state.courseCreate = "topic"; render(); }));
  }
  const help = el("button", "help", "?");
  help.dataset.tip = c.kind === "topic"
    ? "第 1 課背景與核心單字，第 2–4 課三種角度的文章，最後一課複習＋對話。每個新字在系列裡至少出現 3 次。課文在你打開那一課時才產生。大綱檔在 recipes/，可以自己改。"
    : "每課固定數量的新字，每第 5 課複習一次。課文在你打開那一課時才產生。";
  act.append(help);
  info.append(act);
  head.append(ring, info);
  page.append(head);

  const ul = el("ol", "lesson-list");
  let series = 0;
  for (const l of c.lessons) {
    if (c.series > 1 && l.series !== series) {
      series = l.series;
      ul.append(el("li", "series", `第 ${series} 輪`));
    }
    const isNext = l.number === c.next;
    const li = el("li", `ll ${l.status}${isNext ? " next" : ""}`);
    const num = el("span", "num", l.status === "done" ? "" : String(l.number));
    if (l.status === "done") num.append(icon("check"));
    const body = el("div", "ll-body");
    const t = elr("div", "t", l.title.replace(/^第\s*\d+\s*課\s*[：:・]\s*/, "") || `第 ${l.number} 課`, c.lang);
    body.append(t);
    const meta = el("div", "meta");
    if (l.has_article) { const a = el("span", "tag acc3 art"); a.append(icon("book"), document.createTextNode("你的文章")); meta.append(a); }
    if (ROLE_LABEL[l.role]) meta.append(el("span", `tag role-${l.role}`, ROLE_LABEL[l.role]));
    if (l.angle && ANGLE_LABEL[l.angle] && ANGLE_LABEL[l.angle] !== ROLE_LABEL[l.role]) meta.append(el("span", "tag", ANGLE_LABEL[l.angle]));
    if (l.words.length) meta.append(el("span", null, `${l.words.length} 個新字`));
    meta.append(el("span", null, `${l.minutes} 分`));
    body.append(meta);
    const dots = el("div", "udots");
    dots.title = `${l.units_done} / ${l.units.length} 個單元`;
    for (const u of l.units) { const d = el("i", u.done ? "on" : ""); d.title = unitTitle(u.kind, c.lang); dots.append(d); }
    const chip = STATUS_LABEL[l.status] ? el("span", `chip-status s-${l.status}`, STATUS_LABEL[l.status]) : el("span");
    li.append(num, body, dots, chip);
    li.onclick = () => go("courses", { course: c.id, courseLesson: l.number, step: null });
    ul.append(li);
  }
  page.append(ul);
}

// ── the lesson page ────────────────────────────────────

async function renderCourseLesson(page) {
  const id = state.course, n = state.courseLesson;
  // A daily lesson belongs to today: back goes to today (and the nav lights 今日).
  const backFor = (daily) => (daily
    ? ghostButton("back", "今日", () => go("today"))
    : ghostButton("back", "課程大綱", () => go("courses", { course: id, courseLesson: null })));
  viewbar(`第 ${n} 課`, null, backFor(state.fromToday && state.cl?.course?.id === id ? state.cl.course.kind === "daily" : false));
  const sk = el("div", "lesson-wait");
  const waitNote = el("div", "note", "正在準備這一課…");
  waitNote.setAttribute("role", "status");
  sk.append(planningSkeleton(), waitNote);
  page.append(sk);
  state.paintNow?.();
  // A lesson opened for the first time is written now: on a local model that
  // takes minutes. Say so after a while; leaving is fine, it is kept.
  const local = /^llama-server/.test(state.status?.engine ?? "");
  const slow = setTimeout(() => { waitNote.textContent = local ? "第一次打開，正在寫這一課（本機模型要幾分鐘）。可以先去別的地方，寫好會留著。" : "第一次打開，正在寫這一課，約一兩分鐘。可以先去別的地方，寫好會留著。"; }, 8000);
  let o;
  try {
    o = await api(`/api/courses/${encodeURIComponent(id)}/lessons/${n}/open`, {}, "POST", state.signal);
  } catch (e) {
    clearTimeout(slow);
    if (e.name === "AbortError") return; // the learner went elsewhere; the server keeps writing
    sk.remove();
    const err = el("div", "empty cta", e.message);
    if (!state.status?.engine_ready) err.append(primaryButton("連接 Claude", () => openSettings(3)));
    else err.append(ghostButton("refresh", "再試一次", () => render()));
    page.append(err);
    return;
  }
  clearTimeout(slow);
  sk.remove();
  state.cl = o;
  const daily = o.course.kind === "daily";
  state.inDaily = daily;
  markNav();
  const lang = o.course.lang;
  const extra = [backFor(daily)];
  if (lang === "ja") extra.unshift(furiganaButton());
  viewbar("", null, ...extra);
  const h = $("#viewbar h1");
  h.append(elr("span", null, daily ? o.plan.title : lessonLabel(n, o.plan.title), lang));
  h.title = o.course.name;
  const units = o.plan.units;
  const firstOpen = units.findIndex((u) => !u.done);
  let cur = state.step ?? (firstOpen >= 0 ? firstOpen : 0);
  if (cur >= units.length) cur = 0;
  state.step = cur;

  // Steps: done ✓, current lit, the rest waiting.
  const steps = el("ol", "steps-bar");
  steps.setAttribute("aria-label", "課程步驟");
  units.forEach((u, i) => {
    const li = el("li", `${u.done ? "done" : ""}${i === cur ? " cur" : ""}`);
    const b = el("button");
    const c = el("span", "c");
    if (u.done) c.append(icon("check")); else c.append(icon(UNIT_ICON[u.kind] ?? "book"));
    b.append(c, el("span", "l", unitTitle(u.kind, lang)));
    if (u.done) b.append(el("span", "sr-only", "（已完成）"));
    b.setAttribute("aria-current", i === cur ? "step" : "false");
    b.onclick = () => { state.step = i; render(); };
    li.append(b);
    steps.append(li);
  });
  page.append(steps);
  // A new step starts at its top (render keeps the scroll position otherwise).
  if (state.shownStep !== `${id}#${n}#${cur}`) {
    state.shownStep = `${id}#${n}#${cur}`;
    requestAnimationFrame(() => (document.querySelector(".page-area") ?? document.scrollingElement)?.scrollTo(0, 0));
  }
  if (units.every((u) => u.done)) page.append(lessonDoneBanner(o));

  const box = el("div", "step-box");
  page.append(box);
  const u = units[cur];
  const draw = () => {
    box.innerHTML = "";
    if (u.kind === "review") stepReview(box, o, u);
    else if (u.kind === "reading" || u.kind === "quiz") stepReading(box, o, u);
    else if (u.kind === "conversation") stepConversation(box, o, u);
    else stepOutput(box, o, u);
  };
  if (u.done) box.append(stepDoneCard(o, u, draw));
  else draw();
}

/** A step already done: what it earned, today's next task, or practise it again. */
function stepDoneCard(o, u, again) {
  const card = el("div", "step-done");
  const m = el("span", "mark");
  m.append(icon("check"));
  card.append(m, el("b", null, u.points != null ? `已完成 +${fmtPts(u.points)}` : "已完成"));
  const row = el("div", "actions-row");
  const main = primaryButton("回到今日", () => go("today"), true);
  nextTodayStep().then((st) => { main.textContent = st.label; main.onclick = st.onclick; });
  row.append(main, ghostButton("refresh", "再練一次", again));
  card.append(row);
  return card;
}

/**
 * Review without a lesson yet (no Claude needed): the due cards from
 * /api/review, graded like the review page (a round of cards = a unit).
 */
async function renderQuickReview(page) {
  viewbar("今日", null, ghostButton("back", "今日", () => go("today")));
  const due = await api(`/api/review?lang=${state.lang}&limit=${REVIEW_LIMIT}`);
  const box = el("div", "step-box");
  page.append(box);
  if (!due.length) { box.append(quickReviewDone(0, null, null)); return; }
  stepReview(box, { course: { lang: state.lang }, review_cards: due, standalone: true }, { index: 0, kind: "review" });
}

/**
 * The end of a quick review: the cards done, the points when a full round
 * was reached, otherwise how many cards are left to the next point (`round`
 * from /api/review), and today's next step.
 */
function quickReviewDone(cards, award, round) {
  const card = el("div", "step-done");
  const m = el("span", "mark");
  m.append(icon("check"));
  const pts = award?.points ?? 0;
  card.append(m, el("b", null, cards ? "複習完成" : "沒有到期的卡片"));
  if (pts > 0) card.append(el("div", "plus", `+${fmtPts(pts)}`));
  else if (cards) {
    const left = round ? round.size - round.done : 0;
    card.append(el("div", "sub", left > 0 ? `${cards} 張・再 ${left} 張拿 1 分` : `${cards} 張`));
  }
  const row = el("div", "actions-row");
  const main = primaryButton("回到今日", () => go("today"), true);
  nextTodayStep().then((st) => { main.textContent = st.label; main.onclick = st.onclick; });
  row.append(main);
  card.append(row);
  return card;
}
const REVIEW_LIMIT = 20;

function lessonDoneBanner(o) {
  const b = el("div", "lesson-done");
  b.append(icon("check"), el("b", null, "這一課完成了"));
  // What the lesson gave: its points and the words learned.
  const pts = (o.plan.units ?? []).reduce((a, u) => a + (u.points ?? 0), 0);
  const words = (o.plan.words ?? []).map((w) => w.term);
  if (pts || words.length) {
    const sum = el("div", "ld-sum");
    if (pts) sum.append(el("span", "pts", `+${fmtPts(pts)} 分`));
    if (words.length) { sum.append(el("span", null, "學了：")); for (const w of words) sum.append(elr("span", "w", w, o.course.lang)); }
    b.append(sum);
  }
  const next = o.course.next;
  const row = el("div", "actions-row");
  // The main button is today's next task (or back to today).
  const main = primaryButton("回到今日", () => go("today"), true);
  row.append(main);
  nextTodayStep().then((st) => { main.textContent = st.label; main.onclick = st.onclick; });
  if (o.course.kind !== "daily" && next != null) row.append(ghostButton("layers", `第 ${next} 課`, () => go("courses", { course: o.course.id, courseLesson: next, step: null })));
  if (o.notebook) row.append(ghostButton("notebook", "筆記本", () => go("notebook", { nbPage: { lang: o.course.lang, stem: o.notebook } })));
  b.append(row);
  return b;
}

/** 「先做 AI 對話」／「先做中翻英」: a space only before Latin letters. */
const doFirst = (t) => `先做${/^[A-Za-z]/.test(t) ? " " : ""}${t}`;

/** Send a unit result, then the result card with the next step. */
async function reportUnit(box, o, u, body, title, total) {
  let r;
  try {
    r = await api(`/api/courses/${encodeURIComponent(o.course.id)}/lessons/${o.number}/units/${u.index}`, body);
  } catch (e) { flash(e.message, true); return; }
  u.done = u.done || r.passed;
  const star = paintMe(r.summary);
  const nx = r.next ?? {};
  const lang = o.course.lang;
  const daily = o.course.kind === "daily";
  const next = [];
  if (!r.passed) {
    // Positive and forward: redo is the main step, moving on is possible.
    const units = o.plan.units;
    const later = units.find((x) => x.index > u.index && !x.done) ?? units.find((x) => x.index !== u.index && !x.done);
    next.push({ label: "再做一次", primary: true, onclick: () => { state.step = u.index; render(); } });
    if (later) next.push({ label: doFirst(unitTitle(later.kind, lang)), onclick: () => { state.step = later.index; render(); } });
  } else if (r.lesson_done || daily) {
    // The lesson is done, or it is today's daily lesson: today's next task.
    next.push(await nextTodayStep());
    if (!daily && nx.lesson != null && nx.lesson !== o.number) next.push({ label: `第 ${nx.lesson} 課`, onclick: () => go("courses", { course: o.course.id, courseLesson: nx.lesson, step: null }) });
  } else if (nx.lesson === o.number && nx.unit != null) {
    next.push({ label: `下一步：${unitTitle(nx.kind, lang)}`, primary: true, onclick: () => { state.step = nx.unit; render(); } });
    next.push({ label: "回到今日", onclick: () => go("today") });
  } else if (nx.lesson != null) {
    next.push({ label: "下一步", primary: true, onclick: () => go("courses", { course: o.course.id, courseLesson: nx.lesson, step: null }) });
    next.push({ label: "回到今日", onclick: () => go("today") });
  } else {
    next.push({ label: "回到課程", primary: true, onclick: () => go("courses", { course: o.course.id, courseLesson: null }) });
  }
  const gradable = u.kind !== "review";
  const card = resultCard({
    points: r.points,
    title: r.course_finished ? "整個課程完成了！" : r.lesson_done ? "這一課完成了！" : r.passed ? title : "差一點",
    score: gradable ? r.score : null,
    total: gradable ? total : null,
    mistakes: gradable && r.score != null && r.score < 1,
    already: r.already,
    levelUp: r.level_up,
    star,
    difficulty: r.ask_difficulty ? { lang } : null,
    notebook: r.notebook ? { lang, stem: r.notebook } : null,
    next,
  });
  box.append(card);
  // Steps bar: light up the finished step.
  const li = $$(".steps-bar li")[u.index];
  if (li && r.passed) { li.classList.add("done"); const c = $(".c", li); c.innerHTML = ""; c.append(icon("check")); }
  refreshStatus().then(greet);
}

// Step 1: the review round (due cards, then the lesson's words).
function stepReview(box, o, u) {
  const lang = o.course.lang;
  const cards = o.review_cards ?? [];
  const head = el("div", "step-head");
  const help = el("button", "help", "?");
  help.dataset.tip = "先看這一課的單字和文法，再用卡片練：這一課的字，加上以前到期的卡片。按「顯示答案」，再選記得的程度。";
  head.append(el("h2", null, "單字與文法"), el("span", "c", ""), help);
  box.append(head);
  // This lesson's words and grammar first (from the text just read).
  if (o.lesson?.vocabulary?.length || o.lesson?.grammar?.length) {
    const paper = el("div", "paper words-first");
    paper.lang = lang;
    lessonPaper(paper, o.lesson, o.row, { parts: ["words", "grammar"] });
    box.append(paper);
  }
  if (!cards.length) { box.append(el("div", "empty", "沒有要複習的卡片")); return; }
  const bar = ptsBar(0, "thin");
  box.append(bar);
  const stage = el("div", "card");
  box.append(stage);
  // The cards sit under the word list: one button takes the learner there.
  if (box.querySelector(".words-first")) {
    const jump = primaryButton(`練卡片（${cards.length} 張）`, () => {
      stage.scrollIntoView({ block: "center", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
      stage.querySelector(".grades button")?.focus({ preventScroll: true });
    }, true);
    jump.classList.add("jump");
    help.before(jump);
  }
  let i = 0, shown = false, good = 0, award = null, round = null;
  const draw = () => {
    stage.innerHTML = "";
    $(".c", head).textContent = `${Math.min(i + 1, cards.length)} / ${cards.length}`;
    $("i", bar).style.width = `${(i / cards.length) * 100}%`;
    if (i >= cards.length) {
      stage.remove();
      $(".jump", head)?.remove();
      if (o.standalone) {
        // Points come from full review rounds (A stage rules).
        box.append(quickReviewDone(cards.length, award, award ? null : round));
        refreshStatus().then(greet);
        return;
      }
      reportUnit(box, o, u, { cards: cards.length, score: good / cards.length }, "複習完成");
      return;
    }
    const it = cards[i];
    const fc = el("div", "flash-card");
    fc.append(el("div", "kind", it.item_kind === "grammar" ? "文法" : "單字"));
    const front = elr("div", `front${lang === "ja" ? " ja" : ""}`, it.front, lang);
    front.append(Speech.speakerButton(it.front, lang));
    fc.append(front);
    fc.append(el("div", "reading", shown ? it.reading : ""));
    fc.append(el("div", "back", shown ? it.back : ""));
    if (shown && it.example) {
      const ex = elr("div", "example", it.example, lang);
      ex.append(Speech.speakerButton(it.example, lang));
      fc.append(ex);
    }
    const g = el("div", "grades");
    if (!shown) {
      const b = el("button", "primary", "顯示答案");
      b.append(el("small", null, "空白鍵"));
      b.onclick = show;
      g.append(b);
      setTimeout(() => b.focus({ preventScroll: true }), 30);
    } else {
      // Only 記得 is the main button; each grade has its own soft tint and key.
      [[1, "忘了"], [3, "模糊"], [4, "記得"], [5, "很熟"]].forEach(([grade, label], k) => {
        const b = el("button", `grade g${grade}${grade === 4 ? " primary" : ""}`);
        b.append(el("span", null, label), el("small", null, String(k + 1)));
        b.onclick = () => rate(grade);
        g.append(b);
      });
    }
    fc.append(g);
    stage.append(fc);
  };
  let busyRate = false;
  const show = () => { if (!shown && i < cards.length) { shown = true; draw(); } };
  const rate = async (grade) => {
    if (!shown || busyRate || i >= cards.length) return;
    const it = cards[i];
    busyRate = true;
    try {
      if (it.item_id != null) {
        const r = await api("/api/review", { kind: it.item_kind, id: it.item_id, grade, in_course: !o.standalone });
        if (r?.award) award = r.award;
        if (r?.round) round = r.round;
      }
    } catch (e) { flash(e.message, true); busyRate = false; return; }
    busyRate = false;
    if (grade >= 3) good += 1;
    i += 1; shown = false; draw();
  };
  // Keyboard: space shows the answer, 1–4 grade it (while this step is on screen).
  reviewKeys = (e) => {
    if (!stage.isConnected) { reviewKeys = null; return; }
    if (e.key === " ") { e.preventDefault(); show(); }
    const map = { 1: 1, 2: 3, 3: 4, 4: 5 };
    if (map[e.key]) rate(map[e.key]);
  };
  draw();
}

let reviewKeys = null;
document.addEventListener("keydown", (e) => {
  if (!reviewKeys || state.view !== "courses" || e.target.closest?.("input, textarea, select, dialog") || e.metaKey || e.ctrlKey || e.altKey) return;
  reviewKeys(e);
});

// Step 2: today's text, words, grammar and the reading quiz.
function stepReading(box, o, u) {
  const paper = el("div", "paper");
  paper.lang = o.course.lang;
  box.append(paper);
  const quiz = {
    id: o.lesson_id, lang: o.course.lang, total: o.lesson.questions?.length ?? 0, answers: {}, done: false, after: paper,
    onDone: (answers) => reportUnit(box, o, u, { answers }, "閱讀完成", o.lesson.questions?.length ?? 0),
  };
  const words = [...(o.plan.words ?? []).map((w) => w.term), ...(o.plan.review ?? [])];
  // The text and its questions; words and grammar are the next step.
  lessonPaper(paper, o.lesson, o.row, { quiz, highlight: words, parts: ["head", "text", "questions"] });
  // Sentence by sentence through the whole text: listen, then say it.
  const text = o.lesson.text?.trim();
  const tx = paper.querySelector(".lesson-text");
  if (text && tx) {
    const lang = o.course.lang;
    const guide = el("div", "read-guide");
    const listenOnly = !Speech.canListen(lang);
    const label = listenOnly ? "逐句聽" : "逐句跟讀";
    guide.append(el("span", null, listenOnly ? "先一句一句聽，跟著唸出來，再回答下面的問題。" : "先聽一句、跟著唸一句，唸完整篇再回答下面的問題。"));
    const toggle = primaryButton(label, null, true);
    toggle.setAttribute("aria-expanded", "false");
    const smooth = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    const close = () => {
      guide.nextElementSibling?.classList.contains("shadow") && guide.nextElementSibling.remove();
      toggle.textContent = label; toggle.setAttribute("aria-expanded", "false");
    };
    toggle.onclick = () => {
      if (toggle.getAttribute("aria-expanded") === "true") { close(); return; }
      // At the end: on to the questions.
      const toQuestions = () => {
        close();
        // The 問答 heading (the last h2 of the text step).
        const hs = paper.querySelectorAll("h2");
        const q = hs[hs.length - 1];
        (q ?? paper).scrollIntoView({ block: "start", behavior: smooth });
      };
      const panel = Speech.shadowPanel(text.replace(/\n+/g, " "), lang, toQuestions);
      guide.after(panel);
      toggle.textContent = "收起"; toggle.setAttribute("aria-expanded", "true");
      panel.scrollIntoView({ block: "nearest", behavior: smooth });
      panel.querySelector("button.big, button")?.focus({ preventScroll: true });
    };
    guide.append(toggle);
    tx.before(guide);
  }
  // Pictures between the paragraphs (plan §19.4.1), each with its credit;
  // a picture that does not load leaves no gap.
  if (o.pictures?.length && tx) {
    const paras = [...tx.querySelectorAll(":scope > p")];
    for (const pic of o.pictures) {
      const after = paras[pic.paragraph];
      if (!after) continue;
      const fig = el("figure", "lesson-pic");
      const img = el("img");
      img.src = pic.src; img.alt = pic.caption ?? ""; img.loading = "lazy";
      img.onerror = () => fig.remove();
      const cap = el("figcaption");
      const a = el("a", null, `${pic.author}・${pic.licence}`);
      a.href = pic.page; a.target = "_blank"; a.rel = "noopener noreferrer";
      cap.append("圖片：", a);
      fig.append(img, cap);
      after.after(fig);
    }
  }
  if (!quiz.total) reportUnit(box, o, u, { answers: [] }, "閱讀完成");
}

// Step 3: output exercises, checked against the model answer.
function stepOutput(box, o, u) {
  const lang = o.course.lang;
  const all = o.lesson.output ?? [];
  const items = all.filter((x) => x.kind === u.kind).length ? all.filter((x) => x.kind === u.kind) : all;
  const head = el("div", "step-head");
  const help = el("button", "help", "?");
  help.dataset.tip = "先自己寫，再按「對答案」看參考答案，誠實標記對或錯。寫錯的會記進錯題本和這一課的筆記本。";
  head.append(el("h2", null, unitTitle(u.kind, lang)), el("span", "c", `${items.length} 題`), help);
  box.append(head);
  if (!items.length) { box.append(el("div", "empty", "這一課沒有輸出題")); return; }
  const marks = new Array(items.length).fill(null);
  const answers = new Array(items.length).fill("");
  const list = el("div", "out-list");
  items.forEach((it, k) => {
    const q = el("div", "out-item");
    q.append(el("span", "k", String(k + 1)));
    const body = el("div");
    body.append(elr("div", "prompt", it.prompt, it.kind === "translation" ? null : lang));
    const ta = el("textarea");
    ta.rows = 2;
    if (lang === "ja") ta.lang = "ja";
    ta.placeholder = lang === "ja" ? "日本語で" : "Write in English";
    const row = el("div", "row");
    const check = el("button", "ghost", "對答案");
    const fb = el("div", "fb");
    check.onclick = () => {
      answers[k] = ta.value.trim();
      fb.innerHTML = "";
      fb.append(el("b", null, "參考答案 "), elr("span", null, it.answer, lang));
      if (it.hint) fb.append(el("span", "hn", `　${plainRuby(it.hint)}`));
      check.remove();
      // Cloze: the answer can be checked exactly.
      if (u.kind === "cloze" && answers[k]) {
        const ok = plainRuby(it.answer).trim().toLowerCase() === answers[k].toLowerCase();
        mark(ok);
        return;
      }
      const ok = el("button", "ghost ok", "✓ 對了");
      const no = el("button", "ghost no", "✗ 再加油");
      ok.onclick = () => mark(true);
      no.onclick = () => mark(false);
      row.append(ok, no);
    };
    const mark = (v) => {
      marks[k] = v;
      ta.disabled = true;
      $$(".ok, .no", row).forEach((b) => b.remove());
      q.classList.add(v ? "right" : "wrong");
      row.append(el("span", `tag ${v ? "ok" : "bad"}`, v ? "對了" : "記進錯題本"));
      if (marks.every((m) => m !== null)) {
        reportUnit(box, o, u, { items: items.map((x, j) => ({ prompt: x.prompt, answer: answers[j], model: x.answer, hint: x.hint, correct: marks[j] })) }, "輸出完成", items.length);
      }
    };
    row.append(check);
    body.append(ta, row, fb);
    q.append(body);
    list.append(q);
  });
  box.append(list);
}

// Step 4: the conversation, continuing today's text.
function stepConversation(box, o, u) {
  const lang = o.course.lang;
  const conv = o.lesson.conversation ?? {};
  const card = el("div", "conv-start");
  const topic = el("div", "topic");
  topic.append(elr("b", null, conv.topic || o.plan.title, lang));
  card.append(topic);
  if (conv.opening) {
    const b = el("div", `bubble from-ai${lang === "ja" ? " ja" : ""}`);
    setText(b, conv.opening, lang);
    card.append(b);
  }
  const facts = el("div", "facts");
  const f1 = el("span"); f1.append(icon("chat"), document.createTextNode(`${u.items} 回合`));
  const f2 = el("span"); f2.append(icon("clock"), document.createTextNode(`${u.minutes} 分`));
  facts.append(f1, f2);
  card.append(facts);
  if (conv.target_words?.length) {
    const tg = el("div", "targets");
    for (const t of conv.target_words) tg.append(elr("span", "target", t, lang));
    card.append(tg);
  }
  const row = el("div", "row");
  const label = u.done ? "看這段對話" : "開始對話";
  const start = primaryButton(label, () => busy(start, async () => {
    if (!state.status?.engine_ready && !u.done) { openSettings(3); return; }
    start.textContent = "準備中…";
    try {
      const v = await api("/api/conversations", { lang, course: { course: o.course.id, lesson: o.number, unit: u.index } });
      go("conversation", { conv: v });
    } catch (e) { flash(e.message, true); start.textContent = label; }
  }));
  const help = el("button", "help", "?");
  help.dataset.tip = "主題延續今天的課文。試著用上目標字，用到時會亮起。完成全部回合就會評分，這一步也算完成。";
  row.append(start, help);
  card.append(row);
  box.append(card);
}

// ── review notebook ────────────────────────────────────

async function renderNotebook(page) {
  if (!state.status?.user) { viewbar("筆記本", null); page.append(el("div", "empty", "請先選擇使用者")); return; }
  if (state.nbPage) return renderNotebookPage(page);
  const f = state.nbLang ?? state.lang;
  const seg = el("div", "seg nb-filter");
  for (const [v, label] of [["all", "全部"], ["en", "EN"], ["ja", "日本語"]]) {
    const b = el("button", null, label);
    b.dataset.v = v;
    seg.append(b);
  }
  segment(seg, f, (v) => { state.nbLang = v; render(); });
  viewbar("筆記本", null, seg);
  const list = await api(`/api/notebook${f === "all" ? "" : `?lang=${f}`}`);
  if (!list.length) {
    const e = el("div", "empty cta", "做完一課或一段對話，這裡就會多一頁");
    e.append(primaryButton("今日", () => go("today")));
    page.append(e);
    return;
  }
  let day = null;
  const ul = el("ul", "nb-list");
  for (const p of list) {
    if (p.date !== day) { day = p.date; ul.append(el("li", "nb-day", fmtDate(day) || day)); }
    const li = el("li", "item click nb-item");
    const ic = el("span", `nb-ic ${p.kind}`); ic.append(icon(p.kind === "conversation" ? "chat" : "book"));
    const body = el("div");
    body.append(elr("div", "t", p.title, p.lang));
    const meta = el("div", "meta");
    if (p.lang && f === "all") meta.append(el("span", null, langLabel(p.lang)));
    if (p.mistakes) meta.append(el("span", "bad-c", `錯 ${p.mistakes}`));
    if (p.better) meta.append(el("span", null, `改 ${p.better}`));
    if (p.words) meta.append(el("span", null, `字 ${p.words}`));
    if (p.grammar) meta.append(el("span", null, `文法 ${p.grammar}`));
    body.append(meta);
    const side = el("div", "side");
    if (p.has_notes) { const s = el("span", "tag acc", "筆記"); side.append(s); }
    if (p.images) { const s = el("span", "tag"); s.append(icon("image"), document.createTextNode(String(p.images))); side.append(s); }
    li.append(ic, body, side);
    li.onclick = () => go("notebook", { nbPage: { lang: p.lang, stem: p.stem } });
    ul.append(li);
  }
  page.append(ul);
}

async function renderNotebookPage(page) {
  const { lang, stem } = state.nbPage;
  const back = ghostButton("back", "筆記本", () => go("notebook", { nbPage: null }));
  const data = await api(`/api/notebook/${lang}/${encodeURIComponent(stem)}`);
  const p = data.page;
  const d = p.data ?? {};
  const extra = [back];
  if (lang === "ja") extra.unshift(furiganaButton());
  if (data.course) extra.unshift(ghostButton("layers", "回到這一課", () => go("courses", { course: data.course.id, courseLesson: data.course.lesson, step: null })));
  else if (data.lesson_id) extra.unshift(ghostButton("book", "教材", () => go("lessons", { lessonId: data.lesson_id })));
  else if (data.conversation_id) extra.unshift(ghostButton("chat", "對話", () => go("conversation", { conv: { id: data.conversation_id } })));
  viewbar("", null, ...extra);
  $("#viewbar h1").append(elr("span", null, p.title, lang));

  const sheet = el("div", "paper nb-sheet");
  sheet.lang = lang;
  const meta = el("div", "meta");
  meta.append(el("span", null, p.kind === "conversation" ? "對話" : "課"), el("span", null, p.date), el("span", null, langLabel(lang)), infoButton([p.path]));
  sheet.append(meta);
  if (d.summary) sheet.append(elr("div", "summary", d.summary, lang));
  const h2 = (t, n, cls) => { const h = el("h2", cls, t); if (n != null) h.append(el("span", "c", String(n))); return h; };
  if (d.mistakes?.length) {
    sheet.append(h2("錯題", d.mistakes.length, "nb-h bad"));
    for (const m of d.mistakes) {
      const box = el("div", "nb-mistake");
      box.append(elr("div", "nb-q", m.question, lang));
      const yours = el("div", "nb-yours"); yours.append(el("span", "k", "你"), elr("span", null, m.your_answer || "（空白）", lang));
      const right = el("div", "nb-right"); right.append(el("span", "k", "對"), elr("span", null, m.answer, lang));
      box.append(yours, right);
      if (m.explanation) box.append(elr("div", "nb-ex", m.explanation, lang));
      sheet.append(box);
    }
  }
  if (d.better?.length) {
    sheet.append(h2("可以這樣說", d.better.length, "nb-h"));
    const ul = el("ul", "nb-better");
    for (const b of d.better) {
      const li = el("li");
      li.append(elr("span", "o", b.original, lang), document.createTextNode(" → "), elr("b", null, b.suggestion, lang));
      if (b.why) li.append(el("div", "w", b.why));
      ul.append(li);
    }
    sheet.append(ul);
  }
  if (d.words?.length) {
    sheet.append(h2("新單字", d.words.length, "nb-h"));
    lessonPaper(sheet, { vocabulary: d.words.map((w) => ({ term: w.term, reading: w.reading, meaning: w.meaning, example: w.example, part_of_speech: "", example_translation: "" })) }, { lang }, { parts: ["words"] });
    // lessonPaper adds its own heading; keep only ours.
    const hs = $$("h2", sheet);
    hs[hs.length - 1]?.remove();
  }
  if (d.grammar?.length) {
    sheet.append(h2("新文法", d.grammar.length, "nb-h"));
    for (const g of d.grammar) {
      const box = el("div", "gram");
      const h = elr("h3", null, g.pattern, lang); h.append(el("span", "m", g.meaning));
      box.append(h);
      if (g.explanation) box.append(el("p", null, g.explanation));
      if (g.example) {
        const ex = elr("p", "ex", g.example, lang);
        ex.append(Speech.speakerButton(g.example, lang));
        box.append(ex);
      }
      sheet.append(box);
    }
  }
  if (!d.mistakes?.length && !d.better?.length && !d.words?.length && !d.grammar?.length) sheet.append(el("div", "empty", "沒有錯題，也沒有新的字"));
  page.append(sheet);
  page.append(notesEditor(lang, stem, p, data.local_images ?? []));
}

/** My notes: text (saved as you type) and pictures (only on this computer). */
function notesEditor(lang, stem, p, local) {
  const box = el("div", "nb-notes");
  const head = el("div", "step-head");
  const saved = el("span", "saved", "");
  const help = el("button", "help", "?");
  help.dataset.tip = "筆記存在工作區的 notebook/，會跟著同步；App 重新產生上面的內容時不會動到這裡。圖片只存在這台電腦（不進 Git），在別台電腦會顯示提示。";
  head.append(el("h2", null, "我的筆記"), saved, help);
  box.append(head);
  const ta = el("textarea", "notes");
  ta.rows = 6;
  if (lang === "ja") ta.lang = "ja";
  ta.placeholder = "寫下想記住的事…（可以貼上圖片）";
  // Picture references live in the text as ![](jl-image:id); the editor shows them as thumbnails.
  const refRe = /!\[[^\]]*\]\(jl-image:([a-z0-9]+)\)\n?/gi;
  const textOnly = (t) => t.replace(refRe, "").trim();
  let images = [...p.images];
  ta.value = textOnly(p.notes);
  const gallery = el("div", "nb-images");
  const drop = el("div", "drop small nb-drop");
  drop.tabIndex = 0;
  drop.setAttribute("role", "button");
  drop.append(icon("image"), el("span", null, "拖放、貼上或選擇圖片"));
  const file = el("input"); file.type = "file"; file.accept = "image/*"; file.multiple = true; file.hidden = true;
  const compose = () => [ta.value.trim(), ...images.map((id) => `![](jl-image:${id})`)].filter(Boolean).join("\n\n");
  let timer;
  const save = async () => {
    clearTimeout(timer);
    timer = null;
    saved.textContent = "儲存中…";
    try { await api(`/api/notebook/${lang}/${encodeURIComponent(stem)}`, { notes: compose() }, "PUT"); saved.textContent = "✓ 已儲存"; }
    catch (e) { saved.textContent = ""; flash(e.message, true); }
  };
  ta.oninput = () => { saved.textContent = ""; clearTimeout(timer); timer = setTimeout(save, 800); };
  ta.onblur = () => { if (timer) save(); };
  const drawGallery = () => {
    gallery.innerHTML = "";
    for (const id of images) {
      const fig = el("figure");
      if (local.includes(id)) {
        const img = el("img");
        img.src = `/api/notebook/images/${id}`;
        img.alt = "";
        img.loading = "lazy";
        img.onclick = () => window.open(img.src, "_blank", "noopener");
        fig.append(img);
      } else {
        const ph = el("div", "ph");
        ph.append(icon("image"), el("span", null, "圖片只在原本的電腦上"));
        fig.append(ph);
      }
      const del = iconButton("trash", "移除圖片", async () => {
        images = images.filter((x) => x !== id);
        drawGallery();
        await save();
        if (local.includes(id)) api(`/api/notebook/images/${id}`, undefined, "DELETE").catch(() => {});
      });
      fig.append(del);
      gallery.append(fig);
    }
  };
  const addFiles = async (files) => {
    for (const f of files) {
      if (!f.type.startsWith("image/")) { flash("請選一張圖片", true); continue; }
      try {
        saved.textContent = "壓縮圖片…";
        const blob = await compressImage(f, 1600, 0.8);
        const r = await fetch("/api/notebook/images", { method: "POST", body: blob, headers: { "Content-Type": blob.type } });
        const dd = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(dd.error ?? `${r.status}`);
        images.push(dd.id);
        local.push(dd.id);
        drawGallery();
        await save();
      } catch (e) { saved.textContent = ""; flash(e.message, true); }
    }
  };
  drop.onclick = () => file.click();
  drop.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); file.click(); } };
  file.onchange = () => { addFiles([...file.files]); file.value = ""; };
  drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("over"); });
  drop.addEventListener("dragleave", () => drop.classList.remove("over"));
  drop.addEventListener("drop", (e) => { e.preventDefault(); e.stopPropagation(); drop.classList.remove("over"); $("#dropOverlay").hidden = true; addFiles([...e.dataTransfer.files]); });
  ta.addEventListener("paste", (e) => {
    const imgs = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (imgs.length) { e.preventDefault(); addFiles(imgs); }
  });
  drawGallery();
  box.append(ta, gallery, drop, file);
  return box;
}

/** Longest side ≤ max px, WebP (or JPEG where WebP is not available) at `quality`. */
function compressImage(file, max, quality) {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const im = new Image();
    im.onload = () => {
      const scale = Math.min(1, max / Math.max(im.naturalWidth, im.naturalHeight));
      const cv = document.createElement("canvas");
      cv.width = Math.max(1, Math.round(im.naturalWidth * scale));
      cv.height = Math.max(1, Math.round(im.naturalHeight * scale));
      const ctx = cv.getContext("2d");
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, cv.width, cv.height);
      ctx.drawImage(im, 0, 0, cv.width, cv.height);
      URL.revokeObjectURL(url);
      cv.toBlob((b) => {
        if (b && b.type === "image/webp") return resolve(b);
        cv.toBlob((j) => (j ? resolve(j) : reject(new Error("無法處理圖片"))), "image/jpeg", quality);
      }, "image/webp", quality);
    };
    im.onerror = () => { URL.revokeObjectURL(url); reject(new Error("讀不到這張圖片")); };
    im.src = url;
  });
}

// ── the topic library (decisions §22) ──────────────────
// Themes split into lessons that each introduce one thing, with an English
// goal; a lesson text is written once from references (Wikipedia,
// Wikivoyage) and made into a lesson at the learner's level. The admin
// writes more in the background.
let libTimer = null;
async function drawLibrary(box) {
  let data;
  try { data = await api("/api/library"); } catch (e) { box.append(el("div", "note err", e.message)); return; }
  const admin = !!(state.status?.users ?? []).find((x) => x.id === state.status?.user?.id)?.admin;
  if (!data.collections.length) {
    box.append(el("p", "note", admin ? "工作區還沒有主題庫。在私人工作區放一個 library/<名稱>/syllabus.toml（主題清單），就會出現在這裡。" : "還沒有主題庫，請家人幫忙準備。"));
    return;
  }
  for (const c of data.collections) {
    const sec = el("div", "lib");
    const head = el("div", "lib-head");
    head.append(el("b", null, c.name), el("span", "n", `${c.written} 堂可上・${c.themes} 個主題・${c.level}`));
    const help = el("button", "help", "?");
    help.setAttribute("aria-label", "說明：主題庫");
    help.dataset.tip = "每個主題拆成幾堂課，每堂只講一件事，並有一個英文目標（一個生活情境或一組字詞）。課文事先讀過參考資料（Wikipedia、Wikivoyage）寫好、附出處；按「做成課程」時，再依你的程度產生題目和對話。";
    head.append(help);
    sec.append(head);
    const out = el("div", "lib-out");
    out.setAttribute("role", "status");
    let adminRow = null;
    let wasRunning = false;
    const workLeft = c.topics.some((t) => !t.split || t.lessons.some((l) => !l.written));
    if (admin && workLeft) {
      const row = el("div", "actions-row");
      const sel = el("select");
      sel.setAttribute("aria-label", "這次寫幾堂");
      for (const n of [5, 10, 20, 50]) { const o = el("option", null, `${n} 堂`); o.value = String(n); sel.append(o); }
      const go = primaryButton("寫更多課", null, true);
      const poll = async () => {
        if (!out.isConnected) { clearInterval(libTimer); libTimer = null; return; }
        try {
          const r = await api("/api/library/build");
          const p = r.progress;
          if (!p || p.collection !== c.id) return;
          out.innerHTML = "";
          if (p.running) {
            wasRunning = true;
            go.disabled = true; go.textContent = "寫作中…";
            const bar = el("progress"); bar.max = p.total; if (p.done) bar.value = p.done;
            bar.setAttribute("aria-label", "寫作進度");
            out.append(bar, el("div", "note", `${p.done} / ${p.total} 堂${p.current ? `・${p.current}` : ""}`));
          } else {
            clearInterval(libTimer); libTimer = null;
            go.disabled = false; go.textContent = "寫更多課";
            for (const e of p.errors ?? []) out.append(el("div", "note err", e));
            if (p.done && wasRunning) { flash(`寫好 ${p.done} 堂`); render(); }
            else if (p.done) out.append(el("div", "note ok", `上次寫好 ${p.done} 堂。`));
          }
        } catch { /* next tick */ }
      };
      go.onclick = () => busy(go, async () => {
        if (!state.status?.engine_ready) { openSettings(3); return; }
        try {
          await api("/api/library/build", { collection: c.id, limit: Number(sel.value) });
          clearInterval(libTimer); libTimer = setInterval(poll, 3000); poll();
        } catch (e) { out.innerHTML = ""; out.append(el("div", "note err", e.message)); }
      });
      row.append(sel, go);
      adminRow = row;
      // From 「寫更多課」 on the today page: straight to this button.
      if (state.libWrite) {
        state.libWrite = false;
        requestAnimationFrame(() => { row.scrollIntoView({ block: "center" }); go.focus({ preventScroll: true }); });
      }
      poll().then(() => { if (go.disabled) { clearInterval(libTimer); libTimer = setInterval(poll, 3000); } });
    }
    // Written lessons, by category; themes with lessons still to write are folded (admin).
    const ready = c.topics.flatMap((t) => t.lessons.filter((l) => l.written).map((l) => ({ ...l, theme: t })));
    const cats = [...new Set(c.topics.map((t) => t.cat))].filter((k) => admin || ready.some((l) => l.theme.cat === k));
    const chips = el("div", "topic-chips lib-cats");
    let pick = state.libCat && cats.includes(state.libCat) ? state.libCat : null;
    const list = el("ul", "lib-list");
    const later = el("details", "lib-later");
    const drawList = () => {
      list.innerHTML = ""; later.innerHTML = "";
      const rs = ready.filter((l) => !pick || l.theme.cat === pick);
      if (!rs.length) list.append(el("li", "empty-row", admin ? "這一類還沒有寫好的課，可以用下面的「寫更多課」。" : "這一類還沒有課。"));
      rs.forEach((l, i) => {
        const li = el("li");
        const txt = el("div", "t");
        const sub = el("span", "en", `${l.theme.zh}・${l.goal}`);
        sub.title = `${l.title ?? l.en}\n${l.goal}`;
        txt.append(el("b", null, l.zh), sub);
        const b = i === 0 ? primaryButton("做成課程", null, true) : ghostButton("plus", "做成課程", null);
        b.setAttribute("aria-label", `把「${l.zh}」做成課程`);
        b.onclick = () => busy(b, async () => {
          try {
            const r = await api("/api/library/lesson", { collection: c.id, id: l.id });
            go("courses", { course: r.id, courseLesson: 1, courseCreate: null });
          } catch (e) { flash(e.message, true); }
        });
        li.append(el("span", "tag", l.theme.cat), txt, el("span", "n", `${l.words} 字`), b);
        list.append(li);
      });
      const todo = c.topics.filter((t) => (!pick || t.cat === pick) && (!t.split || t.lessons.some((l) => !l.written)));
      if (admin && todo.length) {
        later.append(el("summary", null, `還有課沒寫的主題 ${todo.length} 個`));
        const ul = el("ul", "lib-list");
        for (const t of todo) {
          const li = el("li", "todo");
          const txt = el("div", "t");
          txt.append(el("b", null, t.zh), el("span", "en", t.en));
          const st = t.split ? `${t.lessons.filter((l) => l.written).length} / ${t.lessons.length} 堂` : "還沒拆課";
          li.append(el("span", "tag", t.cat), txt, el("span", "n", st));
          ul.append(li);
        }
        later.append(ul);
      }
    };
    const count = (k) => ready.filter((l) => l.theme.cat === k).length;
    const chip = (label, v) => {
      const s = el("button", `chip-btn${pick === v ? " on" : ""}`, label);
      s.setAttribute("aria-pressed", String(pick === v));
      s.onclick = () => { pick = v; state.libCat = v; $$(".lib-cats .chip-btn", sec).forEach((x) => { const on = x === s; x.classList.toggle("on", on); x.setAttribute("aria-pressed", String(on)); }); drawList(); };
      return s;
    };
    chips.append(chip(`全部 ${ready.length}`, null), ...cats.map((k) => chip(`${k} ${count(k)}`, k)));
    sec.append(chips, list);
    if (admin) sec.append(later);
    if (adminRow) sec.append(adminRow);
    sec.append(out);
    drawList();
    box.append(sec);
  }
}
