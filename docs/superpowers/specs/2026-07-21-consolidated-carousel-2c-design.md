# Consolidated Carousel Overview — Phase 2c (Lens + Focus-Jump) Design

**Date:** 2026-07-21
**Status:** Design — approved; building 2c-render first.
**Builds on:** Phase 1 (config/scoping/state), Phase 2a (card render), Phase 2b (rotating carousel). Feature origin: niri-wm/niri discussion #4337.

## Goal

Complete the carousel: from the screen your eyes are already on, drill into any
output's full window set and **jump focus — and the cursor — to a chosen window
on its real monitor**. This is the "bring the right window forward on the right
monitor" payoff, mouse-driven.

## Interaction (the full flow)

1. Open overview, zoom out → **carousel** (2b): output cards, one centered; rotate
   with `←`/`→` / `Shift`+scroll.
2. Zoom out **further** past a second, deeper threshold → the centered card blooms
   to fill the screen: the **lens** — that output's full overview (all its
   workspaces/windows), big enough to click.
3. **Click a window** in the lens → focus jumps to that window on its **real**
   monitor, the **cursor warps to it**, and the overview closes.

The target output's physical screen stays live throughout — you view and act on it
remotely through the lens on your own screen, never looking away.

## Zoom model — three regimes

A single continuous zoom, gated by two thresholds (`activation-zoom` from Phase 1,
plus a new deeper `expand-zoom`):

| Zoom range | Regime | Render |
|---|---|---|
| `zoom > activation-zoom` (0.25) | single-output overview | host's own overview (unchanged) |
| `expand-zoom < zoom ≤ activation-zoom` | **carousel** (2b) | cards, one centered, rotate |
| `zoom ≤ expand-zoom` (new, default ~0.1) | **lens** | centered output's FULL overview fills the screen |

New config: `overview.consolidated-carousel.expand-zoom` (f64, default `0.1`; must be
`< activation-zoom`). A `Layout::in_carousel_lens()` predicate mirrors
`in_carousel_regime()` but on the deeper threshold.

## Components

### 1. Lens render (Phase **2c-render**)

When `in_carousel_lens()` on the focused output, render the **centered** output's
**full overview** (all workspaces, every window — not just the active workspace like
the 2b card) at a fill-the-screen scale, decoupled from the host's tiny zoom value.
The carousel cards/tucks are not drawn in this regime — just the lens.

- Reuses the overview render path pointed at the target `Monitor` (the centered
  output). Where the 2b card used `render_active_workspace_at_zoom`, the lens needs
  the target's **whole** overview — i.e. a `render_workspaces`-equivalent for a
  *remote* monitor at a chosen fill zoom, scale-normalized (`host_scale /
  target_scale`) into the host screen. No offscreen buffers (carried constraint).
- If centered == host, the lens is just the host's own overview at that zoom
  (degenerate, correct).
- Discrete switch at the `expand-zoom` threshold (carousel → lens); a smooth
  animated bloom is a later refinement, not v1.

### 2. Click-to-jump (Phase **2c-jump** — the hard part)

A pointer click while `in_carousel_lens()` is hit-tested against the rendered target
overview → resolves to a specific window on the target output → focuses it there,
warps the cursor, and closes the overview.

- **Hit-test:** map host-screen click coords → invert the lens transform (the
  scale/relocate that placed the target overview on the host) → target-overview
  coords → the target monitor's existing "window under point in overview" resolution
  → the `Window`/tile. This inverse-transform + remote hit-test is the novel, risky
  piece — de-risk with a small prototype + hardware check (like the 2a scale spike)
  before committing the full path.
- **Jump:** activate the resolved window on the **target** monitor (make it the
  focused window on its output), move the active monitor to the target, close the
  overview (`close_overview` / preserving-zoom variant), and warp the cursor to the
  focused window via the existing `maybe_warp_cursor_to_focus()`.
- **Scope:** mouse-click is the only selection in v1 (no keyboard window-pick).

### 3. Cursor warp

Reuse `maybe_warp_cursor_to_focus()` (already used by the arrow-focus actions) after
the focus jump, so the cursor lands on the focused window on its real output — the
user's explicit requirement.

### 4. Cleanup (folded into 2c)

- Remove `carousel_participants` (host-excluded, from 2a) — unused since 2b switched
  the render to `carousel_outputs`.
- Re-center the carousel index (`reset_carousel_center` / clamp) on active-output or
  output-set changes while the overview is open (the 2b final-review deferral), so
  the centered index can't silently point at a shifted output.

## Non-goals (v1)

- Keyboard window selection in the lens (mouse-click only).
- Animated bloom transition at the `expand-zoom` threshold (discrete switch is fine).
- Dragging windows between outputs from the lens.

## Risks

1. **Remote click hit-test (the main spike).** Inverting the lens transform and
   reusing the target monitor's overview hit-test from a different output's render
   pass is unproven. Prototype it and hardware-verify before building the full jump.
2. **Cross-output focus + cursor warp coherence.** Focusing a window on a non-active
   output and warping the cursor there must leave niri in a consistent active-output
   state; re-invoke `set_monitors_overview_state` after the active-output change.
3. **Redraw dependency.** The lens renders the target's live overview on the host —
   the host must redraw when the target's content changes (carried from earlier
   findings; v1 may accept host-cadence updates).

## Testing

- **Config parse** (`insta`/focused): `expand-zoom` parses, defaults to `0.1`,
  and validation that `expand-zoom < activation-zoom`.
- **`in_carousel_lens()`** unit test: true only when consolidated + overview open +
  `zoom ≤ expand-zoom`; false in the carousel band and above.
- **Hit-test math** unit test (2c-jump): a known lens transform + click point →
  expected target-overview coordinate (pure inverse-transform, testable without a
  renderer).
- **Hardware (user):** lens fills the screen with the centered output's full
  overview (2c-render); clicking a window jumps focus + cursor to it on the real
  monitor and closes the overview (2c-jump); target's physical screen stays live; no
  nvtop spike.

## Build order

1. **2c-render** — `expand-zoom` config + `in_carousel_lens()` + render the centered
   output's full overview filling the screen. Hardware-verify the lens *before*
   wiring clicks. (This spec's immediate build target.)
2. **2c-jump** — click hit-test prototype/spike → focus + cursor warp + close.
3. **Cleanup** — remove `carousel_participants`; re-center on active-output change.
