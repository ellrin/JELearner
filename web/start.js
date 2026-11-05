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
};

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
  $$("#stTabs button").forEach((b) => (b.hidden = !wiz.flow.includes(b.dataset.step)));
  wizShow();
  if (!dlgStart.open) dlgStart.showModal();
}

// The wizard can be closed with Esc only when there is a user to fall back to.
dlgStart.addEventListener("cancel", (e) => { if (!state.status?.user && state.status?.layout !== undefined && wiz.mode !== "first") e.preventDefault(); });

function wizStep() {
  return wiz.flow[wiz.i];
}

function wizShow() {
  const step = wizStep();
  $$("#dlgStart .wizard .step").forEach((n) => (n.hidden = n.dataset.step !== step));
  $$("#stTabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.step === step)));
  const last = wiz.i === wiz.flow.length - 1;
  $("#stPrev").hidden = wiz.i === 0 || step === "done";
  // Skipping is possible where nothing is required.
  $("#stSkip").hidden = !["welcome", "claude", "interests"].includes(step) && !(step === "workspace");
  $("#stNext").textContent = last ? "開始學習" : step === "migrate" ? "開始轉換" : "下一步";
  ({ workspace: loadStWorkspace, user: loadStUser, langs: loadStLangs, claude: loadStClaude, interests: loadStInterests, done: loadStDone, migrate: loadStMigrate }[step] ?? (() => {}))();
}

function wizNext() {
  if (wiz.i < wiz.flow.length - 1) { wiz.i += 1; wizShow(); }
}

function stOut(id, msg, err) {
  const o = $(id);
  o.className = `note${err ? " err" : msg ? " ok" : ""}`;
  o.textContent = msg ?? "";
}

$("#stPrev").onclick = () => { if (wiz.i > 0) { wiz.i -= 1; wizShow(); } };
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
    if (step === "workspace") await stDoWorkspace();
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

// ── workspace ──────────────────────────────────────────
function setStWsMode(m) {
  wiz.wsMode = m;
  segment($("#stWsMode"), m, setStWsMode);
  $$("#dlgStart [data-step='workspace'] [data-mode]").forEach((n) => (n.hidden = n.dataset.mode !== m));
}
setStWsMode("open");

function loadStWorkspace() {
  const s = state.status;
  $("#stWsNow").textContent = s?.workspace ? (s.demo ? "目前用的是示範工作區。" : `目前的工作區：${s.workspace}`) : "還沒有工作區。";
  stOut("#stWsOut", "");
}

async function stDoWorkspace() {
  let r;
  if (wiz.wsMode === "open") r = await api("/api/setup/workspace", { path: $("#stOpenPath").value, init: true });
  else if (wiz.wsMode === "new") r = await api("/api/setup/new", { path: $("#stNewPath").value, remote: $("#stNewRemote").value });
  else if (wiz.wsMode === "clone") r = await api("/api/setup/clone", { url: $("#stCloneUrl").value, parent: $("#stCloneParent").value || "~/" });
  if (r) {
    stOut("#stWsOut", `已開啟 ${r.workspace}${r.git_error ? `；Git：${r.git_error}` : ""}`, !!r.git_error);
    await refreshStatus();
    if (state.status?.layout === "v1") { wiz.flow = FLOWS.migrate; wiz.i = -1; wiz.mode = "migrate"; }
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
  const people = s?.users ?? [];
  for (const u of people) {
    const li = el("li", "person");
    const b = el("button");
    const dot = el("span", "big-dot", u.name.slice(0, 1)); dot.style.background = u.color;
    b.append(dot, el("span", "n", u.name));
    if (u.admin) b.append(el("span", "tag", "管理員"));
    b.onclick = async () => {
      try {
        await api("/api/users/select", { id: u.id });
        await refreshStatus();
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
  ab.append(el("span", "big-dot", "＋"), el("span", "n", "新增使用者"));
  ab.onclick = () => { wiz.creating = true; showNewUser(); };
  add.append(ab);
  ul.append(add);
  $("#stUserNow").textContent = people.length ? "點自己的名字進入，或新增一位使用者。不需要密碼。" : "這個工作區還沒有使用者，先建立你自己。";
  if (!people.length) wiz.creating = true;
  showNewUser();
  stOut("#stUserOut", "");
}

function showNewUser() {
  $("#stNewUser").hidden = !wiz.creating;
  $("#stNext").hidden = !wiz.creating;
  if (wiz.creating) $("#stName").focus();
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
  try { c = await api("/api/claude?fresh=true"); } catch (e) { ul.append(el("li", null, e.message)); return; }
  const line = (ok, text) => { const li = el("li"); li.append(el("span", ok ? "ok" : "mid", ok ? "✓" : "•"), el("span", null, text)); ul.append(li); };
  const sub = c.subscription;
  if (!sub.installed) line(false, `尚未安裝 Claude Code（安裝：${c.install}）`);
  else line(sub.ready, sub.ready ? `已用你的 Claude 帳號登入${sub.account ? `：${sub.account}` : ""}` : "已安裝 Claude Code，尚未登入你的帳號");
  line(c.api.configured, c.api.configured ? `已設定你的 API key（${c.api.masked}）` : "沒有 API key（可選）");
  $("#stLogin").hidden = !sub.installed || sub.ready;
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
function loadStDone() {
  const u = state.status?.user;
  const langs = (u?.languages ?? []).map((l) => `${l === "en" ? "英文" : "日文"} ${u.levels?.[l]?.level ?? ""}`).join("、");
  $("#stDone").textContent = u
    ? `${u.name}，你的學習語言：${langs}。前 5 課結束時會問你難度，系統再慢慢調整到剛好。到「今日」開始吧。`
    : "完成。";
}
