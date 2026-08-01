# Consolidated Carousel Overview — Cover-Flow Redesign

**Date:** 2026-08-01
**Status:** Design — pending spike, then implementation plan
**Supersedes:** the card/lens rendering model of
`2026-07-20-consolidated-carousel-overview-design.md` and
`2026-07-21-consolidated-carousel-2c-design.md`. The problem statement,
participation rules, and containment argument from those documents still stand.
**Origin:** hardware feedback on Phases 2a–2c; niri-wm/niri discussion #4337.

## Why redesign

Interactive testing of the landed phases surfaced four problems:

1. **Cards are visually unattractive.** Cropping a sibling output into a boxed
   card discards its identity. The sibling should appear as its *entire*
   output — its full overview laid out over its own wallpaper — receding at a
   perspective angle, cover-flow style: centered output flat and facing the
   viewer, siblings tilted backwards, deepening with distance (see the sketch
   referenced in the session notes).
2. **The threshold pop is jarring.** Cards appear suddenly when zoom crosses
   `activation-zoom`. The gallery should assemble *continuously with the zoom
   gesture*: side outputs slide and tilt in as zoom decreases.
3. **No physical placement awareness.** Most users have one or two extra
   monitors. A sibling that physically sits to the left must enter from the
   left, and vice versa.
4. **No direct navigation.** Rotating the carousel should be possible straight
   from overview activation (not only after zooming into the band), and
   clicking a side output should rotate it into focus.

Additionally, the direct-composited lens wallpaper commit (`af69e691`)
self-deadlocks `render_inner` via recursive `layer_map_for_output` when the
lens target is the output being rendered (always true on a single monitor).
The redesign removes that code path entirely; see **Invariants**.

The original design's "no true 3D rendering" non-goal is hereby reversed: the
perspective tilt *is* the requested visual, and the rendering approach below
was chosen with the fork's NVIDIA latency history explicitly in mind.

## Behaviour

### One screen tells the whole story (lens world)

The host (focused) output remains the single stage. Rotating the carousel
never moves output focus by itself: landing on a remote output shows that
output's overview *on the host screen*. Actual focus moves only on final
window selection (focus-jump). Sibling physical monitors stay fully live and
interactive throughout.

### Continuous reveal

Two config zoom levels define the reveal band:

- Above `reveal-zoom`: normal overview (or the lens, if a remote output is
  centered). No gallery visible.
- Between `reveal-zoom` and `assembled-zoom`: `reveal_progress` runs 0 → 1.
  Side panels slide in from their physical side, tilting toward their ring
  slot; the centered panel eases from full-view toward its center-slot size.
- At `assembled-zoom` and below: gallery fully assembled. `reveal_progress`
  clamps at 1; zooming further out has no additional visual effect — the
  assembled gallery is the terminal state of the gesture.

At `reveal_progress = 0` the centered panel renders exactly what the normal
overview/lens shows at that zoom, so crossing `reveal-zoom` in either
direction is seamless by construction.

### The ring

All participating outputs (host included — the host is no longer visually
special-cased) form a ring ordered by physical x-position. Outputs left of
the host fill the left stack nearest-first; right likewise; the ring wraps
cyclically. Purely vertical arrangements degrade to y-order. Each ring slot
carries tuned constants: yaw angle (deepening with distance from center),
x-offset, scale, z-depth, and a depth-dim factor (replaces the Phase 2a
gradient edge fades).

`rotation` is a continuous float in ring position: integers = settled on an
output, fractions = mid-swing. A panel's placement = its slot parameters
interpolated by `rotation`, then slid/tilted in by `reveal_progress`.

### Rotation and the pull-back choreography

Rotation targets can be set by:

- `←`/`→` and Shift+scroll — at *any* overview zoom;
- clicking a side panel.

If the gallery is not assembled when rotation is requested, the **pull-back**
runs: animate zoom out to `assembled-zoom` (gallery assembles on the way),
animate `rotation` to the target, animate zoom back to the prior level (the
gallery retracts around the new center — the lens, if remote). All three legs
use niri's standard `Animation`; the reveal needs no separate animation state
because it derives from zoom.

### Selection and exit

- Window-level interaction exists only on the centered, settled panel
  (`rotation` integral). Click/Enter on a window there = focus-jump: focus
  that window on its real output and close the overview.
- Side panels are panel-level click targets only (click = rotate-to). No
  window picking through a perspective warp.
- Closing the overview with a remote output centered resets `rotation` to the
  host; the host returns to its own desktop.

## Rendering architecture

### Retained offscreen per participating output

Each panel owns a persistent GPU texture holding that output's full overview
(workspaces at overview layout, over its own wallpaper) at fit-to-host
resolution (the lens `fill_zoom` sizing). Each offscreen has its own damage
tracker; the texture re-renders **only when that output's content changed**.
During pure carousel motion (reveal / swing / untilt) content is unchanged —
per-frame GPU work is one quad redraw per panel. Mode/scale changes re-fit
the offscreen; allocation failure skips that panel with a log, never kills
the frame.

### PerspectivePanelElement

A custom GLES render element (precedent: `BorderRenderElement`): texture +
four projected corner points, perspective-correct homography sampling in the
fragment shader, per-panel dim factor. Conservative damage (bounding box), no
claimed opaque region. Slot parameters → corner points via a small pure
function (unit-testable). Inverse homography doubles as the hit-test for
side-panel clicks.

### Unification

The lens is not a special path: at `reveal_progress = 0` with a remote output
centered, the "lens" is the center panel drawn flat at full size from the
same texture. The cropped-card renderer, the gradient edge fades, the three
direct-composited lens commits, and `expand-zoom` are all superseded.

### Invariants

- **Panel content rendering never runs inside the host's element-collection
  scope.** An offscreen panel render is its own pass over its own output and
  takes that output's layer-map lock in isolation — never nested inside
  `render_inner` while the host's guard is held. Enforced by a debug
  assertion (thread-local "in render_inner" flag). This structurally removes
  the `af69e691` deadlock class.
- Containment (YaLTeR's overflow objection) now holds trivially: a panel
  samples a bounded texture; nothing can overflow its quad.
- No per-frame offscreen allocation; textures are retained and damage-gated
  (see `per-frame-gpu-alloc-latency`).

## Config

```kdl
overview {
    consolidated-carousel {
        reveal-zoom 0.4      // was activation-zoom: where slide-in begins
        assembled-zoom 0.15  // gallery fully assembled
    }
}
```

- `activation-zoom` is renamed; `expand-zoom` is removed (fork-only config,
  breaking change acceptable).
- Validation: `0 < assembled-zoom < reveal-zoom < 1`.
- Panel angles / depth-dim are tuned constants, not config, until real use
  demands knobs.

## Edge cases

- **Single output:** feature inert; rotate is a no-op; no panels beyond host.
- **Output hotplug:** rebuild the ring; if the centered output vanished, snap
  `rotation` home and free its texture.
- **Participation** unchanged: non-isolated + non-empty, honoring
  isolated-output-config.
- `set_monitors_overview_state` re-invoked on active-output change (folds in
  the Phase 2c leftover).

## Spike gate (before the main build)

Minimal spike: one damage-gated retained offscreen panel + homography shader.
Pass/fail:

1. No NVIDIA latency cliff on sixseven (the `per-frame-gpu-alloc-latency`
   scenario differs — retained + damage-gated, transform-only animation — but
   history demands proof).
2. Correct rendering under virgl in the biri VM (keeps the VM usable for
   carousel testing).

If the spike fails: fall back to direct per-element homography (no offscreen)
at reduced scope — warp window textures and solids only, skip decorations.

## Testing

- Config parse snapshot tests for renamed/removed nodes.
- Layout unit tests: ring ordering from physical positions (1/2/3-monitor,
  vertical-stack degrade), `reveal_progress` derivation, rotation targets and
  cyclic wrap, hotplug ring rebuild.
- Unit tests: slot-parameters → corner-points math; inverse-homography
  hit-test.
- Debug assertion: the no-nested-panel-render invariant.
- Interactive: VM (single output; optionally a second virtio display later),
  then hardware verification on sixseven.

## Cleanup

Remove with the superseded paths: card rendering (Phase 2a), direct lens
compositing (`047d91f6`, `fc57aae0`, `af69e691`), `expand-zoom` config +
fixtures, `carousel_participants` (already noted unused in 2c).
