"use strict";
// Pro Internet Manager Bridge: apply the app theme (from /ping, cached by background.js) as CSS variables.
(() => {
  const KEYS = ["lime", "midnight", "paper", "compact", "graphite", "azeroth"];
  function apply(t) {
    if (!t) return;
    const r = document.documentElement.style;
    const set = (name, value) => { if (value) r.setProperty(name, value); };
    set("--bg", t.bg);
    set("--card", t.card);
    set("--line", t.border);
    set("--lime", t.accent);
    set("--accent-text", t.accent_text);
    set("--primary", t.primary);
    set("--primary-text", t.primary_text);
    set("--text", t.text);
    set("--dim", t.dim);
    r.setProperty("--font", t.serif ? 'Georgia, "Palatino Linotype", serif' : '"Segoe UI", system-ui, sans-serif');
    const key = KEYS.includes(t.icon) ? t.icon : "lime";
    document.querySelectorAll("img[data-pim-icon]").forEach(img => { img.src = "icons/" + key + "/48.png"; });
    const name = document.getElementById("themeName");
    if (name && t.name) name.textContent = t.name;
  }
  chrome.storage.local.get({ pimTheme: null }).then(s => apply(s.pimTheme));
  chrome.storage.onChanged.addListener(changes => { if (changes.pimTheme) apply(changes.pimTheme.newValue); });
  window.pimApplyTheme = apply;
})();
