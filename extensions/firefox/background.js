"use strict";

const DEFAULTS = { apiPort: 18500, takeover: false };
// Per-tab media list. Memory only: found URLs are NEVER written to storage, and a tab's
// list is wiped when that tab navigates (new URL, reload, SPA pushState) or is closed.
const MAX_MEDIA_PER_TAB = 200;
const mediaByTab = new Map();   // tabId -> [url, ...] (deduplicated, at most MAX_MEDIA_PER_TAB)
const pageByTab = new Map();    // tabId -> page URL without #hash, to detect real page changes

// ---------- Pro Internet Manager: theme sync with the app (/ping "theme" field) ----------
const THEME_KEYS = ["lime", "midnight", "paper", "compact", "graphite", "azeroth"];
const DEFAULT_THEME = {
  name: "Pro Lime (default)", key: "lime", icon: "lime",
  accent: "#76b900", accent_text: "#0a0e06", primary: "#76b900", primary_text: "#0a0e06",
  bg: "#090c09", card: "#111511", border: "#303830", text: "#f2f5f0", dim: "#969e94",
  light: false, serif: false
};
let currentTheme = DEFAULT_THEME;
let lastIconKey = null;

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
    try { await browser.browserAction.setIcon({ path: iconSet(merged.icon) }); } catch (_) {}
  }
  try { await browser.storage.local.set({ pimTheme: merged }); } catch (_) {}
  return merged;
}

// One /ping: returns { ok, theme }. Old app versions answer {"ok":true} without a theme.
async function pingApp() {
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

browser.storage.local.get({ pimTheme: null }).then(stored => {
  if (stored.pimTheme) applyTheme(stored.pimTheme);
});
// 0.3.3: ping every 15 s; while the app does not answer, retry every 3 s for the first minute
// (then every 10 s), so the app shows "Bridge connected" within seconds after it starts.
let pingTimer = null;
let pingFailures = 0;
async function pingLoop() {
  clearTimeout(pingTimer);
  const result = await pingApp();
  pingFailures = result.ok ? 0 : pingFailures + 1;
  const delay = result.ok ? 15000 : (pingFailures <= 20 ? 3000 : 10000);
  pingTimer = setTimeout(pingLoop, delay);
}
pingLoop();

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

function pageKey(url) {
  return typeof url === "string" ? url.split("#")[0] : "";
}

function clearBadge(tabId) {
  try {
    const pending = browser.browserAction.setBadgeText({ text: "", tabId });
    if (pending && pending.catch) pending.catch(() => {});
  } catch (_) {}
}

// Forget everything found in this tab. `tellPage` also empties the in-page button/list.
function clearTab(tabId, tellPage) {
  mediaByTab.delete(tabId);
  clearBadge(tabId);
  if (tellPage) browser.tabs.sendMessage(tabId, { type: "mediaReset" }).catch(() => {});
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
  browser.tabs.sendMessage(tabId, { type: "mediaDetected", urls: list.slice() }).catch(() => {});
}

browser.webRequest.onBeforeRequest.addListener(
  details => {
    if (details.tabId >= 0 && isMediaUrl(details.url)) remember(details.tabId, details.url);
  },
  { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other"] }
);

browser.tabs.onRemoved.addListener(tabId => {
  mediaByTab.delete(tabId);
  pageByTab.delete(tabId);
});

browser.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
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

// Found URLs were never persisted, but wipe any leftover key from older builds on start-up.
try { browser.storage.local.remove(["pimMediaByTab"]).catch(() => {}); } catch (_) {}

async function getSettings() {
  const stored = await browser.storage.local.get(DEFAULTS);
  return {
    apiPort: Number(stored.apiPort) || DEFAULTS.apiPort,
    takeover: Boolean(stored.takeover)
  };
}

async function cookieHeader(url, tab) {
  try {
    const query = { url };
    if (tab && tab.cookieStoreId) query.storeId = tab.cookieStoreId;
    const cookies = await browser.cookies.getAll(query);
    const usable = cookies
      .filter(c => c && c.name && c.value !== undefined && !/[\r\n]/.test(c.value))
      .map(c => c.name + "=" + c.value);
    return usable.join("; ");
  } catch (error) {
    console.warn("Cookie read failed", error);
    return "";
  }
}

async function headersFor(url, tab) {
  const headers = {};
  const cookie = await cookieHeader(url, tab);
  if (cookie) headers.Cookie = cookie;
  if (tab && tab.url) headers.Referer = tab.url;
  headers["User-Agent"] = navigator.userAgent;
  return headers;
}

async function notify(title, message) {
  try {
    await browser.notifications.create({
      type: "basic",
      iconUrl: browser.runtime.getURL("icons/128.png"),
      title,
      message: String(message).slice(0, 240)
    });
  } catch (error) {
    console.warn("Notification failed", error);
  }
}

async function send(url, tab, quality) {
  if (!/^https?:\/\//i.test(url)) {
    throw new Error("Only http(s) URLs are supported");
  }
  const settings = await getSettings();
  const headers = await headersFor(url, tab);
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

async function tabFor(info) {
  if (!info || info.tabId === undefined || info.tabId < 0) return null;
  try {
    return await browser.tabs.get(info.tabId);
  } catch (_) {
    return null;
  }
}

browser.runtime.onInstalled.addListener(() => {
  browser.contextMenus.removeAll(() => {
    browser.contextMenus.create({
      id: "rdm-link",
      title: "Download with Pro Internet Manager",
      contexts: ["link", "video", "audio", "image"]
    });
    browser.contextMenus.create({
      id: "rdm-page",
      title: "Send page to Pro Internet Manager",
      contexts: ["page"]
    });
  });
});

browser.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl;
  if (!url) return;
  try {
    await send(url, tab || await tabFor(info));
  } catch (error) {
    await notify("Pro Internet Manager", error.message);
  }
});

browser.downloads.onCreated.addListener(async (item) => {
  try {
    const settings = await getSettings();
    if (!settings.takeover) return;
    if (item.byExtensionId) return;
    const tab = item.tabId >= 0 ? await browser.tabs.get(item.tabId) : null;
    await send(item.url, tab);
    await browser.downloads.cancel(item.id);
    await browser.downloads.erase({ id: item.id });
  } catch (error) {
    console.warn("Download takeover failed", error);
  }
});

browser.runtime.onMessage.addListener(async (message, sender) => {
  if (!message || typeof message.type !== "string") return { ok: false };
  if (message.type === "getMedia") {
    const tabId = sender && sender.tab ? sender.tab.id : -1;
    return { ok: true, urls: (mediaByTab.get(tabId) || []).slice() };
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
});
