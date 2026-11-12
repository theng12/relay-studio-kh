# What's New

## 0.3.4 — 2026-11-12
- Saving a Gateway address checks it and stops there — no sign-in window appears just because you typed a URL.
- The Gateway page says when the loopback address is right, and that pairing a manager token removes the need to sign in at all.
- The manager token is explained: what it is scoped to, where it is minted, and that it is not the worker token an extension uses.
- Audio is described as unfinished rather than optional — it ships as-is and does not block image or video.

## 0.3.3 — 2026-11-12
- Rename a saved account in place: click its name, type, and press Enter.
- Delete an account recoverably: its Chrome profile moves to Relay's own `_trash` and the page tells you where.
- See accounts in one compact line each, or switch to the grid view; your choice is remembered.
- Read the memory arithmetic on Memory & pacing: room left after the reserve and how many more accounts fit.
- Understand a "waiting for memory" refusal: the reason log now records the room, the reserve and the size it measured.
- Keep the measured per-account memory across restarts instead of measuring from scratch each launch.

## 0.3.2 — 2026-11-12
- Start on an Overview dashboard: saved accounts, browsers open, memory used and available, Gateway state and the accounts that need attention.
- Move between Overview, Accounts, Open windows, Memory & pacing, Gateway, Extensions and Settings; every page keeps its own scroll.
- Give each sidebar row and the footer pill a matching icon, with a clear selected row and a version that is no longer a link.
- Fill the whole window at 1100×760, remember the window frame, and keep only 28px page padding — no centred column or side bands.
- Choose System, Light, Dark or Default appearance inside Relay; macOS appearance is never changed.
- Close unbound Chrome profiles immediately; bound profiles ask Gateway only about workers that exist, and a close that takes longer than three seconds shows why and offers a verified Force close.
- Force close now stops only the exact Chrome process Relay started and is blocked only by explicit active or uncertain paid work.
- Read What's New as a full page from the footer pill; Quit Relay lives only in the app menu.

## 0.3.1 — 2026-11-11
- Read each part of Relay as its own card with a clear heading.
- Keep the page in one centred column that never touches the window edge.
- Open Memory and pacing, Gateway connection and Browser extensions from clear header rows.

## 0.3.0 — 2026-11-11
- Keep saved Chrome accounts in one list and open only the ones you need.
- Select accounts by number, or open the account that needs attention.
- Limit open browsers to your memory budget and keep your saved on and off choices.
- See connection and activity separately, with unknown balances shown clearly.
- Request a safe close without replaying uncertain work.
- Keep browser audio optional and off while using image and video accounts.

## 0.2.0 — 2026-11-10
- Keep Relay available as a Mac desktop app with a saved Gateway connection.
- Choose whether Relay starts when you sign in.
- Reconnect to Gateway without resubmitting work.
- Follow browser-specific steps to set up your three extensions.
- Keep earlier extension folders when preparing an update.

## 0.1.1 — 2026-11-10
- Prepare the shared connector from a checkout outside the original Mac folder.
- Keep translated What's New text intact when building the connector.

## 0.1.0 — 2026-11-09
- Connect your video, image and audio extensions to Gateway Studio KH from one place.
- See reported connection details and credits, with clear unknown and waiting states.
- Create a worker token once and revoke access when you need to disconnect.
