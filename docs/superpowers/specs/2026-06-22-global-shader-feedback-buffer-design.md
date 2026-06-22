# Global Shader 3.2 — Dedicated Feedback Buffer (+ previous-screen sampler)

Design spec. Written 2026-06-22. Implements roadmap item 3.2 from
`docs/superpowers/global-shader-next-steps.md` and fixes issue 2.2 (trail smear). Builds on
the 3.1 work (`docs/superpowers/specs/2026-06-22-global-shader-redraw-intelligence-design.md`).
Pick this up cold; everything needed to plan and build is here.

> Status discipline: **[confirmed]** = verified by code reading this session;
> **[bug]** = demonstrated/understood; **[design]** = proposed here, not yet built.

---

## 1. Problem & goal

`comet` and `trail` global shaders smear on scrolling/video content. The goal is clean
feedback trails (and a general history buffer) by giving the shader a feedback texture that
is **independent of the screen**. This spec lands two complementary, additive pieces:

- **A — `niri_screen_prev`** (cheap): expose last frame's *screen* capture as a sampler so the
  existing `prev − screen` recovery becomes exact. Fixes the smear in today's shaders with no
  rewrite.
- **B — dedicated buffer** (`global_buffer` / `niri_buffer` / `tex2D_buffer`): a per-output
  offscreen buffer the shader reads and writes, storing **only the effect**. The general,
  screen-free substrate the roadmap wants (and the basis for 3.3 multi-pass).

Both are opt-in and additive: existing shaders render byte-identically.

---

## 2. Ground truth (verified this session)

- **[bug] Smear root cause.** `niri_prev` is the previous *output* = `blend(screen_prev,
  effect_prev)`. A trail shader recovers its trail as `prev − screen_now`, but the trail was
  composited over `screen_prev`, not `screen_now`; wherever the screen moved, that motion
  leaks into the recovery → false trails. (Visible in `global-comet`/`global-trail`, whose own
  comments document the workaround: *"some smear on fast-moving content is inherent."*)

- **[confirmed] Existing ping-pong, the pattern to mirror.**
  `GlobalShaderElement::draw()` (`src/render_helpers/global_shader_element.rs`): creates
  `screen_tex` via `renderer.create_buffer(Fourcc::Abgr8888, buffer_size)` and fills it with
  `capture::capture_framebuffer_region(frame, dst, &screen_tex)`; renders the program; then
  captures the output into `result_tex` and stores it in the shared `self.result`
  (`Rc<RefCell<Option<GlesTexture>>>`). Post-submit move at `src/backend/tty.rs:1945-1950`:
  `global_shader_result` → `global_shader_prev`. Per-output state on `OutputState`
  (`src/niri.rs:497-503`): `global_shader_prev`, `global_shader_start`, `global_shader_result`;
  cloned into the element at `src/niri.rs:4387-4388`. **A becomes a second copy of this exact
  pattern, storing the screen capture instead of the output.**

- **[confirmed] Render-to-offscreen-texture is established.** `src/render_helpers/offscreen.rs`
  (`OffscreenBuffer`) renders elements into a `GlesTexture` via `renderer.bind(&mut tex)` →
  render target → `render_output(...)` → drop(target) restores the framebuffer. The blur
  (`src/render_helpers/blur.rs`) does multi-pass intermediate-texture rendering the same way.
  **B's buffer pass uses this**, NOT the capture-from-output trick (capturing from the output
  would re-mix the scene into the buffer — the very contamination we're removing).

- **[confirmed] Compile path.** `compile_global_program(renderer, src, hyprland)`
  (`src/render_helpers/shaders/mod.rs`) concatenates `global_prelude.frag` + user `src` +
  `global_epilogue.frag` and registers uniforms/samplers. The epilogue calls `global_color`.
  **B compiles the same source a second time with a buffer epilogue that calls `global_buffer`.**

---

## 3. Architecture

### 3.A — `niri_screen_prev` (previous-screen sampler)

Shader-facing (additive to `global_prelude.frag`):
- New sampler `niri_screen_prev` + helper `tex2D_screen_prev(uv)` — the `niri_screen` capture
  from the *previous* frame. With region mode the same `niri_region` remap applies (but buffer/
  feedback shaders are whole-output anyway; see §3.C).

Engine:
- New `OutputState` fields mirroring the `prev`/`result` pair:
  `global_shader_screen_prev: Option<GlesTexture>` and
  `global_shader_screen_result: Rc<RefCell<Option<GlesTexture>>>`.
- In `draw()`, after capturing `screen_tex`, store a clone into `self.screen_result`
  (`GlesTexture` is a cheap handle). Bind `screen_prev` as the `niri_screen_prev` sampler.
- Post-submit (next to the existing move): `screen_result` → `screen_prev`.
- Reset on config reload, like `global_shader_prev`.

Effect: a trail shader computes `prev − tex2D_screen_prev(c.xy)` to recover the pure prior
effect with no screen-motion contamination. ~one extra capture + one sampler. No second pass.

### 3.B — dedicated feedback buffer

Shader-facing (additive):
- Sampler `niri_buffer` + helper `tex2D_buffer(uv)` — the feedback buffer (last frame's).
- Optional function `vec4 global_buffer(vec3 c)` — returns what to store in the buffer this
  frame (e.g. faded `tex2D_buffer` + fresh dab at `niri_cursor`, **no screen mixed in**).
  Reads `tex2D_buffer`, `niri_screen`, and all uniforms.
- `global_color(c)` (display) may read `tex2D_buffer(uv)` to composite the trail over the
  screen, and it reads **this frame's freshly-written** buffer (see flow below).
- If `global_buffer` is **absent**: `niri_buffer` aliases the displayed-frame feedback
  (identical to `niri_prev`), no buffer pass runs, zero overhead. `niri_prev` is retained for
  back-compat.

Compile:
- When the source defines `global_buffer` (scan: `src.contains("global_buffer")`), compile the
  source a **second** time into a new `ProgramType::GlobalBuffer`, using a buffer epilogue
  (`gl_FragColor = global_buffer(coord)`). Register `niri_buffer` (+ `niri_screen_prev`,
  `niri_screen`, `niri_prev`) as samplers on both programs. Store the second program alongside
  `custom_global` in the `Shaders` registry (e.g. `custom_global_buffer: RefCell<Option<..>>`).

Per-output state (mirror the existing pair):
- `global_shader_buffer_prev: Option<GlesTexture>`,
  `global_shader_buffer_result: Rc<RefCell<Option<GlesTexture>>>`.

Render flow in `draw()` **when a buffer program is present**:
1. capture screen → `screen_tex` (as today).
2. **Buffer pass (offscreen):** create/reuse `buffer_next` texture; `renderer.bind(&mut
   buffer_next)`; clear to transparent; render the `GlobalBuffer` program reading
   `niri_buffer`=`buffer_prev`, `niri_screen`=`screen_tex`, `niri_screen_prev`, uniforms →
   writes `buffer_next`; drop the target (restores the output framebuffer). Store `buffer_next`
   into `self.buffer_result`.
3. **Display pass:** render the display program into the output as today, with
   `niri_buffer`=`buffer_next` (the just-written buffer), plus `niri_screen`, `niri_prev`,
   `niri_screen_prev`.
4. capture output → `result_tex` (existing `niri_prev` ping-pong, unchanged).
5. post-submit ping-pong: `buffer_result` → `buffer_prev` (next to the existing moves).

When no buffer program is present: the buffer pass is skipped entirely; `niri_buffer` binds to
`global_shader_prev` (so `tex2D_buffer` == `tex2D_prev`). Single pass, today's behavior.

### 3.C — integration with 3.1

- **Capability scan (`niri-config`):** extend `GlobalShaderCaps` with `uses_buffer`, set when
  the source references `global_buffer` / `niri_buffer` / `tex2D_buffer`. Fold
  `niri_screen_prev` / `tex2D_screen_prev` into `uses_prev` (a shader using last-frame data is
  feedback-driven). `is_animating()` returns true if `uses_time || uses_prev || uses_buffer` —
  so buffer/feedback shaders **redraw every frame** (the buffer must evolve even when idle) and
  are therefore **whole-output** (excluded from region mode, consistent with 3.1's rule that
  animated effects reshade fully).
- **Reload:** clear `global_shader_buffer_prev`/`_result` and `global_shader_screen_prev`/
  `_result` in the existing reload reset loop (`src/niri.rs:1604-1612`); invalidate the caps
  cache (already done for source/mode changes).

---

## 4. Sampler summary (niri-mode prelude after this change)

| Sampler / helper | Meaning |
|---|---|
| `niri_screen` / `tex2D_screen` | composited frame below this element (this frame) |
| `niri_prev` / `tex2D_prev` | previous frame's **output** (effect + screen) — retained, back-compat |
| `niri_screen_prev` / `tex2D_screen_prev` | **[new A]** previous frame's **screen** capture |
| `niri_buffer` / `tex2D_buffer` | **[new B]** feedback buffer (last frame); == `niri_prev` if no `global_buffer` |
| `vec4 global_buffer(vec3 c)` | **[new B]** optional writer for `niri_buffer` (no screen mixed in) |

All output-normalised `uv`, all region-remapped identically to `niri_screen` (whole-output for
feedback shaders).

---

## 5. Testing

- **`niri-config`:** unit tests for the extended scan — `uses_buffer` set by
  `global_buffer`/`niri_buffer`/`tex2D_buffer`; `uses_prev` set by `niri_screen_prev`;
  `is_animating()` true for each. `cargo test -p niri-config`.
- **Compile:** dev-shell `cargo check --no-default-features --features dbus,systemd`.
- **Identity (regression):** existing shaders with no `global_buffer`/`niri_screen_prev` render
  byte-identically; `comet`/`trail` keep working on `niri_prev`.
- **A fix (manual, sixseven):** patch `global-trail` to recover via `prev − tex2D_screen_prev`;
  scroll a page / play video under the trail — trail follows the cursor with **no smear behind
  the moving content** (today it smears).
- **B fix (manual, sixseven):** rewrite `global-trail` to the buffer contract (`global_buffer`
  writes faded buffer + fresh dab; `global_color` composites `tex2D_buffer` over screen) —
  clean trail, no smear, and the buffer is visibly screen-independent (scroll under it: no
  ghost of the scrolled content in the trail).

---

## 6. Scope boundaries

- **One** feedback buffer (ping-pong pair). No N-buffer history.
- **No** multi-pass chaining of distinct shaders (that is 3.3; this enables it).
- **No** new config fields (purely shader-contract + engine).
- GLES2, **one render target per pass** (no MRT); B uses two sequential passes.
- `hyprland` mode is unaffected (its prelude/epilogue are separate; `global_buffer` and the new
  samplers are niri-mode only). The new samplers are registered for both dialects but a
  hyprland shader simply never references them (location −1 → no-op set, the existing pattern).
- Does not touch screencast/screenshot exclusion, transform (2.4), or iGPU (3.4).

---

## 7. Suggested implementation order (for the plan phase)

1. **A — `niri_screen_prev`** (smallest, immediate smear fix): prelude sampler/helper,
   `OutputState` screen_prev/result pair, draw() clone + bind, post-submit move, reload reset,
   register sampler in compile. Verify by patching `global-trail`'s recovery.
2. **B caps + compile:** extend `GlobalShaderCaps` (`uses_buffer`), the `niri-config` scan +
   tests; add `ProgramType::GlobalBuffer`, the buffer epilogue, second compile gated on
   `global_buffer` presence, registry storage.
3. **B render:** buffer ping-pong state, the offscreen buffer pass in draw(), `niri_buffer`
   binding (buffer_next for display, alias to prev when no buffer program), post-submit move.
4. **B integration + verify:** `is_animating()` includes `uses_buffer`; reload resets; rewrite
   `global-trail`/`global-comet` to the buffer contract and verify no smear.

Phase 1 (A) is independently shippable and already fixes the visible problem. B is the larger,
substrate-building piece.

---

## 8. Open questions for the plan phase

- Exact second-program storage: a sibling `custom_global_buffer` field on `Shaders` vs. a
  generalized map — decide against the current `custom_global` storage.
- Whether the buffer texture should be lazily created/cached on the element vs. allocated each
  frame (the existing code allocates `create_buffer` each frame; match that for v1, optimize
  later).
- Buffer size under multi-output / scale changes — match `screen_tex` sizing (`dst.size`),
  which already tracks the output.

---

## 9. Build / test crib

Inherited from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, per-task
`cargo check`, the insta-snapshot workaround, the flake deploy to "sixseven", KMS-only
recording). Not duplicated here.
