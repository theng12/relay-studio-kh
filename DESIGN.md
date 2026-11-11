---
name: Relay Studio KH
description: Connection register for browser generator extensions on Gateway Studio KH
colors:
  ground-light: "#f3f5f8"
  surface-light: "#ffffff"
  text-light: "#202a38"
  muted-light: "#566474"
  line-light: "#d8dee7"
  accent-light: "#235cc2"
  accent-soft-light: "#edf3ff"
  ok-light: "#226346"
  warn-light: "#82561b"
  bad-light: "#a32630"
  ground-dark: "#151b24"
  surface-dark: "#1e2733"
  text-dark: "#e7edf6"
  muted-dark: "#afbdce"
  line-dark: "#3a4759"
  accent-dark: "#9bbfff"
  accent-soft-dark: "#273a56"
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
    padding: "8px 16px"
  button-primary:
    backgroundColor: "{colors.accent-light}"
    textColor: "#ffffff"
    rounded: "{rounded.control}"
    padding: "8px 16px"
  button-danger:
    textColor: "{colors.bad-light}"
    rounded: "{rounded.control}"
    padding: "8px 16px"
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

Restrained blue/slate palette with complete light and dark variants. The accent is used sparingly on interactive elements; semantic colors (ok, warn, bad) appear only in state indicators and error boundaries.

### Primary
- **Gateway Blue** (#235cc2 light / #9bbfff dark): Links, focus rings, primary buttons, selection highlight, and the soft background tint for notices. The single interactive accent across the entire surface.

### Neutral
- **Ground** (#f3f5f8 light / #151b24 dark): Page background outside cards and dialogs.
- **Surface** (#ffffff light / #1e2733 dark): Cards, dialogs, inputs, and button backgrounds. The working plane.
- **Text** (#202a38 light / #e7edf6 dark): Primary readable content. Headings, body, labels.
- **Muted** (#566474 light / #afbdce dark): Secondary text, timestamps, captions, disabled states. Never used for primary actions.
- **Line** (#d8dee7 light / #3a4759 dark): Borders, dividers, input strokes. The structural grid.

### Semantic
- **OK** (#226346 light / #91d9b0 dark): Connected transport. This does not assert provider readiness.
- **Warn** (#82561b light / #e8c18c dark): Offline or needs attention. State dot and label only.
- **Bad** (#a32630 light / #ffb2b9 dark): Failed, stopped, error boundary. State dot, error text, danger button border.

### Named Rules
**The One Accent Rule.** Gateway Blue is the only non-semantic color used for interaction. No second accent, no gradient, no decorative color. If something is clickable or focused, it uses this blue or its soft variant. Everything else is neutral or semantic.

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

A fixed 220px sidebar beside one scrolling content pane, the same shell as Gateway and Studio Hub. The sidebar carries the app name, two row groups (Accounts: Accounts, Browsers — Setup: Memory & pacing, Gateway, Extensions) and a footer with the version and Quit. The selected row is marked with the accent-soft tint and `aria-current="page"`. The content pane shows one page at a time, titled with the row that is selected, with 28px page padding on every side and no centred max-width column: the pane itself is the page, and the ground fills the whole window at any size.

Spacing uses the shared KH scale exclusively, applied through CSS custom properties set on the `.relay-connector` scope by the build script; `html` and `body` repeat the documented ground hexes so wide and fullscreen windows never show white bands.

The window opens at 1040×760 and cannot be made smaller than 860×560. The account list uses a four-column grid (identity, state, credits, action) that reflows below 980px to identity and action above state, with the column headings hidden. Dialogs are centered overlays capped at 480px width.

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
- **Shape:** Gently rounded corners (6px radius), minimum touch target 44px height.
- **Default:** Surface background, text color, line border. Hover shifts to accent-soft background with accent border.
- **Primary:** Gateway Blue background, white text, matching border. Hover darkens 15% via color-mix. Dark mode primary uses dark text (#152440) on the lighter accent.
- **Danger:** Bad color text and border, Surface background. For destructive confirmation only.
- **Text:** Transparent border and background, accent color text, no padding-right. For low-emphasis inline actions (version link).
- **Focus:** 2px solid accent outline, 3px offset. Applied to buttons, links, summaries, and inputs via :focus-visible.
- **Disabled:** 58% opacity, default cursor.

### Inputs
- **Style:** Full width, 44px minimum height, 12px padding, 16px font size (prevents iOS zoom). Surface background, line border, control radius.
- **Focus:** 2px solid accent outline, 3px offset. Accent caret color.
- **Readonly:** Ground background to signal non-editable state (token display, gateway address).

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

### Browsers Pane
- **Register:** One card per resident or queued profile: number and label, state, PID, measured memory, provider, activity, and the queued or close-waiting reason. Only real Chrome processes Relay opened appear; the pane never draws a simulated browser.
- **Actions:** Focus activates the exact owned window, Close asks Gateway for a safe stop, and Force close opens a confirmation that names the lost tabs and Chrome's session restore. A per-row and bulk Force close is blocked with an explicit reason whenever Gateway reports active or uncertain work.
- **Waiting:** A pending close shows its elapsed wait and Gateway's reason on the row, so a refused SIGTERM never looks like a silent success.
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