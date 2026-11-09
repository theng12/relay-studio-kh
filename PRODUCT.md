# Relay Studio KH

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Delegated by the owner through Developer on 2026-11-09: routine architecture and implementation choices. Initial surface uses plain HTML, CSS and browser JavaScript, hosted by the existing Gateway. No separate queue, daemon or frontend dependency.

## Users

The owner connects their three existing browser-session generator extensions to This Mac's Gateway and checks whether each is connected and ready.

## Product Purpose

Provide a usable connector app for video, image and audio extensions, each adding Gateway Studio KH beside its existing Story Studio and standalone modes. Story keeps one Gateway endpoint.

## Operating Context

Gateway owns worker tokens, registry, queue, availability and artifacts. Extensions poll outbound and drive their signed-in browser sessions. Relay manages pairing and reads reported worker state. Live access through This Mac's existing LAN/Tailscale is authorized; 1301 activation, paid captures/generation, public publishing and unrelated OS/security changes remain excluded.

## Capabilities and Constraints

Initial connector surface: one extension list, connection details, worker pairing/revocation, accurate capabilities and credit readback, errors with recovery, version and What's New. Management routes must be agreed with Gateway lead; fixture data is explicitly labeled until integrated. No balance, price, readiness or model key is guessed. Tokens are shown once, never persisted in browser storage or written to logs.

Original video checkout remains read-only pending verified writer handover. Relay owns unapplied recovery/credit patches and isolated compatibility proof. Existing image/audio provider boundaries remain intact.

## Brand Commitments

Name: Relay Studio KH. Shared KH UI conventions, plain words, fewer controls and one list. No existing icon is replaced. No marketing claims or invented production success.

## Evidence on Hand

Owned main62d7191 has isolated patch proof. Existing video1.3.1, image0.50.9 and audio0.7.26 inventories checked. Production app and new worker routes are not yet deployed. Captures contain sensitive remnants and are not app assets or fixtures.

## Product Principles

- Show unknown and not checked honestly.
- Make pairing deliberate and recoverable through revocation.
- Keep one Gateway transport/store.
- Preserve existing extension modes and paid unknown-outcome holds.

## Open Implementation Facts

Gateway's existing Providers pane hosts the connector fragment. Its management contract uses GET/POST/DELETE /admin/api/extension-workers, snake_case readback and null for unknown credit data. Mint/revoke retain local-app-only authorization; signed-in browsers can read. Relay delivers a pinned fragment/bundle, and the Gateway lead owns integration, normal release and This Mac deployment. The fixture preview is not an installed native package.
