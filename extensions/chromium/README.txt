Pro Internet Manager Bridge - Chromium (Chrome / Edge / Brave), Manifest V3
by Triwar Design

Install (unpacked):
1. Open chrome://extensions (Edge: edge://extensions, Brave: brave://extensions)
2. Turn on "Developer mode"
3. Click "Load unpacked" and choose this folder (chromium-extension)
4. Start Pro Internet Manager (it listens on http://127.0.0.1:18500)

Same functions as the Firefox build: in-page globe/X pill with the slide-out source list
(type + quality), theme sync with the app, popup, settings, right-click menu.
Differences in Chromium:
- Background is a service worker; theme ping runs every 30 s via chrome.alarms (Firefox: 15 s).
- "Take over browser downloads" cancels the download right after it starts; it can briefly
  appear in the download bubble. No tab id for downloads, so the referrer is sent as Referer.
- Cookies come from the default cookie store (no container / cookieStoreId support).
- Unpacked extensions show a "developer mode" notice at browser start; a permanent install
  needs the Chrome Web Store / Edge Add-ons.
