// Speech (docs/plan.md §15): speaking with the operating system's voices,
// listening on this computer when it can.
//
// - Speaking: `speechSynthesis`, a voice per language (Premium, then
//   Enhanced, then the rest; novelty voices last), chosen with the rate in
//   Settings → 語音. Kept per user on this computer.
// - Listening, stage D step 2 (decisions §14): when the server has on-device
//   recognition (`GET /api/speech`, Apple's on the Mac), the page records a
//   16 kHz mono WAV with a 15-second countdown and the server transcribes
//   it; nothing leaves the computer, so it is on by default. Otherwise the
//   browser's `SpeechRecognition`, only when the user turned it on (default
//   off): Chrome / Edge / Safari send the audio to the vendor's cloud.
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
  // recognition: the browser's cloud service (opt-in); localRecognition: on-device (opt-out).
  const prefs = () => ({ voice: {}, rate: 1, recognition: false, localRecognition: true, autoRead: false, ...store.get(key(), {}) });
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
  /**
   * Voice quality from its name or URI (macOS: `com.apple.voice.premium.…`,
   * `….enhanced.…`, `….compact.…`; `com.apple.eloquence.…` and the old
   * `com.apple.speech.synthesis.voice.…` novelty voices sound worst).
   * 0 Premium, 1 Enhanced, 2 standard, 3 Eloquence, 4 novelty.
   */
  // Chrome reports the bare name as voiceURI, so the macOS families are also known by name.
  const ELOQUENCE = new Set(["Eddy", "Flo", "Grandma", "Grandpa", "Reed", "Rocko", "Sandy", "Shelley"]);
  const NOVELTY = new Set(["Albert", "Bad News", "Bahh", "Bells", "Boing", "Bubbles", "Cellos", "Good News", "Jester", "Organ", "Superstar", "Trinoids", "Whisper", "Wobble", "Zarvox", "Fred", "Junior", "Kathy", "Ralph"]);
  function quality(v) {
    const id = `${v.name} ${v.voiceURI ?? ""}`.toLowerCase();
    const base = String(v.name ?? "").replace(/\s*\(.*$/, "");
    if (id.includes("premium")) return 0;
    if (id.includes("enhanced")) return 1;
    if (id.includes("eloquence") || ELOQUENCE.has(base)) return 3;
    if (id.includes("speech.synthesis.voice") || NOVELTY.has(base)) return 4;
    return 2;
  }
  const TIER = ["Premium", "加強版", "", "", ""];
  /** Voices for a language: on this computer first, then by quality, then preferred region. */
  function voicesFor(lang) {
    const pre = PREFERRED[lang] ?? [lang];
    const rank = (v) => {
      const tag = normTag(v.lang);
      const i = pre.findIndex((p) => tag.toLowerCase() === p.toLowerCase());
      return (v.localService ? 0 : 1000) + quality(v) * 100 + (i < 0 ? 50 : i);
    };
    return voices.filter((v) => normTag(v.lang).toLowerCase().startsWith(lang)).sort((a, b) => rank(a) - rank(b));
  }
  /** True when this language has only standard-or-worse voices: worth suggesting a Premium download. */
  const lacksGoodVoice = (lang) => !voicesFor(lang).some((v) => quality(v) <= 1);
  function voiceFor(lang) {
    const list = voicesFor(lang);
    const want = prefs().voice?.[lang];
    return list.find((v) => v.name === want) ?? list[0] ?? null;
  }
  const hasVoices = () => voices.length > 0;
  const canSpeak = (lang) => !!synth && voicesFor(lang).length > 0;

  // On-device recognition offered by the server: { engine, langs: { en: { supported, installed } } }.
  let local = { engine: null, langs: {} };
  const canRecord = () => !!(navigator.mediaDevices?.getUserMedia && (window.AudioContext || window.webkitAudioContext));
  async function loadLocal() {
    try {
      const r = await api("/api/speech");
      local = { engine: r.engine, langs: Object.fromEntries((r.langs ?? []).map((l) => [l.lang, l])) };
    } catch { local = { engine: null, langs: {} }; }
    refreshButtons();
    onVoices.forEach((f) => f());
    return local;
  }
  /** The server can recognise `lang` on this computer (model may still need downloading). */
  const hasLocal = (lang) => !!local.engine && !!local.langs[lang]?.supported && canRecord();
  const usesLocal = (lang) => hasLocal(lang) && prefs().localRecognition !== false;
  const hasRecognition = () => !!Rec;
  /** Whether a microphone shows for `lang`: on-device (default on) or the browser's (opt-in). */
  const canListen = (lang) => usesLocal(lang) || (!!Rec && prefs().recognition === true);

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
    root.querySelectorAll?.(".mic-btn").forEach((b) => (b.hidden = !canListen(b.dataset.lang)));
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

  // ── on-device recognition: record a WAV, the server transcribes ──
  const MAX_SECONDS = 15;
  const RATE = 16000;
  /** 16-bit PCM mono WAV from Float32 samples. */
  function wav(samples, rate) {
    const buf = new ArrayBuffer(44 + samples.length * 2);
    const v = new DataView(buf);
    const str = (o, s) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
    str(0, "RIFF"); v.setUint32(4, 36 + samples.length * 2, true); str(8, "WAVE");
    str(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
    v.setUint32(24, rate, true); v.setUint32(28, rate * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
    str(36, "data"); v.setUint32(40, samples.length * 2, true);
    samples.forEach((x, i) => v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, x)) * 0x7fff, true));
    return new Blob([buf], { type: "audio/wav" });
  }
  /** Average neighbouring samples down to `to` Hz (when the browser would not record at 16 kHz). */
  function downsample(x, from, to) {
    if (from === to) return x;
    const ratio = from / to;
    const out = new Float32Array(Math.floor(x.length / ratio));
    for (let i = 0; i < out.length; i++) {
      const a = Math.floor(i * ratio), b = Math.min(x.length, Math.floor((i + 1) * ratio));
      let s = 0;
      for (let j = a; j < b; j++) s += x[j];
      out[i] = s / Math.max(1, b - a);
    }
    return out;
  }
  let recording = null; // { finish(send), lang }
  /**
   * Record up to 15 s, then send the WAV to the server. `onTick(secondsLeft)`
   * each second; `onBusy()` while the server transcribes; `onText(text, true)`
   * and `onEnd(text)` with the result ("" on failure, with a flash).
   * Returns a stop function that sends what was recorded.
   */
  function recordLocal(lang, { onText, onEnd, onTick, onBusy } = {}) {
    let stream = null, ctx = null, node = null, timer = null, done = false;
    const chunks = [];
    let startedAt = 0;
    const cleanup = () => {
      clearInterval(timer);
      try { node?.disconnect(); } catch {}
      stream?.getTracks().forEach((t) => t.stop());
      try { ctx?.close(); } catch {}
      if (recording?.lang === lang) recording = null;
    };
    const finish = async (send) => {
      if (done) return;
      done = true;
      const rate = ctx?.sampleRate ?? RATE;
      cleanup();
      if (!send) { onEnd?.(""); return; }
      const total = chunks.reduce((n, c) => n + c.length, 0);
      if (total < rate * 0.3) { flash("沒有聽到聲音", true); onEnd?.(""); return; }
      const all = new Float32Array(total);
      let o = 0;
      for (const c of chunks) { all.set(c, o); o += c.length; }
      onBusy?.();
      try {
        const r = await fetch(`/api/speech/transcribe?lang=${lang}`, { method: "POST", headers: { "Content-Type": "audio/wav" }, body: wav(downsample(all, rate, RATE), RATE) });
        const data = await r.json().catch(() => null);
        if (r.status === 409) { flash(`${data?.error ?? "還沒下載語音辨識模型"}：到「設定 → 08 語音」下載`, true); onEnd?.(""); return; }
        if (!r.ok) throw new Error(data?.error ?? `${r.status}`);
        const text = String(data?.text ?? "").trim();
        if (!text) flash("沒有聽到聲音", true);
        onText?.(text, true);
        onEnd?.(text);
      } catch (e) {
        flash(`語音辨識失敗：${e.message}`, true);
        onEnd?.("");
      }
    };
    recording = { lang, finish };
    (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
      } catch (e) {
        flash(e?.name === "NotAllowedError" ? "麥克風沒有權限" : "找不到麥克風", true);
        done = true; cleanup(); onEnd?.(""); return;
      }
      if (done) { cleanup(); return; } // stopped before the microphone opened
      const AC = window.AudioContext || window.webkitAudioContext;
      try { ctx = new AC({ sampleRate: RATE }); } catch { ctx = new AC(); }
      const src = ctx.createMediaStreamSource(stream);
      // ScriptProcessor: deprecated but in every browser, and needs no module file.
      node = ctx.createScriptProcessor(4096, 1, 1);
      node.onaudioprocess = (e) => { if (!done) chunks.push(new Float32Array(e.inputBuffer.getChannelData(0))); };
      src.connect(node);
      node.connect(ctx.destination);
      startedAt = Date.now();
      onTick?.(MAX_SECONDS);
      timer = setInterval(() => {
        const left = MAX_SECONDS - Math.floor((Date.now() - startedAt) / 1000);
        if (left <= 0) finish(true); else onTick?.(left);
      }, 250);
    })();
    return () => finish(true);
  }

  // ── recognition ──────────────────────────────────────
  let rec = null;
  /**
   * Listen once in `lang`: on this computer when it can, else the browser's
   * service. `onText(text, final)` gets interim and final text; `onTick`
   * and `onBusy` only fire for on-device recording. Returns a stop function
   * (on-device: stop and send).
   */
  function listen(lang, { onText, onEnd, onTick, onBusy } = {}) {
    if (!canListen(lang)) return () => {};
    stopListening();
    if (usesLocal(lang)) return recordLocal(lang, { onText, onEnd, onTick, onBusy });
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
  /** Stop listening without a result (on-device: the recording is dropped). */
  function stopListening() {
    recording?.finish(false);
    if (!rec) return;
    try { rec.abort(); } catch { /* ignore */ }
    rec = null;
  }
  /**
   * A microphone button: press to talk, the text goes to `onText` (interim
   * and final); press again to stop. On this computer it records up to 15 s
   * with the seconds left on the button, then sends; while the server
   * transcribes the button shows a busy state. Hidden unless recognition
   * is available for `lang`.
   */
  function micButton(lang, onText, label = "用說的") {
    const b = iconButton("mic", label);
    b.classList.add("mic-btn");
    b.dataset.lang = lang;
    b.hidden = !canListen(lang);
    let stopFn = null;
    const reset = () => {
      stopFn = null;
      b.classList.remove("listening", "busy");
      delete b.dataset.left;
      b.disabled = false;
      b.setAttribute("aria-pressed", "false");
      b.setAttribute("aria-label", label);
    };
    b.onclick = (e) => {
      e.preventDefault();
      if (b.classList.contains("busy")) return;
      if (stopFn) { stopFn(); return; }
      stop();
      b.classList.add("listening");
      b.setAttribute("aria-pressed", "true");
      stopFn = listen(lang, {
        onText,
        onTick: (left) => { b.dataset.left = String(left); b.setAttribute("aria-label", `錄音中，剩 ${left} 秒，按一下送出`); },
        onBusy: () => { delete b.dataset.left; b.classList.add("busy"); b.setAttribute("aria-label", "辨識中"); },
        onEnd: reset,
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
  // Number words 0–20 count the same as digits (recognition often writes "3" for "three").
  const NUMS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty"];
  const normWord = (w) => {
    const x = w.toLowerCase().replace(/[’']/g, "").replace(PUNCT, "");
    const n = NUMS.indexOf(x);
    return n >= 0 ? String(n) : x;
  };
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
  // A period after these (or after a single capital letter, as in "J. Smith") does not end a sentence.
  const ABBR = /(?:^|[\s("“'‘])(?:[Mm]rs?|[Mm]s|[Dd]r|[Ss]t|[Pp]rof|[Mm]t|[Jj]r|[Nn]o|vs|etc|e\.g|i\.e|U\.S|[ap]\.m|[A-Z])\.$/u;
  function splitEnglish(t) {
    const out = [];
    const re = /[.!?…]+["'”’)\]]*\s+(?=\S)/gu;
    let start = 0, m;
    while ((m = re.exec(t))) {
      const mark = m[0].trimEnd();
      const piece = t.slice(start, m.index + mark.length);
      if (mark === "." && ABBR.test(piece)) continue;
      out.push(piece);
      start = m.index + m[0].length;
    }
    out.push(t.slice(start));
    return out;
  }
  /** Sentences of a paragraph. */
  function sentences(text, lang) {
    const t = String(text ?? "").trim();
    const parts = lang === "ja" ? t.split(/(?<=[。！？!?])/u) : splitEnglish(t);
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
    const last = () => i === list.length - 1;
    const goTo = (k) => { i = k; stopListening(); stop(); draw(); };
    const closePanel = () => { stopListening(); stop(); box.remove(); };
    const draw = () => {
      box.innerHTML = "";
      const s = list[i];
      const head = el("div", "sh-head");
      head.append(el("span", "n", `${i + 1} / ${list.length}`));
      const close = iconButton("x", "關閉跟讀", closePanel);
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
        const good = r.score >= 80;
        const pctEl = el("b", `sh-pct${good ? " good" : ""}`, `${r.score}%`);
        if (good) { const ck = el("span", "sh-ok"); ck.append(icon("check")); pctEl.prepend(ck); }
        out.append(pctEl, diff);
        // Next step: 下一句 is the main button once it is good; the last
        // sentence offers the whole paragraph again or closing.
        const acts = el("div", "sh-acts");
        if (!last()) {
          if (good) acts.append(primaryButton("下一句", () => goTo(i + 1), true));
        } else {
          const again = ghostButton("speaker", "再聽整段", () => speak(text, lang));
          if (good) { again.className = "primary small"; again.textContent = "再聽整段"; }
          acts.append(again, ghostButton("x", "關閉", closePanel));
        }
        if (acts.childElementCount) out.append(acts);
      };
      row.append(listenBtn, say);
      if (list.length > 1) {
        const prev = iconButton("back", "上一句", () => goTo(i - 1));
        const next = iconButton("arrow", "下一句", () => goTo(i + 1));
        prev.disabled = i === 0;
        next.disabled = last();
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
    b.dataset.lang = lang;
    b.hidden = !canListen(lang);
    b.onclick = (e) => {
      e.stopPropagation();
      const open = anchor.nextElementSibling?.classList.contains("shadow") ? anchor.nextElementSibling : null;
      if (open) { open.remove(); return; }
      anchor.after(shadowPanel(text, lang));
    };
    return b;
  }

  loadVoices();
  loadLocal();
  return {
    available: () => !!synth, hasVoices, canSpeak, canListen, hasRecognition, voicesFor, voiceFor, prefs, setPref,
    speak, stop, speakerButton, micButton, shadowButton, listen, stopListening, compare, sentences, plain,
    refreshButtons, onVoices, SAMPLE, loadVoices, loadLocal, hasLocal, usesLocal, local: () => local,
    quality, TIER, lacksGoodVoice, wav, downsample, MAX_SECONDS,
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
        const tier = Speech.TIER[Speech.quality(v)];
        const named = /premium|enhanced/i.test(v.name); // Chrome already writes "(Premium)"
        const o = el("option", null, `${v.name}${tier && !named ? `・${tier}` : ""}${v.localService ? "" : "・雲端"}`);
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
    // Better voices are a free download on the Mac; say so only while a language lacks one.
    const langsShown = langs.filter((l) => Speech.voicesFor(l).length);
    if (/Mac/.test(navigator.userAgent) && langsShown.some(Speech.lacksGoodVoice)) {
      const tip = el("div", "voice-tip");
      tip.append(
        el("div", "note", "想要更自然的聲音：在「系統設定 → 輔助使用 → 朗讀內容 → 系統聲音 → 管理聲音」免費下載 Premium 聲音（例如英文 Ava、Zoe，日文 Kyoko 加強版），下載後重新整理這一頁。"),
        ghostButton("external", "打開系統設定", async () => { try { await api("/api/open/voices", {}); } catch (e) { flash(e.message, true); } }),
      );
      form.append(tip);
    }
  }
  box.append(form, recognitionSettings());
}

/** Recognition settings: on this computer when the server offers it, else the browser's (opt-in). */
function recognitionSettings() {
  const recBox = el("div", "git-box");
  const recForm = el("div", "form");
  recBox.append(recForm);
  const local = Speech.local();
  const langs = state.status?.user?.languages?.length ? state.status.user.languages : ["en", "ja"];
  if (local.engine && langs.some(Speech.hasLocal)) {
    const rl = el("label", "check");
    const rc = el("input"); rc.type = "checkbox"; rc.checked = Speech.prefs().localRecognition !== false;
    rc.onchange = () => { Speech.setPref("localRecognition", rc.checked); Speech.refreshButtons(); };
    rl.append(rc, el("span", null, "語音辨識（對話的麥克風、課文跟讀）"));
    recForm.append(rl, el("div", "note", "在這台電腦上辨識，錄音不會離開電腦。每次最多 15 秒，按一下麥克風就送出。"));
    for (const lang of langs.filter(Speech.hasLocal)) {
      const row = el("div", "speech-model");
      row.append(el("span", null, `${langLabel(lang)}辨識模型`));
      if (local.langs[lang].installed) {
        const ok = el("span", "ok");
        ok.append(icon("check"), document.createTextNode("已下載"));
        row.append(ok);
      } else {
        const dl = ghostButton("download", "下載（第一次約 2 分鐘）", async () => {
          dl.disabled = true;
          dl.lastChild.textContent = "下載中…";
          try {
            await api("/api/speech/install", { lang });
            await Speech.loadLocal();
            flash(`${langLabel(lang)}辨識模型下載好了`);
          } catch (e) {
            flash(e.message, true);
            dl.disabled = false;
            dl.lastChild.textContent = "再試一次";
            return;
          }
          recBox.replaceWith(recognitionSettings());
        });
        row.append(dl);
      }
      recForm.append(row);
    }
    return recBox;
  }
  // No on-device engine: the browser's service, off by default; the audio leaves the computer.
  const rl = el("label", "check");
  const rc = el("input"); rc.type = "checkbox"; rc.checked = Speech.prefs().recognition === true && Speech.hasRecognition(); rc.disabled = !Speech.hasRecognition();
  rc.onchange = () => { Speech.setPref("recognition", rc.checked); Speech.refreshButtons(); };
  rl.append(rc, el("span", null, "語音辨識（對話的麥克風、課文跟讀）"));
  recForm.append(rl, el("div", "note", Speech.hasRecognition()
    ? "開啟後，錄音會送到瀏覽器廠商（Google、Microsoft 或 Apple）的雲端服務辨識；不開也可以全部用打字。"
    : "這個瀏覽器不支援語音辨識，可以用打字。"));
  return recBox;
}
// Voices arrive late in some browsers: redraw the tab when they do.
Speech.onVoices.add(() => { const b = $("#setSpeech"); if (b && !b.closest(".step").hidden) renderSpeechSettings(b); });
