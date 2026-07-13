# Animated per-window shaders

**Date:** 2026-07-13
**Status:** Design approved

## Problem

Per-window shaders render their spatial (geometry-driven) output correctly, but any
`niri_time`-driven animation is frozen. The perimeter hue of an RGB-border shader shows,
but the `+ niri_time * k` term never advances.

### Root cause (confirmed in source)

`niri_time` is hardcoded to the constant `0.0` when the window's `ScopedShaderElement` is
constructed:

- `src/layout/tile.rs:1205` — `0.0, // niri_time = 0 (static v1)`. This value flows into
  the uniform at `src/render_helpers/scoped_shader_element.rs:170`
  (`Uniform::new("niri_time", self.time)`).

Per-window shaders were shipped deliberately static in v1 (crt / pixel-mosaic / fisheye are
all marked "Static: does NOT use niri_time"). This is a build limitation, not a shader bug
and not a damage-scheduling subtlety — the uniform is a literal zero.

Two independent things are required for real animation. Feeding live time alone is
insufficient: an idle window would still not repaint, so its shader would only advance while
the window happens to produce damage.

## Design

Mirror the existing global/region shader machinery.

### Piece 1 — feed live time into the window shader element

The window `ScopedShaderElement` is built in `Tile::render` (`src/layout/tile.rs`), which
has no access to `Niri` state. The elapsed-time value is threaded in via the render context.

- Add a field `shader_time: f32` to `RenderCtx` (`src/render_helpers/mod.rs:60`). The two
  reborrow helpers `RenderCtx::r()` and `RenderCtx::as_gles()` propagate it.
- Add a **shared compositor time origin** to `Niri` state: `window_shader_start: Instant`,
  initialized once at construction. Monotonic, never reset. This realizes the chosen
  "shared origin" semantic: all window shaders animate in-phase off one real-seconds clock —
  the same clock model the global shader uses (`start.elapsed().as_secs_f32()`).
- At each `RenderCtx` construction site (19 total), populate `shader_time`:
  - Real output-render / screencast paths → `niri.window_shader_start.elapsed().as_secs_f32()`.
  - Incidental paths (pick-color grab, per-window screencast snapshot, internal tile reborrows
    that already inherit from an outer ctx) → `0.0`.
- In `src/layout/tile.rs:1205`, replace the hardcoded `0.0` with `ctx.shader_time`.

`niri_cursor` stays stubbed `(0.0, 0.0)` — **out of scope** for this change (the reported bug
is time; a live cursor uniform is a separate follow-up).

### Piece 2 — keep quiet windows repainting

Mirror `region_shader_animate` in the per-output redraw decision
(`src/niri.rs:5014-5050`):

- Compute `window_shader_animate`: does any mapped window on this output carry a resolved
  shader whose source uses time? Reuse `niri_config::GlobalShaderCaps::scan_chain` (the same
  scan region shaders use at `src/niri.rs:5021`) over the window's `ResolvedShader.passes`
  source strings, then `.is_animating()`.
- OR the result into `state.unfinished_animations_remain` alongside the global and region
  flags (`src/niri.rs:5048-5050`), gated the same way (`target_renders_shaders` / shaders
  enabled for the target).

Windows with static shaders (crt/pixel-mosaic/fisheye) impose no continuous-redraw cost
because the scan finds no time usage.

## Scope guards

- No new config keys.
- No change to `ResolvedShader`'s shape — animation is detected by scanning sources on the
  fly, exactly as region shaders do (no cached `animates` field, no `PartialEq`/snapshot
  churn).
- `niri_cursor` uniform left at `(0,0)` for a later change.

## Testing

- Config-parse snapshots unaffected (no config surface change).
- Manual verification:
  - An RGB-border (time-using) shader on an **idle** window cycles continuously.
  - A static shader (crt) imposes **no** continuous-redraw wakeups — verify
    `unfinished_animations_remain` stays false for that output (existing redraw logging /
    tracy).

## Notable prior gotchas honored

- Do NOT push `niri_size` as an additional uniform — it is a built-in; pushing it is a
  per-frame `GlesError` that freezes the output. Unchanged here (we only change `niri_time`'s
  value, not the uniform set).
- Time uses a real `Instant` origin (like the global shader), independent of the frame clock
  that is frozen at target presentation time during render.
