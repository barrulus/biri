# Shader-animation FPS cap

**Date:** 2026-07-15
**Status:** Design approved (pending spec review)

## Problem

Time/feedback-driven shaders (global, region, per-window) force **continuous** output
recomposition to animate. niri re-queues a redraw at every vblank while any of the three
`*_shader_animate` flags is set, so an animated shader repaints its whole output at the panel's
native refresh rate — 144 Hz on the laptop panel, 60 Hz on the 3440×1440 ultrawide.

This is the dominant GPU cost in the user's setup:
- The active global (cursor) shader `comet-glow` is `enable`d and uses `niri_time` + feedback +
  two passes → `global_shader_animate` is always true → every output recomposites continuously.
- Per-window shaders (rgb-border, rgb-shimmer, mercury-sheen, ripple-drops) add more.

Hardware makes the ultrawide's dGPU cost unavoidable: DP-2 is wired to the NVIDIA dGPU, so niri
composites that output on the dGPU. The only lever left is **how many shader frames per second**
we render. RGB borders / shimmer / a cursor comet do not need 60–144 fps; ~30 fps looks fine.

## Goal

A configurable cap on the *rate of shader-driven redraws*, leaving every other animation
(window open/close, workspace switch, overview zoom, cursor, drags) at full refresh rate.

## Config surface

One top-level knob:

```kdl
shader-animation-max-fps 30    // 0 or unset = uncapped (current behaviour)
```

- Applies uniformly to global + region + window shader animation.
- `0` / unset → no throttle (byte-for-byte current behaviour; the whole feature is inert).
- KDL scalar parsed with knuffel, same style as other top-level numeric options. Value is fps;
  stored and validated as `u16` (reject negatives at parse; `0` means uncapped).

## Semantics — throttle only the shader-*sole* case

In the per-output redraw decision (`src/niri.rs`, the block that builds
`unfinished_animations_remain`, ~line 5070):

1. Compute the **non-shader** animation flag exactly as today (layout animations, notifications,
   exit dialog, screenshot UI, MRU UI, screen transition, animated cursor, layer surfaces).
2. Compute the **shader** flag = `global_shader_animate || region_shader_animate ||
   window_shader_animate` (all three already computed just above).
3. Decide `unfinished_animations_remain`:
   - Non-shader animation ongoing → `true` (full rate). If a shader is also animating it simply
     rides along on those full-rate frames — **no throttling mid-drag / mid-transition**, so
     nothing looks janky. Refresh `last_shader_frame` on these frames so the throttle clock does
     not immediately fire when the other animation ends.
   - Only shaders animating (idle desktop) → **throttle**: see mechanism below.
   - Nothing animating → `false` (unchanged).

When `shader-animation-max-fps == 0`, skip all of this and keep the current
`unfinished_animations_remain |= *_shader_animate` behaviour.

## Mechanism — throttle the redraw *scheduling* (Option 1)

Per-output state (new fields on `OutputState`):
- `last_shader_frame: Option<Instant>` — when the last shader-driven redraw was allowed.
- `shader_throttle_timer: Option<RegistrationToken>` — the pending wakeup, so it can be
  cancelled/replaced.

Interval = `Duration::from_secs_f64(1.0 / fps)`.

On a redraw where shaders are the sole animator:
- **Due** (`last_shader_frame` is `None`, or `now >= last + interval`): this frame *is* a shader
  frame. Set `last_shader_frame = now`. Do **not** set `unfinished_animations_remain` from the
  shader (leave it `false` unless a non-shader source set it). Instead arm a one-shot timer at
  `now + interval` that calls `queue_redraw(output)`. → exactly `fps` real recomposites/sec.
- **Not due** (`now < last + interval`): also leave the shader out of
  `unfinished_animations_remain`, and ensure the timer is armed for `last + interval`. (This path
  is hit when some *other* damage triggered a redraw between shader frames; we render it but don't
  advance the shader clock or the timer.)

Timer lifecycle:
- Arm via the event-loop `LoopHandle` (`Timer::from_duration`), storing the `RegistrationToken` in
  `shader_throttle_timer`. Model on the existing estimated-vblank timer plumbing in
  `src/backend/tty.rs` (`on_estimated_vblank_timer`) / `RedrawState`.
- When the timer fires: clear the stored token and `queue_redraw(output)`.
- Cancel/replace the timer when: shaders stop animating, a non-shader animation takes over
  (full-rate loop resumes), or the output is removed. Never leave a dangling timer that keeps an
  idle output awake.

Because the throttle only *withholds* the shader contribution to
`unfinished_animations_remain` and drives the next frame via its own timer, it composes with the
existing `RedrawState` machine (Idle / Queued / WaitingForVBlank / WaitingForEstimatedVBlank):
withholding lets the output reach Idle after the current frame, and the timer re-queues it.

### Time source

Use a real monotonic `Instant` for `now` / `last_shader_frame` (consistent with
`window_shader_start` and `global_shader_start`, which also use wall-clock elapsed, not the frozen
per-frame `Clock`). The redraw path already reads `Instant` freely.

## Why not Option 2 (quantize time + suppress damage)

Keeping full-rate vblank wakeups but making shader elements emit no damage on unchanged
quantized-time steps would save GPU but not CPU wakeups, and it fights the "fresh `Id` every
frame" damage trick the region/global shader elements rely on (see
`two-screencast-render-paths` / region-damage notes). That trades scheduling code for
damage-correctness risk on paths that already had damage gotchas — a worse trade. Rejected.

## Scope guards

- No change to shader *rendering* — only to *when* a redraw is scheduled. A shader frame, when it
  renders, is identical to today.
- No per-shader-type caps; one global value (matches "cap them all the same").
- Non-shader animations are never throttled.
- `shader-animation-max-fps 0` is a complete no-op (feature inert), guaranteeing no behaviour
  change for users who don't opt in.

## Testing

- **Unit:** the due/not-due decision is pure logic over `(last_shader_frame, now, interval)` —
  test directly (first frame due; within-interval not due; at/after interval due).
- **Config parse:** snapshot test for `shader-animation-max-fps` present / absent / `0`.
- **Manual:** with `comet-glow` on and `cap 30`, nvtop's dGPU redraw cadence on the docked
  ultrawide should drop to ~30/s and stay smooth; a window drag should still be 144 Hz on eDP-1;
  `cap 0` restores current continuous behaviour; laptop-only still parks the dGPU.

## Out of scope

- The iGPU-offload path (`render-drm-device`) — confirmed a dead end while the dGPU-wired
  ultrawide is docked; unrelated to this change.
- Per-shader-type or per-output caps (could layer on later if needed).
