Pro Internet Manager Bridge (Firefox) - by Triwar Design
========================================================
(Restyled copy of the Rust Download Manager bridge. Same add-on id
rust-download-manager@local, same API port 18500, works with both apps.)

1. Start Rust Download Manager (API 18500, ffmpeg present).
2. Firefox: about:debugging#/runtime/this-firefox
3. Load Temporary Add-on -> manifest.json.

Right-click a link/video/audio/image, use the toolbar popup, or scan the page.
Current tab cookies, Referer and User-Agent are sent to the local API.
Session cookies are included; the client forwards them to HEAD/Range requests
and to ffmpeg for HLS/DASH.

Floating button:
When a video/audio/manifest is detected, a small animated pill (globe + X)
appears at the top-right of the page. Click the globe to slide out the list:
"Download all", then numbered sources with type and quality, for example
"YouTube · MP4 · up to 1080p Full HD", "Stream (HLS) · adaptive",
"VTT · EN subtitles". Drag the pill to move it, X hides it for that page.

Theme:
The app's /ping answer carries the active theme (colours + icon key).
The toolbar icon, popup, settings page and in-page pill follow it
(icons/<theme>/). Old app versions without theme info keep the green look.

YouTube:
YouTube pages get numbered quality choices (2160p, 1440p, 1080p, ..., 144p)
plus a best-quality entry. The client uses yt-dlp for the page and quality
selection; yt-dlp.exe must be next to Pro Internet Manager_restyle.exe.

HLS (.m3u8) and DASH (.mpd) are supported through ffmpeg. DRM-protected streams
are not supported. Optional Settings -> Take over Firefox downloads forwards
normal Firefox downloads too.

Restricted pages (about:, AMO) may block content scripts; allow the extension
under about:addons -> Permissions & Data -> Run on sites with restrictions.

Clean-room bridge; not copied IDM code or assets.
