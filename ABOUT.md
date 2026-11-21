# Relay Studio KH — what it is

Relay Studio KH manages saved Chrome accounts for the owner's browser image and
video generators and connects their workers to Gateway Studio KH. It ships both as
a connector pane inside Gateway's Providers screen and as a signed Mac desktop app.

## What it does

- Pairs browser workers with Gateway through a one-time scoped manager token held
  in a 0600 file in Relay's own app-data folder; it never extracts admin credentials
  or Chrome cookies.
- Reads worker state, balances and control routes from Gateway's existing
  extension-worker API; unknown balances stay unknown, and Gateway alone authorizes
  writes.
- Offers a local account register with numbered Chrome profiles, bulk and range
  selection, open/close actions and an attention action for an exact profile.
- Keeps a memory budget, reserve and optional active-process ceiling for browser
  admission; there is no arbitrary saved-profile count cap.
- Waits for fresh Gateway pause/control revision and a safe-close acknowledgement
  before closing a browser; uncertain or held states stay pending.
- Applies pacing with a 5–15 second default between generations, and requires a
  worker readback before claiming a saved range took effect. Browser TTS is off and
  optional.
- Prepares reviewed image and video extension payloads for the user to load
  unpacked in Chrome; audio is owner-skipped, off and unavailable. No extension
  source is vendored here.
- Ships the Mac Tauri 2 desktop app — bundle id `kh.relay.desktop`, signed with the shared
  `KH Studio Local Signing` identity — with a settings window, a Gateway window, an update hold
  on fixed loopback port 8648 (per the update-hold contract), start-at-login and
  single-instance handling. It is published to the signed 1301 feed as a signed DMG.
- Keeps fixture previews and HTTP proofs explicitly labeled as development only,
  never as production installation or provider readiness.

## How it is built

Node build and connector assets (`desktop/`, `scripts/`), web UI and fixtures
(`web/`, `test/`), and the Tauri 2/Rust app under `src-tauri/`. Python scripts help
prepare icons and release assets. Every release is gated by
`_orchestra/tools/gate-run.sh _orchestra/tools/gate-configs/relay.sh`, which exports
`KH_SHARED_UI` and `RELAY_EXTENSION_REPOS` — build paths are absolute, never resolved
relative to this checkout.

Build the Gateway connector fragment with:

    npm run build

## Where it fits

There is no separate Relay production service. The connector is mounted in Gateway's
Providers pane, and Gateway owns authentication, jobs, models, credits,
reservations, artifacts and deployment. Chrome MV3 extensions run in the user's
browser; a Tauri webview cannot run them. Gateway's lead owns integration and
release of the fragment.

## Current version

0.3.36, from the root `VERSION` file, which the connector build reads; the newest
`CHANGELOG.md` entry agrees. `package.json` has no version field. 0.3.36 is on the
signed 1301 feed (sha256 4689e8b8…0da4, read back 2026-11-22); installs go through
Studio Launcher. Release receipts live in `_orchestra/developer-media/relay-studio-kh/lead/`.
