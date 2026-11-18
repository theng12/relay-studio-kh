# What's New

## 0.3.31 — 2026-11-18
- The download itself is clean. 0.3.30's app still carried this Mac's home folder — once, in symbol names built from a dependency's C code, which the compiler's path remap cannot reach — so the build now strips those symbols and the release gate scans the finished DMG, the exact bytes that go on the feed, instead of the build folder. Nothing about how Relay works changes.

## 0.3.30 — 2026-11-18
- A web page can no longer pause Relay. The update hold still answers Studio Launcher on this Mac, but a request coming from any browser page — any origin other than Relay's own page — is refused, so a stray tab cannot stop Relay starting browsers for fifteen minutes.
- A Gateway on another Mac needs an https:// address: plain HTTP carried your worker token unencrypted, so it is now accepted only for a Gateway on this same Mac. A fleet still on http:// keeps working — the saved address carries that choice forward — and the Gateway page has a visible “Allow plain HTTP” switch to turn it back on for a network you trust.
- Relay's README says where its tokens actually live: a file only you can read in its own data folder, never the Keychain.

## 0.3.29 — 2026-11-16
- Relay's icon is blue everywhere it is drawn. The smallest sizes Finder, the Dock, the menu bar and the DMG window use were packed in a way that turned them red and purple; they now ship packed the way macOS expects, so the mark stays its own blue at every size.

## 0.3.28 — 2026-11-15
- An update holds Relay first, and holds nothing on disk. The hold stops Relay starting any new browser, so queued accounts stay queued and no new work is claimed; it then waits up to 20 seconds (30 at most) for an account that is opening or closing to finish or to keep the id the next start picks up, and says whether restarting now would strand anything. Letting go resumes at once, the pause also expires on its own, and a restart always starts working again — an install that aborts can never leave Relay refusing work.

## 0.3.27 — 2026-11-14
- Below 1080 px the Table layout stacks the same way Compact does, so the status column can never extend past the row on a narrow window.

## 0.3.26 — 2026-11-14
- In the Table layout, long connection and next-step lines wrap inside their column at narrow widths instead of running past the row edge.

## 0.3.25 — 2026-11-14
- The grouped warning above Accounts reads as a sentence for any count ("5 accounts need a worker ID", "1 account needs a worker ID"), and the row's next step points at the ⋯ → Show details menu where the form now lives.

## 0.3.24 — 2026-11-13
- Extensions keep one id across releases: each manifest carries a fixed key, so seeded storage is no longer orphaned when the bundled version folder changes.
- Open and close are one action. Close never asks Gateway first and never needs a manual Force: it asks Chrome to quit, repeats at 10 s, ends the one process Relay started (with its helpers) at 15 s, and at 20 s says plainly what survived. Open during a close queues and opens when the process is gone.
- Seeding writes Chrome's own storage API instead of evaluating the extension's worker, so a sleeping worker cannot make a seed fail, and every result goes to lifecycle.log.
- Bundle Image 0.50.33, Video 1.3.28, Audio 0.7.36: the Flow account comes from the project tab address alone, heartbeats carry the real account and project, a 4xx claim refusal clears its marker, every tick writes a status, and one readiness object feeds the panel and Relay.
- Video and Audio panels say "Connected through Relay · <machine>" when Relay seeded them and show the readiness reason when blocked.
- Video and Audio panels now share the image panel's system — one spacing scale, 32 px controls, one primary action, section titles — with behaviour and ids unchanged.
- Contrast and layering pass, measured not eyeballed: body text 13.8–16.7:1, secondary 5.9–8.6:1, primary buttons 7.1–11.0:1, status text 6.3–11.3:1, in light and dark; the border ramp now separates cards from the ground (≥1.5:1). A test fails if any of these pairs drops below 4.5:1.
- Accounts has a third, remembered layout: Table, with aligned columns and a fixed header; it joins Compact and Card and the choice survives restarts.
- Each account row keeps one primary action; Show details and Delete moved into a ⋯ overflow menu that closes on Escape or a click outside.
- Repeated warnings are grouped above the list with counts ("2 accounts missing a worker ID") instead of repeated on every row.
- The Accounts page refreshes once a second, so an open, close or seed result is not five seconds stale.
- Start at Login is a proper macOS login item now, not a raw LaunchAgent that macOS can list as a nameless entry. The old agent is retired only after the new registration succeeds, and moved to a recoverable trash rather than deleted.

## 0.3.23 — 2026-11-13
- Closing an account is bounded: Relay asks Chrome to quit, repeats at 10 s, forces the one process it started at 15 s, and at 20 s says plainly that the pid survived and keeps the account usable for Open — no more "Waiting to close" that only a manual kill clears. Verified paid work still blocks every step.
- Image extension 0.50.29: Gateway claims no longer depend on reading a Flow account identity. A real Flow project tab claims with an unknown balance and the probe kept as detail; the project id comes from the tab itself, sign-in detection checks several markers, and a tab with no project names what failed.

## 0.3.22 — 2026-11-13
- Every account row now has a visible primary button: Open when its browser is closed, Focus when it is open. Close sits beside it as the account's own safe-stop, and the row still opens on a click.
- An open account shows its window state on the card face ("Open · open requested · memory"), so Open versus Focus is never a guess; a pending close is stated once, never as two buttons.

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
