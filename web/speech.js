// Browser speech (docs/plan.md §15, stage D step 1): the Web Speech API,
// which speaks with the operating system's own voices (macOS, Windows) and
// needs nothing installed. Later the same buttons will use the native or
// local engines behind core's `SpeechProvider` (Whisper, Kokoro).
//
// - Speaking: `speechSynthesis`, a voice per language (local voices first),
//   chosen with the rate in Settings → 語音. Kept per user on this computer.
// - Listening: `SpeechRecognition` only when the browser has it AND the user
//   turned it on (default off): Chrome / Edge / Safari send the audio to the
//   browser vendor's cloud service.
// - Shadowing: listen → say it → the transcript compared with the sentence
//   word by word (Japanese: character by character), an approximate score.
//   No points for now (plan §20).
// Without voices or without the API every button stays hidden.

const Speech = (() => {
  const synth = "speechSynthesis" in window ? window.speechSynthesis : null;
  const Rec = window.SpeechRecognition || window.webkitSpeechRecognition || null;
  const PREFERRED = { en: ["en-US", "en-GB"], ja: ["ja-JP"] };
  const SAMPLE = { en: "Nice to meet you. Let's practise together.", ja: "はじめまして。いっしょに練習しましょう。" };
  const key = () => `jl.speech.${state.status?.user?.id ?? "_"}`;
  const prefs = () => ({ voice: {}, rate: 1, recognition: false, autoRead: false, ...store.get(key(), {}) });
  const setPref = (k, v) => store.set(key(), { ...prefs(), [k]: v });

  let voices = [];
  const loadVoices = () => {
    try { voices = synth ? synth.getVoices() : []; } catch { voices = []; }
    refreshButtons();
    onVoices.forEach((f) => f());
  };
  const onVoices = new Set();
  if (synth) {
    try { synth.addEventListener("voiceschanged", loadVoices); } catch { synth.onvoiceschanged = loadVoices; }
  }

  const normTag = (t) => String(t ?? "").replace("_", "-");
  /** Voices for a language: preferred regions first, local voices before cloud ones. */
  function voicesFor(lang) {
    const pre = PREFERRED[lang] ?? [lang];
    const rank = (v) => {
      const tag = normTag(v.lang);
      const i = pre.findIndex((p) => tag.toLowerCase() === p.toLowerCase());
      return (v.localService ? 0 : 100) + (i < 0 ? 50 : i);
    };
    return voices.filter((v) => normTag(v.lang).toLowerCase().startsWith(lang)).sort((a, b) => rank(a) - rank(b));
  }
  function voiceFor(lang) {
    const list = voicesFor(lang);
    const want = prefs().voice?.[lang];
    return list.find((v) => v.name === want) ?? list[0] ?? null;
  }
  const hasVoices = () => voices.length > 0;
  const canSpeak = (lang) => !!synth && voicesFor(lang).length > 0;
  const hasRecognition = () => !!Rec;
  const canListen = () => !!Rec && prefs().recognition === true;

  /** Text as spoken: Japanese ruby `{漢字|かな}` → 漢字, markup characters dropped. */
  const plain = (text) => plainRuby(text).replace(/[*_#`>]/g, "").replace(/\s+/g, " ").trim();

  let current = null; // { button, utterance }
  function stop() {
    if (!synth) return;
    try { synth.cancel(); } catch { /* ignore */ }
    if (current?.button) current.button.classList.remove("speaking");
    current = null;
  }
  /** Speak `text` in `lang`; `button` (optional) shows the speaking state and stops on a second click. */
  function speak(text, lang, button, onend) {
    if (!canSpeak(lang)) return false;
    const was = current?.button;
    stop();
    if (button && was === button) return false;
    const t = plain(text);
    if (!t) return false;
    const u = new SpeechSynthesisUtterance(t);
    const v = voiceFor(lang);
    if (v) { u.voice = v; u.lang = v.lang; } else u.lang = PREFERRED[lang]?.[0] ?? lang;
    u.rate = Number(prefs().rate) || 1;
    const done = () => {
      if (current?.utterance === u) { current.button?.classList.remove("speaking"); current = null; }
      onend?.();
    };
    u.onend = done;
    u.onerror = done;
    current = { button, utterance: u };
    button?.classList.add("speaking");
    try { synth.speak(u); } catch { done(); return false; }
    return true;
  }

  // Speaker buttons are made before the voices are known (they load
  // asynchronously): hidden until a voice for their language exists.
  function refreshButtons(root = document) {
    root.querySelectorAll?.(".spk[data-lang]").forEach((b) => (b.hidden = !canSpeak(b.dataset.lang)));
    root.querySelectorAll?.(".mic-btn").forEach((b) => (b.hidden = !canListen()));
  }
  /** A small speaker button that reads `text` (a string or a function returning one). */
  function speakerButton(text, lang, label = "朗讀") {
    const b = iconButton("speaker", label);
    b.classList.add("spk");
    b.dataset.lang = lang;
    b.hidden = !canSpeak(lang);
    b.onclick = (e) => { e.stopPropagation(); speak(typeof text === "function" ? text() : text, lang, b); };
    return b;
  }

  // ── recognition ──────────────────────────────────────
  let rec = null;
  /** Listen once in `lang`; `onText(text, final)` gets interim and final text. Returns a stop function. */
  function listen(lang, { onText, onEnd } = {}) {
    if (!canListen()) return () => {};
    stopListening();
    const r = new Rec();
    r.lang = PREFERRED[lang]?.[0] ?? lang;
    r.interimResults = true;
    r.continuous = false;
    r.maxAlternatives = 1;
    let finalText = "";
    r.onresult = (e) => {
      let interim = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const res = e.results[i];
        if (res.isFinal) finalText += res[0].transcript;
        else interim += res[0].transcript;
      }
      onText?.((finalText + interim).trim(), !interim);
    };
    r.onerror = (e) => {
      const msg = { "not-allowed": "麥克風沒有權限", "service-not-allowed": "麥克風沒有權限", "no-speech": "沒有聽到聲音", network: "語音辨識需要網路", "audio-capture": "找不到麥克風" }[e.error];
      if (e.error !== "aborted") flash(msg ?? "語音辨識失敗", true);
    };
    r.onend = () => { if (rec === r) rec = null; onEnd?.(finalText.trim()); };
    rec = r;
    try { r.start(); } catch { rec = null; onEnd?.(""); }
    return () => { try { r.stop(); } catch { /* ignore */ } };
  }
  function stopListening() {
    if (!rec) return;
    try { rec.abort(); } catch { /* ignore */ }
    rec = null;
  }
  /**
   * A microphone button: press to talk, the text goes to `onText` (interim
   * and final); press again to stop. Hidden unless recognition is on.
   */
  function micButton(lang, onText, label = "用說的") {
    const b = iconButton("mic", label);
    b.classList.add("mic-btn");
    b.hidden = !canListen();
    let stopFn = null;
    b.onclick = (e) => {
      e.preventDefault();
      if (stopFn) { stopFn(); return; }
      stop();
      b.classList.add("listening");
      b.setAttribute("aria-pressed", "true");
      stopFn = listen(lang, {
        onText,
        onEnd: () => { stopFn = null; b.classList.remove("listening"); b.setAttribute("aria-pressed", "false"); },
      });
    };
    return b;
  }

  // ── shadowing: compare a transcript with the sentence ────
  const PUNCT = /[\p{P}\p{S}\s]/gu;
  /** Longest common subsequence: which items of `a` are matched in `b`. */
  function matchFlags(a, b) {
    const n = a.length, m = b.length;
    const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    const ok = new Array(n).fill(false);
    let i = 0, j = 0;
    while (i < n && j < m) {
      if (a[i] === b[j]) { ok[i] = true; i++; j++; } else if (dp[i + 1][j] >= dp[i][j + 1]) i++; else j++;
    }
    return ok;
  }
  const normWord = (w) => w.toLowerCase().replace(/[’']/g, "").replace(PUNCT, "");
  /**
   * Compare what was said with the target sentence. Returns display tokens
   * `{ text, ok }` (`ok` null for punctuation) and a 0–100 score.
   * English: words, ignoring case and punctuation. Japanese: characters,
   * without ruby and punctuation; a sentence with readings is also
   * compared in kana (recognition may write either) and the better wins.
   */
  function compare(target, said, lang) {
    if (lang === "ja") {
      const variants = [plainRuby(target), String(target ?? "").replace(RUBY_RE, "$2")];
      const heard = [...String(said ?? "").replace(PUNCT, "")];
      let best = null;
      for (const v of variants) {
        const chars = [...v.replace(/\s+/g, "")];
        const keep = chars.map((c) => !c.match(PUNCT));
        const core = chars.filter((_, i) => keep[i]);
        const flags = matchFlags(core, heard);
        let k = 0;
        const tokens = chars.map((c, i) => ({ text: c, ok: keep[i] ? flags[k++] : null }));
        const score = core.length ? Math.round((flags.filter(Boolean).length / core.length) * 100) : 0;
        if (!best || score > best.score) best = { tokens, score };
      }
      return best;
    }
    const words = plain(target).split(/\s+/).filter(Boolean);
    const norm = words.map(normWord);
    const heard = String(said ?? "").split(/\s+/).map(normWord).filter(Boolean);
    const core = norm.filter(Boolean);
    const flags = matchFlags(core, heard);
    let k = 0;
    const tokens = words.map((w, i) => ({ text: w, ok: norm[i] ? flags[k++] : null }));
    const score = core.length ? Math.round((flags.filter(Boolean).length / core.length) * 100) : 0;
    return { tokens, score };
  }
  /** Sentences of a paragraph. */
  function sentences(text, lang) {
    const t = String(text ?? "").trim();
    const parts = lang === "ja" ? t.split(/(?<=[。！？!?])/u) : t.split(/(?<=[.!?…])\s+(?=\S)/u);
    return parts.map((s) => s.trim()).filter((s) => plainRuby(s).replace(PUNCT, "").length > 0);
  }

  /**
   * The shadowing panel for a paragraph: one sentence at a time, listen,
   * then say it; the words that matched light up, with a percentage.
   */
  function shadowPanel(text, lang) {
    const list = sentences(text, lang);
    const box = el("div", "shadow");
    let i = 0;
    const draw = () => {
      box.innerHTML = "";
      const s = list[i];
      const head = el("div", "sh-head");
      head.append(el("span", "n", `${i + 1} / ${list.length}`));
      const close = iconButton("x", "關閉跟讀", () => { stopListening(); stop(); box.remove(); });
      head.append(el("span", "grow"), close);
      const target = elr("div", "sh-target", s, lang);
      const out = el("div", "sh-out");
      const row = el("div", "sh-row");
      const listenBtn = speakerButton(s, lang, "先聽一次");
      listenBtn.classList.add("big");
      const say = micButton(lang, (t) => { heard.textContent = t; }, "換你說");
      say.classList.add("big");
      say.hidden = false;
      const heard = el("div", "sh-heard");
      if (lang === "ja") heard.lang = "ja";
      // When listening ends: the comparison.
      const orig = say.onclick;
      say.onclick = (e) => {
        if (say.classList.contains("listening")) { orig(e); return; }
        heard.textContent = "";
        out.innerHTML = "";
        orig(e);
        const watch = setInterval(() => {
          if (say.classList.contains("listening")) return;
          clearInterval(watch);
          if (heard.textContent.trim()) showResult(heard.textContent);
        }, 200);
      };
      const showResult = (said) => {
        const r = compare(s, said, lang);
        out.innerHTML = "";
        const diff = el("div", "sh-diff");
        if (lang === "ja") diff.lang = "ja";
        r.tokens.forEach((tk, k) => {
          const span = el("span", tk.ok == null ? "p" : tk.ok ? "hit" : "miss", tk.text);
          diff.append(span);
          if (lang !== "ja" && k < r.tokens.length - 1) diff.append(document.createTextNode(" "));
        });
        const pctEl = el("b", `sh-pct${r.score >= 80 ? " good" : ""}`, `${r.score}%`);
        out.append(pctEl, diff);
      };
      row.append(listenBtn, say);
      if (list.length > 1) {
        const prev = iconButton("back", "上一句", () => { i = (i - 1 + list.length) % list.length; stopListening(); draw(); });
        const next = iconButton("arrow", "下一句", () => { i = (i + 1) % list.length; stopListening(); draw(); });
        row.append(el("span", "grow"), prev, next);
      }
      box.append(head, target, row, heard, out);
    };
    if (list.length) draw();
    return box;
  }
  /** The 跟讀 button for a paragraph: opens the panel after `anchor`. Hidden unless recognition is on. */
  function shadowButton(text, lang, anchor) {
    const b = iconButton("mic", "跟讀");
    b.classList.add("mic-btn");
    b.hidden = !canListen();
    b.onclick = (e) => {
      e.stopPropagation();
      const open = anchor.nextElementSibling?.classList.contains("shadow") ? anchor.nextElementSibling : null;
      if (open) { open.remove(); return; }
      anchor.after(shadowPanel(text, lang));
    };
    return b;
  }

  loadVoices();
  return {
    available: () => !!synth, hasVoices, canSpeak, canListen, hasRecognition, voicesFor, voiceFor, prefs, setPref,
    speak, stop, speakerButton, micButton, shadowButton, listen, stopListening, compare, sentences, plain,
    refreshButtons, onVoices, SAMPLE, loadVoices,
  };
})();

// ── Settings → 08 語音 ──────────────────────────────────
// A voice per language, the rate, a test button; recognition (off by
// default, with the one line on where the audio goes); auto-read for the
// conversation. Everything applies at once and is kept per user on this
// computer.
function renderSpeechSettings(box) {
  box.innerHTML = "";
  const p = Speech.prefs();
  const form = el("div", "form speech-form");
  if (!Speech.available() || !Speech.hasVoices()) {
    form.append(el("div", "now", "這台電腦的瀏覽器沒有內建語音"));
  } else {
    const langs = state.status?.user?.languages?.length ? state.status.user.languages : ["en", "ja"];
    for (const lang of langs) {
      const list = Speech.voicesFor(lang);
      const label = el("label");
      label.append(el("span", null, `${langLabel(lang)}聲音`));
      if (!list.length) {
        label.append(el("span", "note", lang === "ja" ? "沒有日文語音（Windows 需要安裝日文語言套件）" : "沒有英文語音"), el("span"));
        form.append(label);
        continue;
      }
      const sel = el("select");
      sel.dataset.lang = lang;
      const chosen = Speech.voiceFor(lang)?.name;
      for (const v of list) {
        const o = el("option", null, `${v.name}${v.localService ? "" : "・雲端"}`);
        o.value = v.name;
        if (v.name === chosen) o.selected = true;
        sel.append(o);
      }
      sel.onchange = () => Speech.setPref("voice", { ...Speech.prefs().voice, [lang]: sel.value });
      const test = iconButton("speaker", "試聽", () => Speech.speak(Speech.SAMPLE[lang], lang, test));
      test.classList.add("spk");
      test.dataset.lang = lang;
      const inl = el("span", "inline");
      inl.append(test);
      label.append(sel, inl);
      form.append(label);
    }
    const rate = el("label");
    const range = el("input");
    Object.assign(range, { type: "range", min: "0.6", max: "1.4", step: "0.1", value: String(p.rate ?? 1) });
    const out = el("em", null, `${Number(p.rate ?? 1).toFixed(1)}×`);
    range.oninput = () => { out.textContent = `${Number(range.value).toFixed(1)}×`; Speech.setPref("rate", Number(range.value)); };
    const inl = el("span", "inline");
    inl.append(range, out);
    rate.append(el("span", null, "速度"), inl);
    form.append(rate);
    const auto = el("label", "check");
    const ac = el("input"); ac.type = "checkbox"; ac.checked = !!p.autoRead;
    ac.onchange = () => Speech.setPref("autoRead", ac.checked);
    auto.append(ac, el("span", null, "對話時自動朗讀 AI 的話"));
    form.append(auto);
  }
  // Recognition: off by default; the audio leaves the computer.
  const recBox = el("div", "git-box");
  const rl = el("label", "check");
  const rc = el("input"); rc.type = "checkbox"; rc.checked = Speech.canListen(); rc.disabled = !Speech.hasRecognition();
  rc.onchange = () => { Speech.setPref("recognition", rc.checked); Speech.refreshButtons(); };
  rl.append(rc, el("span", null, "語音辨識（對話的麥克風、課文跟讀）"));
  const recForm = el("div", "form");
  recBox.append(recForm);
  recForm.append(rl, el("div", "note", Speech.hasRecognition()
    ? "開啟後，錄音會送到瀏覽器廠商（Google、Microsoft 或 Apple）的雲端服務辨識；不開也可以全部用打字。"
    : "這個瀏覽器不支援語音辨識，可以用打字。"));
  box.append(form, recBox);
}
// Voices arrive late in some browsers: redraw the tab when they do.
Speech.onVoices.add(() => { const b = $("#setSpeech"); if (b && !b.closest(".step").hidden) renderSpeechSettings(b); });
