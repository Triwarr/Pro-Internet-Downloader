"use strict";
// Chromium (MV3) build of the Pro Internet Manager Bridge: uses chrome.* promise APIs.

// Pro Internet Manager Bridge - in-page button.
// Compact animated pill (themed globe + X). Click the globe to slide out the full list:
// "Download all", then numbered sources with type and quality (e.g. "MP4 · 1080p Full HD").
// Detection and the messages to background.js (send / getMedia) are unchanged.

(() => {
  if (window.top !== window) return;

  const HOST_ID = "rdm-floating-host";
  const YT_QUALITIES = [2160, 1440, 1080, 720, 480, 360, 240, 144];
  // Common YouTube itags (googlevideo URLs) -> container / quality.
  const ITAGS = {
    18: ["MP4", "360p"], 22: ["MP4", "720p HD"],
    133: ["MP4", "240p"], 134: ["MP4", "360p"], 135: ["MP4", "480p"], 136: ["MP4", "720p HD"], 137: ["MP4", "1080p Full HD"],
    160: ["MP4", "144p"], 264: ["MP4", "1440p 2K"], 266: ["MP4", "2160p 4K"], 298: ["MP4", "720p HD 60 fps"], 299: ["MP4", "1080p Full HD 60 fps"],
    242: ["WEBM", "240p"], 243: ["WEBM", "360p"], 244: ["WEBM", "480p"], 247: ["WEBM", "720p HD"], 248: ["WEBM", "1080p Full HD"],
    271: ["WEBM", "1440p 2K"], 313: ["WEBM", "2160p 4K"], 302: ["WEBM", "720p HD 60 fps"], 303: ["WEBM", "1080p Full HD 60 fps"],
    308: ["WEBM", "1440p 2K 60 fps"], 315: ["WEBM", "2160p 4K 60 fps"],
    139: ["M4A", "audio 48 kbps"], 140: ["M4A", "audio 128 kbps"], 141: ["M4A", "audio 256 kbps"],
    249: ["WEBM", "audio Opus 50 kbps"], 250: ["WEBM", "audio Opus 70 kbps"], 251: ["WEBM", "audio Opus 160 kbps"]
  };
  const LANGS = { en: "EN", nl: "NL", de: "DE", fr: "FR", es: "ES", it: "IT", pt: "PT", ja: "JA", ko: "KO", zh: "ZH", ru: "RU" };

  let sources = [];
  let hidden = false;
  let expanded = false;
  let host = null;
  let shadow = null;
  let panel = null;
  let statusEl = null;
  let busy = false;
  let dragging = false;
  let moved = false;
  let dragStart = null;
  let dragOrigin = null;
  let theme = null;
  // The list belongs to ONE page: it is wiped as soon as the page URL changes (normal
  // navigation, reload, or SPA history.pushState such as YouTube). Nothing is stored.
  const MAX_SOURCES = 28;
  let pageKey = currentPageKey();

  function currentPageKey() {
    return String(location.href).split("#")[0];
  }

  function isHttp(url) {
    return /^https?:\/\//i.test(url || "");
  }

  function isYouTubePage(url) {
    try {
      const parsed = new URL(url);
      if (/(^|\.)youtube\.com$/i.test(parsed.hostname)) {
        return /\/(watch|shorts|embed|live)\b/i.test(parsed.pathname);
      }
      return /(^|\.)youtu\.be$/i.test(parsed.hostname);
    } catch (_) {
      return false;
    }
  }

  function heightLabel(h) {
    h = Number(h);
    if (!h) return "";
    if (h >= 2160) return h + "p 4K";
    if (h >= 1440) return h + "p 2K";
    if (h >= 1080) return h + "p Full HD";
    if (h >= 720) return h + "p HD";
    return h + "p";
  }

  function qualityFromText(text) {
    const m = /(?:^|[^0-9])(2160|1440|1080|720|576|480|360|240|144)p(?:[^0-9]|$)/i.exec(text || "");
    if (m) return heightLabel(m[1]);
    if (/(^|[^a-z])4k([^a-z]|$)/i.test(text || "")) return "2160p 4K";
    return "";
  }

  const EXT_TYPES = {
    mp4: "MP4", m4v: "MP4", mkv: "MKV", webm: "WEBM", mov: "MOV", flv: "FLV", avi: "AVI", ts: "TS", m4s: "M4S",
    mp3: "MP3", m4a: "M4A", aac: "AAC", flac: "FLAC", wav: "WAV", ogg: "OGG", opus: "OPUS",
    vtt: "VTT", srt: "SRT"
  };

  // { kind, quality, detail } for a URL; `hint` can carry videoHeight, track language, mime.
  function describe(url, hint) {
    hint = hint || {};
    try {
      const parsed = new URL(url);
      const host = parsed.hostname.replace(/^www\./, "");
      if (/googlevideo\.com$/i.test(parsed.hostname)) {
        const mime = parsed.searchParams.get("mime") || "";
        const itag = Number(parsed.searchParams.get("itag") || 0);
        const known = ITAGS[itag];
        const sub = (mime.split(";")[0].split("/")[1] || "").toUpperCase();
        const kind = known ? known[0] : (sub || (mime.startsWith("audio") ? "audio" : "video"));
        const quality = known ? known[1] : (mime.startsWith("audio") ? "audio" : heightLabel(hint.height) || "video");
        return { kind: "YouTube · " + kind, qualityText: quality, detail: "separate " + (mime.startsWith("audio") ? "audio" : "video") + " stream" + (itag ? " · itag " + itag : "") };
      }
      const path = parsed.pathname.toLowerCase();
      const name = decodeURIComponent(parsed.pathname.split("/").pop() || "media");
      if (path.endsWith(".m3u8")) return { kind: "Stream (HLS)", qualityText: qualityFromText(url) || heightLabel(hint.height) || "adaptive", detail: "M3U8 · " + host };
      if (path.endsWith(".mpd")) return { kind: "Stream (DASH)", qualityText: qualityFromText(url) || heightLabel(hint.height) || "adaptive", detail: "MPD · " + host };
      const ext = (path.split(".").pop() || "").replace(/[^a-z0-9]/g, "");
      let kind = EXT_TYPES[ext] || "";
      if (!kind && hint.mime) kind = (hint.mime.split(";")[0].split("/")[1] || "").toUpperCase();
      if (!kind) kind = hint.audio ? "audio" : "video";
      let quality;
      if (kind === "VTT" || kind === "SRT") {
        const lang = (hint.lang || "").toLowerCase().split("-")[0];
        quality = (LANGS[lang] || (lang ? lang.toUpperCase() : "")) + (hint.track === "captions" ? " captions" : " subtitles");
        quality = quality.trim();
      } else if (["MP3", "M4A", "AAC", "FLAC", "WAV", "OGG", "OPUS"].includes(kind) || hint.audio) {
        quality = "audio";
      } else {
        quality = heightLabel(hint.height) || qualityFromText(name) || "quality unknown";
      }
      return { kind, qualityText: quality, detail: name + " · " + host };
    } catch (_) {
      return { kind: "video", qualityText: "", detail: String(url).slice(0, 60) };
    }
  }

  function labelFor(url, hint) {
    const d = describe(url, hint);
    return d.kind + (d.qualityText ? " · " + d.qualityText : "") + " · " + d.detail;
  }

  function priority(url) {
    if (/googlevideo\.com\/videoplayback/i.test(url)) return 40;
    const path = url.split(/[?#]/)[0].toLowerCase();
    if (path.endsWith(".m3u8") || path.endsWith(".mpd")) return 20;
    if (/\.(mp4|webm|mkv|m4v|mov|flv)$/.test(path)) return 30;
    if (/\.(vtt|srt)$/.test(path)) return 45;
    return 50;
  }

  function addSource(source, hint) {
    if (!source || !isHttp(source.url)) return false;
    if (/googlevideo\.com\/videoplayback/i.test(source.url) && isYouTubePage(location.href)) return false;
    if (source.quality !== null && source.quality !== undefined) {
      source.quality = Number(source.quality);
      if (!Number.isFinite(source.quality)) source.quality = null;
    } else {
      source.quality = null;
    }
    if (!source.kind) Object.assign(source, describe(source.url, hint));
    if (!source.label) source.label = source.kind + (source.qualityText ? " · " + source.qualityText : "");
    const key = source.url + "|" + (source.quality || "");
    source.key = key;
    if (sources.some(item => item.key === key)) return false;
    if (!source.order && source.order !== 0) source.order = priority(source.url);
    sources.push(source);
    sources.sort((a, b) => a.order - b.order || String(a.kind).localeCompare(String(b.kind)));
    if (sources.length > MAX_SOURCES) sources.length = MAX_SOURCES;
    return true;
  }

  function addYouTubeSources() {
    if (!isYouTubePage(location.href)) return false;
    const title = (document.title || "").replace(/\s*-\s*YouTube\s*$/i, "");
    let changed = addSource({
      url: location.href, quality: null, order: 0,
      kind: "YouTube · MP4", qualityText: "best quality", detail: (title || "video") + " · video + audio via yt-dlp"
    });
    YT_QUALITIES.forEach((height, index) => {
      if (addSource({
        url: location.href, quality: height, order: index + 1,
        kind: "YouTube · MP4", qualityText: "up to " + heightLabel(height), detail: (title || "video") + " · video + audio via yt-dlp"
      })) changed = true;
    });
    return changed;
  }

  // ---------- theme ----------
  const FALLBACK = { accent: "#76b900", accent_text: "#0a0e06", primary: "#76b900", primary_text: "#0a0e06",
    bg: "#090c09", card: "#111511", border: "#303830", text: "#f2f5f0", dim: "#969e94", icon: "lime", serif: false, light: false };

  function applyThemeVars() {
    if (!host) return;
    const t = Object.assign({}, FALLBACK, theme || {});
    const s = host.style;
    s.setProperty("--pim-accent", t.accent);
    s.setProperty("--pim-accent-text", t.accent_text);
    s.setProperty("--pim-primary", t.primary);
    s.setProperty("--pim-primary-text", t.primary_text);
    s.setProperty("--pim-bg", t.bg);
    s.setProperty("--pim-card", t.card);
    s.setProperty("--pim-border", t.border);
    s.setProperty("--pim-text", t.text);
    s.setProperty("--pim-dim", t.dim);
    s.setProperty("--pim-font", t.serif ? 'Georgia, "Palatino Linotype", serif' : '"Segoe UI", system-ui, sans-serif');
    const img = shadow && shadow.querySelector(".globe img");
    if (img) img.src = chrome.runtime.getURL("icons/" + (t.icon || "lime") + "/48.png");
  }

  async function loadTheme() {
    try {
      const response = await chrome.runtime.sendMessage({ type: "getTheme" });
      if (response && response.theme) theme = response.theme;
    } catch (_) {}
    applyThemeVars();
  }

  const CSS = `
    :host { all: initial; }
    * { box-sizing: border-box; }
    .wrap {
      font: 12px/1.25 var(--pim-font);
      color: var(--pim-text);
      user-select: none;
      display: flex; flex-direction: column; align-items: flex-end;
    }
    .pill {
      display: flex; align-items: center; gap: 2px;
      padding: 3px;
      background: color-mix(in srgb, var(--pim-bg) 92%, transparent);
      border: 1px solid color-mix(in srgb, var(--pim-accent) 55%, transparent);
      border-radius: 999px;
      box-shadow: 0 6px 22px rgba(0,0,0,.45), 0 0 0 0 var(--pim-accent);
      cursor: grab;
      animation: pim-in .35s cubic-bezier(.2,.9,.3,1.2) both, pim-glow 2.8s ease-in-out 0.4s infinite;
    }
    .pill:active { cursor: grabbing; }
    .globe {
      width: 34px; height: 34px; border-radius: 50%; border: 0; padding: 0;
      background: radial-gradient(circle at 50% 45%, color-mix(in srgb, var(--pim-accent) 28%, transparent), transparent 70%);
      display: grid; place-items: center; cursor: pointer;
    }
    .globe img { width: 28px; height: 28px; border-radius: 8px; animation: pim-pulse 2.8s ease-in-out infinite; }
    .globe:hover img { animation: pim-spin 1.2s ease-in-out; }
    .count {
      min-width: 18px; height: 18px; padding: 0 5px; border-radius: 9px;
      background: var(--pim-accent); color: var(--pim-accent-text);
      font: 700 10px/18px var(--pim-font); text-align: center; margin-right: 2px;
    }
    .x {
      width: 22px; height: 22px; border-radius: 50%; border: 0; padding: 0;
      background: transparent; color: var(--pim-dim); cursor: pointer;
      font: 15px/22px var(--pim-font); text-align: center;
    }
    .x:hover { background: color-mix(in srgb, var(--pim-text) 12%, transparent); color: var(--pim-text); }
    .panel {
      margin-top: 6px; width: 380px; max-height: 0; opacity: 0; overflow: hidden;
      transform: translateY(-6px) scale(.98); transform-origin: top right;
      background: var(--pim-card);
      border: 1px solid color-mix(in srgb, var(--pim-accent) 45%, var(--pim-border));
      border-radius: 12px;
      box-shadow: 0 14px 38px rgba(0,0,0,.5);
      transition: max-height .32s ease, opacity .22s ease, transform .32s cubic-bezier(.2,.9,.3,1.1);
    }
    .open .panel { max-height: 460px; opacity: 1; transform: none; }
    .head { display: flex; align-items: center; gap: 8px; padding: 9px 10px 7px; border-bottom: 1px solid var(--pim-border); }
    .head b { color: var(--pim-text); font-size: 13px; }
    .head span { color: var(--pim-accent); font-size: 10.5px; font-weight: 600; }
    .head .status { margin-left: auto; color: var(--pim-dim); font-weight: 400; max-width: 170px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .error .head .status { color: #ff8b7b; }
    .list { max-height: 380px; overflow: auto; padding: 5px; }
    .row {
      display: flex; align-items: center; gap: 9px; width: 100%;
      padding: 6px 8px; border: 1px solid transparent; border-radius: 8px;
      background: transparent; color: var(--pim-text); cursor: pointer; text-align: left;
      font: inherit; opacity: 0; animation: pim-row .28s ease forwards;
    }
    .row:hover { background: color-mix(in srgb, var(--pim-accent) 14%, transparent); border-color: color-mix(in srgb, var(--pim-accent) 55%, transparent); }
    .row:disabled { opacity: .5; cursor: default; }
    .num { width: 20px; color: var(--pim-dim); font-size: 11px; text-align: right; flex: none; }
    .badge {
      flex: none; min-width: 40px; padding: 2px 6px; border-radius: 6px; text-align: center;
      font: 700 9.5px/14px var(--pim-font); letter-spacing: .03em;
      color: var(--pim-accent); border: 1px solid color-mix(in srgb, var(--pim-accent) 60%, transparent);
    }
    .txt { display: grid; gap: 1px; min-width: 0; }
    .txt .main { font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .txt .sub { font-size: 10.5px; color: var(--pim-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .all { background: var(--pim-primary); color: var(--pim-primary-text); margin-bottom: 4px; }
    .all:hover { background: var(--pim-primary); filter: brightness(1.1); }
    .all .num, .all .txt .sub { color: inherit; opacity: .8; }
    .all .badge { color: inherit; border-color: currentColor; }
    @keyframes pim-in { from { opacity: 0; transform: translateX(14px) scale(.9); } to { opacity: 1; transform: none; } }
    @keyframes pim-glow {
      0%, 100% { box-shadow: 0 6px 22px rgba(0,0,0,.45), 0 0 0 0 color-mix(in srgb, var(--pim-accent) 0%, transparent); }
      50% { box-shadow: 0 6px 22px rgba(0,0,0,.45), 0 0 0 4px color-mix(in srgb, var(--pim-accent) 22%, transparent); }
    }
    @keyframes pim-pulse { 0%, 100% { transform: scale(1); } 50% { transform: scale(1.07); } }
    @keyframes pim-spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
    @keyframes pim-row { from { opacity: 0; transform: translateX(8px); } to { opacity: 1; transform: none; } }
    @media (prefers-reduced-motion: reduce) { .pill, .globe img, .row { animation: none; opacity: 1; } .panel { transition: none; } }
  `;

  function ensureHost() {
    if (host && host.isConnected) return;
    if (host) {
      document.documentElement.appendChild(host);
      applyThemeVars();
      return;
    }
    host = document.createElement("div");
    host.id = HOST_ID;
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.top = "76px";
    host.style.right = "18px";
    host.style.zIndex = "2147483647";
    shadow = host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = CSS;
    panel = document.createElement("div");
    panel.className = "wrap";
    panel.innerHTML = `
      <div class="pill" title="Pro Internet Manager - drag to move">
        <button class="globe" title="Show sources"><img alt=""></button>
        <span class="count">0</span>
        <button class="x" title="Hide for this page">×</button>
      </div>
      <div class="panel">
        <div class="head"><b>Pro Internet Manager</b><span>by Triwar Design</span><span class="status"></span></div>
        <div class="list"></div>
      </div>
    `;
    shadow.appendChild(style);
    shadow.appendChild(panel);
    document.documentElement.appendChild(host);
    statusEl = panel.querySelector(".status");
    applyThemeVars();
    loadTheme();

    panel.querySelector(".globe").addEventListener("click", () => {
      if (moved) return;
      expanded = !expanded;
      panel.classList.toggle("open", expanded);
      if (expanded) { loadTheme(); refreshSources(); }
    });
    panel.querySelector(".x").addEventListener("click", () => {
      hidden = true;
      host.remove();
    });

    const pill = panel.querySelector(".pill");
    pill.addEventListener("mousedown", event => {
      if (event.target.closest(".x")) return;
      const rect = host.getBoundingClientRect();
      dragging = true;
      moved = false;
      dragStart = { x: event.clientX, y: event.clientY };
      dragOrigin = { x: rect.left, y: rect.top, w: rect.width };
      event.preventDefault();
    });
    window.addEventListener("mousemove", event => {
      if (!dragging) return;
      const dx = event.clientX - dragStart.x;
      const dy = event.clientY - dragStart.y;
      if (Math.abs(dx) + Math.abs(dy) > 4) moved = true;
      if (!moved) return;
      const x = Math.max(4, Math.min(window.innerWidth - 90, dragOrigin.x + dx));
      const y = Math.max(4, Math.min(window.innerHeight - 44, dragOrigin.y + dy));
      host.style.right = Math.max(4, window.innerWidth - x - dragOrigin.w) + "px";
      host.style.top = y + "px";
    });
    window.addEventListener("mouseup", () => {
      dragging = false;
      setTimeout(() => { moved = false; }, 0);
    });
  }

  function setStatus(text, error) {
    if (!statusEl) return;
    statusEl.textContent = text;
    statusEl.title = text;
    panel.classList.toggle("error", Boolean(error));
  }

  function uniqueSources() {
    const seen = new Set();
    return sources.filter(source => {
      if (seen.has(source.url)) return false;
      seen.add(source.url);
      return true;
    });
  }

  function makeRow(num, badge, main, sub, extraClass, delay, onClick) {
    const row = document.createElement("button");
    row.className = "row" + (extraClass ? " " + extraClass : "");
    row.style.animationDelay = Math.min(delay, 12) * 28 + "ms";
    const n = document.createElement("span"); n.className = "num"; n.textContent = num;
    const b = document.createElement("span"); b.className = "badge"; b.textContent = badge;
    const t = document.createElement("span"); t.className = "txt";
    const m = document.createElement("span"); m.className = "main"; m.textContent = main;
    const s = document.createElement("span"); s.className = "sub"; s.textContent = sub;
    t.appendChild(m); t.appendChild(s);
    row.appendChild(n); row.appendChild(b); row.appendChild(t);
    row.title = main + (sub ? "\n" + sub : "");
    row.disabled = busy;
    row.addEventListener("click", onClick);
    return row;
  }

  function badgeFor(source) {
    const kind = String(source.kind || "video").replace(/^YouTube · /, "");
    if (/^Stream \((HLS|DASH)\)$/.test(kind)) return RegExp.$1;
    return kind.toUpperCase().slice(0, 5);
  }

  function refreshSources() {
    if (!panel) return;
    panel.querySelector(".count").textContent = String(sources.length);
    if (!expanded) return;
    const list = panel.querySelector(".list");
    list.innerHTML = "";
    const unique = uniqueSources().length;
    list.appendChild(makeRow("", "ALL", "Download all (" + unique + ")", unique === sources.length ? "every source below" : "one per URL, best entry each", "all", 0, sendAll));
    sources.forEach((source, index) => {
      const quality = source.qualityText || "";
      const main = source.kind + (quality ? " · " + quality : "");
      list.appendChild(makeRow((index + 1) + ".", badgeFor(source), main, source.detail || "", "", index + 1, () => sendSource(source)));
    });
  }

  function show() {
    if (!sources.length || hidden) {
      if (host) host.remove();
      return;
    }
    ensureHost();
    refreshSources();
    if (!busy) setStatus(sources.length + (sources.length === 1 ? " source" : " sources"), false);
  }

  function setBusy(on) {
    busy = on;
    if (panel) panel.querySelectorAll(".row").forEach(row => { row.disabled = on; });
  }

  async function sendOne(source) {
    const response = await chrome.runtime.sendMessage({ type: "send", url: source.url, quality: source.quality });
    return response || { ok: false };
  }

  async function sendSource(source) {
    if (!source || busy) return;
    setBusy(true);
    setStatus("Sending…", false);
    try {
      const response = await sendOne(source);
      if (response.ok) setStatus("Added to queue", false);
      else setStatus(response.error || "Download manager offline", true);
    } catch (_) {
      setStatus("Download manager offline", true);
    }
    setBusy(false);
  }

  async function sendAll() {
    const list = uniqueSources();
    if (!list.length || busy) return;
    setBusy(true);
    setStatus("Queuing " + list.length + "…", false);
    let failed = 0;
    for (const source of list) {
      try {
        if (!(await sendOne(source)).ok) failed += 1;
      } catch (_) {
        failed += 1;
      }
    }
    setStatus(failed ? failed + " failed" : "All added to queue", failed > 0);
    setBusy(false);
  }

  // Empty the list, the counter and the slide-out panel. `tellBackground` also clears the
  // background's per-tab list (used when this script notices the URL change first).
  function resetSources(tellBackground) {
    sources = [];
    hidden = false;
    expanded = false;
    if (panel) {
      panel.classList.remove("open");
      panel.classList.remove("error");
      const list = panel.querySelector(".list");
      if (list) list.innerHTML = "";
      const count = panel.querySelector(".count");
      if (count) count.textContent = "0";
    }
    if (statusEl) { statusEl.textContent = ""; statusEl.title = ""; }
    if (host) host.remove();
    if (tellBackground) {
      try {
        const pending = chrome.runtime.sendMessage({ type: "resetMedia", page: location.href });
        if (pending && pending.catch) pending.catch(() => {});
      } catch (_) {}
    }
  }

  let rescanTimer = 0;
  function checkPage() {
    const key = currentPageKey();
    if (key === pageKey) return false;
    pageKey = key;
    resetSources(true);
    // SPA pages update the title/player a moment after the URL: rescan shortly after.
    clearTimeout(rescanTimer);
    rescanTimer = setTimeout(scanDom, 800);
    return true;
  }

  function scanDom() {
    checkPage();
    let changed = addYouTubeSources();
    document.querySelectorAll("video, audio").forEach(element => {
      const hint = { height: element.videoHeight || 0, audio: element.tagName === "AUDIO" };
      const src = element.currentSrc || element.src;
      if (isHttp(src) && addSource({ url: src }, hint)) changed = true;
      element.querySelectorAll("source").forEach(source => {
        if (isHttp(source.src) && addSource({ url: source.src }, Object.assign({ mime: source.type || "" }, hint, { height: Number(source.getAttribute("size") || source.getAttribute("res") || 0) || hint.height }))) changed = true;
      });
      element.querySelectorAll("track[src]").forEach(track => {
        if (isHttp(track.src) && addSource({ url: track.src }, { lang: track.srclang || "", track: track.kind || "subtitles" })) changed = true;
      });
    });
    if (changed) show();
  }

  chrome.runtime.onMessage.addListener(message => {
    if (!message) return;
    if (message.type === "mediaReset") {
      pageKey = currentPageKey();
      resetSources(false);
      clearTimeout(rescanTimer);
      rescanTimer = setTimeout(scanDom, 800);
      return;
    }
    if (message.type !== "mediaDetected") return;
    checkPage();
    let changed = false;
    (message.urls || []).forEach(url => {
      if (addSource({ url })) changed = true;
    });
    if (changed) {
      hidden = false;
      show();
    }
  });

  async function init() {
    scanDom();
    try {
      const response = await chrome.runtime.sendMessage({ type: "getMedia" });
      if (response && response.ok) {
        let changed = false;
        (response.urls || []).forEach(url => {
          if (addSource({ url })) changed = true;
        });
        if (changed) show();
      }
    } catch (_) {
      // Background may still be starting; webRequest messages will follow.
    }
    window.setInterval(scanDom, 2500);
    window.setInterval(checkPage, 1000);
    window.addEventListener("popstate", checkPage);
    window.addEventListener("hashchange", checkPage);
    window.addEventListener("pageshow", event => { if (event.persisted) { resetSources(true); pageKey = currentPageKey(); scanDom(); } });
    window.setInterval(() => { if (host && host.isConnected) loadTheme(); }, 20000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init, { once: true });
  } else {
    init();
  }
})();
