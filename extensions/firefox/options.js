"use strict";

const $ = id => document.getElementById(id);

async function load() {
  const settings = await browser.storage.local.get({ apiPort: 18500, takeover: false });
  $("port").value = settings.apiPort;
  $("takeover").checked = Boolean(settings.takeover);
}

async function save() {
  const port = Math.max(1024, Math.min(65535, Number($("port").value) || 18500));
  await browser.storage.local.set({ apiPort: port, takeover: $("takeover").checked });
  $("status").textContent = "Saved.";
  $("status").className = "status";
}

async function test() {
  const port = Math.max(1024, Math.min(65535, Number($("port").value) || 18500));
  try {
    const response = await fetch("http://127.0.0.1:" + port + "/ping", { cache: "no-store" });
    $("status").textContent = response.ok ? "Connected on port " + port + "." : "No valid response.";
    $("status").className = "status" + (response.ok ? "" : " error");
  } catch (error) {
    $("status").textContent = "Not reachable: " + error.message;
    $("status").className = "status error";
  }
}

document.addEventListener("DOMContentLoaded", async () => {
  await load();
  $("save").addEventListener("click", save);
  $("test").addEventListener("click", test);
});
