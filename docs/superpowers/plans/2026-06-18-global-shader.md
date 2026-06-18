# Global Post-Process Shader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a config-driven, full-output GLSL post-processing shader to biri that runs every frame over the entire composited screen across all clients, with access to time, resolution, cursor position, the screen texture, and the previous frame.

**Architecture:** Mirror the existing custom-animation-shader machinery (`shaders/mod.rs` compile/set/replace trio + prelude/epilogue `.frag` wrapping) for compilation, and the existing `FramebufferEffectElement` (capture-framebuffer-below + run-a-shader) for rendering. A new `GlobalShaderElement` captures the composited output, binds the previous frame, runs the user program full-screen, and stores its result as next frame's previous. It is pushed in `render_inner()` only when a program is compiled — so with no shader configured the element list, damage tracking, and scanout are byte-for-byte unchanged.

**Tech Stack:** Rust, smithay (`GlesRenderer`/`GlesFrame`/`GlesTexture`), KDL config via `knuffel` derive + hand-written `ConfigPart::decode_children`, GLSL `#version 100` (GLES2), `insta` snapshot tests.

## Global Constraints

- GLSL is compiled as `#version 100` (GLES2 / ESSL 1.00): use `varying`, `gl_FragColor`, `texture2D` — not `in`/`out`/`texture`. (`src/render_helpers/shader_element.rs:74`)
- Config uses the two-type convention: a final `Foo` type (with `Default`) plus a `FooPart` type (`#[derive(knuffel::Decode)]`, all-`Option`/`Flag` fields) and a `MergeWith<FooPart> for Foo` impl. Top-level nodes are dispatched in `ConfigPart::decode_children` (`niri-config/src/lib.rs:193`).
- Bool config children use `Option<Flag>` in the `Part` and the `merge!` macro (NOT raw `bool`), matching `CursorPart::hide_when_typing` (`niri-config/src/misc.rs:42`).
- Never hard-fail config parsing on a bad shader block: emit a `warn!` and treat as disabled.
- A bad/uncompilable shader must never crash the compositor: log `warn!` and keep the previous program (matching `set_custom_close_program`, `src/render_helpers/shaders/mod.rs:293`).
- No `Co-Authored-By` / AI-attribution lines in commits (user global preference).
- Run `cargo +nightly fmt` before each commit; renderer tasks must pass `cargo build` and `cargo clippy`.
- biri has **no automated GLES/render unit tests**. Config tasks use strict TDD (`cargo test -p niri-config`). Renderer tasks are verified by `cargo build` + `cargo clippy` + the manual TTY checklist in the final task.

---

## File Structure

**Created:**
- `niri-config/src/global_shader.rs` — `GlobalShader` / `GlobalShaderPart` / `MergeWith` + parse tests.
- `src/render_helpers/shaders/global_prelude.frag` — niri-mode uniforms + helpers.
- `src/render_helpers/shaders/global_epilogue.frag` — niri-mode `main()` wrapper.
- `src/render_helpers/shaders/global_hypr_prelude.frag` — hyprland-compat aliases.
- `src/render_helpers/global_shader_element.rs` — the `GlobalShaderElement` render element.
- `docs/wiki/Configuration:-Global-Shader.md` — user docs.

**Modified:**
- `niri-config/src/lib.rs` — `pub mod global_shader;`, re-export, `Config.global_shader` field, `"global-shader"` dispatch.
- `src/render_helpers/shaders/mod.rs` — `custom_global` field, `ProgramType::Global`, compile/set/replace, prelude selection by mode.
- `src/render_helpers/mod.rs` — `pub mod global_shader_element;`.
- `src/niri.rs` — `OutputState` fields, config-reload detection, `render_inner()` wiring + cursor ordering.
- `src/backend/tty.rs` — force software cursor when `reads-cursor` is active.
- `resources/default-config.kdl` — commented example block.

---

## Task 1: Config — `GlobalShader` parsing

**Files:**
- Create: `niri-config/src/global_shader.rs`
- Modify: `niri-config/src/lib.rs` (module decl + re-export + `Config` field + dispatch)
- Test: `niri-config/src/global_shader.rs` (`#[cfg(test)]`) and `niri-config/src/lib.rs` (existing `parse` snapshot area)

**Interfaces:**
- Produces:
  - `pub struct GlobalShader { pub enable: bool, pub source: Option<String>, pub path: Option<String>, pub mode: String, pub reads_cursor: bool }` (impl `Default`, `mode` defaults to `"niri"`).
  - `pub struct GlobalShaderPart` (knuffel-decoded).
  - `impl MergeWith<GlobalShaderPart> for GlobalShader`.
  - `Config.global_shader: GlobalShader`.

- [ ] **Step 1: Write the failing tests**

Create `niri-config/src/global_shader.rs`:

```rust
use crate::utils::{Flag, MergeWith};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShader {
    pub enable: bool,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub reads_cursor: bool,
}

impl Default for GlobalShader {
    fn default() -> Self {
        Self {
            enable: false,
            source: None,
            path: None,
            mode: String::from("niri"),
            reads_cursor: false,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq, Eq)]
pub struct GlobalShaderPart {
    #[knuffel(child)]
    pub enable: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(child)]
    pub reads_cursor: Option<Flag>,
}

impl MergeWith<GlobalShaderPart> for GlobalShader {
    fn merge_with(&mut self, part: &GlobalShaderPart) {
        merge!((self, part), enable, reads_cursor);
        merge_clone_opt!((self, part), source, path);
        merge_clone!((self, part), mode);
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn global_shader_defaults_disabled() {
        let config = Config::parse_mem("").unwrap();
        assert!(!config.global_shader.enable);
        assert_eq!(config.global_shader.mode, "niri");
        assert!(config.global_shader.source.is_none());
        assert!(!config.global_shader.reads_cursor);
    }

    #[test]
    fn global_shader_inline_source() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
                mode "niri"
                reads-cursor
            }
            "##,
        )
        .unwrap();
        assert!(config.global_shader.enable);
        assert!(config.global_shader.reads_cursor);
        assert_eq!(config.global_shader.mode, "niri");
        assert_eq!(
            config.global_shader.source.as_deref(),
            Some("vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }")
        );
    }

    #[test]
    fn global_shader_path_and_hyprland_mode() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                path "/tmp/crt.frag"
                mode "hyprland"
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.global_shader.path.as_deref(), Some("/tmp/crt.frag"));
        assert_eq!(config.global_shader.mode, "hyprland");
    }
}
```

Note: `merge!`, `merge_clone!`, `merge_clone_opt!` are crate-level macros (used in `niri-config/src/misc.rs:48-53`); `#[macro_use] pub mod macros;` at `lib.rs:28-29` makes them available crate-wide. `Flag` and `MergeWith` come from `crate::utils` (see `misc.rs:2`).

- [ ] **Step 2: Wire the module and `Config` field**

In `niri-config/src/lib.rs`, add the module decl alongside the others (after `pub mod gestures;`, `lib.rs:36`):

```rust
pub mod global_shader;
```

Add the re-export near the other `pub use` lines (after `lib.rs:52`):

```rust
pub use crate::global_shader::{GlobalShader, GlobalShaderPart};
```

Add the field to `Config` (`lib.rs:69-95`), after the `gestures` field:

```rust
    pub global_shader: GlobalShader,
```

Add the dispatch arm in `ConfigPart::decode_children`, in the `m_merge!` group (after the `"gestures" => m_merge!(gestures),` line, `lib.rs:201`):

```rust
                "global-shader" => m_merge!(global_shader),
```

- [ ] **Step 3: Run tests to verify they fail, then pass**

Run: `cargo test -p niri-config global_shader -- --nocapture`
Expected before Step 2 wiring: compile error / FAIL. After Step 2: PASS (3 tests).

- [ ] **Step 4: Update the big `parse` snapshot test**

The `parse` test at `niri-config/src/lib.rs:658` snapshots a full `Config`. Adding a field changes the `Debug` output. Run:

Run: `cargo test -p niri-config parse 2>&1 | head -40`
Expected: FAIL with an `insta` mismatch showing a new `global_shader: GlobalShader { ... }` line.

Add `global_shader: GlobalShader { enable: false, source: None, path: None, mode: "niri", reads_cursor: false }` to the inline snapshot at the correct position (mirroring the field order in `Config`). If the project's `insta` workflow is available and reliable, instead run `cargo insta accept` — but per project memory `cargo insta` can hang, so prefer editing the inline snapshot by hand from the test failure diff.

Run: `cargo test -p niri-config parse`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
cargo +nightly fmt
git add niri-config/src/global_shader.rs niri-config/src/lib.rs
git commit -m "config: add global-shader block parsing"
```

---

## Task 2: Shader contract `.frag` files + registry (compile/set/replace)

**Files:**
- Create: `src/render_helpers/shaders/global_prelude.frag`
- Create: `src/render_helpers/shaders/global_epilogue.frag`
- Create: `src/render_helpers/shaders/global_hypr_prelude.frag`
- Modify: `src/render_helpers/shaders/mod.rs`

**Interfaces:**
- Produces:
  - `Shaders.custom_global: RefCell<Option<ShaderProgram>>`
  - `ProgramType::Global`
  - `pub fn set_custom_global_program(renderer: &mut GlesRenderer, src: Option<&str>, hyprland: bool)`
  - `Shaders::replace_custom_global_program(&self, Option<ShaderProgram>) -> Option<ShaderProgram>`
  - `Shaders::program(ProgramType::Global)` returns `custom_global`.
- Consumes: `ShaderProgram::compile` (`src/render_helpers/shader_element.rs:161`); uniform/texture conventions from `compile_close_program` (`shaders/mod.rs:270`).

- [ ] **Step 1: Create `global_prelude.frag`**

`src/render_helpers/shaders/global_prelude.frag` (niri mode). Modeled on `close_prelude.frag`:

```glsl
precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

varying vec2 niri_v_coords;   // 0..1 across the output
uniform vec2 niri_size;       // output size in physical px
uniform float niri_scale;
uniform float niri_alpha;

uniform float niri_time;      // seconds since shader activation
uniform vec2 niri_cursor;     // cursor position, output coords (px)

uniform sampler2D niri_screen; // composited frame below this element
uniform sampler2D niri_prev;   // previous frame's output

vec4 tex2D_screen(vec2 uv) { return texture2D(niri_screen, uv); }
vec4 tex2D_prev(vec2 uv) { return texture2D(niri_prev, uv); }

// User defines: vec4 global_color(vec3 coord);
```

- [ ] **Step 2: Create `global_epilogue.frag`**

`src/render_helpers/shaders/global_epilogue.frag` (modeled on `close_epilogue.frag`):

```glsl

void main() {
    vec3 coord = vec3(niri_v_coords, 1.0);
    vec4 color = global_color(coord);

    color = color * niri_alpha;

#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
```

- [ ] **Step 3: Create `global_hypr_prelude.frag`**

`src/render_helpers/shaders/global_hypr_prelude.frag`. The user's raw shader (with its own `main()` writing `gl_FragColor`) is appended verbatim after this, with no epilogue:

```glsl
precision highp float;

varying vec2 niri_v_coords;
uniform sampler2D niri_screen;
uniform float niri_time;

#define tex niri_screen
#define v_texcoord niri_v_coords
#define time niri_time
#define wl_output 0

// Hyprland-style user shader (its own main(), writes gl_FragColor) appended below.
```

- [ ] **Step 4: Add the registry field + `ProgramType::Global`**

In `src/render_helpers/shaders/mod.rs`, add to the `Shaders` struct (after `custom_open`, `mod.rs:23`):

```rust
    pub custom_global: RefCell<Option<ShaderProgram>>,
```

Initialize it in `Shaders::compile`'s returned `Self { ... }` (after `custom_open: RefCell::new(None),`, `mod.rs:161`):

```rust
            custom_global: RefCell::new(None),
```

Add the variant to `ProgramType` (`mod.rs:26-33`):

```rust
    Global,
```

In `Shaders::program` (`mod.rs:199-211`), add the match arm:

```rust
            ProgramType::Global => self.custom_global.borrow().clone(),
```

- [ ] **Step 5: Add compile/set/replace functions**

Add the `replace` method near `replace_custom_open_program` (`mod.rs:192`):

```rust
    pub fn replace_custom_global_program(
        &self,
        program: Option<ShaderProgram>,
    ) -> Option<ShaderProgram> {
        self.custom_global.replace(program)
    }
```

Add the compile + set functions near `set_custom_close_program` (`mod.rs:270`):

```rust
fn compile_global_program(
    renderer: &mut GlesRenderer,
    src: &str,
    hyprland: bool,
) -> Result<ShaderProgram, GlesError> {
    let mut program = if hyprland {
        include_str!("global_hypr_prelude.frag").to_string()
    } else {
        include_str!("global_prelude.frag").to_string()
    };
    program.push_str(src);
    if !hyprland {
        program.push_str(include_str!("global_epilogue.frag"));
    }

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_time", UniformType::_1f),
            UniformName::new("niri_cursor", UniformType::_2f),
        ],
        &["niri_screen", "niri_prev"],
    )
}

pub fn set_custom_global_program(renderer: &mut GlesRenderer, src: Option<&str>, hyprland: bool) {
    let program = if let Some(src) = src {
        match compile_global_program(renderer, src, hyprland) {
            Ok(program) => Some(program),
            Err(err) => {
                warn!("error compiling custom global shader: {err:?}");
                return;
            }
        }
    } else {
        None
    };

    if let Some(prev) = Shaders::get(renderer).replace_custom_global_program(program) {
        if let Err(err) = prev.destroy(renderer) {
            warn!("error destroying previous custom global shader: {err:?}");
        }
    }
}
```

Note: `niri_size`, `niri_scale`, `niri_alpha`, and the `niri_tint` debug uniform are standard in every `ShaderProgram` (declared/located in `compile_program`, `shader_element.rs:80-87`), so they are NOT passed in `additional_uniforms`.

- [ ] **Step 6: Build + clippy**

Run: `cargo build 2>&1 | tail -20`
Expected: builds (warnings about unused `set_custom_global_program` / `ProgramType::Global` are acceptable at this stage — they are consumed in Tasks 4 & 6).

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new errors.

- [ ] **Step 7: Format and commit**

```bash
cargo +nightly fmt
git add src/render_helpers/shaders/
git commit -m "render: add global shader compile/set/replace + contract frag files"
```

---

## Task 3: `GlobalShaderElement` — passthrough capture + draw

This task builds the render element that captures the composited framebuffer below it and re-draws it through the global program. It starts as a **passthrough**: `niri_prev` is bound to the freshly-captured screen and `niri_time`/`niri_cursor` are fixed zeros. Prev-frame ping-pong, real time, and cursor come in Task 4. Verified by build + manual TTY check.

**Files:**
- Create: `src/render_helpers/global_shader_element.rs`
- Modify: `src/render_helpers/mod.rs` (module decl)

**Interfaces:**
- Consumes: `Shaders::get_from_frame(frame).custom_global` / `Shaders::program(ProgramType::Global)`; the framebuffer-capture pattern from `src/render_helpers/framebuffer_effect.rs:156-319`; `ShaderRenderElement` uniform/texture mechanism (`src/render_helpers/shader_element.rs`).
- Produces:
  - `pub struct GlobalShaderElement` implementing `RenderElement<GlesRenderer>` and the TTY renderer (`TtyRenderer`) the codebase uses for output elements.
  - `pub fn GlobalShaderElement::new(id: Id, area: Rectangle<f64, Logical>, scale: f32, time: f32, cursor: (f32, f32), prev: Option<GlesTexture>) -> Self`
  - `pub fn GlobalShaderElement::into_texture(self) -> Option<GlesTexture>` returning the captured-result texture for ping-pong (populated after `draw`). For this task it may return `None`; Task 4 wires it.

- [ ] **Step 1: Scaffold the module by copying `framebuffer_effect.rs`**

`framebuffer_effect.rs` already implements exactly the hard part: a `RenderElement` that, in `draw()`, blits the framebuffer region below it into a `GlesTexture` and re-renders it through a program. Create `src/render_helpers/global_shader_element.rs` by adapting it:

- Keep the imports block from `framebuffer_effect.rs:1-21` (the `ffi`, `GlesFrame`, `GlesTexture`, `Rectangle`, `UserDataMap`, `Shaders` imports).
- Replace the struct with:

```rust
#[derive(Debug, Clone)]
pub struct GlobalShaderElement {
    id: Id,
    commit: CommitCounter,
    area: Rectangle<f64, Logical>,
    scale: f32,
    time: f32,
    cursor: (f32, f32),
    prev: Option<GlesTexture>,
}

impl GlobalShaderElement {
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        prev: Option<GlesTexture>,
    ) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            area,
            scale,
            time,
            cursor,
            prev,
        }
    }
}
```

- [ ] **Step 2: Implement the `Element` trait**

Mirror the `impl Element for FramebufferEffectElement` block in `framebuffer_effect.rs` (search for `fn id`, `fn geometry`, `fn src`, `fn transform`, `fn current_commit`, `fn opaque_regions`). For `GlobalShaderElement`:
- `id()` → `&self.id`
- `current_commit()` → `self.commit`
- `geometry(scale)` → `self.area.to_physical_precise_round(scale)` (full output area)
- `opaque_regions()` → return empty (`vec![]`) — the shader may produce translucency and reads what's below.
- `src()` → `Rectangle::from_size(self.area.size.to_buffer(1.0, Transform::Normal))` matching the framebuffer-effect element's `src` form.

Copy the exact method shapes from `framebuffer_effect.rs`'s `Element` impl; only the `geometry`/`opaque_regions` bodies differ as above.

- [ ] **Step 3: Implement `draw` (capture + passthrough)**

Implement `RenderElement<GlesRenderer>::draw` by copying the capture machinery from `FramebufferEffectElement::capture_framebuffer` (`framebuffer_effect.rs:156-319`) — specifically:
- The `GlesTexture` allocation block (`framebuffer_effect.rs:225-232`): `renderer.create_buffer(Fourcc::Abgr8888, size)?`, sized to the physical `dst` size.
- The `glBlitFramebuffer` block verbatim (`framebuffer_effect.rs:252-298`) to copy the framebuffer below into that texture (`niri_screen`).

Then render the captured texture through the global program, modeled on `FramebufferEffectElement::draw` (`framebuffer_effect.rs:391-409`) but using the global program and binding two textures + the extra uniforms:

```rust
        let program = Shaders::get_from_frame(frame).program(ProgramType::Global);
        let Some(program) = program else {
            // No global program: draw the captured screen unchanged.
            return frame.render_texture_from_to(
                &screen_tex,
                Rectangle::from_size(screen_tex.size().to_f64()),
                dst,
                damage,
                &[],
                frame.transformation().invert(),
                1.,
                None,
                &[],
            );
        };

        // Passthrough for this task: bind the freshly captured screen as prev too.
        let prev_tex = self.prev.clone().unwrap_or_else(|| screen_tex.clone());

        let uniforms = [
            Uniform::new("niri_time", self.time),
            Uniform::new("niri_cursor", (self.cursor.0, self.cursor.1)),
        ];
        let textures = [
            ("niri_screen", &screen_tex),
            ("niri_prev", &prev_tex),
        ];
```

For binding multiple named texture samplers + custom uniforms, follow how `ShaderRenderElement::draw` sets `texture_uniforms` and `additional_uniforms` (`src/render_helpers/shader_element.rs`, the `draw` impl that binds `self.textures` and `self.additional_uniforms`). The cleanest path: construct a `ShaderRenderElement` with `ProgramType::Global`, the two textures, and the two uniforms, and delegate to its `draw` for the program pass — but since `ShaderRenderElement` does not capture the framebuffer, the capture (blit) must happen first in this element's `draw`, producing `screen_tex`, which is then passed as the `niri_screen` texture.

> **Implementation note for the engineer:** the rendering-with-named-samplers plumbing lives in `ShaderRenderElement::draw` (`shader_element.rs:290+`). Reuse it rather than re-deriving GL uniform-binding. The novel code here is only: (a) the blit capture (copied from `framebuffer_effect.rs`), and (b) handing the captured texture to the existing shader-render path as `niri_screen`. Do not write new raw `gl.*` uniform code.

- [ ] **Step 4: Register the module**

In `src/render_helpers/mod.rs`, add alongside the other decls (after `pub mod gradient_fade_texture;`, near `mod.rs:37`):

```rust
pub mod global_shader_element;
```

- [ ] **Step 5: Build + clippy**

Run: `cargo build 2>&1 | tail -30`
Expected: builds. `into_texture`/`time`/`cursor`/`prev` may warn as unused until Task 4/5 — acceptable.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new errors.

- [ ] **Step 6: Format and commit**

```bash
cargo +nightly fmt
git add src/render_helpers/global_shader_element.rs src/render_helpers/mod.rs
git commit -m "render: add GlobalShaderElement (capture + passthrough)"
```

---

## Task 4: Per-output state + prev-frame ping-pong + time/cursor

**Files:**
- Modify: `src/niri.rs` (`OutputState` struct)
- Modify: `src/render_helpers/global_shader_element.rs` (`into_texture`, real prev binding)

**Interfaces:**
- Consumes: `OutputState` (`src/niri.rs:447-487`); `GlobalShaderElement::new` / `into_texture` (Task 3).
- Produces:
  - `OutputState.global_shader_prev: Option<GlesTexture>`
  - `OutputState.global_shader_start: Option<std::time::Instant>`

- [ ] **Step 1: Add `OutputState` fields**

In `src/niri.rs`, add to `OutputState` (after `screen_transition`, `niri.rs:483`):

```rust
    /// Previous frame's output, fed to the global shader as `niri_prev`.
    pub global_shader_prev: Option<GlesTexture>,
    /// When the global shader became active; origin for `niri_time`.
    pub global_shader_start: Option<std::time::Instant>,
```

Initialize both to `None` wherever `OutputState` is constructed (search `OutputState {` in `niri.rs` — the field-init struct literal where `screen_transition: None` is set). Add:

```rust
            global_shader_prev: None,
            global_shader_start: None,
```

Ensure `GlesTexture` is imported in `niri.rs` (it is used by other render paths; if not present, add `use smithay::backend::renderer::gles::GlesTexture;`).

- [ ] **Step 2: Make the element output its captured result for ping-pong**

In `src/render_helpers/global_shader_element.rs`, after the program pass in `draw`, capture the rendered result (the element's own output region) into a texture stored in interior state, and expose it via `into_texture`. Use the same `create_buffer` + `glBlitFramebuffer` capture used for `niri_screen`, but performed *after* the program draw, reading the now-updated framebuffer region. Store it in a `RefCell<Option<GlesTexture>>` field on the element so `draw` (which takes `&self`) can write it:

```rust
    result: std::cell::RefCell<Option<GlesTexture>>,
```

Add to `new` (`result: RefCell::new(None)`) and:

```rust
    pub fn into_texture(self) -> Option<GlesTexture> {
        self.result.into_inner()
    }
```

Bind `self.prev` (not the screen) as `niri_prev` now that ping-pong is wired; on the first frame `self.prev` is `None`, so fall back to `screen_tex` (already handled in Task 3 Step 3).

- [ ] **Step 3: Build + clippy**

Run: `cargo build 2>&1 | tail -30`
Expected: builds.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new errors.

- [ ] **Step 4: Format and commit**

```bash
cargo +nightly fmt
git add src/niri.rs src/render_helpers/global_shader_element.rs
git commit -m "render: per-output global shader prev-frame state + result capture"
```

---

## Task 5: Wiring — config reload, `render_inner` push, gating

**Files:**
- Modify: `src/niri.rs` (config-reload detection + `render_inner` push)
- Modify: `resources/default-config.kdl`

**Interfaces:**
- Consumes: `set_custom_global_program` (Task 2); `GlobalShaderElement::new` / `into_texture` (Tasks 3-4); `OutputState.global_shader_*` (Task 4); pointer push site (`niri.rs:4212-4214`); screen-transition push pattern (`niri.rs:4217-4221`).

- [ ] **Step 1: Config-reload detection**

In `src/niri.rs`, in the config-reload block alongside the existing custom-shader detection (`niri.rs:1548-1576`), add (after the `window_open` block):

```rust
        if config.global_shader.enable != old_config.global_shader.enable
            || config.global_shader.source != old_config.global_shader.source
            || config.global_shader.path != old_config.global_shader.path
            || config.global_shader.mode != old_config.global_shader.mode
        {
            let src = global_shader_source(&config.global_shader);
            let hyprland = config.global_shader.mode == "hyprland";
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_global_program(renderer, src.as_deref(), hyprland);
            });
            // Reset per-output time origin / prev texture on (re)load.
            for state in self.output_state.values_mut() {
                state.global_shader_start = None;
                state.global_shader_prev = None;
            }
            shaders_changed = true;
        }
```

Add a free helper near the other config helpers in `niri.rs` (it resolves inline vs path and validates):

```rust
fn global_shader_source(cfg: &niri_config::GlobalShader) -> Option<String> {
    if !cfg.enable {
        return None;
    }
    match (&cfg.source, &cfg.path) {
        (Some(s), None) => {
            if s.trim().is_empty() {
                warn!("global-shader: empty source, disabling");
                None
            } else {
                Some(s.clone())
            }
        }
        (None, Some(p)) => match std::fs::read_to_string(p) {
            Ok(s) => Some(s),
            Err(err) => {
                warn!("global-shader: cannot read path {p:?}: {err}; disabling");
                None
            }
        },
        (Some(_), Some(_)) => {
            warn!("global-shader: both source and path set, disabling");
            None
        }
        (None, None) => {
            warn!("global-shader: enabled but no source or path, disabling");
            None
        }
    }
}
```

Also apply the global shader once at startup / initial renderer creation: find where `set_custom_resize_program` (or `shaders::init`) is first applied after the primary renderer is created, and add a matching `set_custom_global_program` call using `global_shader_source(&config.global_shader)`. (Search `set_custom_resize_program` in `niri.rs` for the initial-apply site, distinct from the reload diff at 1548.)

- [ ] **Step 2: Push the element in `render_inner` (gated)**

In `src/niri.rs` `render_inner`, just after the pointer push (`niri.rs:4212-4214`) and before the screen-transition block (`niri.rs:4217`), add:

```rust
        // Global post-process shader: only when a program is compiled (gate → zero overhead otherwise).
        if include_pointer
            && Shaders::get(ctx.renderer).program(ProgramType::Global).is_some()
        {
            let state = self.output_state.get_mut(output).unwrap();
            let start = *state
                .global_shader_start
                .get_or_insert_with(std::time::Instant::now);
            let time = start.elapsed().as_secs_f32();
            let scale = output.current_scale().fractional_scale() as f32;
            let area = Rectangle::from_size(output_size_logical(output)); // logical output rect at (0,0)
            let cursor = self.global_cursor_in_output(output); // (f32, f32) output-local px, or (-1,-1) if absent
            let prev = state.global_shader_prev.clone();
            let elem = GlobalShaderElement::new(Id::new(), area, scale, time, cursor, prev);
            // Element captures its own result; retrieve after the frame for next-frame prev.
            // Store an Id→element handle so the post-draw hook can call into_texture; see Step 3.
            push(elem.into());
        }
```

> The exact `output_size_logical` / `global_cursor_in_output` helpers: reuse existing niri helpers. Output logical size is available via the output's current mode/scale (search for how `render_inner` computes the output rect for other full-screen elements like the screen transition / backdrop). Cursor position in global/output coords is obtained the same way `render_pointer` does (`niri.rs:3697-3700`: `tablet_cursor_location` or `seat.get_pointer().current_location()`), converted to output-local by subtracting the output's global location.

- [ ] **Step 3: Capture result back into `OutputState` for ping-pong**

`render_to_vec` returns owned elements, so the simplest reliable ping-pong is: after the element's `draw` runs for the real output, move its captured `result` texture into `state.global_shader_prev`. Because the element is consumed by the element list, store the prev via a small per-output channel: have `GlobalShaderElement::draw` write the result into a `Rc<RefCell<Option<GlesTexture>>>` that is *also* held by `OutputState`. Concretely:

- Change the element's `result` field to `Rc<RefCell<Option<GlesTexture>>>`.
- Add `OutputState.global_shader_result: Rc<RefCell<Option<GlesTexture>>>` (default `Rc::new(RefCell::new(None))`).
- In Step 2, build the element with a clone of `state.global_shader_result` so `draw` writes there.
- After the render submission (in the redraw path, where the frame is presented — search the call site of `render_to_vec`/`render` for the output, e.g. in `src/backend/tty.rs` after `render_frame`), move the value: `state.global_shader_prev = state.global_shader_result.borrow_mut().take();`

This keeps the texture alive for exactly one frame and avoids threading a return value through the element list.

- [ ] **Step 4: Add commented example to default config**

In `resources/default-config.kdl`, add (commented out):

```kdl
// Apply a full-screen post-process shader to everything on screen.
// global-shader {
//     enable
//     // Either an inline GLSL string (niri mode: define `vec4 global_color(vec3 c)`)...
//     source "vec4 global_color(vec3 c) { vec4 s = tex2D_screen(c.xy); return vec4(s.rgb * vec3(1.0, 0.9, 0.8), s.a); }"
//     // ...or a path to a .frag file:
//     // path "~/.config/niri/shaders/crt.frag"
//     mode "niri"        // "niri" or "hyprland"
//     // reads-cursor     // forces software cursor so the shader can read/transform the cursor
// }
```

- [ ] **Step 5: Build + clippy**

Run: `cargo build 2>&1 | tail -30`
Expected: builds.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new errors.

- [ ] **Step 6: Format and commit**

```bash
cargo +nightly fmt
git add src/niri.rs resources/default-config.kdl
git commit -m "render: wire global shader into render_inner with config reload and gating"
```

---

## Task 6: `reads-cursor` → force software cursor + ordering

**Files:**
- Modify: `src/backend/tty.rs` (cursor-plane flag)
- Modify: `src/niri.rs` (element ordering relative to pointer)

**Interfaces:**
- Consumes: cursor-plane flag block (`src/backend/tty.rs:1900-1928`); pointer push site (`niri.rs:4212-4214`); `config.global_shader.reads_cursor`.

- [ ] **Step 1: Force software cursor when active**

In `src/backend/tty.rs`, in the `flags` block (`tty.rs:1900-1928`), extend the cursor-plane removal condition (`tty.rs:1917-1919`) so the cursor plane is also disabled when a global shader that reads the cursor is active:

```rust
            let global_shader_reads_cursor = self.config.borrow().global_shader.enable
                && self.config.borrow().global_shader.reads_cursor;
            if debug.disable_cursor_plane || global_shader_reads_cursor {
                flags.remove(FrameFlags::ALLOW_CURSOR_PLANE_SCANOUT);
            }
```

(`debug` is already borrowed at `tty.rs:1901`; reuse the existing `self.config` borrow pattern used elsewhere in this function to avoid a double-borrow — read the two bools into locals before the `flags` builder if needed.)

- [ ] **Step 2: Order the element relative to the cursor**

In `render_inner` (Task 5 Step 2), the global element is currently pushed right after the pointer. Since elements are front-to-back (first pushed = topmost, per `niri.rs:4212` "The pointer goes on the top"), pushing the global element *after* the pointer places it **below** the cursor — correct for the default (`reads-cursor` off): the cursor stays untouched on a hardware plane.

When `reads_cursor` is on, the cursor must be part of `niri_screen`, i.e. the global element must be **above** the cursor. Wrap the pointer push so that, when `reads_cursor` is on, the global element is pushed *before* `render_pointer`:

```rust
        let reads_cursor =
            self.config.borrow().global_shader.enable && self.config.borrow().global_shader.reads_cursor;
        let has_global = include_pointer
            && Shaders::get(ctx.renderer).program(ProgramType::Global).is_some();

        if has_global && reads_cursor {
            push_global_shader_element(self, ctx, output, &mut push); // helper from Task 5 Step 2 body
        }
        if include_pointer && self.pointer_visibility.is_visible() {
            self.render_pointer(ctx.renderer, output, &mut |elem| push(elem.into()));
        }
        if has_global && !reads_cursor {
            push_global_shader_element(self, ctx, output, &mut push);
        }
```

Refactor the Task 5 Step 2 inline block into a small helper `push_global_shader_element(...)` (or a closure) so it can be called from either branch without duplication.

- [ ] **Step 3: Build + clippy**

Run: `cargo build 2>&1 | tail -30`
Expected: builds.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new errors.

- [ ] **Step 4: Format and commit**

```bash
cargo +nightly fmt
git add src/backend/tty.rs src/niri.rs
git commit -m "render: force software cursor and order element when reads-cursor is set"
```

---

## Task 7: Manual verification on TTY + docs

This is the integration gate. Because biri has no GLES unit tests, correctness is confirmed by running the compositor on the DRM/TTY backend with real shaders.

**Files:**
- Create: `docs/wiki/Configuration:-Global-Shader.md`

- [ ] **Step 1: Build a release binary**

Run: `cargo build --release 2>&1 | tail -10`
Expected: builds clean.

- [ ] **Step 2: Manual checks (run biri on a TTY, edit config live)**

Verify each, since these cannot be unit-tested:
- [ ] With no `global-shader` block: everything renders normally; no regression. (Gate works.)
- [ ] niri-mode warm-tint shader (the default-config example): all apps (a terminal, a browser, a file manager) are tinted, including across workspaces.
- [ ] Edit the config to change the tint and save: hot-reload applies without restart.
- [ ] A `niri_time`-driven shader animates (e.g. a slow pulsing vignette).
- [ ] A `niri_cursor`-driven glow follows the pointer with `reads-cursor` **off** and the cursor still on a hardware plane (no cursor lag/jank).
- [ ] A `tex2D_prev`-based feedback shader leaves a fading trail (confirms ping-pong).
- [ ] With `reads-cursor` on, a shader that distorts `niri_screen` near the cursor also distorts the cursor pixels (confirms software-cursor compositing).
- [ ] A deliberately broken shader logs a `warn!` and leaves the screen rendering normally (no crash).
- [ ] A known Hyprland community `screen_shader` (GLES2 `#version 100` dialect) renders in `mode "hyprland"`.

Record results in the commit message / PR description.

- [ ] **Step 3: Write the docs page**

Create `docs/wiki/Configuration:-Global-Shader.md` documenting: the `global-shader` block and every field; the two modes; niri-mode contract (`vec4 global_color(vec3 coord)`, available uniforms `niri_time`/`niri_size`/`niri_scale`/`niri_cursor`, samplers `niri_screen`/`niri_prev`, helpers `tex2D_screen`/`tex2D_prev`); hyprland-mode aliases (`tex`, `v_texcoord`, `time`, `wl_output`) and the `#version 100`-only caveat (300-es shaders need manual `texture()→texture2D()`, `out`→`gl_FragColor`, `in`→`varying`); the `reads-cursor` cost (forces software cursor); and the general performance note (full-screen damage every frame, no direct scanout/overlay-plane offload while active). Note v1 is TTY/DRM only.

- [ ] **Step 4: Format and commit**

```bash
cargo +nightly fmt
git add docs/wiki/Configuration:-Global-Shader.md
git commit -m "docs: document global-shader configuration"
```

---

## Self-Review Notes

**Spec coverage:** Config block + both source forms + mode + reads-cursor (Task 1, 5); guard-railed + hyprland contracts (Task 2); `custom_global` registry trio (Task 2); `GlobalShaderElement` with screen/prev/time/cursor inputs (Tasks 3-4); per-output prev ping-pong (Tasks 4-5); render_inner gating → zero overhead (Task 5); config-reload hot recompile (Task 5); software-cursor forcing + ordering (Task 6); error handling — bad shader/bad config never crash (Task 2 `set_*` warns, Task 5 `global_shader_source` warns); TTY-only scope + perf/limitations docs (Task 7). All spec sections map to a task.

**Known soft spots flagged for the implementer (not placeholders — explicit reuse directives):** the named-sampler/uniform GL binding is delegated to the existing `ShaderRenderElement::draw` rather than re-derived (Task 3 Step 3); the blit capture is copied verbatim from `framebuffer_effect.rs:252-298`; the output-rect and cursor-in-output helpers reuse existing niri code paths cited by file:line. These are reuse instructions, not "figure it out later."

**Type consistency:** `set_custom_global_program(renderer, src, hyprland)` signature is identical across Task 2 (def), Task 5 (call). `GlobalShaderElement::new(id, area, scale, time, cursor, prev)` identical across Tasks 3, 5. `global_shader_prev: Option<GlesTexture>` and `global_shader_result: Rc<RefCell<Option<GlesTexture>>>` consistent across Tasks 4-5. `mode == "hyprland"` string check consistent across Tasks 5, 6.
