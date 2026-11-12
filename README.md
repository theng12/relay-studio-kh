# Relay Studio KH

Relay manages saved Chrome accounts for the existing image and video browser generators and connects their workers to Gateway Studio KH. Browser TTS is owner-skipped, optional and off. The existing connector remains a pane in Gateway's Providers screen; the Mac shell adds real on-demand Chrome profile lifecycle and a local account list. Gateway owns authentication, jobs, models/capabilities, shared account credits, reservations, artifacts and deployment. No separate production service is added.

## Browser account manager — 0.3.0 candidate

One account register supports stable numbered profiles, bulk/range selection, open/close and an attention action for the exact profile. Saving one hundred profiles does not open one hundred browsers. Native admission measures memory and applies an adjustable budget/reserve and optional active-process ceiling; saved profiles have no arbitrary count cap. Chrome owns each managed profile's cookies/storage; Relay does not copy credentials or touch the user's existing Chrome profiles.

The native manager persists local enabled/open intent atomically and preserves OFF choices through restart/wake. Exact-owned Chrome processes are reconciled before another launch. Opening a browser never submits a job. Close requests first disable dispatch for both bound image/video workers and wait for fresh Gateway pause/control-revision and safe-close acknowledgement; missing/stale/active/held/uncertain state stays pending rather than forcing termination or replay.

Connection, provider sign-in and readiness are separate from generating/cooling/resting/waiting/recovering. Process liveness never proves readiness. Gateway account balances are deduplicated across image/video and retain source/observed time/reset/buckets only when actually observed. Unknown/stale is not zero; fifty free daily credits and midnight/timezone resets are not universal defaults. Local profile IDs do not identify provider accounts. Gateway's independent account spending flag starts off and cannot be enabled by the manager.

Pairing uses a one-time scoped manager token created through Gateway's normal app-only route for exact worker IDs. Relay retains that token only in its own origin-bound macOS Keychain item. It cannot claim/upload work, mint workers or enable account spending. No Gateway key/cookie extraction or plaintext fallback. Manager read/control uses the existing Gateway service, authenticated Bearer scope, bounded responses, no redirects and no native capabilities granted to remote web content.

Default random inter-generation pause is five to fifteen seconds; explicit zero/custom values and longer provider cooldowns remain. A saved desired range is not proof of application: only fresh worker pacing/next-eligible readback establishes it. Existing image/video worker changes must reach the actual generation boundary. Browser TTS stays optional/off/nonblocking; Lightpanda remains optional/off and has no execution claim.

Source code, fixtures and preview do not prove a compiled or installed manager. Native compilation/lock, real Chrome lifecycle, Keychain, login/wake/restart, authenticated Gateway control, signing, normal distribution and intended-Mac checks still require their admitted evidence. `scripts/desktop-preview.mjs` is an explicitly labeled disposable UI fixture, never a real browser manager and never bundled in the app. Frozen native/payload candidates and original extension work remain preserved.

Each extension keeps Story Studio and standalone modes and gains Gateway Studio KH as its third mode. Video integration comes first. Story keeps one Gateway endpoint. A built pane is not evidence that all three extension modes are installed or live.

## Mac desktop source candidate

The owner now prefers a persistent Mac app; Windows native support can wait. `src-tauri/` prepares a Tauri 2/Rust shell without adding a queue, backend server or extension runtime. This source candidate is not compiled, packaged, installed or qualified. Main portable 0.1.1 remains source/offline qualified only until the candidate is reviewed and integrated.

Relay's local settings window uses the existing system-font styles and shared What's New. Its Gateway window opens the existing `/admin#extension-workers` interface as a same-origin top-level page. Sign in using Gateway's ordinary browser password; the app does not extract or inject admin/owner credentials or Chrome cookies. Its separate, least-privilege manager token is paired explicitly and kept only in Relay's own origin-bound secure storage. The webview has its own browser session, not the owner's Chrome profile. Gateway alone decides which management actions are authorized. No extra local-app privileges are claimed for this webview, and remote content receives no native settings capability.

Start at Login is initially on and can be changed. Saved opt-out is never replaced by the first-launch default. OS registration/readback failures are errors, not success. URL and login preference are stored in Relay's app-config folder using replacement of a temporary file; unreadable settings are held rather than reset. Normal OS cookie persistence and restart/login behavior still require native proof. Closing settings hides that window; the Gateway window closes independently. The Dock and a second launch restore settings. The native menu or Quit Relay exits. A second instance focuses the existing app. Health checks can reopen the existing Gateway page after recovery; they never claim, generate, fail, upload, retry or settle worker jobs. Possibly charged work remains held at Gateway/extensions.

Chrome MV3 extensions continue running externally in Chrome. A Tauri webview cannot run them. Browser/provider switches are never enabled by this shell. The Gateway address is an explicit saved HTTP(S) origin without a path, embedded credentials, query or fragment; no production port/address is guessed.

Cheap web assets/static checks (no Rust/native build):

```bash
npm run desktop:assets
```

```bash
node --test test/desktop.test.mjs
```

For source UI inspection, `relay-browser-manager-source-preview` serves these assets on loopback 7921 with a visibly labeled, in-memory fake native bridge. It tests browser rendering and UI wiring only, never OS login settings, file persistence, real Gateway connectivity, Tauri permissions or WKWebView behavior. Never deploy `scripts/desktop-preview.mjs` or include its bridge in the native assets.

Desktop asset preparation also generates the Mac icon set from the versioned artwork in `scripts/make-icon.py`, using Python's standard library. It uses Relay's existing blue accent and a white three-to-one connection mark. PNGs contain source provenance; PNG/ICNS outputs stay ignored under `src-tauri/icons/`. The normal Tauri configuration references those generated assets while bundling stays disabled. Icon asset checks do not prove a native package or Dock appearance.

The native qualification window additionally needs the Tauri 2 CLI, Rust toolchain and Apple build tools. Cargo dependency resolution/lock, compilation, WKWebView rendering/session persistence, login registration and opt-out/restart/single-instance/Quit behavior have NOT been verified. Native bundling stays disabled until the normal coordinated packaging step; it must use the existing KH Studio Local Signing identity, approved app assets and normal release/update discipline. Do not infer a working native app or Windows compatibility from static checks or mocked recovery tests. No native build/package/install is permitted in the current source-preparation window.

## Removing Relay

Run `scripts/uninstall.sh`. It quits Relay, removes the Start at Login item and moves the app to the
Trash, and it leaves `~/Library/Application Support/kh.relay.desktop` alone so your saved accounts,
Chrome profiles, extension folders and settings survive a reinstall. Delete that folder yourself if
you want the data gone too. The app cannot reliably delete itself, which is why this is a script.

## Distribution and browser setup

Owner2026-11-10 requests the normal signed `Relay Studio KH.app` through Studio Launcher KH and the existing Updater feed. No end-user Node/Python or custom updater/server is added. Existing generic `app-dmg` support needs no bespoke Launcher registry or code. After native qualification, Stack coordinates the exact signed DMG and normal feed/catalogue data with original Developer; distribution and delivered-app acceptance remain pending. The shell's bundle identifier is `kh.relay.desktop`, product name `Relay Studio KH`, executable `relay-studio-kh`, and signing identity remains `KH Studio Local Signing`. Feed metadata and What's New follow the existing shared release contract; do not invent a second distribution mechanism.

`desktop/extensions.json` is the reviewed release-input lock, initially all unavailable. It records candidate versions/commits without claiming their integration acceptance. After original-writer handover and integration review, accepted inputs additionally pin `manifest_sha256` and every `{path,sha256}` in a reviewed build payload. `RELAY_EXTENSION_PAYLOADS` points to those exported build directories (`video/`, `image/`, `audio/`), never a dirty original checkout. The helper refuses extra/missing/tampered files, unsafe paths, links, wrong MV3/version metadata and existing output. No extension source is vendored into this repository. Runtime payload resources are generated under ignored `desktop-resources/extensions/` and mapped to the signed app's `relay/extensions/` resources.

Source assets can honestly show unavailable rows. Release asset preparation refuses every unavailable required extension. Image/video remain required by default; owner-skipped audio is explicitly `required:false`, off and unavailable, so it cannot block their release or silently become available:

```bash
npm run desktop:release-assets
```

Tauri's normal before-build command uses that strict preparation. Bundling remains disabled until coordinated release qualification. Missing/unreviewed required image/video payloads still block release, even when the local account UI can be previewed. Optional/off audio never changes acceptance of the required payloads. Payload preparation is a release input step, not browser installation.

The setup screen lists three extensions and their bundled version, or the missing-payload reason/candidate version. Native detection checks standard Mac application folders and Spotlight; not detected does not mean uninstalled. Chrome comes first. Edge and Brave are separately named, unverified candidates. The browser definitions are source data in `desktop/browsers.json`, not inferred compatibility.

The user opens that browser's extension settings, turns on Developer mode themselves, chooses Load unpacked and selects each prepared folder. Relay never changes browser profiles, policies, permissions or security settings; no silent installation, Web Store publishing or enterprise policy is authorized. Firefox needs background-script/sidebar adaptation, and Safari needs a separate extension/container conversion. Neither port is implemented.

Preparing a folder copies only verified resources to a stable app-support path `extensions/<kind>/<version>-<payload-sha256>/` outside the signed app. Its path never changes when Relay updates. A different extension build gets a separate folder and preserves the previous one; an existing modified folder fails instead of being replaced. No browser reload, worker change or installation happens automatically. Stop or resolve active/unknown jobs in Gateway before manually switching browser versions; staging cannot acknowledge or bypass those holds. Installation, handshake and provider readiness remain unknown until actual browser proof. Custom or independently installed extensions are untouched.

Packaging tests use synthetic reviewed MV3 inputs, not the real extensions:

```bash
node --test test/extension-resources.test.mjs
```

Native resource lookup, hash-verified export, browser discovery/opening, clipboard access, actual path persistence across app updates and Chrome/Edge/Brave behavior remain unverified until their native qualification windows. No browser or OS action is run in source preparation.

Official guidance: [Chrome Load unpacked](https://developer.chrome.com/docs/extensions/get-started/tutorial/hello-world#load-unpacked), [Edge porting and sideload testing](https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/port-chrome-extension), [Firefox background support](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background).

## Build and Gateway integration

Building requires Node with built-in fetch, FormData and `node:test`, Python 3 supporting `-I -X utf8`, and the existing `kh-shared-ui` checkout. There are no frontend packages to install. End users do not need Node or Python to use the connector served by Gateway.

Put `kh-shared-ui` next to this repository, or set `KH_SHARED_UI` to its folder. Relative overrides resolve from the command's working directory; paths containing spaces are supported. The build uses `python3` on macOS and `python` on Windows. Set `RELAY_PYTHON` to a Python 3 executable name or full path if different; pass only the executable, without arguments or shell quotes in the value. Python runs isolated with explicit UTF-8 output so translated What's New entries do not depend on Windows' console encoding.

From the repository, run:

```bash
npm run build
```

The build reads `VERSION`, validates the newest `CHANGELOG.md` entry with the shared KH What's New tool and generates `dist/`. Missing shared tools, a missing interpreter or mismatched release metadata fail the build. Generated outputs are not committed. Gateway's lead pins a reviewed Relay commit and rebuilds or vendors these two outputs:

- `relay-pane.html`: markup and styles scoped under `.relay-connector`, with `relay-` prefixed element IDs.
- `relay-pane.js`: the browser bundle exposing `window.mountRelayConnector`.

Integrate the markup into the existing Providers pane and load the bundle after Gateway's normal UI helpers. Mount once with the container and the existing auth boundary:

```js
window.mountRelayConnector(container.querySelector('.relay-connector'), {
  canManage: IN_APP,
  request: (path, options) => fetch(path, {
    ...options,
    headers: adminHeaders(options.headers),
  }),
});
```

`IN_APP` and `adminHeaders` are Gateway's existing helpers. Relay never reads or persists the injected admin credential. The Gateway server still enforces local-app-only token writes; hiding controls in a browser is only a usability measure. Browser readback uses existing same-origin admin sessions. The fragment does not auto-mount or create a new static mount. Gateway owns its version bump, review, normal release and This Mac deployment.

The accepted management boundary is:

- `GET /admin/api/extension-workers`: snake_case worker readback; `404` means the connector is off.
- `POST /admin/api/extension-workers`: `{worker_id, kind}`; returns a token once. Existing ID minting replaces its previous token.
- `DELETE /admin/api/extension-workers/{worker_id}`: revoke access while retaining its record.

Unknown credits remain unknown; a reported zero stays zero. Online readback means transport connected, not provider readiness. Pairing blocks duplicate requests and dialog dismissal while a token is being created, and clears the displayed token on close. Failed requests are never retried automatically. Gateway supports the actual limit and auth; this surface does not add pricing or paid submission policy.

The fixture preview runs via the `relay-connector-preview` configuration in the parent `.claude/launch.json`. It serves the same compiled fragment on loopback port 7896 with explicit preview labels and fake data. Its bootstrap, control route and in-memory workers are development fixtures only; do not deploy `scripts/preview.mjs` or the preview `index.html`.

## Mac, Windows and browsers

Relay is shared web content plus Chrome Manifest V3 extension sources. A Windows browser worker can pair to the existing Mac-hosted Gateway over the approved LAN/Tailscale connection. This does not supply a standalone Windows Gateway, native Relay installer or another server. Management writes remain local-app-only; a remote browser does not gain token mint/revoke authority. Use Gateway's existing browser authentication for permitted readback, and pair the extension with its scoped worker token.

Compatibility acceptance requires disposable native Chrome checks on both Mac and Windows, including Gateway mode, preserved Story/Standalone modes, authenticated connection, suspension/restart holds and exact retained artifact delivery without paid generation. Edge must be checked separately before being claimed. Firefox and Safari are not claimed. No minimum browser version is asserted without evidence. A successful source build or embedded browser preview does not establish native Chrome/Windows/Edge compatibility.

For a bounded offline build and management-schema check, with shared tools configured:

```bash
node --test test/build.test.mjs test/connector-api.test.mjs
```

Build fixtures use Node's temporary directory by default; set `RELAY_TEST_ROOT` to an existing disposable workspace to retain them elsewhere. They run the actual build and shared tools from relocated paths containing spaces, preserve Unicode release text, verify the generated mount entrypoint and refuse missing tools/interpreters or mismatched versions. These tests do not launch a real extension or contact Gateway. The development preview exercises the generated fragment against fake workers; its management controls do not demonstrate production authorization.

## Sources and ownership

- Canonical extension handoff: `/build/Developer/chrome-extensions/_handoffs/2026-11-09-gateway-extension-provider-handoff.md`.
- Extension sources: `/build/Developer/chrome-extensions/story-studio-{video,image,audio}-generator`.
- Gateway: `/build/Developer/gateway-kh/repository`, owned by its existing lead.
- Canonical request: claude-mem `My Own Apps / relay-studio-kh-requests`; Developer's list is a coordinator pointer.
- Operational evidence: `/build/Developer/_orchestra/relay-lead/`.
- Owner deliverables: `~/Desktop/All YouTube Stuff/Apps/Relay Studio KH/`.

The video checkout contains intentional uncommitted owner work. Read-only use is approved; overlapping edits require verified writer handover. Do not stash, reset or commit those files here. Scoped worker access over This Mac's existing LAN/Tailscale is approved. Paid generation/capture, public publishing, unrelated OS/security changes and 1301 activation remain excluded.

## Isolated checks

Run `npm test` with Node VM module support (the script supplies the flag). Tests load current extension source read-only; set `RELAY_VIDEO_SOURCE` to test another checkout. Fetch is intercepted, Chrome storage and provider responses are fixtures, and no paid calls or real browser profile are used. A pass proves the named mocked behavior only. Connector API tests exercise Gateway's accepted management schema separately.

`patches/video-reference-recovery.patch` is an unapplied extension patch: retain staged references until a result is accepted, and refuse a missing reference instead of generating a paid replacement. Tests copy only `src/background.js` into a fresh folder under `/Volumes/KH-STUDIO-ATLAS/KH-Worktrees/relay-studio-kh/`, apply it there and run the patched source in a VM. Test copies are retained for inspection; set `RELAY_TEST_ROOT` to another existing test workspace. The original source is never patched by these checks.

`patches/video-flow-credits.patch` adds the captured read-only Flow `nzlxg` operation with source and timestamp. Missing, malformed, negative or rejected balances fail explicitly. No price or generation call is inferred. Credit telemetry and reference fixes still require source handover, integration checks and the extension's normal release process.

## Corrected Gateway HTTP proof

`scripts/gateway-proof-lock.json` is the shared source lock for the fixture and client proof. It remains `pending-final-merged-gateway`: no current merged-source proof is claimed. Both entry points refuse a pending lock before starting a fixture or contacting HTTP. After reviewing the final merged Gateway commit, record its exact 40-character commit and the SHA-256 of both worker source files in the lock, then mark it `reviewed-merged`. The fixture requires that exact clean checkout, verifies that the commit belongs to `refs/heads/main`, and checks the hashes at startup and on each proof-info read. `RELAY_GATEWAY_SOURCE` selects the checkout; `RELAY_GATEWAY_COMMIT` cannot override the lock.

Only in a scheduler-admitted window, use `relay-gateway-proof` in the parent preview configuration to run `scripts/gateway-proof.py` with Gateway's existing virtualenv on loopback 7916. Fixture storage defaults to the existing sibling ATLAS folder `../_evidence/worker-contract-20261109/`; `RELAY_PROOF_ROOT` may select another existing isolated workspace and must match in both processes. Each run retains a fresh `gateway-http-proof-*` directory, disables application lifespan/startup services, and exposes fixture jobs to the real Worker/adapter/API. No signed-in provider or production queue is involved. Never deploy this fixture or expose its control routes.

Set `RELAY_VIDEO_SOURCE` to the exact clean video checkout recorded in the lock and desktop catalogue. The client verifies its commit and HTTP-client hash before importing it, validates the fixture's exact Gateway identity and storage root, and rechecks both sources before writing a receipt. Once the admitted preview is ready, run:

```bash
node test/gateway-http-proof.mjs
```

Set `RELAY_PROOF_RECEIPT` to retain a JSON receipt with both source identities. This is excluded from `npm test`: it requires that fresh disposable server. It validates text/image-key reference bytes, exact result artifact bytes, claim deduplication, silent/ambiguous holds, definite late failure, late result ownership, JSON-safe telemetry, stated zero, hashed token storage and immediate revocation. Tokens stay in process memory; receipts contain only non-secret metadata and hashes.

Gateway's focused `ConcurrencyTests` complement this HTTP proof. Earlier fixture results apply only to their recorded source identities; they do not qualify the pending final merged Gateway. Production behavior, installed binaries and a real extension/provider session remain unverified by these tests.

The connector fragment has its own version and What's New. It ships inside Gateway's release; it is not a separately signed native app, feed or installed package. Gateway owns its native Browser workers pane integration; Relay's fragment is a compatible integration artifact, not evidence that it has been vendored unchanged. Live handshake, telemetry and nonpaid artifact proof remain required after reviewed Gateway deployment.
