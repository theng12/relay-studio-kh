# What's New

## 0.3.21 — 2026-11-13
- Seeding no longer waits for a close-safety binding: a saved, validated Gateway address is enough, so an account that binds only image (or only video) now connects when it opens instead of staying on Standalone with no connection.
- The generation check is per bound kind: an image-only account verifies its image worker instead of being asked for a video sibling it never had. The close safeguards keep the same rules.
- Read-back reads Chrome's actual reply level, so a seed that landed reports verified instead of falsely failing; covered by tests on the real nested shape.

## 0.3.20 — 2026-11-13
- Opening an account now seeds its extension on every open, reads the connection back out of the extension over Chrome's own control pipe and retries once — the panel no longer opens with an empty Gateway form, and the account row carries the verified result or the exact reason.
- Image extension 0.50.28 puts the connection in one place: one Mode control, one primary Connect, address/token/worker ID behind Details, "Connected through Relay" naming this Mac, a 32 px icon button for Open workspace, and one spacing scale at 360 and 375 in light and dark.
- Bundle Image 0.50.28, Video 1.3.24 and Audio 0.7.32: Gateway mode can reach http://127.0.0.1:8787 again — the loopback host permissions came out with the old Story server and are restored, with a test that fails if Gateway mode ever ships without them.

## 0.3.19 — 2026-11-13
- Accounts has one search for a label or account number, with a plain message when nothing matches; the range and All/None selectors are unchanged.
- Overview no longer repeats Open windows and Gateway as buttons — the sidebar already has them.
- Open windows cards show the account, its state and any reason on the face; process ID, memory, provider and activity sit behind a Show details button.
- Measured at 1440 and 768: no clipping, no overlap, and lists scroll inside their page.

## 0.3.18 — 2026-11-13
- The Accounts page renders again: a missing next-step element crashed the row render, left "Reading saved accounts…" on screen and looked like native management was unavailable. The element is wired and guarded, and a render failure now says what failed; Card is the default view.
- Extensions have exactly two modes — Standalone (default) and Gateway Studio; a stored Studio choice moves silently to Standalone, and the Story Studio UI, settings, permissions and tests are gone.
- Bundle Image 0.50.27, Video 1.3.23 and Audio 0.7.31: Flow request ids are discovered from the live page each time and kept only as editable fallback values in Settings — a Google change is a settings edit, not an extension rebuild.
- An id that cannot be resolved is named with what was tried, in the panel and at worker start ("Account probe failed: …"), never collapsed into "signed out".

## 0.3.17 — 2026-11-13
- Dispatch is turned on for a worker when it pairs and when its browser opens — image, video and audio alike — and the account row says Dispatch on, or off with the reason jobs will not be claimed. Gateway starts every worker with dispatch off, so this is what makes a freshly paired account claim work.
- Bundle Image 0.50.25 and Video 1.3.21: a signed-in Flow project whose account probe fails now claims with an unknown balance instead of being treated as signed out, and the panel says whether Flow is missing or could not be read.
- Carries the 0.3.16 interaction patch: click a row to open or focus, Focus instead of dead actions, non-overlapping compact rows, Card as the default view, and the same worker-id → seed → dispatch path for audio as image and video.

## 0.3.16 — 2026-11-13
- An account row is clickable: it opens the browser, or focuses it when already open, and its primary button reads Focus.
- "Open selected" becomes "Focus selected" when the selection is already open, instead of looking live and doing nothing.
- Compact rows never overlap: identity and state own separate columns at every width, and Card is the default view (Compact remains a choice).
- Closing a browser stops it; clicking the account reopens the same profile — same cookies and extension — and the row says no jobs are claimed while it is closed.
- The row names the next pairing step in plain words: type a worker ID, or open the Gateway page to finish.
- Saving a worker ID and opening the Gateway page re-run automatic pairing at once — no restart needed.
- Audio worker IDs are bound, seeded and dispatched like image and video.
- Dispatch, which Gateway starts off, is turned on for a worker when it pairs and when its browser opens; the row says Dispatch on, or off with the reason jobs will not be claimed.

## 0.3.15 — 2026-11-13
- Relay finds the Gateway on this Mac by itself — no address to type — and keeps its worker tokens in a file only you can read, never the Keychain.
- An address you do type needs no http:// or port; Relay fills both in and shows what it used, and any failure names the address it actually tried.
- An extension that is paired but waiting for a signed-in Flow tab says so instead of looking idle.
- Updates belong to Studio Launcher KH: Relay's update button opens it and Relay never installs its own update.

## 0.3.14 — 2026-11-12
- Enter each extension worker's id and token once on the Gateway page. Relay keeps them in this Mac's Keychain, never shows the token again, and seeds them into the matching extension when it opens a browser.
- Only a worker whose id is bound to that account is seeded; a kind with no token or no binding says so on the account row instead of looking configured.
- Audio has no per-account worker binding yet, so it is left unseeded and says so.
- The sidebar footer is one tidy line with equal insets; the version truncates instead of wrapping.

## 0.3.13 — 2026-11-12
- Bundle Image 0.50.24, Video 1.3.20 and Audio 0.7.30: every options page now shows only real choices — the mode, the Gateway Studio block, and the Story Studio connection only outside Gateway mode. The Google Flow address is fixed to https://flow.google.com and its section is gone.
- The image worker panel names a saved connection with no Gateway mode selected, and an alarm that exists but never fires is visible in storage.

## 0.3.12 — 2026-11-12
- Bundle Image 0.50.21, Video 1.3.19 and Audio 0.7.29: Fleet Hub is removed from all three extensions, so it is gone from Relay's browsers too. The image worker's claim tick now starts with the worker and reports when a Flow tab or inventory blocks it.

## 0.3.11 — 2026-11-12
- Bundle Image extension 0.50.18: the Gateway claim alarm is recreated on browser startup and extension update, and the side panel says whether the claim loop is ticking or not ticking at all.

## 0.3.10 — 2026-11-12
- Bundle Image extension 0.50.17: an armed Gateway worker opens the Flow tab it needs and reports "waiting" instead of looking idle, so Gateway no longer withdraws an image job unsent. Video 1.3.18 and Audio 0.7.28 are unchanged.

## 0.3.9 — 2026-11-12
- Browsers Relay opens from Accounts now load the bundled Image, Video and Audio extensions themselves — no Load unpacked step. Chrome no longer accepts the old load-extension flag, so Relay uses Chrome's own debugging control.
- Each extension has a "Load in Relay's browsers" switch on the Extensions page; turn one off and the next browser Relay opens leaves it out.
- "Show folder" prepares and reveals a version folder without copying a path, and "Check loaded in open browsers" asks Chrome what it actually loaded, with versions.

## 0.3.8 — 2026-11-12
- The second accounts view is called Card; a previously saved Grid choice still opens Card.
- Account details open from a Show details button. The disclosure triangle is gone from every screen, including the Gateway worker list.
- Delete removes the saved account and its Chrome profile for good; a refusal, such as an open browser, stays on that account's row.

## 0.3.7 — 2026-11-12
- Delete works. It asks in Relay's own window, moves the account's Chrome profile to Relay's own _trash, reloads the list and says where the folder went.
- A refused Delete says why on that account's own row, such as a browser that must be closed first.
- Ship the current Image (0.50.16), Video (1.3.18) and Audio (0.7.28) extensions inside the app; Audio still ships unfinished and does not block the other two.
- The build refuses to bundle an extension that is behind its repo or would drop its Gateway account code.
- On launch, Relay copies a newer bundled extension into its own folder; existing version folders and browsers are left alone.

## 0.3.6 — 2026-11-12
- Rename and Delete are always visible on every account row; they are no longer hidden until the pointer hovers the row.

## 0.3.5 — 2026-11-12
- Gateway's admin pages button sits apart from pairing, says it asks for the Gateway app's browser password, and says it is never needed to pair or to claim work.
- Pairing reads as what it is: mint a manager token in Gateway → Providers → Browser workers, then paste it here.

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
