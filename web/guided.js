// A lesson taught out loud (docs/plan.md §19.4.2): the teacher's script,
// written in advance, played in order — small talk, what we learn today,
// then each paragraph (the learner reads it aloud, the teacher explains its
// words and grammar, the learner makes a sentence), then a spoken quiz in
// which "I don't know" or silence gets a hint. The learner taps to talk
// (at most 20 s, 2.5 s of silence ends it) or types. The results are the
// lesson's own units: the reading quiz and, on a sentence-making lesson,
// the sentences — so points, mistakes and the notebook work as always.

const GUIDE = { max: 20, silence: 2.5 };
const READY = "\u0000ready";
const OUTPUT_KINDS = ["translation", "sentence-making", "cloze", "paraphrase"];
const DONT_KNOW_RE = /\b(i\s*don'?t\s*know|no\s*idea|not\s*sure|i\s*forgot|i\s*have\s*no\s*idea)\b|わかりません|わからない|分かりません|分からない|知らない|しらない|不知道|不會|忘了/i;
/** "I don't know" as the whole answer (a short reply), not inside a real one. */
const dontKnow = (said) => !String(said ?? "").trim() || (DONT_KNOW_RE.test(said) && String(said).trim().split(/\s+/).length <= 5 && String(said).length <= 30);
/** The option letter a learner said: "B", "b.", "option B", "bee", full-width Ｂ. */
function saidLetter(said) {
  const t = String(said ?? "").normalize("NFKC").trim().toLowerCase().replace(/[^\p{L}\p{N}\s]/gu, "").replace(/^(option|answer|letter|the answer is|it'?s|its)\s+/, "").trim();
  return { a: "A", ay: "A", eh: "A", b: "B", bee: "B", be: "B", c: "C", see: "C", sea: "C", d: "D", dee: "D" }[t] ?? null;
}

/** Words of a term, for "did the learner use it": English by word start, Japanese as written. */
function usesTerm(said, term, lang) {
  const s = String(said ?? "").toLowerCase();
  const t = plainRuby(term).toLowerCase().replace(/[〜~…().,!?]/g, " ").trim();
  if (!t) return false;
  if (lang === "ja") return t.split(/\s+/).every((p) => s.includes(p));
  const parts = t.split(/[\s/+]+/).filter((w) => w.length >= 3 && !["the", "and", "for", "with"].includes(w));
  return (parts.length ? parts : [t]).every((w) => new RegExp(`\\b${w.slice(0, Math.max(3, w.length - 2)).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`, "i").test(s));
}

/** A spoken answer against the quiz line's keywords (half of them, at least one). */
function rightAnswer(said, q, lang) {
  const keys = (q.keywords ?? []).filter((k) => k.trim());
  if (!keys.length) return usesTerm(said, q.answer, lang);
  const hit = keys.filter((k) => usesTerm(said, k, lang)).length;
  return hit >= Math.max(1, Math.ceil(keys.length / 2));
}

/** The button on the reading step that starts the lesson out loud. */
function guidedButton(box, o, u) {
  const b = primaryButton("老師帶著上", () => runGuided(box, o, u), true);
  b.title = "閒聊、簡介、每段唸一遍＋講解＋造句、口說問答";
  return b;
}

async function runGuided(box, o, u) {
  const lang = o.course.lang;
  const units = o.plan.units;
  const out = units.find((x) => OUTPUT_KINDS.includes(x.kind) && !x.done);
  const minutes = units.filter((x) => x === u || x === out).reduce((n, x) => n + (x.minutes ?? 0), 0) || 15;
  box.innerHTML = "";
  const wrap = el("div", "guided");
  box.append(wrap);
  const wait = el("div", "lesson-wait");
  const wn = el("div", "note", "老師在準備講稿…");
  wn.setAttribute("role", "status");
  wait.append(planningSkeleton(), wn);
  wrap.append(wait);
  let script;
  try {
    script = (await api(`/api/courses/${encodeURIComponent(o.course.id)}/lessons/${o.number}/script`, {}, "POST", state.signal)).script;
  } catch (e) {
    if (e.name === "AbortError") return;
    wait.remove();
    const err = el("div", "empty cta", e.message);
    err.append(ghostButton("refresh", "再試一次", () => runGuided(box, o, u)), ghostButton("back", "自己上", () => { state.step = u.index; render(); }));
    wrap.append(err);
    return;
  }
  wait.remove();

  // ── the page: where we are, the time, the text, the teacher, the learner ──
  const dock = el("div", "g-dock");
  const head = el("div", "g-head");
  const stages = el("ol", "g-stages");
  const STAGES = ["閒聊", "今天", "課文", "問答"];
  STAGES.forEach((t) => stages.append(el("li", null, t)));
  const clock = el("span", "g-clock");
  const end = ghostButton("x", "結束", () => finish(true));
  head.append(stages, clock, end);
  const paper = el("div", "paper g-paper");
  paper.lang = lang;
  lessonPaper(paper, o.lesson, o.row, { parts: ["text"], highlight: [...(o.plan.words ?? []).map((w) => w.term), ...(o.plan.review ?? [])] });
  const tx = paper.querySelector(".lesson-text");
  if (tx && o.pictures?.length) addPictures(tx, o.pictures);
  paper.querySelectorAll(".say-tools").forEach((x) => x.remove());
  const paras = tx ? [...tx.querySelectorAll(":scope > p")] : [];
  const teacher = el("div", `g-teacher${lang === "ja" ? " ja" : ""}`);
  teacher.setAttribute("aria-live", "polite");
  const tText = el("div", "t");
  const again = iconButton("speaker", "再說一次", () => Speech.speak(tText.dataset.say ?? "", lang));
  teacher.append(el("span", "who", "老師"), tText, again);
  const me = el("div", "g-me");
  const heard = el("div", "heard");
  const ctl = el("div", "g-ctl");
  me.append(heard, ctl);
  dock.append(head, teacher, me);
  wrap.append(dock, paper);
  // The paragraph being read scrolls to just below the dock.
  const ro = window.ResizeObserver ? new ResizeObserver(() => wrap.style.setProperty("--dock-h", `${dock.offsetHeight}px`)) : null;
  ro?.observe(dock);

  let stopped = false;
  const started = Date.now();
  const tick = setInterval(() => {
    if (!wrap.isConnected) { clearInterval(tick); ro?.disconnect(); Speech.stopListening(); Speech.stop(); stopped = true; return; }
    const left = Math.max(0, minutes * 60 - (Date.now() - started) / 1000);
    clock.textContent = left > 0 ? `剩 ${Math.ceil(left / 60)} 分` : "超過時間了";
    clock.classList.toggle("over", left <= 0);
  }, 1000);
  const stage = (i) => [...stages.children].forEach((li, k) => { li.classList.toggle("on", k === i); li.classList.toggle("done", k < i); });
  // Behind time: optional explanations are left out.
  let progress = 0; // 0–1 through the text
  const behind = () => (Date.now() - started) / 1000 > minutes * 60 * (0.15 + 0.65 * progress) + 45;

  const say = (text) => new Promise((resolve) => {
    if (stopped || !text) return resolve();
    setText(tText, text, lang);
    tText.dataset.say = text;
    teacher.classList.add("speaking");
    const done = () => { teacher.classList.remove("speaking"); resolve(); };
    if (!Speech.speak(text, lang, null, done)) setTimeout(done, Math.min(5000, 600 + plainRuby(text).length * 45));
  });

  /**
   * The learner's turn: tap the microphone (or type). `ready` = any sound is
   * enough ("Ready?"). Resolves with what was said ("" when nothing), or
   * null when skipped.
   */
  const hear = ({ ready = false, placeholder = "" } = {}) => new Promise((resolve) => {
    if (stopped) return resolve(null);
    ctl.innerHTML = "";
    heard.textContent = "";
    me.classList.add("turn");
    let stopRec = null, settled = false;
    const settle = (v) => {
      if (settled) return;
      settled = true;
      if (v === READY) Speech.stopListening(); else stopRec?.();
      me.classList.remove("turn", "listening");
      ctl.innerHTML = "";
      if (v) heard.textContent = v === READY ? "Ready!" : v;
      resolve(v);
    };
    const canMic = Speech.canListen(lang);
    if (canMic) {
      const mic = el("button", "primary g-mic");
      mic.append(icon("mic"), el("span", "l", ready ? "說一聲" : "按一下說話"));
      const secs = el("span", "secs", "");
      mic.append(secs);
      mic.onclick = () => {
        if (me.classList.contains("listening")) { stopRec?.(); return; }
        me.classList.add("listening");
        mic.querySelector(".l").textContent = "說完了";
        stopRec = Speech.listen(lang, {
          max: GUIDE.max,
          silence: GUIDE.silence,
          onLoud: ready ? () => settle(READY) : null,
          onTick: (n) => (secs.textContent = `${n}`),
          onBusy: () => { mic.disabled = true; mic.querySelector(".l").textContent = "聽寫中…"; },
          onText: (t) => (heard.textContent = t),
          onEnd: (t) => settle(t ?? ""),
        });
      };
      ctl.append(mic);
      setTimeout(() => mic.focus({ preventScroll: true }), 30);
    }
    if (ready) {
      // One main button: the microphone when it works.
      const rb = canMic ? ghostButton("check", "Ready!", () => settle(READY)) : primaryButton("Ready!", () => settle(READY), true);
      ctl.append(rb);
    }
    else {
      const f = el("form", "g-type");
      const i = el("input");
      i.placeholder = placeholder || (canMic ? "或用打的" : lang === "ja" ? "日本語で" : "Type here");
      if (lang === "ja") i.lang = "ja";
      i.setAttribute("aria-label", "用打的回答");
      const go1 = el("button", "ghost", "送出");
      go1.type = "submit";
      f.append(i, go1);
      f.onsubmit = (e) => { e.preventDefault(); settle(i.value.trim()); };
      ctl.append(f);
      if (!canMic) setTimeout(() => i.focus({ preventScroll: true }), 30);
    }
    ctl.append(ghostButton("arrow", "跳過", () => settle(null)));
  });
  const pick = (list) => list[Math.floor(Math.random() * list.length)] ?? "";
  /**
   * A live reply (small talk, a sentence of the learner's own) from the
   * conversation engine; null when it fails or takes over 10 s, and the
   * script's own line is said instead.
   */
  const live = async (ask) => {
    const ctl2 = new AbortController();
    const t = setTimeout(() => ctl2.abort(), 10000);
    try {
      const r = await api(`/api/courses/${encodeURIComponent(o.course.id)}/lessons/${o.number}/reply`, ask, "POST", ctl2.signal);
      return r?.reply ? r : null;
    } catch { return null; } finally { clearTimeout(t); }
  };
  const RIGHT = lang === "ja" ? ["そのとおり！", "正解です！", "いいですね！"] : ["That's right!", "Exactly!", "Well done!"];
  const EXAMPLE = lang === "ja" ? "たとえば、" : "For example: ";

  // ── the lesson ──
  const items = [];
  const answers = [];
  try {
    stage(0);
    // Two short exchanges; the teacher answers what was actually said.
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
      paras[i]?.scrollIntoView({ block: "start", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
      await say(p.read);
      const read = await hear();
      if (read !== null) await say(p.after_read);
      for (const ex of p.explain) {
        if (ex.optional && behind()) continue;
        paras[i]?.querySelectorAll("mark.tw").forEach((m) => m.classList.toggle("g-term", usesTerm(m.textContent, ex.term, lang)));
        await say(ex.line);
      }
      paras[i]?.querySelectorAll("mark.g-term").forEach((m) => m.classList.remove("g-term"));
      if (p.sentence?.target) {
        // The teacher's word on the sentence: live when it comes, else the rule (the term used).
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
          const again2 = await hear({ placeholder: p.sentence.target });
          if (again2) { said = again2; j = await judge(said); }
          else j = { ok: false, reply: null };
        }
        const ok = j.ok;
        if (ok) await say(j.reply ?? p.sentence.praise);
        else if (said !== null) await say(`${EXAMPLE}${p.sentence.example}`);
        if (said !== null) items.push({ prompt: p.sentence.prompt, answer: said ?? "", model: p.sentence.example, correct: ok });
      }
    }
    paras.forEach((x) => x.classList.remove("g-now"));
    progress = 1;

    stage(3);
    // Every question of the lesson is asked (the reading step scores them
    // all): the script's spoken version, else the lesson's own wording; a
    // choice question says its options.
    const qs = o.lesson.questions ?? [];
    const HINT = lang === "ja" ? "もう一度、本文を見てみましょう。" : "Have another look at the text.";
    const paired = script.quiz.length === qs.length;
    const quiz = (qs.length ? qs : script.quiz).map((lq, k) => {
      const sq = qs.length ? (paired ? script.quiz[k] : null) : lq;
      const opts = lq.kind === "choice" && lq.options?.length ? lq.options : null;
      const ask = opts ? `${plainRuby(lq.question)} ${opts.map((x, i) => `${"ABCD"[i]}: ${plainRuby(x)}.`).join(" ")}` : sq?.question ?? lq.question;
      return { question: ask, answer: lq.answer ?? sq?.answer ?? "", keywords: opts ? [] : sq?.keywords ?? [], hint: sq?.hint || HINT, reveal: sq?.reveal || lq.answer || "", options: opts };
    });
    for (const [k, q] of quiz.entries()) {
      if (stopped) break;
      await say(q.question);
      let said = await hear();
      if (said === null) continue;
      if (dontKnow(said)) {
        await say(q.hint);
        said = await hear();
        if (said === null) continue;
      }
      // A choice: the option's words, or its letter.
      const letter = q.options ? "ABCD"[q.options.indexOf(q.answer)] : null;
      const ok = !dontKnow(said) && (rightAnswer(said, q, lang) || (!!letter && saidLetter(said) === letter));
      await say(ok ? pick(RIGHT) : q.reveal);
      if (k < qs.length) answers.push({ index: k, answer: said, correct: ok });
    }
    await say(script.goodbye);
  } catch (e) {
    flash(e.message, true);
  }
  await finish(false);

  async function finish(early) {
    if (wrap.dataset.finished) return;
    wrap.dataset.finished = "1";
    stopped = true;
    clearInterval(tick);
    Speech.stopListening();
    Speech.stop();
    dock.remove();
    ro?.disconnect();
    // The units this lesson covered: the reading quiz (every question
    // answered) and, on a sentence-making lesson, the sentences.
    const results = [];
    let reading = null;
    const qn = (o.lesson.questions ?? []).length;
    const base = `/api/courses/${encodeURIComponent(o.course.id)}/lessons/${o.number}/units/`;
    try {
      if (!u.done && answers.length && answers.length >= qn) { reading = await api(`${base}${u.index}`, { answers }); results.push(reading); if (reading.passed) u.done = true; }
      if (out && !out.done && out.kind === "sentence-making" && items.length) { const r = await api(`${base}${out.index}`, { items }); results.push(r); if (r.passed) out.done = true; }
    } catch (e) { flash(e.message, true); }
    const pts = results.reduce((n, r) => n + (r.points ?? 0), 0);
    const last = results[results.length - 1];
    if (last?.summary) paintMe(last.summary);
    // The quiz decides the reading step: how it went, and what is missing to pass.
    const missed = reading && !reading.passed;
    const card = resultCard({
      points: pts,
      title: early ? "先上到這裡" : missed ? "差一點" : "這段上完了！",
      score: reading ? reading.score : null,
      total: reading ? qn : null,
      mistakes: answers.some((a) => !a.correct),
      levelUp: results.find((r) => r.level_up)?.level_up,
      next: missed || early
        ? [{ label: "再上一次", primary: true, onclick: () => runGuided(box, o, u) }, { label: "回到這一課", onclick: () => { state.step = null; render(); } }]
        : [await nextTodayStep(), { label: "回到這一課", onclick: () => { state.step = null; render(); } }],
    });
    if (!results.length) card.append(el("div", "score", early ? "還沒有記分；問答全部答完才算課文這一步。" : "問答沒有全部答完，課文這一步還沒記分。"));
    wrap.prepend(card);
    refreshStatus().then(greet);
  }
}
