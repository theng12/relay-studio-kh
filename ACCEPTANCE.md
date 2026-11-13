# Relay acceptance — Accounts page and extension panel (0.3.24)

Mandatory before the page gate: a task list with one end-to-end test per task, a capability
cross-check against live data, and a standard-practice comparison naming its reference and
listing what is present, deliberately absent, and missing. Automated tests run in the gate;
anything marked **live** needs an installed build and the owner's window slot.

## 1. Task list and end-to-end tests

| Task | End-to-end steps | Observable result | Test | Status |
|---|---|---|---|---|
| Open a closed account | Press the row's primary **Open** | Chrome opens that profile; primary becomes **Focus**; row shows **Open**, memory and the seeding line | `desktop` fixture click (render gate); `manager_open_profiles` path in Rust tests; `close_step` suite | Live pending (lane + owner) |
| Focus an open account | Press **Focus** or click the row | The same window comes forward; no second profile starts | Fixture click; pid identity tests | Live pending |
| Close an account without Force | Press **Close** | Row says **Closing**, then **Closed**; no `Waiting to close` beyond 20 s; paid work blocks it | `close_step` unit test; fixture Close click | Live pending |
| Open during a close | Press **Open** while Closing | Intent is saved; the account opens when the old process is gone | Source path `desired_open` + reconcile launch | Live pending |
| Seeding after open | Open an account whose worker is bound | Row ends with "connection verified in the browser"; extension storage holds URL/token/worker/mode | `seeding` Rust tests (write/read shapes), `seed_and_verify` path | Live pending (needs real Chrome) |
| Extension connected state | Open the side panel after Relay seeds | Panel says "Connected through Relay · <machine>", fields behind Details, no prompt | Image/video/audio source tests; contrast test | Live pending |
| Switch layout | Choose Compact / Card / Table | Layout changes at once; choice survives restart | `accounts.test.mjs` view assertions; Rust view tests | Render gate |
| Row overflow | Press ⋯ | Menu opens with Show details and Delete; Escape or outside click closes it | Source test (`row-menu`) | Render gate |
| Grouped warnings | Have two accounts missing a worker ID | One chip "2 accounts missing a worker ID" above the list | Source test (`warning-group`) | Render gate |
| Search and select | Type a label or number; All/None/range | List filters with an honest no-match message; selection counts match | Existing accounts tests | Render gate |
| Rename in place | Click the name, type, Enter | Saved via `manager_update_profile`; Escape cancels | Existing rename tests | Live pending |
| Delete permanently | ⋯ → Delete, confirm in Relay's dialog | Row disappears; banner names the account; no native dialog | Delete dialog tests | Live pending |
| Start at Login | Toggle the setting in an installed build | macOS lists Relay as a login item; the old raw agent is gone | `login_item` tests (decision logic) | Live pending (signed build only) |

## 2. Capability cross-check from live data

| Capability | Source of truth | Read by | Status |
|---|---|---|---|
| Browser process state | `ps` start time + retained spawn lease | `reconcile` (1 s with the page open) | Verified from live process data; not exercised in this gate |
| Extension connection | Chrome `Extensions.getStorageItems` after seeding | Relay seeding read-back | Code path tested; live read-back pending |
| Extension readiness | `gatewayReadiness` object written by the worker each tick | Panel and Relay row | Source tests; live pending |
| Gateway worker state | `/api/workers/manager` `connectionState` (G1, 114af12) | Row label via manager token | Mapping tested; live pending |
| Gateway account/credits | `/api/workers/manager` accounts | Credits line | Existing tests; live pending |
| Memory budget | Host memory probe | Admission and Memory page | Existing tests |
| Flow project | The open Flow tab's URL (no page probe) | Account claim | Extension tests; live pending |

No fixture data is presented as live: the desktop preview labels itself as a fixture, and
missing reads show as unknown, never as zero.

## 3. Standard-practice comparison

Reference: **Apple Human Interface Guidelines — Lists and Tables, Toolbars, and Menus**
(macOS), cross-checked against Chrome's profile picker for account rows.

Present:
- A table view with a fixed header and aligned columns, plus compact and card rows; the
  layout is remembered (HIG: view state persists).
- Exactly one primary action per row; secondary Close; destructive and disclosure actions
  in a ⋯ menu that closes on Escape or outside click (HIG: menus, one default action).
- Checkbox selection for bulk actions, with counts; status text carries state, not colour
  alone; contrast measured at ≥4.5:1 body text in both appearances.
- Rows are in the tab order; Enter or Space runs the row's primary action, and keys inside
  the row's controls are never intercepted.
- Warnings grouped with counts instead of repeated per row.

Deliberately absent:
- Column sorting and per-column filtering: no live need on a list this size, and each
  extra control would compete with the row's primary action.
- Inline editing of worker IDs in the table: Details owns that form so a stray keystroke
  cannot rebind a running account.
- Hover-only actions: banned by owner rule; every control is drawn.

Missing (known, backlog):
- A true per-field column table (Account / State / Connection / Credits / Memory as
  separate cells); today's Table is an aligned dense layout over the same row.
- Cell-to-cell keyboard navigation inside the table (rows themselves are reachable and act).
- (Closed) The earlier `browsers.mjs` tone note was stale: the file has imported
  `processTier` from accounts.mjs since 9cb673a; a regression test now pins it.
