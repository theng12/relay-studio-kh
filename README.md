# Relay Studio KH

Relay connects the existing video, image and audio browser generators to Gateway Studio KH. The connector app is a pane in Gateway's existing Providers screen. This repository owns that plain HTML/CSS/JavaScript fragment and isolated extension compatibility checks; Gateway owns authentication, the worker registry, queue, availability, artifacts and deployment. No separate production service is required.

Each extension keeps Story Studio and standalone modes and gains Gateway Studio KH as its third mode. Video integration comes first. Story keeps one Gateway endpoint. A built pane is not evidence that all three extension modes are installed or live.

## Mac desktop source candidate

The owner now prefers a persistent Mac app; Windows native support can wait. `src-tauri/` prepares a Tauri 2/Rust shell without adding a queue, backend server or extension runtime. This source candidate is not compiled, packaged, installed or qualified. Main portable 0.1.1 remains source/offline qualified only until the candidate is reviewed and integrated.

Relay's local settings window uses the existing system-font styles and shared What's New. Its Gateway window opens the existing `/admin#extension-workers` interface as a same-origin top-level page. Sign in using Gateway's ordinary browser password; the app does not read, inject or persist admin/owner/worker credentials. The webview has its own browser session, not the owner's Chrome profile. Gateway alone decides which management actions are authorized. No extra local-app privileges are claimed for this webview, and remote content receives no native settings capability.

Start at Login is initially on and can be changed. Saved opt-out is never replaced by the first-launch default. OS registration/readback failures are errors, not success. URL and login preference are stored in Relay's app-config folder using replacement of a temporary file; unreadable settings are held rather than reset. Normal OS cookie persistence and restart/login behavior still require native proof. Closing settings hides that window; the Gateway window closes independently. The Dock and a second launch restore settings. The native menu or Quit Relay exits. A second instance focuses the existing app. Health checks can reopen the existing Gateway page after recovery; they never claim, generate, fail, upload, retry or settle worker jobs. Possibly charged work remains held at Gateway/extensions.

Chrome MV3 extensions continue running externally in Chrome. A Tauri webview cannot run them. Browser/provider switches are never enabled by this shell. The Gateway address is an explicit saved HTTP(S) origin without a path, embedded credentials, query or fragment; no production port/address is guessed.

Cheap web assets/static checks (no Rust/native build):

```bash
npm run desktop:assets
```

```bash
node --test test/desktop.test.mjs
```

For source UI inspection, `relay-desktop-source-preview` serves these assets on loopback 7920 with a visibly labeled, in-memory fake native bridge. It tests browser rendering and UI wiring only, never OS login settings, file persistence, real Gateway connectivity, Tauri permissions or WKWebView behavior. Never deploy `scripts/desktop-preview.mjs` or include its bridge in the native assets.

The native qualification window additionally needs the Tauri 2 CLI, Rust toolchain and Apple build tools. Cargo dependency resolution/lock, compilation, WKWebView rendering/session persistence, login registration and opt-out/restart/single-instance/Quit behavior have NOT been verified. Native bundling stays disabled until the normal coordinated packaging step; it must use the existing KH Studio Local Signing identity, approved app assets and normal release/update discipline. Do not infer a working native app or Windows compatibility from static checks or mocked recovery tests. No native build/package/install is permitted in the current source-preparation window.

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

`relay-gateway-proof` in the parent preview configuration runs `scripts/gateway-proof.py` with Gateway's existing virtualenv on loopback 7916. The fixture pins corrected Gateway commit `5b0ed72`, isolates every Gateway path in a new retained folder under `_orchestra/relay-lead/worker-contract-20261109/`, disables application lifespan/startup services, and exposes only fixture jobs to the real Worker/adapter/API. No signed-in provider or production queue is involved. Never deploy this fixture script or expose its control routes.

Start that named preview and wait for its ready log. Then run the opt-in client proof:

```bash
node test/gateway-http-proof.mjs
```

Set `RELAY_PROOF_RECEIPT` to retain a JSON receipt. This is excluded from `npm test`: it requires that fresh disposable server. It imports the original extension's real HTTP client, validates text/image-key reference bytes, exact result artifact bytes, claim deduplication, silent/ambiguous holds, definite late failure, late result ownership, JSON-safe telemetry, stated zero, hashed token storage and immediate revocation. Tokens stay in process memory; receipts contain only non-secret metadata and hashes. The known-bad `c0973d7` baseline is refused. Review a new candidate before changing the pins; `RELAY_GATEWAY_SOURCE` and `RELAY_GATEWAY_COMMIT` on the fixture select a separately reviewed source checkout, and the client pin must be updated to match.

The deterministic lease/result and first-terminal-answer race checks live in Gateway's focused `ConcurrencyTests` suite; they complement this HTTP proof. The first external-drive-backed fixture startup stalled before listening and is retained as a failed attempt; a small local isolated instance completed the proof. Production behavior, installed binaries and a real extension/provider session remain unverified by these tests.

The connector fragment has its own version and What's New. It ships inside Gateway's release; it is not a separately signed native app, feed or installed package. Gateway owns its native Browser workers pane integration; Relay's fragment is a compatible integration artifact, not evidence that it has been vendored unchanged. Live handshake, telemetry and nonpaid artifact proof remain required after reviewed Gateway deployment.
