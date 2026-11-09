# Relay Studio KH

Relay owns compatibility between the existing browser generators and Gateway Studio KH. This repository contains isolated checks and lead instructions. It does not run a separate bridge or app server.

Story keeps its single Gateway connection. Extensions make outbound worker requests; Gateway owns authentication, job storage, provider availability and returned artifacts. Video comes first, then image and audio using their existing provider adapters.

## Sources and ownership

- Canonical handoff: `/build/Developer/chrome-extensions/_handoffs/2026-11-09-gateway-extension-provider-handoff.md` (reported baseline, not fresh live proof).
- Extension sources: `/build/Developer/chrome-extensions/story-studio-{video,image,audio}-generator`.
- Gateway: `/build/Developer/gateway-kh/repository`, owned by its existing lead.
- Canonical Relay request list: claude-mem `relay-studio-kh-requests` in project `My Own Apps`. Developer's `relay-studio-kh` list is a coordinator pointer to that request.
- Operational evidence: `/build/Developer/_orchestra/relay-lead/`.
- Owner deliverables: `~/Desktop/All YouTube Stuff/Apps/Relay Studio KH/`.

The video checkout contains intentional uncommitted owner work. Read-only use is approved; overlapping edits require an explicit writer handover. Do not stash, reset or commit those files from this repository.

## Isolated checks

Requires Node with `node:test`, fetch, FormData and VM modules; no dependencies. Run:

```bash
npm test
```

Tests load the current extension source read-only. Set `RELAY_VIDEO_SOURCE` to test another checkout. Transport uses an intercepted fetch; background tests use a fresh VM, in-memory Chrome storage and fake Flow responses. There are no live providers, listening ports, Chrome profile access or paid calls. A passing test proves only the named mocked behavior, not a live integration.

`patches/video-reference-recovery.patch` is an unapplied extension patch: retain staged references until a result is accepted, and refuse a missing reference instead of generating a paid replacement. Its tests copy only `src/background.js` into a new folder under `/Volumes/KH-STUDIO-ATLAS/KH-Worktrees/relay-studio-kh/`, apply the patch there with Git, and run the patched text in the VM. Test copies are retained for inspection; they are not Git worktrees or install kits. Set `RELAY_TEST_ROOT` to an existing writable test workspace on another machine. The original source is never patched by these checks.

Writer handover is still required to integrate this patch into the extension. A future integration must rerun the relevant extension checks and normal release process; this repository cannot claim the patch is installed.

There is no installable Relay app yet. Version, What's New, signing and feed requirements apply if a standalone product is explicitly scoped later; integration releases belong to the existing apps.
