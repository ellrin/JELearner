// Start-up wizard (docs/plan.md §3.2): first run, choosing or adding a
// user, and moving a first-version workspace into the multi-user layout.
// Same look as the settings window: tabs on top, 上一步／略過／下一步 below.

const dlgStart = $("#dlgStart");
const wiz = {
  flow: [],       // step names for this run
  i: 0,
  creating: false, // adding a new user (vs. entering an existing one)
  wsMode: "open",
  chips: new Set(),
  readLangs: () => ({ languages: ["en"], majors: {} }),
};

const FLOWS = {
  first: ["welcome", "workspace", "user", "langs", "claude", "interests", "done"],
  pick: ["user", "langs", "claude", "interests", "done"],
  add: ["user", "langs", "claude", "interests", "done"],
  migrate: ["migrate", "done"],
  // The admin sets up a family member's computer (plan §19.2.4).
  assist: ["welcome", "assist", "user", "langs", "claude", "interests", "pin", "done"],
};

/** Avatar colours; a new user gets the first one nobody uses yet. */
const USER_COLORS = ["#ff6b35", "#5b3df5", "#11965a", "#e5484d", "#0ea5e9", "#d4a017", "#c026d3", "#64748b"];
function freeColor() {
  const used = new Set((state.status?.users ?? []).map((u) => u.color.toLowerCase()));
  return USER_COLORS.find((c) => !used.has(c)) ?? USER_COLORS[(state.status?.users ?? []).length % USER_COLORS.length];
}

const INTEREST_PRESETS = ["科技", "AI", "程式設計", "遊戲", "動漫", "電影與影集", "音樂", "旅行", "美食", "咖啡", "運動", "健康", "商業與創業", "科學", "攝影", "時尚", "歷史", "心理學"];

/** Decide whether the wizard is needed after the status changed. */
function checkStart() {
  const s = state.status;
  if (!s || dlgStart.open) return;
  if (s.layout === "v1") return startWizard("migrate");
  if (!s.user && (s.users ?? []).length) return startWizard("pick");
  if (!s.user && s.workspace) return startWizard("add");
  if (s.demo && !store.get("jl.welcomed", false)) return startWizard("first");
}

function startWizard(mode) {
  store.set("jl.welcomed", true);
  wiz.flow = FLOWS[mode];
  wiz.i = 0;
  wiz.creating = mode === "add";
  wiz.mode = mode;
  wiz.chips = new Set();
  if (wiz.creating) $("#stColor").value = freeColor();
  numberTabs();
  wizShow();
  if (!dlgStart.open) dlgStart.showModal();
  tabsInView($("#stTabs"));
}

/** Show this flow's tabs, numbered from 1 (the last one keeps its check mark). */
function numberTabs() {
  $$("#stTabs button").forEach((b) => { b.hidden = !wiz.flow.includes(b.dataset.step); b.tabIndex = -1; });
  wiz.flow.forEach((step, i) => {
    const c = $(`#stTabs button[data-step="${step}"] .c`);
    if (c && step !== "done") c.textContent = String(i + 1);
  });
}

/** Switch flow from the welcome tiles and go to step `at` of it. */
function setFlow(mode, at) {
  wiz.flow = FLOWS[mode];
  wiz.mode = mode;
  wiz.i = at;
  numberTabs();
  wizShow();
}

// The wizard can be left (×, Esc) only when there is a user to return to.
const canLeave = () => !!state.status?.user && wiz.mode !== "migrate";
dlgStart.addEventListener("cancel", (e) => { if (!canLeave()) e.preventDefault(); });
$("#stClose").onclick = () => { if (canLeave()) dlgStart.close(); };

function wizStep() {
  return wiz.flow[wiz.i];
}

function wizShow() {
  const step = wizStep();
  $$("#dlgStart .wizard .step").forEach((n) => (n.hidden = n.dataset.step !== step));
  $$("#stTabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.step === step)));
  tabsInView($("#stTabs"));
  const last = wiz.i === wiz.flow.length - 1;
  $("#stPrev").hidden = wiz.i === 0 || step === "done" || (wiz.mode === "assist" && step === "user");
  // Skipping is possible where nothing is required.
  $("#stSkip").hidden = !["welcome", "claude", "interests"].includes(step) && !(step === "workspace");
  $("#stNext").textContent = last ? "開始學習" : step === "migrate" ? "開始升級" : "下一步";
  $("#stClose").hidden = !canLeave();
  $("#stNext").hidden = false; // the workspace questions hide it while a tile is the answer
  ({ welcome: loadStWelcome, assist: loadStAssist, pin: loadStPin, workspace: loadStWorkspace, user: loadStUser, langs: loadStLangs, claude: loadStClaude, interests: loadStInterests, done: loadStDone, migrate: loadStMigrate }[step] ?? (() => {}))();
  // The main button takes the focus (not the step tabs), unless a field asks for it.
  setTimeout(() => {
    const a = document.activeElement;
    if (!dlgStart.contains(a) || a === dlgStart || a.tabIndex < 0) $("#stNext").focus();
  }, 0);
}

function wizNext() {
  if (wiz.i < wiz.flow.length - 1) { wiz.i += 1; wizShow(); }
}

function stOut(id, msg, err) {
  const o = $(id);
  o.className = `note${err ? " err" : msg ? " ok" : ""}`;
  o.textContent = msg ?? "";
}

$("#stPrev").onclick = () => {
  if (wizStep() === "workspace" && cwBack()) return; // back through the setup questions first
  if (wizStep() === "assist" && faBack()) return;
  if (wiz.i > 0) { wiz.i -= 1; wizShow(); }
};
$("#stSkip").onclick = () => {
  if (wizStep() === "welcome" || wizStep() === "workspace") {
    // Keep the demo workspace; carry on with the user step when one is missing.
    if (state.status?.user) { dlgStart.close(); return; }
    wiz.i = wiz.flow.indexOf("user"); wizShow(); return;
  }
  wizNext();
};

$("#stNext").onclick = () => busy($("#stNext"), async () => {
  const step = wizStep();
  try {
    if (step === "workspace") { if (!(await stDoWorkspace())) return; }
    else if (step === "assist") { if (!(await stDoAssist())) return; }
    else if (step === "pin") { if (!(await stDoPin())) return; }
    else if (step === "user") { if (!(await stDoUser())) return; }
    else if (step === "langs") await stDoLangs();
    else if (step === "interests") await stDoInterests();
    else if (step === "migrate") await stDoMigrate();
    else if (step === "done") { dlgStart.close(); await refreshStatus(); greet(); render(); return; }
    wizNext();
  } catch (e) {
    flash(e.message, true);
  }
});

// ── workspace: the setup wizard (plan §19.2.3) ─────────
// One question at a time; the answer picks the next pane. Before creating or
// downloading, the server checks the path (Git, location not inside another
// repo, the repo answers, private, empty) and the wizard stops on what fails:
// only the failing checks show, each with its fix, in the dialog itself.
const CW_DEFAULT = "~/JELearner-private";
const cw = { pane: "used", history: [], url: "", path: CW_DEFAULT, drawnAt: 0, done: null };

const CW_PANES = {
  used: { title: "在別台電腦用過 JELearner 嗎？", question: true },
  sync: { title: "要同步到 GitHub 嗎？", question: true },
  clone: { title: "下載你的工作區", plan: "clone", url: true, go: "檢查並下載", did: "已下載" },
  create: { title: "建立新的工作區", plan: "create", url: true, go: "檢查並建立", did: "已建立" },
  local: { title: "只存在這台電腦", plan: "local", go: "檢查並建立", did: "已建立" },
  open: { title: "開啟現有的工作區", go: "開啟", did: "已開啟" },
  done: { title: "工作區設定好了", go: "下一步" },
};

/** `/Users/me/x` → `~/x` (the full path stays in the tooltip). */
const tildePath = (p) => String(p ?? "").replace(/^\/(Users|home)\/[^/]+/, "~");

function cwTo(pane) {
  cw.history.push(cw.pane);
  cw.pane = pane;
  drawCw();
}
function cwBack() {
  if (!cw.history.length) return false;
  cw.pane = cw.history.pop();
  drawCw();
  return true;
}

function cwTile(ic, title, sub, onclick, recommend) {
  const b = el("button", `pick-tile${recommend ? " rec" : ""}`);
  const i = el("span", "pi"); i.append(icon(ic));
  b.append(i, el("b", null, title), el("span", null, sub));
  if (recommend) b.append(el("span", "rec-pill", "建議"));
  // A double click must not also answer the next question, drawn in the same place.
  b.onclick = (e) => { if (e.detail > 1 || Date.now() - cw.drawnAt < 250) return; onclick(); };
  return b;
}

function cwField(label, id, value, placeholder) {
  const l = el("label");
  const input = el("input"); input.id = id; input.value = value; input.placeholder = placeholder ?? "";
  input.oninput = () => { $("#cwChecks").hidden = true; stOut("#stWsOut", ""); }; // old results no longer apply
  input.onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); $("#stNext").click(); } };
  l.append(el("span", null, label), input);
  return l;
}

function drawCw() {
  const p = CW_PANES[cw.pane];
  cw.drawnAt = Date.now();
  $("#cwTitle").textContent = p.title;
  const now = state.status?.workspace && !state.status.demo ? state.status.workspace : "";
  const nowEl = $("#stWsNow");
  nowEl.textContent = cw.pane === "used" && now ? `目前：${tildePath(now)}` : "";
  nowEl.title = now;
  const box = $("#cwPane");
  box.innerHTML = "";
  $("#cwChecks").hidden = true;
  stOut("#stWsOut", "");
  if (cw.pane === "used") {
    const tiles = el("div", "create-pick");
    tiles.append(
      cwTile("cloud", "用過", "從 GitHub 下載我的工作區", () => cwTo("clone")),
      cwTile("plus", "第一次用", "建立新的工作區", () => cwTo("sync")),
    );
    const more = el("div", "cw-more");
    const open = el("button", "linkish", "已經有工作區資料夾？開啟它");
    open.onclick = () => cwTo("open");
    more.append(open);
    box.append(tiles, more);
  } else if (cw.pane === "sync") {
    // The recommended answer sits where the pointer already is (under 第一次用).
    const tiles = el("div", "create-pick");
    tiles.append(
      cwTile("folder", "先不要", "只存在這台電腦，之後可以再開", () => cwTo("local")),
      cwTile("refresh", "要", "換電腦、讓家人加入都需要", () => cwTo("create"), true),
    );
    box.append(tiles);
  } else if (cw.pane === "done") {
    const d = el("div", "cw-done");
    const ck = el("span", "big-ok"); ck.append(icon("check"));
    const path = el("div", "path", tildePath(cw.done?.path)); path.title = cw.done?.path ?? "";
    const again = el("button", "linkish", "換一個工作區");
    again.onclick = () => { cw.done = null; cw.history = []; cw.pane = "used"; drawCw(); };
    d.append(ck, el("b", null, cw.done?.label ?? "已設定"), path, again);
    box.append(d);
  } else {
    const form = el("div", "form");
    if (cw.pane === "create") {
      const row = el("div", "cw-gh");
      row.append(
        ghostButton("external", "在 GitHub 建立 private repo", () => api("/api/open/github-new-repo", {}).catch((e) => flash(e.message, true))),
        el("span", "note", "選 Private，不要勾 README，建好後複製網址"),
      );
      box.append(row);
    }
    if (p.url) form.append(cwField("repo 網址", "cwUrl", cw.url, "https://github.com/你/JELearner-private.git"));
    form.append(cwField(cw.pane === "open" ? "資料夾" : "放在", "cwPath", cw.pane === "open" ? "" : cw.path, CW_DEFAULT));
    box.append(form);
  }
  // 下一步 only acts on the panes that do something; questions are answered by the tiles.
  $("#stNext").hidden = !!p.question;
  $("#stNext").textContent = p.go ?? "下一步";
  $("#stPrev").hidden = !cw.history.length && wiz.i === 0;
  // Keyboard: the recommended tile, else the first one, else the first field.
  const focus = $("#cwPane .pick-tile.rec") ?? $("#cwPane .pick-tile") ?? $("#cwPane input");
  // After wizShow's own focus handling (which would aim at the hidden 下一步).
  if (focus) setTimeout(() => focus.focus(), 30);
}

function loadStWorkspace() {
  // Back from 使用者 after setting one up: show what was set, not the first question.
  const s = state.status;
  if (cw.done && s?.workspace === cw.done.path) { cw.pane = "done"; cw.history = []; }
  else { cw.done = null; cw.pane = "used"; cw.history = []; }
  drawCw();
}

/** Only what needs attention, each with its fix (and a one-tap fix where there is one). */
function drawChecks(checks) {
  const ul = $("#cwChecks");
  ul.innerHTML = "";
  ul.hidden = false;
  const passed = checks.filter((c) => c.ok);
  for (const c of checks.filter((c) => !c.ok)) {
    const li = el("li");
    li.append(el("span", "bad", "✗"));
    const body = el("div");
    body.append(el("span", null, c.msg));
    if (c.fix) body.append(el("div", "fix", c.fix));
    // The repo is not what this path expects: switch to the other path, keeping the URL.
    if (c.id === "empty" && (cw.pane === "create" || cw.pane === "clone")) {
      const to = cw.pane === "create" ? "clone" : "create";
      const b = el("button", "ghost small cw-switch", to === "clone" ? "改成下載這個 repo" : "改成建立新的工作區");
      b.onclick = () => { cw.url = $("#cwUrl")?.value.trim() ?? cw.url; cw.history = to === "clone" ? ["used"] : ["used", "sync"]; cw.pane = to; drawCw(); };
      body.append(b);
    }
    if (c.id === "identity") {
      const f = el("div", "cw-id");
      const n = el("input"); n.placeholder = "名字"; n.autocomplete = "name"; n.setAttribute("aria-label", "Git 作者名字"); n.value = state.status?.user?.name ?? "";
      const m = el("input"); m.placeholder = "email"; m.type = "email"; m.autocomplete = "email"; m.setAttribute("aria-label", "Git 作者 email");
      const save = el("button", "primary small", "儲存並繼續");
      save.onclick = () => busy(save, async () => {
        try { await api("/api/setup/git-identity", { name: n.value, email: m.value }); if (await stDoWorkspace()) wizNext(); }
        catch (e) { stOut("#stWsOut", e.message, true); }
      });
      [n, m].forEach((i) => (i.onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); save.click(); } }));
      f.append(n, m, save);
      body.append(f);
    }
    li.append(body);
    ul.append(li);
  }
  if (passed.length) {
    const li = el("li", "passed");
    li.title = passed.map((c) => c.msg).join("\n");
    li.append(el("span", "ok", "✓"), el("span", null, passed.length === checks.length ? "都沒問題" : `其他 ${passed.length} 項沒問題`));
    ul.append(li);
  }
}

/**
 * The pane's action: check, then create / download / open. Returns true when
 * done (the wizard moves on); false to stay, with what went wrong shown in
 * the dialog (never in a toast behind it).
 */
async function stDoWorkspace() {
  const p = CW_PANES[cw.pane];
  if (cw.pane === "done") return true;
  const urlEl = $("#cwUrl"), pathEl = $("#cwPath");
  const url = urlEl?.value.trim() ?? "";
  const typed = pathEl?.value.trim() ?? "";
  if (p.url && !url) { urlEl.focus(); stOut("#stWsOut", "請貼上 repo 網址", true); return false; }
  if (cw.pane === "open" && !typed) { pathEl.focus(); stOut("#stWsOut", "請輸入工作區資料夾的位置", true); return false; }
  const path = typed || CW_DEFAULT;
  cw.url = url; if (cw.pane !== "open") cw.path = path;
  let r;
  try {
    if (cw.pane === "open") {
      r = await api("/api/setup/workspace", { path, init: true });
    } else {
      stOut("#stWsOut", "檢查中…");
      const c = await api("/api/setup/check", { plan: p.plan, url, path });
      stOut("#stWsOut", "");
      drawChecks(c.checks);
      if (!c.ready) return false;
      stOut("#stWsOut", p.plan === "clone" ? "下載中…" : "建立中…");
      r = p.plan === "clone"
        ? await api("/api/setup/clone", { url, path })
        : await api("/api/setup/new", { path, remote: p.plan === "create" ? url : "" });
    }
  } catch (e) {
    stOut("#stWsOut", "");
    drawChecks([{ id: "action", ok: false, msg: e.message }]);
    return false;
  }
  await refreshStatus();
  if (state.status?.layout === "v1") { wiz.flow = FLOWS.migrate; wiz.i = -1; wiz.mode = "migrate"; return true; }
  // A moment of "done" before the next step (principle 3: positive feedback).
  cw.done = { label: r.git_error ? `${p.did}（Git：${r.git_error}）` : p.did, path: r.workspace };
  cw.history = []; cw.pane = "done"; drawCw();
  await new Promise((ok) => setTimeout(ok, 900));
  return true;
}

// ── welcome: who sets this computer up (plan §19.2.2) ──
function loadStWelcome() {
  const who = $("#stWho");
  const first = wiz.mode === "first" || wiz.mode === "assist";
  who.hidden = !first;
  if (!first) return;
  who.innerHTML = "";
  cw.drawnAt = Date.now();
  who.append(
    cwTile("folder", "我自己用", "建立或下載我的工作區", () => { if (wiz.mode !== "first") setFlow("first", 1); else wizNext(); }),
    cwTile("star", "請家人幫我設定", "家人會幫你把這台電腦準備好", () => setFlow("assist", 1)),
  );
  $("#stNext").hidden = true; // the tiles are the answer
  $("#stSkip").hidden = true; // two answers, nothing else (plan §19.2.2)
}

// ── assist: the admin, on a family member's computer (plan §19.2.4) ──
// hand → url → key (GitHub only) → place; then the usual user, language,
// Claude and interest steps, then the PIN. The member never sees Git.
const fa = { pane: "hand", history: [], url: "", key: null };
const FA_PANES = {
  hand: { title: "請把電腦交給幫你設定的人", go: "我是來幫忙設定的" },
  url: { title: "你的 private repo", go: "下一步" },
  key: { title: "讓這台電腦連上你的 repo", go: "我加好了，測試連線" },
  place: { title: "放在這台電腦的哪裡", go: "下載" },
};
function faTo(pane) { fa.history.push(fa.pane); fa.pane = pane; drawFa(); }
function faBack() { if (!fa.history.length) return false; fa.pane = fa.history.pop(); drawFa(); return true; }

async function copyText(text, btn) {
  try { await navigator.clipboard.writeText(text); }
  catch {
    const t = el("textarea"); t.value = text; document.body.append(t); t.select();
    try { document.execCommand("copy"); } catch {}
    t.remove();
  }
  const was = btn.textContent; btn.textContent = "已複製"; setTimeout(() => (btn.textContent = was), 1500);
}

function drawFa() {
  const p = FA_PANES[fa.pane];
  $("#faTitle").textContent = p.title;
  $("#faHelp").hidden = fa.pane === "hand"; // the technical note is for the admin's screens
  const box = $("#faPane");
  box.innerHTML = "";
  stOut("#faOut", "");
  if (fa.pane === "hand") {
    const d = el("div", "fa-hand");
    const ic = el("span", "big-ok"); ic.append(icon("star"));
    d.append(ic, el("p", null, "接下來由幫你設定的家人操作，大約 5 分鐘。設定好之後，你只要打開 App 就能學習。"));
    box.append(d);
  } else if (fa.pane === "url") {
    const form = el("div", "form");
    form.append(cwField("repo 網址", "faUrl", fa.url, "https://github.com/你/JELearner-private"));
    box.append(form);
  } else if (fa.pane === "key") {
    const k = fa.key;
    const ol = el("ol", "fa-steps");
    const step = (...kids) => { const li = el("li"); li.append(...kids); ol.append(li); };
    step(el("span", null, "打開 repo 的 Deploy keys 頁面"), ghostButton("external", "打開", () => api("/api/family/open-key", { url: fa.url }).catch((e) => stOut("#faOut", e.message, true))));
    const tb = el("button", "ghost small", "複製");
    tb.onclick = () => copyText(k.title, tb);
    step(el("span", null, "Title 填："), el("code", null, k.title), tb);
    const kb = el("button", "ghost small", "複製");
    kb.onclick = () => copyText(k.public_key, kb);
    const ta = el("textarea", "fa-key"); ta.readOnly = true; ta.rows = 3; ta.value = k.public_key; ta.onclick = () => ta.select();
    step(el("span", null, "Key 貼上："), kb, ta);
    step(el("span", null, "勾選 "), el("b", null, "Allow write access"), el("span", null, "，按 Add key"));
    box.append(ol);
  } else if (fa.pane === "place") {
    const form = el("div", "form");
    form.append(cwField("放在", "faPath", CW_DEFAULT, CW_DEFAULT));
    box.append(form);
  }
  $("#stNext").hidden = false;
  $("#stNext").textContent = p.go;
  $("#stPrev").hidden = !fa.history.length && wiz.i === 0;
  const focus = $("#faPane input") ?? $("#stNext");
  setTimeout(() => focus.focus(), 30);
}

function loadStAssist() { fa.pane = "hand"; fa.history = []; drawFa(); }

/** The pane's action; true when the assist step is done (on to 使用者). */
async function stDoAssist() {
  try {
    if (fa.pane === "hand") { faTo("url"); return false; }
    if (fa.pane === "url") {
      const url = $("#faUrl").value.trim();
      if (!url) { $("#faUrl").focus(); stOut("#faOut", "請貼上 repo 網址", true); return false; }
      fa.url = url;
      stOut("#faOut", "準備中…");
      fa.key = await api("/api/family/key", { url });
      faTo(fa.key.public_key ? "key" : "place");
      return false;
    }
    if (fa.pane === "key") {
      stOut("#faOut", "測試中…");
      const t = await api("/api/family/test", { url: fa.url });
      if (!t.ok) {
        stOut("#faOut", t.reason === "offline" ? "這台電腦連不到網路，連上後再試一次" : "還連不上：確認 Key 貼對了、有勾 Allow write access，再按一次", true);
        return false;
      }
      if (t.empty) { stOut("#faOut", "這個 repo 還是空的：請先在你自己的電腦建立工作區並同步上去", true); return false; }
      faTo("place");
      return false;
    }
    // place: download with the key
    stOut("#faOut", "下載中…");
    const r = await api("/api/family/clone", { url: fa.url, path: $("#faPath").value.trim() || CW_DEFAULT });
    stOut("#faOut", "");
    const box = $("#faPane");
    box.innerHTML = "";
    const d = el("div", "cw-done");
    const ck = el("span", "big-ok"); ck.append(icon("check"));
    d.append(ck, el("b", null, "已下載"), el("div", "path", tildePath(r.workspace)));
    box.append(d);
    await refreshStatus();
    wiz.creating = false;
    await new Promise((ok) => setTimeout(ok, 900));
    return true;
  } catch (e) {
    stOut("#faOut", e.message, true);
    return false;
  }
}

// ── PIN: guards admin settings on this computer (plan §19.2.5) ──
function loadStPin() {
  $("#stPin").value = ""; $("#stPin2").value = "";
  stOut("#stPinOut", "");
  for (const id of ["#stPin", "#stPin2"]) $(id).onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); $("#stNext").click(); } };
  setTimeout(() => $("#stPin").focus(), 30);
}

async function stDoPin() {
  const a = $("#stPin").value.trim(), b = $("#stPin2").value.trim();
  if (!/^\d{4,6}$/.test(a)) { stOut("#stPinOut", "PIN 要 4 到 6 位數字", true); $("#stPin").focus(); return false; }
  if (a !== b) { stOut("#stPinOut", "兩次輸入的不一樣", true); $("#stPin2").focus(); return false; }
  try {
    const r = await api("/api/family/finish", { member: state.status?.user?.id, pin: a });
    await refreshStatus();
    if (!r.synced) flash("設定好了；同步會在連上網路後自動完成");
    return true;
  } catch (e) {
    stOut("#stPinOut", e.message, true);
    return false;
  }
}

// ── migrate ────────────────────────────────────────────
function loadStMigrate() {
  stOut("#stMigrateOut", "");
  $("#stMigrateName").focus();
}

async function stDoMigrate() {
  const r = await api("/api/workspace/migrate", { name: $("#stMigrateName").value });
  const m = r.migration;
  stOut("#stMigrateOut", `已搬移：${m.moved.join("、") || "（無）"}${m.skipped.length ? `；保留原位：${m.skipped.join("、")}` : ""}`);
  await refreshStatus();
}

// ── user ───────────────────────────────────────────────
function loadStUser() {
  const s = state.status;
  const ul = $("#stPeople");
  ul.innerHTML = "";
  // Setting up a family member's computer: the admin's own profile is not a choice.
  const people = (s?.users ?? []).filter((u) => !(wiz.mode === "assist" && u.admin));
  // Adding someone (or a workspace with nobody yet): only the form.
  if (wiz.mode === "add" || !people.length) wiz.creating = true;
  $("#stUserNow").textContent = wiz.mode === "assist" ? (wiz.creating ? "新增這位家人" : "這台電腦是誰在用？") : wiz.creating ? (people.length ? "新增使用者" : "建立你的使用者") : "你是誰？";
  ul.hidden = wiz.creating && (wiz.mode === "add" || !people.length);
  for (const u of people) {
    const li = el("li", "person");
    const b = el("button");
    const dot = el("span", "big-dot", u.name.slice(0, 1).toUpperCase()); setAvatar(dot, u);
    b.append(dot, el("span", "n", u.name));
    if (u.admin) b.append(el("span", "tag", "管理員"));
    b.onclick = async () => {
      try {
        await api("/api/users/select", { id: u.id });
        resetUserState();
        await refreshStatus();
        // Setting up a family member's computer: their profile exists; on to their Claude and the PIN.
        if (wiz.mode === "assist") { wiz.creating = false; wiz.i = wiz.flow.indexOf("langs"); wizShow(); return; }
        dlgStart.close();
        greet();
        render();
      } catch (e) { stOut("#stUserOut", e.message, true); }
    };
    li.append(b);
    ul.append(li);
  }
  const add = el("li", "person");
  const ab = el("button", "add");
  ab.append(el("span", "big-dot", "＋"), el("span", "n", "新增"));
  ab.onclick = () => { wiz.creating = true; $("#stColor").value = freeColor(); showNewUser(); ab.setAttribute("aria-pressed", "true"); };
  add.append(ab);
  ul.append(add);
  showNewUser();
  stOut("#stUserOut", "");
}

function showNewUser() {
  $("#stNewUser").hidden = !wiz.creating;
  $("#stNext").hidden = !wiz.creating;
  if (wiz.creating) setTimeout(() => $("#stName").focus(), 0);
}

async function stDoUser() {
  if (!wiz.creating) return false;
  if (!$("#stName").value.trim()) { stOut("#stUserOut", "請輸入名字", true); $("#stName").focus(); return false; }
  return true; // created after the language step, with everything at once
}

// ── languages ──────────────────────────────────────────
function loadStLangs() {
  $("#stNext").hidden = false;
  wiz.readLangs = langPicker($("#stLangs"), wiz.creating ? null : state.status?.user, false);
  stOut("#stLangsOut", "");
}

async function stDoLangs() {
  const { languages, majors } = wiz.readLangs();
  if (!languages.length) throw new Error("至少選一種學習語言");
  const daily = Number($("#stDaily").value) || 30;
  if (wiz.creating) {
    await api("/api/users", { name: $("#stName").value, color: $("#stColor").value, languages, majors, daily_minutes: daily });
    resetUserState();
    wiz.creating = false;
  } else {
    await api("/api/user", { languages, majors, daily_minutes: daily }, "PUT");
  }
  await refreshStatus();
}

// ── Claude ─────────────────────────────────────────────
async function loadStClaude() {
  $("#stKeyBox").hidden = true;
  const ul = $("#stClaude");
  ul.innerHTML = "";
  let c;
  try { c = await api("/api/claude?fresh=true"); } catch (e) { ul.append(el("li", null, e.message)); $("#stOther").open = true; return; }
  // Something already works (subscription or API key): say so, and keep the
  // other ways (install, sign-in) folded away until asked for.
  $("#stReady").hidden = !c.active;
  $("#stReadyVia").textContent = c.active ? c.active_label ?? "" : "";
  $("#stOther").open = !c.active;
  const line = (ok, text) => {
    const li = el("li");
    li.append(el("span", ok === true ? "ok" : ok === false ? "bad" : "mid", ok === true ? "✓" : ok === false ? "•" : "!"), el("span", null, text));
    ul.append(li);
  };
  $("#stOther").querySelector("summary").textContent = c.active ? "換一種方式" : "連接方式";
  const sub = c.subscription;
  if (!sub.installed) {
    // The install command stays behind a copy button (no raw shell line in the text).
    line(false, "尚未安裝 Claude Code");
    const copy = iconButton("copy", "複製安裝指令", () => navigator.clipboard?.writeText(c.install).then(() => flash("已複製安裝指令")).catch(() => flash(c.install)));
    copy.title = c.install;
    copy.classList.add("inline-ico");
    ul.lastElementChild.lastElementChild.append(copy);
  }
  else if (sub.ready && sub.account) line(true, `已登入：${sub.account}`);
  else if (sub.ready) line(null, "已登入，但無法確認是哪個帳號；若不是你的帳號，請按「用瀏覽器登入」");
  else line(false, "尚未登入你的 Claude 帳號");
  if (c.api.configured) line(true, c.api.source === "env" ? `API key 來自環境變數（${c.api.masked}），不是存給你的 key` : `你的 API key（${c.api.masked}）`);
  $("#stLogin").hidden = !sub.installed || (sub.ready && !!sub.account);
}
$("#stRecheck").onclick = () => loadStClaude();
$("#stUseKey").onclick = () => { $("#stKeyBox").hidden = false; $("#stKey").focus(); };
$("#stKeySave").onclick = async () => {
  try { await api("/api/settings/claude", { key: $("#stKey").value }); $("#stKey").value = ""; flash("已儲存你的 API key"); loadStClaude(); }
  catch (e) { flash(e.message, true); }
};
$("#stLogin").onclick = async () => {
  try {
    await api("/api/claude-code/login", {});
    flash("已開啟瀏覽器，請在 Anthropic 的頁面登入你自己的 Claude 帳號");
    for (let i = 0; i < 100 && dlgStart.open && wizStep() === "claude"; i++) {
      await new Promise((r) => setTimeout(r, 3000));
      const c = await api("/api/claude?fresh=true");
      if (c.subscription.ready) { flash("已登入"); loadStClaude(); break; }
    }
  } catch (e) { flash(e.message, true); }
};

// ── interests ──────────────────────────────────────────
function loadStInterests() {
  const box = $("#stChips");
  box.innerHTML = "";
  const all = [...new Set([...INTEREST_PRESETS, ...wiz.chips])];
  for (const k of all) {
    const b = el("button", "chip-btn", k);
    b.setAttribute("aria-pressed", String(wiz.chips.has(k)));
    b.onclick = () => { wiz.chips.has(k) ? wiz.chips.delete(k) : wiz.chips.add(k); b.setAttribute("aria-pressed", String(wiz.chips.has(k))); };
    box.append(b);
  }
}
$("#stKeyword").onkeydown = (e) => {
  if (e.key !== "Enter") return;
  const k = $("#stKeyword").value.trim();
  if (k) { wiz.chips.add(k); $("#stKeyword").value = ""; loadStInterests(); }
};

async function stDoInterests() {
  if (wiz.chips.size) await api("/api/user", { interests: [...wiz.chips] }, "PUT");
}

// ── done ───────────────────────────────────────────────
$("#stDoneAvatar").onclick = () => $("#meAvatarFile").click();
$("#stDoneImport").onclick = () => { dlgStart.close(); $("#impResults").innerHTML = ""; openImport(); };

function loadStDone() {
  const u = state.status?.user;
  // Assisted setup: hand the computer back; the "next" cards are for later.
  const assist = wiz.mode === "assist";
  $("#dlgStart [data-step='done'] .st-title").textContent = assist ? "設定好了" : "準備好了";
  $("#dlgStart .done-next").hidden = assist;
  if (assist) $("#stNext").textContent = `交還給 ${u?.name ?? "使用者"}`;
  const box = $("#stDone");
  box.innerHTML = "";
  for (const l of u?.languages ?? []) {
    const c = el("div", "lang-badge");
    c.append(el("b", null, l === "en" ? "英文" : "日文"), el("span", null, u.levels?.[l]?.level ?? ""));
    box.append(c);
  }
}

// However the wizard closes (finish, ×, Esc, choosing a person), the rest of
// the page catches up with the user and workspace it ended on.
dlgStart.addEventListener("close", async () => {
  await refreshStatus();
  greet();
  render();
});
