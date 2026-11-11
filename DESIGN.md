---
name: Relay Studio KH
description: Connection register for browser generator extensions on Gateway Studio KH
colors:
  ground-light: "#f5f3fa"
  surface-light: "#ffffff"
  text-light: "#1f1b2e"
  muted-light: "#5f5a70"
  line-light: "#ddd8e8"
  accent-light: "#6d28d9"
  accent-soft-light: "#f1eafe"
  ok-light: "#226346"
  warn-light: "#82561b"
  bad-light: "#a32630"
  ground-dark: "#14121c"
  surface-dark: "#1e1b2a"
  text-dark: "#ece9f5"
  muted-dark: "#b3aec4"
  line-dark: "#3a3450"
  accent-dark: "#c4b5fd"
  accent-soft-dark: "#2b2440"
  ok-dark: "#91d9b0"
  warn-dark: "#e8c18c"
  bad-dark: "#ffb2b9"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "13px"
    fontWeight: 400
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  control: "6px"
  card: "8px"
  sheet: "12px"
spacing:
  xs: "4px"
  tight: "6px"
  s: "8px"
  m: "12px"
  l: "16px"
  xl: "24px"
  page: "28px"
components:
  button-default:
    backgroundColor: "{colors.surface-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.control}"
    padding: "6px 12px"
  button-primary:
    backgroundColor: "{colors.accent-light}"
    textColor: "#ffffff"
    rounded: "{rounded.control}"
    padding: "6px 12px"
  button-danger:
    textColor: "{colors.bad-light}"
    rounded: "{rounded.control}"
    padding: "6px 12px"
  input-field:
    backgroundColor: "{colors.surface-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.control}"
    padding: "12px"
---

# Design System: Relay Studio KH

## Overview

**Creative North Star: "The Connection Register"**

A flat operator surface for three browser extensions. The design is a direct expression of shared KH conventions: system sans type, cool white/slate surfaces, one restrained blue accent, and honest states that never pretend to know what they do not. No imagery, no charts, no decorative form — just a list that shows what is connected, what it reports, and what needs attention.

Light mode is the desktop working default; dark mode follows `prefers-color-scheme` with its own complete palette. Every token comes from either the shared KH design-tokens.json (spacing, radii, type scale) or the connector's own app.css (colors). Nothing is invented.

**Key Characteristics:**
- Flat list layout with expandable details in place
- Shared KH spacing and radius scale, applied through scoped CSS custom properties
- System font stack only; no web fonts
- One blue accent for interactive elements; semantic colors for state only
- Light and dark palettes are both first-class, not afterthoughts

## Colors

Restrained iris/slate palette with complete light and dark variants. Relay's own iris accent replaces the shared Gateway blue; semantic colors (ok, warn, bad) appear only in state indicators and error boundaries.

### Primary
- **Iris** (#6d28d9 light / #c4b5fd dark): Links, focus rings, primary buttons, the selected sidebar row and the soft background tint for notices. The single interactive accent across the entire surface.

### Neutral
- **Ground** (#f5f3fa light / #14121c dark): Page background outside cards and dialogs, with a faint iris cast.
- **Surface** (#ffffff light / #1e1b2a dark): Cards, dialogs, inputs, and button backgrounds. The working plane.
- **Text** (#1f1b2e light / #ece9f5 dark): Primary readable content. Headings, body, labels.
- **Muted** (#5f5a70 light / #b3aec4 dark): Secondary text, timestamps, captions, disabled states. Never used for primary actions.
- **Line** (#ddd8e8 light / #3a3450 dark): Borders, dividers, input strokes. The structural grid.

### Semantic
- **OK** (#226346 light / #91d9b0 dark): Connected transport. This does not assert provider readiness.
- **Warn** (#82561b light / #e8c18c dark): Offline or needs attention. State dot and label only.
- **Bad** (#a32630 light / #ffb2b9 dark): Failed, stopped, error boundary. State dot, error text, danger button border.

### Named Rules
**The One Accent Rule.** Iris is the only non-semantic color used for interaction. No second accent, no gradient, no decorative color. If something is clickable or focused, it uses this iris or its soft variant. Everything else is neutral or semantic.

## Typography

**Body Font:** System sans stack (-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)
**Mono Font:** System mono stack (ui-monospace, SFMono-Regular, Consolas, monospace)

**Character:** Native platform rendering at small sizes. No web fonts, no display faces. The type scale comes directly from shared KH design-tokens.json and is applied as CSS custom properties by the build script.

### Hierarchy
- **Page Title** (semibold, 26px, letter-spacing -0.02em): App identity in the header. Single instance per surface.
- **Section** (semibold, 15px): List headings, dialog titles, detail section labels.
- **Credit Value** (regular, 13px): Reported balance in the connection list; worker names use Section sizing.
- **Body** (regular, 13px, line-height 1.6): Descriptions, help text, dialog paragraphs. Max width 65-75ch in dialogs.
- **Secondary** (regular, 12px): Subtitles, status labels, readback text, gateway URL.
- **Caption** (regular, 11px): Credit labels, footer text, version badge.
- **Mono** (regular, 12px): Gateway URLs in code elements. Worker IDs and token inputs retain the system sans stack.

### Named Rules
**The System Type Rule.** All typography uses the platform's native font stack. No @font-face, no Google Fonts, no variable font axes. Sizes follow the shared token scale, with 16px form inputs for mobile usability.

## Layout

A fixed 220px sidebar beside one content pane, the same shell as Gateway and Studio Hub. The sidebar carries the app mark and name, then Overview, Browsers (Accounts, Open windows, Memory & pacing), Connection (Gateway, Extensions) and App (Settings). Every row and the footer pill carry one leading icon from a single stroke set. Group labels are 11px muted capitals; rows are 32px with an 8px radius and 8px gaps. The selected row uses the accent-soft tint, accent text, a 2px leading accent rule and `aria-current="page"`. The footer is one centred pill with clickable What’s New and static `v<version>` text; the version is never a link, and Quit lives only in the app menu.

Each row is its own page with its own scroll position: Overview opens as the dashboard (saved accounts, browsers open, memory used/budget/available, Gateway connection state, up to five attention items and quick links), and Accounts, Open windows, Memory & pacing, Gateway, Extensions and Settings each keep their place. Pages use 28px padding on every side and stretch to the pane: no centred max-width column, no wide gutters. Cards and lists fill the width; prose inside cards caps at about 75ch. The ground fills the whole window at any size, and each page scrolls independently rather than the shell moving as one long page.

Spacing uses the shared KH scale exclusively, applied through CSS custom properties set on the `.relay-connector` scope by the build script; `html` and `body` repeat the documented ground hexes so wide and fullscreen windows never show white bands.

The window opens at 1100×760 and cannot be made smaller than 860×560. Relay remembers its last frame and restores that size and position on the next launch. The account list uses a four-column grid (identity, state, credits, action) that reflows below 980px to identity and action above state, with the column headings hidden. Dialogs are centered overlays capped at 460px width.

The spacing system is app-wide: body 13/1.6, help 12/1.55 (max 68ch), caption 11/1.5; label 12 semibold → 6px → 36px control (10px 12px padding, 13px text) → 8px → help; field to field 20px; card padding 24px with 16px title-to-content and 24px between groups; button rows sit 16px above with 8px gaps; checkbox to label 8px; paragraphs 12px apart and groups 16px apart. One set of shared `field` and form rules provides it.

Responsive behavior below 980px: page padding drops to 16px, account rows reflow to a two-line layout, browser bulk actions wrap under the selection row, and definition lists go single-column.

## Elevation & Depth

Flat by design. No box-shadows anywhere in the implemented surface. Depth is conveyed through surface color contrast (ground vs. surface), border lines, and the native dialog backdrop (#101b3480). The only animation is the native dialog appearance (translate-y 6px over 160ms), gated behind `prefers-reduced-motion: no-preference`.

## Shapes

Three radius values from the shared KH token scale, applied consistently:
- **Control** (6px): Buttons, inputs, notices, focus outlines.
- **Card** (8px): Worker list container.
- **Sheet** (12px): Dialogs.

All borders are 1px solid using the Line color. No clipping, no rounded-corner asymmetry, no decorative geometry.

## Components

### Buttons
- **Shape:** 32px height (30px minimum), 0 12px padding, 6px radius, 13px/600, inline-flex with 8px group gaps. No full-width bars.
- **Hierarchy:** One primary action per group (Iris background, contrast text; dark mode uses dark text on the lighter iris). Everything else is secondary (Surface background, text color, line border) or ghost (transparent border and background, muted text, for quiet inline actions such as Reconnect, Show folder and Copy path). Danger uses Bad text on the Surface background.
- **Focus:** 2px solid accent outline, 2px offset. Applied via :focus-visible.
- **Disabled:** 50% opacity, default cursor.

### Inputs
- **Style:** Full width, 36px control height, 10px 12px padding, 13px text. Surface background, line border, 6px radius.
- **Focus:** 2px solid accent outline, 2px offset. Accent caret color.
- **Readonly:** Ground background to signal non-editable state (token display, gateway address).

### Appearance
Settings offers System, Light, Dark and Default. System follows macOS; Default is Relay’s designed iris look (light); all four apply only inside Relay and never change macOS. The choice is saved and restored on launch. New installs start on System.

### Connection List
- **Container:** Card-radius (8px) border, surface background, overflow hidden. Workers separated by 1px line dividers.
- **Row:** Four-column grid with generous padding (24px). Identity column carries title (semibold 15px) and subtitle (muted 12px). State column shows a colored dot (6px circle via ::before pseudo-element) plus label. Credits column stacks caption label above tabular-nums value. Action column holds the context-appropriate button.
- **Expandable Details:** Native `<details>` element. Summary styled as accent-colored semibold text. Definition list uses a two-column grid (144px label column) capped at 720px. Model list uses standard ul with left padding.

### Native Browser Accounts
- **Register:** Extend the connection list into stable numbered saved profiles. Selection and range tools sit above the list; saved entries and resident browsers remain distinct.
- **Readouts:** Keep measured memory, budget and queue reason visible above the register. Each row shows local process/intent, independent connection/readiness and activity, and reported credits or an explicit unknown/stale label.
- **Details:** Provider identity, credit source/time/reset/buckets and image/video worker bindings expand inline. Memory/pacing preferences, Gateway pairing and extension setup disclose progressively.
- **Actions:** Open/Close use exact saved IDs; attention exposes Show profile for the same managed Chrome profile. Pending close remains visible until authoritative safe-stop readback. Pairing uses a concealed one-time input cleared after either outcome.
- **Identity:** Reuse the existing palette, system type, spacing and flat hierarchy. The desktop account grid adds a selection column and collapses at the existing 680px breakpoint. Fixture screenshots prove rendering only, never native lifecycle or account readiness.

### What’s New Page
- **Surface:** A full page in the content pane, not a dialog: version sections stacked newest first, each with a heading and a roomy bulleted list in one card. The footer pill holds a clickable What’s New entry and static `v<version>` text separated by a middle dot; the version is never a link and has no hover or click handler.

### Browsers Pane
- **Register:** One card per resident or queued profile: number and label, state, PID, measured memory, provider, activity, and the queued or close-waiting reason. Only real Chrome processes Relay opened appear; the pane never draws a simulated browser.
- **Actions:** Focus activates the exact owned window. Close stops an unbound profile locally and immediately; a bound profile asks Gateway only about the workers it actually has, for at most five seconds. Force close opens a confirmation that names the lost tabs and Chrome's session restore, verifies the exact PID, start time and binary, then SIGKILLs only that process. It is blocked only while Gateway explicitly reports active or uncertain paid work.
- **Waiting:** A pending close shows its elapsed time and reason on the row and refreshes every 500 ms. After three seconds alive it reads “Chrome did not exit” and offers Force close, so a refused SIGTERM never looks like a silent success.
- **Empty state:** "No browsers open." when nothing is resident or queued.

### State Indicators
- **Style:** Inline-flex row with 6px colored dot and semibold 12px label. Connection labels use ok (green), warn (amber), and off/info (muted). Error text and danger controls use bad (red).
- **Behavior:** Dot and label share the same color. No background pill, no border, no icon.

### Dialogs
- **Shape:** Sheet radius (12px), surface background, line border. Centered with min(480px, calc(100% - 32px)) width.
- **Backdrop:** Semi-transparent dark overlay (#101b3480).
- **Animation:** Translate-y 6px entrance over 160ms with cubic-bezier(.16,1,.3,1), only when reduced motion is not preferred.
- **Internal Layout:** Flex header with space-between alignment. Muted paragraph text capped at 65ch. Actions flex-end with wrap and gap.

## Do's and Don'ts

### Do:
- **Do** use the shared KH spacing scale for layout rhythm; fixed viewport safety margins and focus offsets remain literal in the implementation.
- **Do** keep the connection list flat. Expandable details stay inline; never create a separate detail page or modal for worker information.
- **Do** show unknown honestly. "Not reported", "Unknown", "Time unavailable" are valid display states. Never substitute zero, empty string, or a guessed value.
- **Do** use semantic colors only for state. OK/warn/bad appear in state dots, error text, and danger buttons. They never decorate headers, backgrounds, or non-state elements.

### Don't:
- **Don't** add shadows, gradients, or decorative elevation. The surface is intentionally flat; depth comes from color contrast and borders.
- **Don't** introduce a second accent color. Gateway Blue is the sole interactive color. Charts, illustrations, and multi-color encoding are out of scope.
- **Don't** use web fonts or custom typefaces. The system font stack is a deliberate choice for native rendering quality and zero load cost.
- **Don't** animate beyond the dialog entrance. No hover transitions, no loading spinners, no progress bars. Loading states use text ("Reading connections...") and aria-busy.
- **Don't** invent token values outside the shared scale. If a spacing or size need falls between existing steps, use the next larger step rather than creating an intermediate value.