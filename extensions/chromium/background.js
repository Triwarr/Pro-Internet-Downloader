"use strict";
// Pro Internet Manager Bridge - Chromium (Manifest V3) service worker.
// Same features as the Firefox build: theme sync via /ping, media detection (observe-only
// webRequest), context menu, optional download takeover and the /add bridge to 127.0.0.1.
// MV3 notes: per-tab media lists live in worker memory only (never in chrome.storage), so
// nothing is remembered after the user leaves a page; the in-page button keeps its own list
// for the current page if the worker sleeps. The periodic ping uses chrome.alarms.

const DEFAULTS = { apiPort: 18500, takeover: false };
const MEDIA_KEY = "pimMediaByTab"; // legacy storage key (0.3.1), only used to wipe old state
const MAX_MEDIA_PER_TAB = 200;

// ---------- theme sync with the app (/ping "theme" field) ----------
const THEME_KEYS = ["lime", "midnight", "paper", "compact", "graphite", "azeroth"];
const DEFAULT_THEME = {
  name: "Pro Lime (default)", key: "lime", icon: "lime",
  accent: "#76b900", accent_text: "#0a0e06", primary: "#76b900", primary_text: "#0a0e06",
  bg: "#090c09", card: "#111511", border: "#303830", text: "#f2f5f0", dim: "#969e94",
  light: false, serif: false
};
let currentTheme = DEFAULT_THEME;
let lastIconKey = null;
let themeLoaded = null;

function iconKey(theme) {
  const key = theme && (theme.icon || theme.key);
  return THEME_KEYS.includes(key) ? key : "lime";
}

function iconSet(key) {
  const out = {};
  ["16", "32", "48", "96"].forEach(size => { out[size] = "icons/" + key + "/" + size + ".png"; });
  return out;
}

async function applyTheme(theme) {
  const merged = Object.assign({}, DEFAULT_THEME, theme || {});
  merged.icon = iconKey(merged);
  currentTheme = merged;
  if (merged.icon !== lastIconKey) {
    lastIconKey = merged.icon;
    try { await chrome.action.setIcon({ path: iconSet(merged.icon) }); } catch (_) {}
  }
  try { await chrome.storage.local.set({ pimTheme: merged }); } catch (_) {}
  return merged;
}

function ensureTheme() {
  if (!themeLoaded) {
    themeLoaded = chrome.storage.local.get({ pimTheme: null })
      .then(stored => { if (stored.pimTheme) return applyTheme(stored.pimTheme); })
      .catch(() => {});
  }
  return themeLoaded;
}

// One /ping: returns { ok, theme }. Old app versions answer {"ok":true} without a theme.
async function pingApp() {
  await ensureTheme();
  const settings = await getSettings();
  try {
    const response = await fetch("http://127.0.0.1:" + settings.apiPort + "/ping", { cache: "no-store" });
    if (!response.ok) return { ok: false, theme: currentTheme };
    let data = {};
    try { data = await response.json(); } catch (_) {}
    const theme = data && data.theme ? await applyTheme(data.theme) : currentTheme;
    return { ok: true, theme };
  } catch (_) {
    return { ok: false, theme: currentTheme };
  }
}

// chrome.alarms replaces setInterval (MV3 workers sleep). Chrome's minimum period is 30 s.
// 0.3.3: after a failed ping, retry every 5 s (up to 6 times) while the worker is awake,
// so the app shows "Bridge connected" within seconds after it starts.
let pingRetryTimer = null;
let pingRetries = 0;
async function pingWithRetry() {
  clearTimeout(pingRetryTimer);
  const result = await pingApp();
  if (result.ok || pingRetries >= 6) {
    pingRetries = 0;
  } else {
    pingRetries += 1;
    pingRetryTimer = setTimeout(pingWithRetry, 5000);
  }
  return result;
}
chrome.alarms.create("pim-ping", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(alarm => { if (alarm.name === "pim-ping") pingWithRetry(); });
chrome.runtime.onStartup.addListener(() => { pingWithRetry(); });
ensureTheme().then(pingWithRetry);

// ---------- media detection ----------
function isMediaUrl(url) {
  if (!/^https?:\/\//i.test(url)) return false;
  if (/googlevideo\.com\/videoplayback/i.test(url)) return true;
  if (/[?&](?:mime|format)=(?:video|audio)/i.test(url)) return true;
  const path = url.split(/[?#]/)[0].toLowerCase();
  return /\.(m3u8|mpd|mp4|webm|mkv|m4v|mov|flv|ts|m4s|mp3|m4a|aac|flac|wav)$/.test(path);
}

function mediaPriority(url) {
  if (/googlevideo\.com\/videoplayback/i.test(url)) return 2;
  const path = url.split(/[?#]/)[0].toLowerCase();
  if (path.endsWith(".m3u8") || path.endsWith(".mpd")) return 0;
  if (/\.(mp4|webm|mkv|m4v|mov|flv)$/.test(path)) return 1;
  return 2;
}

// Per-tab media list. Memory only: found URLs are NEVER written to storage, and a tab's
// list is wiped when that tab navigates (new URL, reload, SPA pushState) or is closed.
const mediaByTab = new Map();   // tabId -> [url, ...] (deduplicated, at most MAX_MEDIA_PER_TAB)
const pageByTab = new Map();    // tabId -> page URL without #hash, to detect real page changes

function pageKey(url) {
  return typeof url === "string" ? url.split("#")[0] : "";
}

function mediaFor(tabId) {
  return (mediaByTab.get(tabId) || []).slice();
}

function clearBadge(tabId) {
  try {
    const pending = chrome.action.setBadgeText({ text: "", tabId });
    if (pending && pending.catch) pending.catch(() => {});
  } catch (_) {}
}

// Forget everything found in this tab. `tellPage` also empties the in-page button/list.
function clearTab(tabId, tellPage) {
  mediaByTab.delete(tabId);
  clearBadge(tabId);
  if (tellPage) {
    try {
      const pending = chrome.tabs.sendMessage(tabId, { type: "mediaReset" });
      if (pending && pending.catch) pending.catch(() => {});
    } catch (_) {}
  }
}

function remember(tabId, url) {
  if (tabId < 0) return;
  let list = mediaByTab.get(tabId);
  if (!list) {
    list = [];
    mediaByTab.set(tabId, list);
  }
  if (list.includes(url)) return;
  list.push(url);
  list.sort((a, b) => mediaPriority(a) - mediaPriority(b));
  if (list.length > MAX_MEDIA_PER_TAB) list.length = MAX_MEDIA_PER_TAB;
  if (!list.includes(url)) return; // dropped by the cap: nothing new to show
  try {
    const pending = chrome.tabs.sendMessage(tabId, { type: "mediaDetected", urls: list.slice() });
    if (pending && pending.catch) pending.catch(() => {});
  } catch (_) {}
}

// Observe-only (non-blocking) webRequest is allowed in MV3.
chrome.webRequest.onBeforeRequest.addListener(
  details => {
    if (details.tabId >= 0 && isMediaUrl(details.url)) remember(details.tabId, details.url);
  },
  { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other"] }
);

chrome.tabs.onRemoved.addListener(tabId => {
  mediaByTab.delete(tabId);
  pageByTab.delete(tabId);
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  const previous = pageByTab.get(tabId);
  const url = changeInfo.url || (tab && tab.url) || "";
  if (changeInfo.url) {
    // New page URL (normal navigation or SPA history.pushState). A #hash-only change is the same page.
    const page = pageKey(changeInfo.url);
    pageByTab.set(tabId, page);
    if (previous !== page) {
      clearTab(tabId, true);
      return;
    }
  }
  if (changeInfo.status === "loading" && !(changeInfo.url && previous === pageKey(changeInfo.url))) {
    // Top-level document load (reload, or navigation that reports no new URL).
    if (url) pageByTab.set(tabId, pageKey(url));
    clearTab(tabId, true);
  }
});

// Wipe per-tab media state that 0.3.1 kept in chrome.storage.session/local.
function wipeStoredMedia() {
  [chrome.storage.session, chrome.storage.local].forEach(area => {
    if (!area) return;
    try {
      const pending = area.remove(MEDIA_KEY);
      if (pending && pending.catch) pending.catch(() => {});
    } catch (_) {}
  });
}
wipeStoredMedia();

// ---------- bridge to the app ----------
async function getSettings() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  return {
    apiPort: Number(stored.apiPort) || DEFAULTS.apiPort,
    takeover: Boolean(stored.takeover)
  };
}

async function cookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    const usable = cookies
      .filter(c => c && c.name && c.value !== undefined && !/[\r\n]/.test(c.value))
      .map(c => c.name + "=" + c.value);
    return usable.join("; ");
  } catch (error) {
    console.warn("Cookie read failed", error);
    return "";
  }
}

async function headersFor(url, tab, referrer) {
  const headers = {};
  const cookie = await cookieHeader(url);
  if (cookie) headers.Cookie = cookie;
  const ref = (tab && tab.url) || referrer;
  if (ref) headers.Referer = ref;
  headers["User-Agent"] = navigator.userAgent;
  return headers;
}

async function notify(title, message) {
  try {
    await chrome.notifications.create({
      type: "basic",
      iconUrl: chrome.runtime.getURL("icons/128.png"),
      title,
      message: String(message).slice(0, 240)
    });
  } catch (error) {
    console.warn("Notification failed", error);
  }
}

async function send(url, tab, quality, referrer) {
  if (!/^https?:\/\//i.test(url)) {
    throw new Error("Only http(s) URLs are supported");
  }
  const settings = await getSettings();
  const headers = await headersFor(url, tab, referrer);
  const payload = { url, headers };
  if (Number.isFinite(Number(quality)) && quality) payload.quality = Number(quality);
  const response = await fetch("http://127.0.0.1:" + settings.apiPort + "/add", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload)
  });
  if (!response.ok) {
    throw new Error("Download Manager answered " + response.status);
  }
  const data = await response.json();
  if (!data.ok) throw new Error(data.error || "Download Manager refused the URL");
  await notify("Sent to Pro Internet Manager", url);
  return true;
}

chrome.runtime.onInstalled.addListener(async () => {
  try { await chrome.contextMenus.removeAll(); } catch (_) {}
  chrome.contextMenus.create({
    id: "rdm-link",
    title: "Download with Pro Internet Manager",
    contexts: ["link", "video", "audio", "image"]
  });
  chrome.contextMenus.create({
    id: "rdm-page",
    title: "Send page to Pro Internet Manager",
    contexts: ["page"]
  });
  pingWithRetry();
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl;
  if (!url) return;
  try {
    await send(url, tab || null);
  } catch (error) {
    await notify("Pro Internet Manager", error.message);
  }
});

// Takeover: Chromium has no download tab id, so the referrer is used as Referer.
// The download is cancelled right after it starts (it may flash in the download bubble).
chrome.downloads.onCreated.addListener(async (item) => {
  try {
    const settings = await getSettings();
    if (!settings.takeover) return;
    if (item.byExtensionId) return;
    const url = item.finalUrl || item.url;
    if (!/^https?:\/\//i.test(url)) return;
    await send(url, null, null, item.referrer);
    try { await chrome.downloads.cancel(item.id); } catch (_) {}
    try { await chrome.downloads.erase({ id: item.id }); } catch (_) {}
  } catch (error) {
    console.warn("Download takeover failed", error);
  }
});

// Chromium does not accept a returned Promise from onMessage: use sendResponse + return true.
async function handleMessage(message, sender) {
  if (!message || typeof message.type !== "string") return { ok: false };
  if (message.type === "getMedia") {
    const tabId = sender && sender.tab ? sender.tab.id : -1;
    return { ok: true, urls: tabId >= 0 ? mediaFor(tabId) : [] };
  }
  if (message.type === "resetMedia") {
    // The page noticed its own URL change (e.g. SPA) before tabs.onUpdated did.
    const tabId = sender && sender.tab ? sender.tab.id : -1;
    if (tabId >= 0) {
      if (typeof message.page === "string") pageByTab.set(tabId, pageKey(message.page));
      clearTab(tabId, false);
    }
    return { ok: true };
  }
  if (message.type === "ping") {
    return await pingApp();
  }
  if (message.type === "getTheme") {
    await ensureTheme();
    return { ok: true, theme: currentTheme };
  }
  if (message.type === "send") {
    try {
      await send(message.url, message.tab || (sender && sender.tab) || null, message.quality);
      return { ok: true };
    } catch (error) {
      return { ok: false, error: error.message };
    }
  }
  return { ok: false, error: "Unknown message" };
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  handleMessage(message, sender).then(sendResponse, error => sendResponse({ ok: false, error: String(error && error.message || error) }));
  return true;
});
