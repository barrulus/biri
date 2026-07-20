# Consolidated Carousel Overview — Design

**Date:** 2026-07-20
**Status:** Design — pending implementation plan
**Origin:** niri-wm/niri discussion #4337 ("view multiple monitors in a single overview")

## Problem

niri renders each output independently and keeps monitors as separate render
surfaces. There is no way to see, and act on, several monitors' windows from a
single screen. The discussion request was to "zoom out and expose additional
monitors on one screen."

The maintainer (YaLTeR) declined the naive form for a concrete reason:

> "Monitors in niri are separate because the workspaces can be infinitely long
> and windows shouldn't overflow onto the next monitor."

If you drew two monitors' workspaces side-by-side in one view, one monitor's
infinitely-scrolling workspace would visually overflow into its neighbour's
territory. Any design must keep each monitor's content **contained** even when
drawn together.

## Goal

Let the user, from the screen their eyes are already on, glance at every
monitor's windows and quickly **jump focus to a specific window on another
monitor** — without physically looking away, and without the sibling monitors
changing at all.

Non-goals (explicitly out of scope for v1):

- Dragging windows/workspaces between monitors in the consolidated view.
- Any change to the sibling **physical** monitors — they stay fully live.
- True 3D (perspective-warped) Cover Flow rendering.
- An explicit per-output opt-in list, or a hard cap on output count.

## Behaviour

### The focused output is a lens

When the consolidated mode is enabled and the user opens the overview, **only
the focused output enters overview**. Every other physical monitor keeps
showing its normal live desktop — zero changes. The focused output acts as a
*lens*: it can display its own overview, a carousel of all outputs, or a remote
output's overview, but the other screens never move.

This is the one behavioural change to niri's existing overview, which today is
global (all monitors zoom together). It is gated behind config (see below); with
the option off, niri's default global overview is unchanged.

### One continuous zoom, two regimes

A single, continuous zoom gesture drives everything. A configurable **activation
zoom threshold** (default `0.25`) separates two regimes:

- **Above the threshold — single-output overview (same in-output interaction as
  today).**
  The focused output shows its own overview (vertical stack of its workspaces).
  `←` / `→` and `Shift`+scroll move window selection / scroll *within* that
  output, exactly as they do now.

- **Crossing the threshold (zooming further out).**
  Sibling outputs **slide in from the sides**, dimmed and tucked behind the
  centered card — the carousel forms. This is a **2D stacked-peek** layout (no
  3D rotation): the centered output is front-and-center; siblings are scaled
  down, darkened, and half-tucked on each side, with the outer edges faded.

- **Below the threshold — carousel regime.**
  The *same* `←` / `→` and `Shift`+scroll now **slide the carousel** — cycling
  which output is centered — instead of moving windows. The binding's meaning is
  determined purely by the current zoom regime.

### Selecting and jumping to a window

1. Zoom out below the threshold → carousel appears.
2. Slide the carousel (`←`/`→`, `Shift`+scroll) to center the output you want.
3. Zoom back in above the threshold → the host screen now shows the **centered
   output's full overview**, rendered from that output's live content.
4. Navigate to the specific window using the existing overview navigation.
5. Close the overview onto the selected window → focus jumps to that window **on
   its real physical monitor**, and the host screen returns to its own live
   desktop.
6. `Esc` / cancel at any point → host returns to its own desktop, no focus
   change.

So the **zoom-out picks the monitor; the zoom-in picks the window.** Window-level
browsing (the hard requirement) happens above the threshold, on whichever output
the carousel centered.

### Containment (answering YaLTeR's objection)

Each output's content is rendered into its **own card box** and cropped to that
box with `CropRenderElement`. A workspace's infinite horizontal scroll is clipped
at the card edge and cannot bleed into a neighbour. The dimming + edge-fade of
tucked siblings reinforces the boundary visually. Containment is structural, not
cosmetic.

### Participation

An output appears as a card when it is **both**:

- **not `isolated`** — reusing the existing isolated-output gate, so chrome
  isolation is honoured for free (an isolated output neither hosts the carousel
  nor appears as a card), and
- **non-empty** — it has at least one window. Empty outputs are skipped.

No hard cap on output count. Four or more outputs simply produce more tucked
side-cards / cards past the fade edge; the carousel degrades gracefully rather
than special-casing a limit.

*Future work (not in v1):* a dedicated way to suppress a specific output from the
carousel while still using it (either a new isolation sub-flag or an explicit
opt-in list). Deferred until real usage shows it is needed.

## Architecture

### Rendering strategy — direct element compositing (no offscreen buffers)

The focused output's render pass (`render_inner` → today only
`monitor_for_output(output)`) is extended so that, when the host output is in
consolidated overview, it also builds render elements for sibling monitors and
composites them into the host's own element list, using the existing
`RescaleRenderElement` (zoom) + `RelocateRenderElement` (position) +
`CropRenderElement` (clip) primitives — the exact toolkit the current overview
already uses to composite multiple scaled workspaces.

**Offscreen textures are explicitly rejected.** Rendering each sibling to its own
`OffscreenBuffer` would reintroduce the ~1s NVIDIA latency this fork already
diagnosed: a retained texture clone defeats `is_unique_reference` and forces a
full re-render every frame on a render-hot path. Direct compositing never
allocates, so it avoids that cliff entirely.

Key insertion point: `src/niri.rs` `render_inner` (~4849–4984), where instead of
a single `mon`, the participating sibling monitors (`self.layout.monitors()`) are
iterated and each reuses a `render_workspaces`-style scale/relocate/crop with an
added per-card offset.

### Carousel state

New state, on `Layout` (or the host `Monitor`), tracks:

- the **centered/target output** index,
- the **carousel slide offset** and its animation,
- the **regime** (derived from current overview zoom vs the threshold).

This mirrors the existing per-monitor overview-zoom state pattern
(`overview_zoom_target` / `overview_zoom_anim` on `Monitor`).

### Focused-output-only overview

Today `overview_open` is a `Layout`-level bool that `set_monitors_overview_state`
fans out to every non-isolated monitor. In consolidated mode this must instead
target **only the focused output**. This is the most delicate change and needs
care to avoid destabilising the existing overview animation/gesture machinery —
it should be a clearly separated code path selected by the config flag, not a
rewrite of the shared path.

### Rendering a remote output's overview through the lens

When the carousel is centered on a sibling and the user zooms back in, the host
output renders **that sibling monitor's** overview (built from the sibling's live
`Monitor` workspaces), not its own. Window selection and the final focus-jump
operate on the **target** monitor: closing the overview activates the selected
window and moves focus to the target output.

## Config

A new block under `overview`, off by default so niri's global overview is
unchanged when unused:

```kdl
overview {
    consolidated-carousel {
        // presence of the block enables the lens/carousel mode
        activation-zoom 0.25   // zoom threshold at which siblings slide in
    }
}
```

- Parsed with `knuffel` derive, following the existing `Output`/overview config
  patterns (`niri-config/src/`).
- Participation is automatic (non-isolated, non-empty); no per-output config.
- No new keybindings: `←`/`→` and `Shift`+scroll are re-interpreted by zoom
  regime. (A later explicit fallback action can be added if the gesture-only
  entry proves awkward.)

## Testing

- **Config parse snapshots** (`insta`, inline `assert_debug_snapshot!` in
  `niri-config/src/lib.rs`) for the new `consolidated-carousel` block, including
  defaults and the `activation-zoom` value. (Note: inline snapshots there are
  large; pending snaps land in `.lib.rs.pending-snap`.)
- **Participation selection** unit test: given a set of monitors with mixed
  `isolated` / empty / populated states, the correct set becomes cards.
- **Carousel state transitions:** threshold crossing enters/exits the carousel
  regime; slide indexing cycles outputs correctly and clamps/wraps as designed.
- **Focus-jump resolution:** selecting a window on a centered target output
  resolves to the right window + output on overview close.
- **Manual / hardware testing (required, not optional):** render correctness of
  the tucked cards, dimming, edge-fade, and — critically — **frame latency on the
  NVIDIA path**, since this is a render-hot-path feature. "Acceptable perf" is
  unverified until measured on hardware (watch nvtop + intel_gpu_top).

## Risks & open questions (spikes)

1. **Output-scale mismatch (main spike).** Sibling render elements are built
   with the sibling's own `view_size`/scale, then scaled into the host output's
   canvas whose scale may differ. The scale/relocate/crop math and culling
   coordinates must reconcile the two. Bounded and known, but the first thing to
   prototype.

2. **Cross-output redraw dependency.** The host now renders sibling content, so
   the host must redraw when a sibling's content changes. Today outputs redraw
   independently. v1 may accept cards updating on the host's own redraw cadence;
   a sibling→host redraw wakeup is a likely refinement. Flag for the plan.

3. **Focused-output-only overview vs global overview state.** Isolating the
   overview to one output touches the most delicate layout code. Must be a
   separate, config-gated path.

4. **Regime-dependent input rebinding.** `←`/`→` and `Shift`+scroll changing
   meaning at the threshold — decide precisely where in input handling
   (`src/input/`) the regime is checked, and how the transition feels mid-drag.

5. **Slide-in animation at threshold crossing.** Needs to feel continuous with
   the zoom, not a discrete pop.

## Precedent in this fork

The `isolated` output flag already follows the exact config → `Monitor` →
render-gate pattern this feature needs, and already provides the participation
exclusion lever. The overview itself already proves multiple independently
scaled/positioned/cropped workspaces can be composited into one output's element
list. This design is an extension of established patterns, not a greenfield
subsystem.
