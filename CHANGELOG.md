# What's New

## 0.3.40 — 2026-11-25
- Relay checks at every launch that it is still the app macOS will start at login, and re-registers itself if the entry belongs to an older copy. When macOS is holding that change for your approval, Relay says so and names the one place to allow it, instead of reporting it as done.
- Nothing else about how Relay works changes.

## 0.3.39 — 2026-11-24
- Relay's README and release page now show what Relay looks like, with screenshots taken from this build.
- Relay now bundles newer browser extensions: video 1.3.31, image 0.50.35 and audio 0.7.37 (manifest sha256 a739cf048178…, 929818a9fa66…, 82ed80db018d…). They are the versions their repos serve today.
- A Relay built by hand from source can no longer produce a download that names this Mac's home folder. The build stops and says what to run instead.

## 0.3.38 — 2026-11-23
- Relay now updates itself. When a new version is on the release page it asks Relay to pause first — browsers stay open, queued accounts stay queued — and only a hold that answers "safe, nothing would be stranded" lets the update go ahead. A missing or invalid hold token refuses the update instead of guessing.
- Update checks use Relay's own signing key, so an update that was not signed for this app is never installed.

## 0.3.37 — 2026-11-22
- Relay's release card now carries its full description on the Launcher, alongside the one-line summary 0.3.36 added. Nothing about how Relay works changes.

## 0.3.36 — 2026-11-22
- An account that has stopped trying to open is no longer counted as waiting. When an update takes its pause, Relay used to say "N waiting" for accounts it will not reopen until you press Open on them — so a hold could promise work would resume by itself when it would not.
- Accounts keep their turn across a restart. Before, restarting Relay — an update restart included — began the rotation at the top of the list again, so an account further down waited longer than its turn.
- Relay's build no longer depends on folders that moved on this Mac. Nothing about how Relay works changes.
- Relay's release entry now carries its one-line description, so the Launcher card is not blank.

## 0.3.35 — 2026-11-22
- The saved accounts are back on screen. The add form is one row, its help is one line and the bulk buttons sit in the filter bar, so the first account starts well inside the window at every size — before, it began below the fold.
- Account rows speak plainly: "Closed · Relay can open it" instead of "open not requested", and "Gateway has not confirmed it" instead of a readback term. The memory line on that page is one short sentence; the Memory page keeps the full breakdown.
- "5 accounts need a worker ID" is a button that opens the first of those accounts' details, where the field is. The row menu closes before the Delete dialog opens, so two destructive buttons are never on screen together.
- Overview attention rows say what to do ("Sign in to this account in its browser."), and the Gateway card describes the Gateway instead of repeating the account count. The audio extension is marked optional and not finished, and its switch starts off.
- Memory & pacing lays its five fields out in one even row, and the sidebar footer — "Not paired · 0 connected" — fits inside the smallest window instead of being cut off. A failed folder or pairing read says one plain sentence with a next step, and the folder buttons stay disabled until a path is known.
- Open windows no longer offers "Open selected", which could never become available on a page that only lists browsers already open.

## 0.3.34 — 2026-11-21
- When several accounts are waiting to open and memory frees up one at a time, Relay now takes them in turn instead of always starting the first account in the list again, so an account further down the list no longer waits forever.
- The update hold now says how many accounts are waiting for memory. It used to answer "nothing in flight, safe to restart" while work was still queued — correct for "would a restart strand anything", but it read as idle when it was not.
- Settings has an Image price: what Relay reports to Gateway as the price of one image, in credits. It starts at 0 — free — and can be changed at any time.

## 0.3.33 — 2026-11-20
- Force close now tells Gateway to stop sending work to that account. Before, closing an account normally did this but force closing did not, so Gateway kept handing jobs to a browser that was no longer running: the work was accepted, nothing could pick it up, and no error said so. Force close now does the same thing Close does.
- Reopening an account no longer shows the previous launch's connection result while the new one is still connecting. It shows that nothing is checked yet, then the real result.

## 0.3.32 — 2026-11-19
- Relay now signs itself with the same certificate as the other Studio apps, instead of an anonymous one. An anonymous signature gave every update a different identity, so macOS treated each new Relay as a stranger and forgot the permissions you had granted it — you had to allow screen recording and accessibility again every time. Now Relay keeps its identity between updates, and those permissions stay.

## 0.3.31 — 2026-11-18
- The download itself is clean. 0.3.30's app still carried this Mac's home folder — once, in symbol names built from a dependency's C code, which the compiler's path remap cannot reach — so the build now strips those symbols and the release gate scans the finished DMG, the exact bytes that go on the feed, instead of the build folder. Nothing about how Relay works changes.

## 0.3.30 — 2026-11-18
- A web page can no longer pause Relay. The update hold still answers Studio Launcher on this Mac, but a request coming from any browser page — any origin other than Relay's own page — is refused, so a stray tab cannot stop Relay starting browsers for fifteen minutes.
- A Gateway on another Mac needs an https:// address: plain HTTP carried your worker token unencrypted, so it is now accepted only for a Gateway on this same Mac. A fleet still on http:// keeps working — the saved address carries that choice forward — and the Gateway page has a visible “Allow plain HTTP” switch to turn it back on for a network you trust.
- Relay's README says where its tokens actually live: a file only you can read in its own data folder, never the Keychain.
