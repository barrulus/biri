# Shaders in Capture — opt-in shader rendering for screencast/screenshot

Design spec. Written 2026-06-23. Implements roadmap item 3.5 "screencast inclusion"
(`docs/superpowers/global-shader-next-steps.md`) plus the equivalent for region/window shaders.
Lets portal/PipeWire screenshare (Google Meet, OBS, browser/Zoom), screenshots, and `wl-screenrec`
show the compositor's shaders, behind one opt-in switch. Pick this up cold; everything needed to
plan and build is here.

> Status discipline: **[confirmed]** = verified by code reading this session;
> **[design]** = proposed here, not yet built.

---

## 1. Problem & goal

**[confirmed]** Shaders are gated to the real scanout (`RenderTarget::Output`), so portal/screencopy
captures show the **unprocessed** frame. The user wants the comet/CRT/etc. visible when screensharing
in Google Meet (and OBS, screenshots, `wl-screenrec`). KMS capture (`gpu-screen-recorder -w
<connector>`) already shows the effect, but cannot feed portal-based screenshare (Meet/Zoom/browser)
or per-window/region capture.

**Goal:** a single opt-in config switch that renders **global + region + window** shaders into both
capture render paths (`Screencast` and `ScreenCapture`), without corrupting the live output's
feedback state. Default off (privacy-sane: shaders don't leak into shares unless asked).

---

## 2. Ground truth (verified this session)

- **[confirmed] Capture renders reuse the same `render()` path.** Screencast renders call
  `self.render(ctx { target: RenderTarget::Screencast }, output, …)` (`src/screencasting/mod.rs:601`)
  → `render_inner`. Screenshot/screencopy renders use `RenderTarget::ScreenCapture` (`src/niri.rs`
  multiple sites, e.g. 5666/5744/5951/6006/6172). So relaxing the in-`render_inner` target gates
  includes shaders in both.
- **[confirmed] Three different gates today:**
  - **Global shader** — `src/niri.rs:4357`: `ctx.target == RenderTarget::Output && Shaders…GlobalPass(0).is_some()`.
  - **Region shaders** — `src/niri.rs:4497`: `if ctx.target == RenderTarget::Output { … push per region … }`.
  - **Window shader** — `src/layout/tile.rs:1182`: `if let Some(resolved) = self.window.rules().shader.clone() { … }` — **NOT gated on target**, so window shaders currently render into capture unconditionally (an inconsistency this spec corrects).
- **[confirmed] `RenderCtx` carries `target`** (`src/render_helpers/mod.rs:62`); `Tile::render` already
  reads `ctx.target` (e.g. `tile.rs:1115` block-out). `RenderTarget` = `{ Output, Screencast, ScreenCapture }`
  (`mod.rs:90`).
- **[confirmed] Global feedback ping-pong is per-output and shared.** The element is built from
  `OutputState.global_shader_chain` (per-pass `prev`/`result`/`pass_offscreen`/`buffer`/`buffer_prev`/
  `buffer_result`), constructed at `src/niri.rs:~4400`; the real ping-pong move (`result → prev`) runs
  ONLY in the Output post-submit (`src/backend/tty.rs`). A capture render writing the shared `result`
  sinks would corrupt the live output's feedback — the original reason for the Output-only gate
  (roadmap 2.3). Region/window shaders are **stateless** (capture-and-shade; `scoped_shader_element.rs`
  binds all feedback samplers to the live input) — no such hazard.
- **[confirmed] `debug.preview_render`** already retargets Output→Screencast/ScreenCapture
  (`src/niri.rs:4300`) to preview the shaderless capture path; that comment ("shaderless by design")
  becomes "shaderless unless `shaders-in-capture`".
- **[confirmed] Block-out happens before the shader capture.** `block-out-from "screencast"/"screen-capture"`
  blacks out windows in the capture frame; the shader then captures that already-blocked frame, so
  blocked content stays blocked. No special handling needed.

---

## 3. Architecture

### 3.A — Config (`niri-config`)

Add a top-level bool `shaders_in_capture: bool` (default `false`) to `Config`, parsed from a bare
KDL node `shaders-in-capture` (mirror an existing top-level bool/flag node). Reachable at render time
via `self.config.borrow().shaders_in_capture`.

### 3.B — Render policy helper

One predicate captures the gate everywhere:

```rust
// true if shaders should render for this target.
fn target_renders_shaders(target: RenderTarget, capture_enabled: bool) -> bool {
    target == RenderTarget::Output || capture_enabled
}
```

`capture_enabled = config.shaders_in_capture`, read once per `render_inner` (and threaded to
`Tile::render`). When `capture_enabled` is true it covers BOTH `Screencast` and `ScreenCapture`
(any non-Output target).

### 3.C — Relax the three gates

1. **Global** (`niri.rs:4357`): `ctx.target == RenderTarget::Output` → `target_renders_shaders(ctx.target, capture_enabled)`.
2. **Region** (`niri.rs:4497`): same swap on that `if` block.
3. **Window** (`tile.rs:1182`): wrap the `if let Some(resolved) …` branch in
   `if target_renders_shaders(ctx.target, capture_enabled)`. Thread `capture_enabled` into `Tile::render`
   (add to `RenderCtx` or pass the resolved value down — `RenderCtx` is the natural carrier since it
   already holds `target`). **Behavior change:** window shaders, currently always in capture, now
   follow the switch (excluded by default).

### 3.D — Global feedback isolation (capture renders only)

At the global element construction (`src/niri.rs:~4400`), branch on `ctx.target != RenderTarget::Output`:
- **Read** the live trail: pass each pass's real `prev` / `buffer_prev` handle clones from
  `OutputState.global_shader_chain` (same as today) so the capture shows the current comet trail.
- **Discard writes:** give the capture element **fresh throwaway** `result` / `buffer_result` sinks
  (`Rc::new(RefCell::new(None))` created here, NOT the OutputState ones), `screen_result` throwaway,
  and **fresh** intermediate `pass_offscreen` / `buffer` offscreens (fresh `OffscreenBuffer`s, not the
  shared per-output ones). Its feedback writes go nowhere; the live output's ping-pong is untouched.

  Concretely: build the `Vec<GlobalPassState>` for the capture case with `prev`/`buffer_prev` cloned
  from `chain`, but `result`/`buffer_result`/`pass_offscreen`/`buffer` fresh per element. Region/window
  elements need no isolation.

> Latency note: the capture trail reads the live `prev` (last Output frame's feedback) — at most one
> frame behind. Acceptable; the capture is not driving the feedback evolution.

---

## 4. Config example

```kdl
// Render global / region / window shaders into screencast + screenshot captures (default: off).
shaders-in-capture true
```

---

## 5. Testing

- **`niri-config`:** parse test — `shaders-in-capture` present → `true`; absent → default `false`;
  default-config inline snapshot updated. `cargo test -p niri-config`.
- **Compile:** dev-shell `cargo check --no-default-features --features dbus,systemd`.
- **Regression / default-off (critical):** flag absent/false → captures show NO shader, INCLUDING
  window shaders (the intentional behavior change). Verify a window-shaded app (discord parchment) no
  longer appears shaded in a portal capture when the flag is off; live output unchanged.
- **Manual (sixseven), flag on:**
  1. `shaders-in-capture true` + comet active → a portal/Meet/OBS capture shows the comet trail; the
     **live output** comet is unaffected (no stutter/corruption from the capture render running).
  2. A screenshot (grim) and `wl-screenrec` (ScreenCapture path) also show the shader.
  3. A region shader and a window shader appear in a portal capture.
  4. Flag off → none appear in capture; live output unchanged throughout.

---

## 6. Scope boundaries (YAGNI)

- One global on/off switch (`shaders-in-capture`). No per-shader, per-output, or per-target-kind
  granularity.
- Capture render **reads** live feedback and **discards** its writes — no separate per-target feedback
  evolution (capture trail mirrors the live one, ≤1 frame latency).
- Applies to `Screencast` + `ScreenCapture` together. Does not touch `block-out-from`, 8-bit feedback
  precision, the shader contract, or `debug.preview_render` (beyond a stale-comment update).
- Default **off** — opt-in; private by default.

---

## 7. Suggested implementation order (for the plan phase)

1. **Config:** `shaders_in_capture` field + `shaders-in-capture` parse + snapshot + test (`niri-config`,
   no dev shell).
2. **Policy + global/region gates:** the `target_renders_shaders` helper; swap the two `niri.rs` gates;
   read `capture_enabled` in `render_inner`. The global feedback isolation (throwaway sinks/offscreens
   for non-Output). `cargo check`.
3. **Window gate:** thread `capture_enabled` to `Tile::render`; wrap the `tile.rs:1182` branch.
   `cargo check`.
4. **Docs + manual verify:** wiki note; hardware checks per §5.

Steps 2–3 are the render-path meat; the global feedback isolation in step 2 is the only nuanced part
and is verified on hardware (live output unaffected while a capture is active).

---

## 8. Build / test crib

Inherited from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, per-task `cargo check`,
the insta-snapshot workaround for `niri-config/src/lib.rs`, the flake deploy to "sixseven"). For
capture testing: a portal recorder (`gpu-screen-recorder -w portal`, or OBS via xdg-desktop-portal)
exercises `Screencast`; `grim` / `wl-screenrec` exercise `ScreenCapture`.
