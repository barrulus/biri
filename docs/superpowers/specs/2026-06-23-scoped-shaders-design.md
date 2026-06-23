# Scoped Shaders — Per-window & Per-region Post-process (global-shader 3.3, part 2)

Design spec. Written 2026-06-23. Implements the **scoped shaders** half of roadmap item 3.3 in
`docs/superpowers/global-shader-next-steps.md` (the multi-pass half shipped 2026-06-23, commits
`d7302de7`..`57350d79`). Builds on the global-shader contract (`global_prelude.frag` /
`global_color`) and the 3.3 multi-pass chain machinery. Pick this up cold; everything needed to
plan and build is here.

> Status discipline: **[confirmed]** = verified by code reading this session;
> **[design]** = proposed here, not yet built.

---

## 1. Problem & goal

Today a custom shader is whole-output only (`global-shader`). The goal: run a post-process shader
scoped to **one window's own content** (e.g. CRT/grayscale on a terminal) or to a **fixed screen
region** (e.g. a vignette over part of the output), with **multiple independent** scoped shaders
active at once. Layer-shell and backdrop-behind targets are explicitly out of scope (the latter is
already served by `background_effect` blur).

**Headline property:** scoped shaders reuse the **existing niri-mode `global_color` contract and
the multi-pass `pass{}` chain verbatim** — only the bound `niri_screen` texture and the target
area differ. An existing whole-output shader runs unchanged on a window or region.

**v1 scope decisions (settled in brainstorming):**
- Targets: per-window (via `window-rule`) + per-region (top-level list). Multiple independent.
- Contract: static filters + `niri_time` + `niri_cursor` + multi-pass chains. **No per-scope
  feedback** (`niri_prev`/`niri_buffer`/`global_buffer` deferred to v2).
- Config: window shaders via a `shader` child on `window-rule`; region shaders via a repeatable
  top-level `region-shader` block.

---

## 2. Ground truth (verified this session)

- **[confirmed] Per-window animation shaders are the window-content analog.** Open/close/resize
  animations render a window to an offscreen texture and run a user GLSL program over it
  (`src/layout/opening_window.rs:66-123`, `src/layout/closing_window.rs:226-272`,
  `src/layout/tile.rs:1089-1151`). Contract: `niri_tex` sampler = window snapshot, window-geometry
  coords, mat3 geo/tex transforms. Compiled as **global singletons** (`set_custom_open_program`
  etc., `src/render_helpers/shaders/mod.rs`). This proves "render one window to a texture, post-
  process it" is established; the gaps for scoped shaders are (a) it must run in steady state (not
  just during an animation) and (b) the program must be per-target, not a singleton.

- **[confirmed] `AlphaAnimation` is the cleanest steady-routing pattern to extend.**
  `Tile::render()` (`src/layout/tile.rs:1317`, alpha path ~1364-1387) renders the whole tile
  (`render_inner`) into an `OffscreenBuffer`, then emits the offscreen with a modified alpha. A
  window scoped shader extends exactly this: route the tile through an `OffscreenBuffer`, then wrap
  the result in a shader element instead of (or in addition to) the alpha tweak.

- **[confirmed] `OffscreenBuffer` exists and is reused per-tile.** `src/render_helpers/offscreen.rs`
  (`OffscreenBuffer`, `render() -> (OffscreenRenderElement, SyncPoint, OffscreenData)`,
  `texture()`). Already held one-per-tile by `OpenAnimation`/`AlphaAnimation`/`ResizeAnimation`.
  Its `is_unique_reference` recreation gives ping-pong; here we only need single-shot offscreen +
  shader, so no ping-pong is required for v1.

- **[confirmed] `background_effect` is the per-region capture analog.**
  `src/render_helpers/background_effect.rs` + `render_for_tile` (`src/layer/mapped.rs:240`,
  window side similarly) scope a `FramebufferEffect` to a tile's geometry/subregion/clip and apply
  blur/noise/saturation. This proves "capture the composited framebuffer scoped to a rect and
  post-process it" is established. A region shader is the global-shader element with a static rect.

- **[confirmed] Global-shader element = the region analog directly.**
  `GlobalShaderElement::draw` (`src/render_helpers/global_shader_element.rs`) captures the
  composited framebuffer (`capture::capture_framebuffer_region`) into `niri_screen` and runs the
  multi-pass chain over an `area`/`region_norm`. The existing cursor-radius region mode
  (`src/niri.rs:~4377`) already shrinks `area` to a sub-box and sets `region_norm`. A region shader
  is this with a **config-static** rect instead of the cursor box, and multiple instances.

- **[confirmed] Window rules: matching + render-time consumption.** `WindowRule`/`Match`
  (`niri-config/src/window_rule.rs`) match on `app_id`/`title`/`is_focused`/`is_floating`/… and
  resolve via `ResolvedWindowRules::compute()` (`src/window/mod.rs:182-363`, later match overrides
  earlier for scalars). Consumed in `Tile::render_inner` via `self.window.rules()`
  (`src/layout/tile.rs:1067`). The natural home for a per-window `shader`.

- **[confirmed] Multi-pass registry to generalize.** `Shaders` (`src/render_helpers/shaders/mod.rs`)
  holds `custom_global_passes: RefCell<Vec<ShaderProgram>>` indexed by `ProgramType::GlobalPass(i)`,
  installed by `set_custom_global_passes(renderer, &[(String, bool)])`, with
  `compile_global_program`/`compile_global_buffer_program` (prelude + source + epilogue). Scoped
  shaders need **N** such chains keyed by source, not the single global chain.

- **[confirmed] Global-shader insertion zone.** `Niri::render_inner` (`src/niri.rs:~4324-4480`)
  builds the global-shader element gated on `RenderTarget::Output` and pushes it into the
  `OutputRenderElements` stream. Region shaders insert in the same zone; window shaders insert
  inside the tile render path (deeper, per-tile).

---

## 3. Architecture

### 3.A — Source-keyed program cache (shared substrate)

`Shaders` gains:

```rust
pub scoped: RefCell<HashMap<u64, ScopedChain>>,   // key = hash(resolved sources + mode)
```

where `ScopedChain` is the compiled multi-pass program list (the same shape as
`custom_global_passes` — a `Vec<ShaderProgram>`; v1 has no per-pass buffer programs since feedback
is deferred). A free function `scoped_key(passes: &[(String, bool)]) -> u64` hashes the resolved
pass sources + hyprland flags.

- On config (re)load, collect every distinct scoped source set across all window-rule `shader`s and
  all `region-shader`s, compile each missing key into `scoped` (reusing `compile_global_program`),
  and drop+`destroy()` keys no longer referenced. Identical sources across many windows share one
  compiled chain.
- New `ProgramType::Scoped(u64, usize)` (key, pass index) resolves via
  `scoped.borrow().get(&key)?.get(i)`. `ShaderRenderElement` already resolves by `ProgramType` at
  draw, so it needs only the new variant.
- A `ScopedShader` resolved config type carries the resolved `passes: Vec<(String,bool)>` and its
  `key` so render sites look up by key without recompiling.

### 3.B — `ScopedShaderElement` (one element, both targets)

A new render element (sibling of `GlobalShaderElement`) parameterized by:

```text
{ key: u64, area: Rectangle<Logical>, region_norm: [f32;4], size_phys, cursor, time,
  source: ScopedSource }
ScopedSource = Capture            // region: capture the composited framebuffer in `area`
             | Texture(GlesTexture)  // window: the pre-rendered tile offscreen
```

`draw()` mirrors the multi-pass loop in `GlobalShaderElement::draw` but:
- For `Capture` (region): capture the framebuffer region into `niri_screen` (as global does).
- For `Texture` (window): bind the provided offscreen texture as `niri_screen` (no capture).
- Bind `niri_source`/`niri_prev`/`niri_screen_prev`/`niri_buffer` all to that same `niri_screen`
  texture (v1: no per-scope history — keeps existing feedback shaders compiling and running, just
  without trails).
- Run passes `ProgramType::Scoped(key, i)` in order; intermediate passes → offscreen, last pass →
  composite into `dst`. No result capture (no ping-pong; feedback deferred).
- Uniforms: `niri_time`, `niri_cursor`, `niri_region`, `niri_output_size`, `niri_size` per §4.

### 3.C — Region shaders (render)

Resolved config: `Vec<RegionShader { geometry: Rectangle<Logical>, output: Option<String>, key }>`.

In `Niri::render_inner`, in the global-shader insertion zone, for each `RegionShader` whose
`output` matches the current output (or `None` = every output): compute `area` (the rect, output-
local logical) and `region_norm` (rect normalised to the output), and push a `ScopedShaderElement`
with `source = Capture`. Multiple regions push multiple elements (config order = draw order).
Gated `RenderTarget::Output`, like the global shader.

### 3.D — Window shaders (render)

`ResolvedWindowRules` gains `shader: Option<ScopedShader>` (resolved source set + key), computed in
`ResolvedWindowRules::compute()` with scalar-override semantics (later match wins).

In `Tile::render()`, extend the offscreen-routing branch (alongside `AlphaAnimation`): when the
window's resolved rule has a `shader` **and** the program for its key exists:
1. Render the tile's normal content (`render_inner` output) into the tile's `OffscreenBuffer`
   (reuse/extend the alpha path's offscreen; one buffer per tile).
2. Emit a `ScopedShaderElement` with `source = Texture(offscreen)`, `area` = the window geometry,
   `region_norm = (0,0,1,1)`, `niri_size` = window physical size, `niri_cursor` = window-local.
No shader configured → the existing path runs unchanged (no offscreen, byte-identical). The shader
applies to the **window content only**; borders/shadow/popups render around it as today.

### 3.E — Caps / redraw

Reuse `GlobalShaderCaps::scan_chain(passes)` per scoped source.
- A region shader using `niri_time` → its output schedules continuous redraw while it is active
  (same mechanism the global shader uses; OR'd into the per-output redraw decision).
- A window shader using `niri_time` → the tile marks itself damaged each frame (hook the tile's
  existing animation-redraw signal so an animated scoped shader keeps the window repainting).
- Static scoped shaders redraw only on the target's own damage (window damage / region overlap
  damage) — no forced idle redraws.

### 3.F — Config (niri-config)

- **Window:** `WindowRule` gains `shader: Option<ScopedShaderPart>`; `ScopedShaderPart` mirrors the
  global-shader shape (`source`/`path`/`mode`/repeatable `pass{}`). Resolution into `ScopedShader`
  reuses the global `pass_sources` resolution logic (factor it to be reusable).
- **Region:** a repeatable top-level `region-shader` node:
  `region-shader { geometry x=.. y=.. width=.. height=..; output "NAME"; source/path/mode; pass{} }`.
  `geometry` = output-local logical px; `output` optional (default = same rect on every output).
- Both resolve to the shared `ScopedShader { passes: Vec<(String,bool)>, key }` (the `(String,bool)`
  being `(resolved source, hyprland)` per pass, exactly like the global path).

---

## 4. Shader contract per scope (niri-mode `global_prelude` reused)

| Binding | Window shader | Region shader |
|---|---|---|
| `niri_screen` / `tex2D_screen` | window's own content (offscreen); `c.xy`=0..1 across window geo | composited screen in the rect; `c.xy`=0..1 across the region |
| `niri_source`, `niri_prev`, `niri_screen_prev`, `niri_buffer` | all alias `niri_screen` (no v1 history) | all alias `niri_screen` |
| `niri_size` | window size, physical px | region size, physical px |
| `niri_region` | `(0,0,1,1)` | rect in output-normalised coords (the `tex2D_*` remap divisor) |
| `niri_cursor` | window-local physical px (0,0 = window top-left) | output-local physical px |
| `niri_output_size` | full output physical px | full output physical px |
| `niri_time` | seconds since program activation | same |
| multi-pass `pass{}` | runs on the window offscreen; last pass composites in place | runs on the captured region |
| `global_buffer` | ignored (v1) | ignored (v1) |

No new GLSL dialect, prelude, or epilogue. The prelude is unchanged; only the host binds the
samplers/uniforms differently. An existing whole-output `global_color` shader runs unmodified.

---

## 5. Testing

- **`niri-config`:** `shader` window-rule child parses (source/path/pass/mode), override stacking
  (two matching rules → later `shader` wins); `region-shader` parses (geometry/output/source/pass);
  resolved `(String,bool)` pass list matches the global resolver; `scan_chain` reuse; new wiki KDL
  examples parse (`wiki_docs_parses`). `cargo test -p niri-config`.
- **Compile:** dev-shell `cargo check --no-default-features --features dbus,systemd`.
- **Regression (identity) — critical:** no `shader`/`region-shader` configured → byte-identical to
  today. Windows without a shader take the existing non-offscreen path; no region elements pushed.
- **Manual (sixseven):** grayscale/CRT on one window (terminal) while the rest of the screen is
  untouched; a `region-shader` rect running vignette over part of the output; a time-animated
  scoped shader animating while idle; two windows with *different* shaders simultaneously (proves
  multiple-independent + the source-keyed cache); the same shader on two windows (proves dedupe).
  KMS capture only.

---

## 6. Scope boundaries (YAGNI / v1)

- **No per-scope feedback:** `niri_prev`/`niri_screen_prev`/`niri_buffer` alias `niri_screen`;
  `global_buffer` ignored. (Per-window/per-region trails = v2 — needs per-target ping-pong state.)
- **No layer-shell target, no backdrop-behind target** (background_effect blur already covers the
  latter).
- Window shader covers **window content only** (not border/shadow/popups).
- One rect per `region-shader`; overlapping regions draw in config order; rects are static (no
  animation/follow).
- Reuses the global-shader multi-pass chain + niri-mode contract; no new authoring dialect.
- TTY/DRM only; excluded from screencast/screenshot sinks (same as global-shader).
- Does not touch the existing global-shader, transform, or iGPU items.

---

## 7. Suggested build order (for the plan phase)

Likely **two implementation plans** sharing substrate #1; one spec (this) covers the whole model.

1. **Shared substrate:** `ScopedShader`/`ScopedShaderPart` resolved config types + reusable pass
   resolver; `Shaders.scoped` source-keyed cache + `ProgramType::Scoped(key, i)` +
   `set_scoped_programs` (compile/dedupe/destroy on reload); `ScopedShaderElement` (both `Capture`
   and `Texture` sources). Verified by `cargo check` + niri-config tests.
2. **Region shaders:** `region-shader` config + resolution + render insertion in `render_inner` +
   per-output redraw for animated regions. Smaller; closest to the existing global-shader element.
   Independently shippable + manually verifiable.
3. **Window shaders:** `shader` window-rule field + `ResolvedWindowRules` resolution + the tile
   offscreen-routing path in `Tile::render` + per-tile animated-redraw. The larger piece; touches
   the layout/tile render path.

---

## 8. Open questions for the plan phase

- **`scoped_key` hashing:** hash the concatenated resolved pass sources + hyprland flags (a stable
  `DefaultHasher`/FNV). Confirm no `Date.now()`/nondeterminism; pure function of source.
- **Per-tile `OffscreenBuffer` ownership for window shaders:** reuse the alpha-animation offscreen
  field on the tile, or add a dedicated one? Decide against double-routing when both an alpha
  animation and a scoped shader are active on the same window (chain them: tile → offscreen →
  shader → alpha, or document the interaction).
- **Region rect under output scale/transform:** `geometry` is logical px; confirm `region_norm`
  derivation matches the existing cursor-radius normalisation (`src/niri.rs:~4385`).
- **Window-local `niri_cursor`:** confirm window geometry origin in output-local physical px is
  available at the tile render site (it is used for hit-testing already).
- **Redraw hook for animated window shaders:** identify the exact tile/layout signal that an open/
  resize animation uses to keep repainting, and reuse it for `niri_time` scoped shaders.

---

## 9. Build / test crib

Inherited from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, per-task `cargo
check`, the insta-snapshot workaround for `niri-config/src/lib.rs`, the flake deploy to "sixseven",
KMS-only recording). Not duplicated here.
