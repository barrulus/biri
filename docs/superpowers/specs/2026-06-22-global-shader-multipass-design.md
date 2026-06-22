# Global Shader 3.3 — Layers / Multi-pass (pass chains)

Design spec. Written 2026-06-22. Implements the **multi-pass** half of roadmap item 3.3 in
`docs/superpowers/global-shader-next-steps.md`. Builds directly on the 3.2 dedicated feedback
buffer (`docs/superpowers/specs/2026-06-22-global-shader-feedback-buffer-design.md`) and the 3.1
redraw intelligence (`docs/superpowers/specs/2026-06-22-global-shader-redraw-intelligence-design.md`).
Pick this up cold; everything needed to plan and build is here.

> Status discipline: **[confirmed]** = verified by code reading this session;
> **[design]** = proposed here, not yet built.

> **Scope note.** Roadmap item 3.3 bundles two independent features: (a) **multi-pass chains**
> and (b) **scoped shaders** (apply a shader to a region / window / layer-shell layer). This spec
> covers (a) only. Scoped shaders are a much larger model change ("output post-process" →
> "compositable shader layers") that integrates with the layout/window machinery; it gets its
> own spec → plan → build cycle later.

---

## 1. Problem & goal

Today a global shader is a single full-output post-process pass. Effects that are naturally a
sequence — blur → color-grade → vignette — cannot be expressed without hand-fusing them into one
unwieldy shader. The goal: run **N shaders in sequence**, each reading the prior pass's output,
the last one compositing into the scene.

The 3.2 buffer pass already renders a program into an `OffscreenBuffer` and feeds the result
forward. Multi-pass is the direct generalization of exactly that mechanism: turn every
"global shader" singleton into a per-pass indexed collection and run them in a loop.

**Core invariant:** a chain of length 1 (or zero `pass{}` blocks) is **byte-identical** to
current single-shader behavior. Every existing shader drops into a chain unchanged.

---

## 2. Ground truth (verified this session)

- **[confirmed] Program resolution is by `ProgramType`.** `ShaderRenderElement` stores a
  `ProgramType` (`src/render_helpers/shader_element.rs:24`) and resolves the compiled program
  lazily at draw via `Shaders::get_from_frame(frame).program(self.program)`
  (`shader_element.rs:303`). Adding an **indexed** variant therefore needs no structural change to
  the element — only a new `ProgramType` case and the registry lookup.

- **[confirmed] Singleton registry.** `Shaders` (`src/render_helpers/shaders/mod.rs:13`) holds
  `custom_global: RefCell<Option<ShaderProgram>>` and `custom_global_buffer:
  RefCell<Option<ShaderProgram>>`; `ProgramType::Global` / `GlobalBuffer` index them in
  `program()` (`mod.rs:219-233`). `set_custom_global_program(renderer, src, hyprland)` compiles
  both (display + optional buffer epilogue when the source contains `global_buffer`) and swaps
  them in, destroying the old ones (`mod.rs:427-466`).

- **[confirmed] Single-pass draw + ping-pong, the pattern to generalize.**
  `GlobalShaderElement::draw()` (`src/render_helpers/global_shader_element.rs`):
  (1) `create_buffer` + `capture::capture_framebuffer_region` → `screen_tex`;
  (2) optional buffer sub-pass via `OffscreenBuffer::render` (reads `buffer_prev`, writes a fresh
  texture, stashes the clone into `buffer_result` — the retained clone forces
  `OffscreenBuffer` to allocate a distinct texture next frame, giving ping-pong);
  (3) the display pass via `ShaderRenderElement` (`ProgramType::Global`) into the output;
  (4) `capture_framebuffer_region` → `result_tex` stashed into `result`.
  Post-submit moves (`src/backend/tty.rs:~1945`): `result`→`prev`, `screen_result`→`screen_prev`,
  `buffer_result`→`buffer_prev`.

- **[confirmed] Per-output state (singletons to generalize).** `OutputState`
  (`src/niri.rs:497-514`): `global_shader_prev: Option<GlesTexture>`,
  `global_shader_result: Rc<RefCell<Option<GlesTexture>>>`, `global_shader_screen_prev/_result`,
  `global_shader_buffer: Rc<OffscreenBuffer>`, `global_shader_buffer_prev/_result`. Cloned into
  the element at `src/niri.rs:4410-4416`. Reset on reload at `src/niri.rs:1608-1628`.

- **[confirmed] Caps scan.** `GlobalShaderCaps::scan(src, hyprland)`
  (`niri-config/src/global_shader.rs:16`) substring-scans one source; `is_animating()` =
  `uses_time || uses_prev || uses_buffer`. Cached on `Niri.global_shader_caps`
  (`src/niri.rs:2292`), invalidated on reload.

- **[confirmed] `OffscreenBuffer` / `ShaderRenderElement` render-to-texture is established**
  (`src/render_helpers/offscreen.rs`, used by the 3.2 buffer pass and `blur.rs`). Each
  intermediate pass needs its own `OffscreenBuffer` slot (recreated across frames via the
  existing unique-reference check).

---

## 3. Architecture

One idea: **every global-shader singleton becomes a per-pass indexed collection; `draw()` runs
the passes in a loop, piping each output into the next.**

### 3.A — Program storage (registry)

- New `ProgramType` variants: `GlobalPass(usize)` and `GlobalPassBuffer(usize)`.
- `Shaders` gains `custom_global_passes: RefCell<Vec<ShaderProgram>>` and
  `custom_global_pass_buffers: RefCell<Vec<Option<ShaderProgram>>>` (parallel-indexed: index `i`
  is pass `i`'s display program and its optional `global_buffer` program).
- `program(GlobalPass(i))` → `passes.get(i).cloned()`; `program(GlobalPassBuffer(i))` →
  `pass_buffers.get(i).cloned().flatten()`. The legacy `Global` / `GlobalBuffer` variants are
  **retained** and alias `GlobalPass(0)` / `GlobalPassBuffer(0)` (so any other caller and the
  byte-identity story keep working); `custom_global` / `custom_global_buffer` singletons are
  removed and folded into index 0 of the vecs.
- `set_custom_global_program` generalizes to **`set_custom_global_passes(renderer, passes:
  &[(String /*src*/, bool /*hyprland*/)])`**: compiles each pass's display program (and, niri-mode
  only, a buffer program when its source contains `global_buffer`), replaces both vecs wholesale,
  and destroys every previously-installed program. Empty slice → both vecs empty (chain disabled).
  Each pass compiles via the existing `compile_global_program` / `compile_global_buffer_program`
  (extended only to register the new `niri_source` sampler — see §3.C).

### 3.B — Per-output state

Generalize each singleton on `OutputState` to a `Vec` sized to the chain length:
- `global_shader_prev: Vec<Option<GlesTexture>>` — pass `i`'s output last frame (its `niri_prev`).
- `global_shader_result: Vec<Rc<RefCell<Option<GlesTexture>>>>` — sink for pass `i`'s output this
  frame.
- `global_shader_pass_offscreen: Vec<Rc<OffscreenBuffer>>` — **[new]** the *display* offscreen
  each intermediate pass renders into (its output becomes the next pass's input). The last pass
  renders to `dst`, so its slot is unused (allocated but idle — see §9).
- `global_shader_buffer: Vec<Rc<OffscreenBuffer>>` — the *dedicated `global_buffer`* offscreen per
  pass slot (the 3.2 mechanism, indexed). Distinct from `_pass_offscreen` so a `global_buffer` pass
  can hold both its accumulator and its display output alive at once (§3.D).
- `global_shader_buffer_prev: Vec<Option<GlesTexture>>`,
  `global_shader_buffer_result: Vec<Rc<RefCell<Option<GlesTexture>>>>` — per-pass dedicated buffer
  ping-pong.
- `global_shader_screen_prev` / `_screen_result` stay **scalars** (frame-level: the real screen,
  shared by all passes).

The vecs are (re)sized whenever the chain length changes (reload). When the chain is disabled
they are empty.

### 3.C — Shader contract (samplers, per pass)

Each pass is an ordinary niri-mode `global_color` shader (or hyprland). Samplers seen by pass `N`:

| Sampler / helper | Meaning for pass `N` |
|---|---|
| `niri_screen` / `tex2D_screen` | **pass N−1's output** (the pipe). Pass 0 = real composited screen. |
| `niri_source` / `tex2D_source` | **[new]** the original composited screen, unfiltered; same for every pass. |
| `niri_prev` / `tex2D_prev` | **pass N's own output last frame** (per-pass feedback). |
| `niri_screen_prev` / `tex2D_screen_prev` | previous frame's real screen (frame-level). |
| `niri_buffer` / `tex2D_buffer` + `global_buffer` | pass N's own dedicated accumulator (3.2, per-pass). Optional. |
| `niri_time`, `niri_cursor`, `niri_region`, `niri_output_size` | chain-level uniforms, same for all passes. |

- **Pass 0 is identical to a single shader today**: `niri_screen` = real screen, `niri_prev` =
  own last output, `niri_buffer` = own accumulator. Hence the length-1 byte-identity invariant.
- **`niri_source`** is the only genuinely new sampler. Add to `global_prelude.frag` (sampler +
  `tex2D_source(uv)` helper) and register it in `compile_global_program` /
  `compile_global_buffer_program` sampler lists (`["niri_screen", "niri_prev", "niri_screen_prev",
  "niri_buffer", "niri_source"]`). Registered for both dialects; a hyprland pass never references
  it (location `−1` → no-op set, the existing pattern). For pass 0, `niri_source == niri_screen`.

### 3.D — Render flow (`GlobalShaderElement::draw`)

The element is constructed with the chain: a `Vec` of per-pass `{ has_buffer_program: bool, prev:
Option<GlesTexture>, result: Rc<RefCell<..>>, buffer: Rc<OffscreenBuffer>, buffer_prev, buffer_result }`
plus the per-pass display `OffscreenBuffer` for intermediate output, cloned from `OutputState`.
`draw()`:

1. **Capture screen → `source_tex`** (today's `screen_tex`). It is `niri_source` for all passes
   and `niri_screen` for pass 0. Stash clone → `screen_result` (next frame's `niri_screen_prev`).
   If the chain is empty / any pass program missing → draw `source_tex` through unchanged and
   return (the existing passthrough branch; never render a half-built chain).
2. **`let mut input = source_tex;`** then for `i` in `0..N`:
   a. **Buffer sub-pass** (only if pass `i` has a `GlobalPassBuffer(i)` program): render it into
      `buffer[i]`'s dedicated offscreen, reading `niri_buffer`=`buffer_prev[i]`, `niri_screen`=
      `input`, `niri_source`=`source_tex`, `niri_screen_prev`, uniforms → `buffer_next`; stash
      clone → `buffer_result[i]`. (3.2 logic, indexed.) Bind `buffer_next` as this pass's
      `niri_buffer`; if no buffer program, `niri_buffer` aliases `prev[i]` (today's fallback).
   b. **Pass render** of `GlobalPass(i)` with textures { `niri_screen`=`input`, `niri_source`=
      `source_tex`, `niri_prev`=`prev[i]`, `niri_screen_prev`, `niri_buffer` } and the chain
      uniforms:
      - **Intermediate** (`i < N−1`): render into `buffer[i]`'s **display** offscreen (a second
        `OffscreenBuffer` distinct from the buffer-sub-pass one). The resulting texture becomes the
        next pass's `input` **and** is stashed → `result[i]` (its next-frame `niri_prev`). Same
        retained-clone ping-pong trick.
      - **Last** (`i == N−1`): render directly to the output framebuffer (`dst`) via
        `ShaderRenderElement` so it composites into the scene; then `capture_framebuffer_region` →
        `result_tex` → `result[N−1]`.
3. **Post-submit (`src/backend/tty.rs`):** the existing single move becomes per-pass `Vec` moves —
   for each `i`: `result[i]`→`prev[i]`, `buffer_result[i]`→`buffer_prev[i]`; plus the unchanged
   scalar `screen_result`→`screen_prev`.

**Offscreen slots per pass.** A pass that uses `global_buffer` and is intermediate needs *two*
distinct offscreen textures alive simultaneously (its dedicated buffer + its display output), and
each also needs its own ping-pong read texture. The plan phase will allocate two `OffscreenBuffer`s
per pass slot (display + buffer); the last pass needs no display offscreen (renders to `dst`). Keep
v1 simple: allocate the full set per slot; optimize unused slots later.

### 3.E — Caps, redraw, reload

- **Caps (`niri-config`):** `GlobalShaderCaps` for the chain = **union over all passes** — scan
  each pass's resolved source and OR the flags. Add a helper
  `GlobalShaderCaps::scan_chain(passes: &[(String, bool)]) -> GlobalShaderCaps` that folds `scan`
  over the list. If *any* pass uses time/prev/buffer → chain `is_animating()` → redraws every
  frame and is whole-output. `RedrawMode` logic unchanged.
- **Whole-output:** multi-pass chains (length ≥ 2) are always whole-output; region/`cursor_radius`
  mode applies only to a length-1 chain (consistent with 3.1: animated/feedback effects reshade
  fully). The plan ignores `cursor_radius` when `N ≥ 2`.
- **Reload (`src/niri.rs:1608`):** add `passes` to the change-diff. On change: rebuild the pass
  source list, call `set_custom_global_passes`, re-size and clear all per-pass `Vec`s on every
  `OutputState`, reset `screen_prev`/`_result`, invalidate the caps cache (as today). The caps
  helper (`src/niri.rs:2292`) folds over the pass list.

---

## 4. Config

In `niri-config/src/global_shader.rs`:

- **`GlobalShaderPassPart`** (`knuffel::Decode`): `source: Option<String>`, `path: Option<String>`,
  `mode: Option<String>` children — same inline-or-path resolution and validation as the top-level
  shader. A pass that resolves to nothing / fails validation → warn and treat the **whole chain**
  as off (never render a partial chain).
- **`GlobalShaderPart`** gains `#[knuffel(children("pass"))] passes: Vec<GlobalShaderPassPart>`;
  **`GlobalShader`** gains `passes: Vec<GlobalShaderPass>` (resolved form: `{ source, path, mode }`).
- **Back-compat:** `passes` empty → the top-level `source`/`path`/`mode` define a length-1 chain
  (today's behavior, byte-identical). `passes` non-empty → it *is* the chain; top-level
  `source`/`path` are ignored for rendering, and a warning is logged if both are set.
- `enable`, `reads_cursor`, `cursor_radius`, `redraw` stay **chain-level**. `mode` is **per-pass**,
  defaulting to the top-level `mode` (default `"niri"`) — so a hyprland pass can sit in a niri chain.
- **`MergeWith`:** `passes` replaces wholesale on merge (list, not field-merged). Confirm the exact
  idiom against a sibling list-typed config field when writing the plan.

```kdl
global-shader {
    enable
    pass { path "blur.frag" }
    pass { path "grade.frag" }
    pass { source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }" }
}
```

---

## 5. Sampler summary (niri-mode prelude after this change)

| Sampler / helper | Meaning |
|---|---|
| `niri_screen` / `tex2D_screen` | this pass's input = prior pass output (pass 0 = real screen) |
| `niri_source` / `tex2D_source` | **[new]** original composited screen, unfiltered (all passes) |
| `niri_prev` / `tex2D_prev` | this pass's own output last frame (per-pass feedback) |
| `niri_screen_prev` / `tex2D_screen_prev` | previous frame's real screen (frame-level) |
| `niri_buffer` / `tex2D_buffer` | this pass's dedicated accumulator (3.2); == `niri_prev` if no `global_buffer` |
| `vec4 global_buffer(vec3 c)` | optional per-pass writer for `niri_buffer` |

---

## 6. Testing

- **`niri-config`:** unit tests —
  - pass-list parse: inline `source`, `path`, and per-pass `mode`; ordered.
  - empty-passes back-compat: top-level `source` → length-1 chain.
  - `scan_chain` union: a static blur pass + a `niri_time` final pass → chain `is_animating()`;
    all-static chain → not animating.
  - both top-level `source` and `pass{}` set → warning path (parse still succeeds, passes win).
  - `wiki_docs_parses`: the multi-pass wiki example must parse.
  - `cargo test -p niri-config`.
- **Compile:** dev-shell `cargo check --no-default-features --features dbus,systemd`.
- **Regression (identity):** zero `pass{}` blocks → byte-identical to the current single-shader
  path; a single `pass{}` with the same source → same result.
- **Manual (sixseven):** a 2–3 pass chain — e.g. a blur pass → the existing `comet`/`trail` pass —
  shows both effects composed, and the per-pass trail still accumulates over frames (proves
  per-pass `niri_prev` ping-pong) with no smear under scrolling (proves `niri_source` /
  `niri_screen_prev` are correct). KMS capture only (`gpu-screen-recorder -w eDP-1`).

---

## 7. Scope boundaries (YAGNI)

- Feed-forward **pipe + per-pass feedback** only. No DAG/branching; a pass reads only its input
  (`niri_screen` = prior pass), its own last-frame output (`niri_prev`), the original screen
  (`niri_source`), and its own accumulator (`niri_buffer`). No reading an arbitrary other pass.
- **No MRT** — one render target per pass; `global_buffer` remains a second sequential sub-pass.
- **No** new per-pass uniforms beyond the existing set; **no** new config fields beyond the `pass`
  list (and per-pass `mode`).
- **No scoped/windowed/layer shaders** — that is the other half of 3.3, deferred to its own spec.
- TTY/DRM only; still excluded from screencast/screenshot sinks (unchanged from v1; 2.3).
- `hyprland` passes are allowed in the list but use their own prelude; the new niri samplers
  (`niri_source`, `niri_buffer`, …) are niri-mode only (registered for both, no-op in hyprland).
- Does not touch transform (2.4) or iGPU (3.4).

---

## 8. Suggested implementation order (for the plan phase)

1. **Config + caps (`niri-config`):** `GlobalShaderPassPart`, `passes` field, resolution +
   back-compat, `MergeWith`, `scan_chain`; unit tests. Independently testable, no render code.
2. **Registry:** `ProgramType::GlobalPass(i)`/`GlobalPassBuffer(i)`, the two program `Vec`s,
   `program()` lookup, `set_custom_global_passes`. `Global`/`GlobalBuffer` alias index 0.
3. **Shader contract:** add `niri_source` sampler + `tex2D_source` helper to `global_prelude.frag`;
   register on both compile paths.
4. **Per-output state + element:** generalize the `OutputState` singletons to `Vec`s; thread the
   chain into `GlobalShaderElement`; the `draw()` pass loop (start with intermediates rendering to
   offscreen, last to `dst`); per-pass ping-pong.
5. **Backend + reload:** per-pass post-submit moves in `tty.rs`; reload diff + resize/clear;
   `is_animating()` union; whole-output for `N ≥ 2`.
6. **Docs + verify:** `docs/wiki/Configuration:-Global-Shader.md` multi-pass section + a wiki
   example that parses; rewrite a real chain on sixseven and verify composition + per-pass trail.

Steps 1–3 are pure-logic and independently shippable/testable; step 4 is the meat.

---

## 9. Open questions for the plan phase

- **Offscreen slot count per pass.** Two `OffscreenBuffer`s per pass (display + dedicated buffer)
  is the safe upper bound; decide whether to allocate lazily (only for passes that are intermediate
  / use `global_buffer`) or uniformly per slot in v1. Lean uniform-then-optimize.
- **`MergeWith` for the `passes` list** — exact idiom (wholesale replace vs. presence-merge) per a
  sibling list field in the config crate.
- **Buffer/texture sizing under multi-output / scale changes** — match the existing `dst.size`
  sizing used for `screen_tex`; confirm the per-pass offscreens track output resize on reload.
- **Last-pass-with-`global_buffer`** ordering: the dedicated buffer sub-pass runs before the
  display render in every pass, so the last pass's `global_buffer` is fine (it writes its buffer,
  then composites to `dst`). Confirm no extra capture is needed for the last pass's buffer (it
  isn't — `buffer_result[N−1]` is captured from the offscreen, like intermediates).

---

## 10. Build / test crib

Inherited from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, per-task `cargo
check`, the insta-snapshot workaround for `niri-config/src/lib.rs`, the flake deploy to
"sixseven", KMS-only recording). Not duplicated here.
