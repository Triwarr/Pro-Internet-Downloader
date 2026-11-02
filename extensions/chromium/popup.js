"use strict";

let activeTab = null;

const $ = id => document.getElementById(id);

function setStatus(text, error) {
  const el = $("status");
  el.textContent = text || "";
  el.className = "status" + (error ? " error" : "");
}

function setConnection(ok) {
  const el = $("connection");
  el.textContent = ok ? "online" : "offline";
  el.className = "pill " + (ok ? "ok" : "bad");
}

function tabInfo() {
  if (!activeTab) return null;
  return { url: activeTab.url };
}

async function send(url) {
  if (!url) return;
  setStatus("Sending…", false);
  const response = await chrome.runtime.sendMessage({ type: "send", url, tab: tabInfo() });
  if (response && response.ok) {
    setStatus("Added to the download queue.", false);
  } else {
    setStatus((response && response.error) || "Could not reach the download manager.", true);
  }
}

function mediaRow(url) {
  const row = document.createElement("div");
  row.className = "mediaRow";
  const text = document.createElement("span");
  text.textContent = url.length > 82 ? url.slice(0, 79) + "…" : url;
  text.title = url;
  const button = document.createElement("button");
  button.className = "small";
  button.textContent = "Send";
  button.addEventListener("click", () => send(url));
  row.appendChild(text);
  row.appendChild(button);
  return row;
}

async function scan() {
  if (!activeTab) return;
  setStatus("Scanning page…", false);
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId: activeTab.id },
      func: () => {
        const out = new Set();
        document.querySelectorAll("video, audio, source").forEach(el => {
          const src = el.currentSrc || el.src;
          if (src) out.add(src);
        });
        document.querySelectorAll("a[href]").forEach(a => {
          if (/\.(zip|7z|rar|tar|gz|mp4|mkv|webm|m3u8|mpd|mp3|flac|wav|pdf|iso|exe|msi|dmg|apk|safetensors)(\?|$)/i.test(a.href)) out.add(a.href);
        });
        return Array.from(out).slice(0, 80);
      }
    });
    const urls = (results && results[0] && results[0].result) || [];
    const box = $("media");
    box.innerHTML = "";
    if (!urls.length) {
      box.className = "list muted";
      box.textContent = "No media or direct file links found on this page.";
      return;
    }
    box.className = "list";
    urls.forEach(url => box.appendChild(mediaRow(url)));
    setStatus(urls.length + " item(s) found.", false);
  } catch (error) {
    setStatus(error.message, true);
  }
}

async function init() {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  activeTab = tabs[0] || null;
  if (activeTab) {
    $("pageUrl").textContent = activeTab.url || "—";
  } else {
    $("sendPage").disabled = true;
  }
  $("sendPage").addEventListener("click", () => activeTab && send(activeTab.url));
  $("scan").addEventListener("click", scan);
  $("sendCustom").addEventListener("click", () => {
    const url = $("customUrl").value.trim();
    if (!/^https?:\/\//i.test(url)) {
      setStatus("Enter a valid http(s) URL.", true);
      return;
    }
    send(url);
  });
  $("customUrl").addEventListener("keydown", event => {
    if (event.key === "Enter") $("sendCustom").click();
  });
  $("options").addEventListener("click", () => chrome.runtime.openOptionsPage());

  try {
    const pong = await chrome.runtime.sendMessage({ type: "ping" });
    setConnection(Boolean(pong && pong.ok));
    if (pong && pong.theme && window.pimApplyTheme) window.pimApplyTheme(pong.theme);
  } catch (_) {
    setConnection(false);
  }
}

document.addEventListener("DOMContentLoaded", init);
