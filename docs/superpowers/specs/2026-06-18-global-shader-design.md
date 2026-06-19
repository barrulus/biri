# Global Post-Process Shader — Design

**Date:** 2026-06-18
**Status:** Approved (pending spec review)
**Target:** biri (niri Wayland compositor fork)

## Summary

Add a compositor-level **global shader**: a user-supplied GLSL fragment shader that
runs as a full-output post-processing pass every frame, applied to the entire
composited screen across all clients (Chromium, LibreOffice, terminals, etc.).

The shader can read the composited screen, the previous frame, the cursor
position, and time/resolution — enabling color grading, CRT/vignette/night
effects, distortion, and cursor-following effects (glow, trails via frame
feedback). It is config-driven, hot-recompiled on config reload, and **gated**:
when no shader is configured there is zero rendering overhead and normal
damage tracking / scanout behavior is preserved.

## Goals

- A single config-driven full-output GLSL pass, applied to real content of all apps.
- Shader inputs: elapsed time, output size/scale, cursor position, the composited
  screen texture, and the previous frame's output texture.
- Two authoring modes: a guard-railed niri-style contract, and a Hyprland
  `screen_shader`-compatible raw mode so existing community shaders mostly drop in.
- Hot reload on config change, matching the existing custom animation shaders.
- Zero overhead when no global shader is configured.

## Non-Goals (v1)

- Backends other than TTY/DRM. winit/headless render without the effect.
- Applying the effect to the screencast and screenshot render sinks.
- Per-output / per-window distinct shaders (single global shader for v1).
- A general multi-pass / shader-chain pipeline (single pass only).
- Transpiling Hyprland `#version 300 es` shaders (only the GLES2 `#version 100`
  dialect is fully supported; see Limitations).

## Background: existing machinery this reuses

The codebase already ships every primitive needed; this feature composes them.

- **Custom shader pattern** — `src/render_helpers/shaders/mod.rs`. The
  resize/open/close window animations are config-driven GLSL wrapped in
  `*_prelude.frag` + user source + `*_epilogue.frag`, compiled via
  `ShaderProgram::compile()`, stored in `Shaders` as
  `custom_resize/close/open: RefCell<Option<ShaderProgram>>`, swapped at runtime
  by `set_custom_*_program()` / `replace_custom_*_program()`. This is the exact
  template for the global shader.
- **Full-output effect pattern** — `src/ui/screen_transition.rs`. `ScreenTransition`
  captures the whole output into a texture and redraws it as one full-screen
  render element. Template for "cover the whole screen."
- **Framebuffer capture** — `src/render_helpers/framebuffer_effect.rs`.
  `FramebufferEffect::capture_framebuffer()` does `glBlitFramebuffer` mid-render to
  grab the pixels rendered below it, then runs a post-process shader in `draw()`.
  This provides the `niri_screen` sampler.
- **GLSL dialect** — `src/render_helpers/shader_element.rs:74` compiles all custom
  shaders as `#version 100` (GLES2 / ESSL 1.00): `varying`, `gl_FragColor`,
  `texture2D`. This matches the dialect most Hyprland community `screen_shader`s
  use, making compat mode mostly name-aliasing rather than transpilation.
- **Element collection** — `Niri::render_inner()` in `src/niri.rs` (~4192) gathers
  render elements per output, front-to-back (first pushed = topmost); the cursor
  is pushed near the top (~4213). `render_to_vec()` (~4149) returns the list to the
  backend.
- **Cursor / software cursor** — `Niri::render_pointer()` in `src/niri.rs`
  (~3686). Hardware cursor plane is toggled by a flag in `src/backend/tty.rs`
  (~1917, `disable_cursor_plane` / removing the cursor-plane scanout flag).

## Architecture

```
render_inner()  [src/niri.rs ~4192]
  ├─ push cursor element (topmost)            unless reads-cursor → see Cursor handling
  ├─ push GlobalShaderElement                 ONLY if a compiled global program exists
  │     └─ draw():
  │          1. glBlitFramebuffer: capture framebuffer below → niri_screen texture
  │          2. bind last frame's output texture as niri_prev
  │          3. run global program (uniforms: niri_time/size/scale/cursor; samplers)
  │          4. draw full-output quad with the program's result
  │          5. capture result → stored as next frame's niri_prev (per output)
  ├─ push screen transition / dialogs / windows / backdrop  (unchanged)
  └─ return via render_to_vec()
```

When no global program is compiled, `GlobalShaderElement` is **not pushed**, so the
element list, damage tracking, and plane/scanout behavior are byte-for-byte
unchanged from upstream.

### Components

#### 1. Config — `niri-config`

New `global-shader { }` block (KDL), parsed with the existing `knuffel` derive
pattern used elsewhere in the config crate:

```kdl
global-shader {
    // off by default; absence of the block == disabled
    enable

    // exactly one source: inline or file path
    source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
    // path "/home/user/.config/niri/shaders/crt.frag"

    mode "niri"        // "niri" (guard-railed) | "hyprland" (raw compat). default "niri"
    reads-cursor       // if present, forces software cursor (effect consumes the pointer)
}
```

- `enable` (bool child) and `reads-cursor` (bool child) follow the existing
  `#[knuffel(child)]` bool-field convention.
- `source` / `path` are mutually exclusive; `path` is read from disk at config
  load/reload time. If both or neither are present with `enable`, emit a config
  warning and treat as disabled (do not hard-fail config parsing).
- `mode` is an enum string with default `niri`.
- Validation: an empty/whitespace source with `enable` → warning + disabled.

Snapshot/parse tests added to `niri-config/src/lib.rs` (inline
`assert_debug_snapshot!`), consistent with existing config tests.

#### 2. Shader registry & compilation — `src/render_helpers/shaders/mod.rs`

- Add field `custom_global: RefCell<Option<ShaderProgram>>` to `Shaders`
  (initialized `None` in `compile()`).
- Add `fn compile_global_program(renderer, src, mode) -> Result<ShaderProgram, GlesError>`:
  - **niri mode:** `global_prelude.frag` + user source + `global_epilogue.frag`.
  - **hyprland mode:** `global_hypr_prelude.frag` + user source verbatim (no
    epilogue; the user shader supplies its own `main()`).
  - Registered uniforms: `niri_time` (`_1f`), `niri_cursor` (`_2f`).
    (`niri_size`, `niri_scale`, `niri_alpha`, matrices are already standard in
    `ShaderProgram`/`compile_program`.)
  - Texture uniforms: `niri_screen`, `niri_prev`.
- Add `pub fn set_custom_global_program(renderer, src: Option<&str>, mode)` and
  `replace_custom_global_program(...)`, mirroring the resize/close/open trio
  (compile → replace → destroy previous program).

#### 3. Shader contract files — `src/render_helpers/shaders/`

`global_prelude.frag` (niri mode) declares, in `#version 100` style:

```glsl
precision highp float;
#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif
varying vec2 niri_v_coords;     // 0..1 across the output
uniform vec2 niri_size;         // output size in physical px
uniform float niri_scale;
uniform float niri_alpha;
uniform float niri_time;        // seconds since shader activation
uniform vec2 niri_cursor;       // cursor position in output coords
uniform sampler2D niri_screen;  // composited frame below this element
uniform sampler2D niri_prev;    // previous frame's output

vec4 tex2D_screen(vec2 uv) { return texture2D(niri_screen, uv); }
vec4 tex2D_prev(vec2 uv)   { return texture2D(niri_prev, uv); }
// user defines: vec4 global_color(vec3 coord);
```

`global_epilogue.frag`:

```glsl
void main() {
    vec3 coord = vec3(niri_v_coords, 1.0);
    vec4 color = global_color(coord);
    color = color * niri_alpha;
#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0) color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
```

`global_hypr_prelude.frag` (hyprland mode) defines the Hyprland names as aliases,
then the user's raw shader (with its own `main()`) follows:

```glsl
precision highp float;
varying vec2 niri_v_coords;
uniform sampler2D niri_screen;
uniform float niri_time;
#define tex niri_screen
#define v_texcoord niri_v_coords
#define time niri_time
#define wl_output 0
// user source (contains its own main(), writes gl_FragColor) appended verbatim
```

#### 4. `GlobalShaderElement` — `src/render_helpers/global_shader_element.rs` (new)

Modeled on `FramebufferEffectElement` + `ScreenTransition`.

- Constructed each frame (when a program exists) with: output size/scale, current
  cursor position (output coords), elapsed time, and a handle to the per-output
  `niri_prev` texture.
- Implements `RenderElement<GlesRenderer>` (and the TTY renderer alias used in the
  codebase). `draw()`:
  1. `glBlitFramebuffer` the area below into the `niri_screen` capture texture
     (reuse the approach in `framebuffer_effect.rs`).
  2. Bind the stored previous-frame texture as `niri_prev` (first frame: bind the
     freshly captured `niri_screen` so feedback shaders start stable).
  3. Set uniforms (`niri_time`, `niri_cursor`) and draw a full-output quad through
     the global program.
  4. Capture the drawn result into the per-output texture to serve as next frame's
     `niri_prev`.
- `opaque_regions`: report none (the shader may produce translucency / read below).
- Damage: report the full output as damaged each frame while active (accepted cost).

#### 5. Per-output state — `src/niri.rs`

Add to the per-output state struct (`OutputState`):
- `global_shader_prev: Option<GlesTexture>` — ping-pong previous-frame texture.
- `global_shader_start: Option<Instant>` — origin for `niri_time` (set when the
  program becomes active; cleared when disabled).

These are owned outside the element so the element stays cheap to construct and
the texture persists across frames.

#### 6. Wiring — `src/niri.rs`

- **Config reload:** alongside the existing resize/close/open shader handling
  (~1548–1576), detect changes to `config.global-shader` (source/path/mode) and
  call `shaders::set_custom_global_program(...)` via
  `backend.with_primary_renderer(...)`. Reset `global_shader_start`/`_prev` when
  the program toggles on/off or its source changes.
- **`render_inner()`:** if `Shaders::get(renderer)` has a compiled global program,
  build and push `GlobalShaderElement` at the top of the element stack. Ordering vs
  the cursor:
  - default (`reads-cursor` off): push the global element **below** the cursor, so
    the cursor renders on top untouched; keep hardware cursor.
  - `reads-cursor` on: push the global element **above** the cursor element (so the
    cursor is part of `niri_screen`) and force software cursor for the output.
- **Software cursor forcing:** when `reads-cursor` is on and a program is active,
  set the existing software-cursor path in `src/backend/tty.rs` (~1917). Restore
  hardware cursor when disabled.

### Cursor handling

| `reads-cursor` | Cursor plane | Element order | Shader sees cursor pixels |
|----------------|--------------|---------------|---------------------------|
| off (default)  | hardware     | shader below cursor | no — but `niri_cursor` position is still available for position-driven effects |
| on             | software (forced) | shader above cursor | yes — cursor is composited into `niri_screen` |

Position-driven effects (glow/highlight that follow the pointer) work without
forcing software cursor, because `niri_cursor` is always provided. Only effects
that must distort or blend the actual cursor pixels need `reads-cursor`.

## Data flow (per frame, shader active)

1. `render_inner()` reads cursor position, computes `niri_time` from
   `global_shader_start`, builds `GlobalShaderElement` with the per-output
   `global_shader_prev` texture.
2. Element pushed into the list; backend composites elements below it normally.
3. `GlobalShaderElement::draw()` captures the screen, binds prev, runs the program,
   draws, and captures the result into `global_shader_prev`.
4. Next frame, that texture is `niri_prev`.

## Error handling

- **Compile failure:** `set_custom_global_program` logs a warning (matching
  existing custom shaders) and leaves the previous program in place / disabled.
  Compositor never crashes on a bad shader.
- **Bad config:** invalid `global-shader` block → warning + treated as disabled;
  config parsing does not hard-fail.
- **Missing/unreadable `path`:** warning + disabled.
- **First frame / no prev texture:** bind the freshly captured screen as `niri_prev`
  to avoid sampling an uninitialized texture.
- **Disable path:** clearing the config destroys the program, frees
  `global_shader_prev`, restores hardware cursor, and stops pushing the element —
  returning to zero-overhead behavior.

## Testing

- **Config parse tests** (`niri-config/src/lib.rs`, inline `assert_debug_snapshot!`):
  block present/absent, inline vs path, both/neither source (warning path),
  `mode` values, `reads-cursor` on/off.
- **Shader compile tests:** a trivial niri-mode `global_color` and a trivial
  hyprland-mode raw shader each compile without error; a deliberately broken
  shader fails gracefully (program remains `None`, no panic).
- **Gating assertion:** with no `global-shader` configured, the element list from
  `render_to_vec()` is identical to upstream (no `GlobalShaderElement` present).
- **Manual / visual (TTY):** color-grade shader visibly affects all apps; a
  cursor-position glow tracks the pointer with hardware cursor; a feedback-trail
  shader (samples `niri_prev`) leaves a fading trail; toggling the config block on/off
  hot-reloads; a known Hyprland community shader (GLES2 dialect) renders in
  `mode "hyprland"`.

## Limitations / future work

- Only the GLES2 (`#version 100`) Hyprland dialect is supported directly; Hyprland
  shaders authored as `#version 300 es` need manual edits (`texture()` →
  `texture2D()`, `out` color → `gl_FragColor`, `in` → `varying`). Documented.
- While active: full-output damage every frame (no partial-damage power saving),
  no direct scanout / overlay-plane offload. This is inherent to a whole-screen
  post-process and is documented.
- v1 is TTY/DRM only. winit/headless and the screencast/screenshot sinks render
  without the effect; extending to them is future work.
- Single global shader only; no multi-pass chain and no per-output/per-window
  variants in v1.

## Docs

- New section in `docs/wiki/` (e.g. `Configuration:-Global-Shader.md`) documenting
  the config block, both authoring modes, available uniforms/samplers, the
  `reads-cursor` cost, and the Hyprland-compat caveats.

## Touched files (anticipated)

- `niri-config/src/...` — new `GlobalShader` config struct + wiring into the root
  config; parse tests in `niri-config/src/lib.rs`.
- `src/render_helpers/shaders/mod.rs` — `custom_global` field, compile/set/replace.
- `src/render_helpers/shaders/global_prelude.frag`,
  `global_epilogue.frag`, `global_hypr_prelude.frag` — new.
- `src/render_helpers/global_shader_element.rs` — new element.
- `src/render_helpers/mod.rs` — module registration.
- `src/niri.rs` — per-output state, config-reload detection, `render_inner()` wiring,
  cursor ordering.
- `src/backend/tty.rs` — software-cursor forcing when `reads-cursor` is active.
- `resources/default-config.kdl` — commented-out example block.
- `docs/wiki/Configuration:-Global-Shader.md` — new docs page.
