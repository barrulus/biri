# Global Shader 3.2 — Dedicated Feedback Buffer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give global shaders a screen-independent feedback texture so trails stop smearing — first a cheap `niri_screen_prev` sampler that fixes today's recovery math, then a full dedicated `niri_buffer` written by an optional `global_buffer()` function.

**Architecture:** Two additive pieces. (A) `niri_screen_prev` mirrors the existing `niri_prev` ping-pong but stores the per-frame *screen* capture instead of the *output*. (B) A second compiled program (`ProgramType::GlobalBuffer`) renders the shader's `global_buffer()` into a clean offscreen texture via the existing `OffscreenBuffer`; the display pass reads that as `niri_buffer`. Both leave existing shaders byte-identical.

**Tech Stack:** Rust, smithay GLES2 renderer (`OffscreenBuffer`, `capture_framebuffer_region`), knuffel/KDL config (`niri-config` scan only), GLSL ES `#version 100`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-22-global-shader-feedback-buffer-design.md`. Read it first.
- **Additive / identity:** existing shaders (no `niri_screen_prev`/`global_buffer`) must render byte-identically. New samplers default-bind to existing textures so no shader breaks.
- **Sampler/texture invariant:** every sampler registered in `compile_global_program` MUST be inserted into the `textures` HashMap in `GlobalShaderElement::draw()` (an unprovided sampler defaults to texture unit 0 → wrong sampling). Keep the registered list and the map in lockstep within each task.
- **`uses_buffer` ⇒ animating ⇒ whole-output:** buffer/feedback shaders redraw every frame and are excluded from region mode (consistent with 3.1).
- **No AI attribution** in commit messages.
- **Build crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; main-crate check `cargo check --no-default-features --features dbus,systemd` inside it; `niri-config` standalone via `cargo test -p niri-config`. GPU verification deploys to "sixseven" via the flake; KMS-only recording (`gpu-screen-recorder -w eDP-1`).

---

## File Structure

- `src/render_helpers/shaders/global_prelude.frag` — **modify**: add `niri_screen_prev`+helper (Task 1), `niri_buffer`+helper (Task 3).
- `src/render_helpers/shaders/mod.rs` — **modify**: register samplers (Tasks 1, 3); add `ProgramType::GlobalBuffer`, `custom_global_buffer` storage, second compile (Task 3).
- `src/render_helpers/shaders/global_buffer_epilogue.frag` — **create**: epilogue calling `global_buffer` (Task 3).
- `src/render_helpers/global_shader_element.rs` — **modify**: carry+bind `screen_prev` (Task 1), the offscreen buffer pass + `niri_buffer` binding (Task 4).
- `src/niri.rs` — **modify**: `OutputState` fields + init + reload reset + element-build wiring (Tasks 1, 4); `GlobalShaderCaps` consumed in scheduling already from 3.1.
- `src/backend/tty.rs` — **modify**: post-submit ping-pong for screen_prev (Task 1) and buffer (Task 4).
- `niri-config/src/global_shader.rs` — **modify**: extend `GlobalShaderCaps` scan (Task 2).
- `docs/wiki/Configuration:-Global-Shader.md` — **modify**: document new samplers + `global_buffer` (Tasks 1, 4).

---

## Task 1: `niri_screen_prev` sampler (the cheap smear fix)

Self-contained; mirrors the existing `niri_prev` ping-pong exactly, storing the screen capture. Ships the smear fix on its own.

**Files:** `global_prelude.frag`, `shaders/mod.rs`, `global_shader_element.rs`, `src/niri.rs`, `src/backend/tty.rs`, wiki.

**Interfaces:**
- Produces: `GlobalShaderElement::new(..., screen_prev: Option<GlesTexture>, screen_result: Rc<RefCell<Option<GlesTexture>>>, ...)` (two new params); `OutputState.global_shader_screen_prev` + `global_shader_screen_result`.

- [ ] **Step 1: Register the sampler in the prelude**

In `src/render_helpers/shaders/global_prelude.frag`, after the `niri_prev` declaration and helper, add:

```glsl
uniform sampler2D niri_screen_prev; // previous frame's niri_screen capture (screen only, no effect)
vec4 tex2D_screen_prev(vec2 uv) { return texture2D(niri_screen_prev, (uv - niri_region.xy) / niri_region.zw); }
```

- [ ] **Step 2: Register the sampler name in compile**

In `src/render_helpers/shaders/mod.rs` `compile_global_program`, change the samplers list:

```rust
        &["niri_screen", "niri_prev", "niri_screen_prev"],
```

- [ ] **Step 3: Add OutputState fields**

In `src/niri.rs` (after `global_shader_result`, ~line 503):

```rust
    /// Previous frame's `niri_screen` capture (screen only), exposed as `niri_screen_prev`.
    pub global_shader_screen_prev: Option<GlesTexture>,
    /// Sink written by `GlobalShaderElement::draw` with this frame's screen capture; moved into
    /// `global_shader_screen_prev` after submit.
    pub global_shader_screen_result: Rc<RefCell<Option<GlesTexture>>>,
```

Initialize them in the `OutputState { ... }` literal (~line 2949, after `global_shader_result`):

```rust
            global_shader_screen_prev: None,
            global_shader_screen_result: Rc::new(RefCell::new(None)),
```

- [ ] **Step 4: Reset on config reload**

In `src/niri.rs` reload reset loop (~line 1611-1613), add inside the loop:

```rust
                state.global_shader_screen_prev = None;
                *state.global_shader_screen_result.borrow_mut() = None;
```

- [ ] **Step 5: Add element fields + new() params**

In `src/render_helpers/global_shader_element.rs`, add fields (after `prev`):

```rust
    /// Previous frame's screen capture, bound as `niri_screen_prev`.
    screen_prev: Option<GlesTexture>,
    /// Sink for this frame's screen capture (clone of `screen_tex`), ping-ponged like `result`.
    screen_result: Rc<RefCell<Option<GlesTexture>>>,
```

Add the two params to `new()` (insert after `prev`, before `result`) and set them in the struct literal. Match the existing param ordering style.

- [ ] **Step 6: Bind screen_prev + store the screen capture in draw()**

In `draw()`, after `screen_tex` is captured (after line 116) store a clone for next frame:

```rust
        // Stash this frame's screen capture for next frame's niri_screen_prev.
        *self.screen_result.borrow_mut() = Some(screen_tex.clone());
```

Then in the `textures` map (after the `niri_prev` insert, ~line 157) add:

```rust
        let screen_prev_tex = self.screen_prev.clone().unwrap_or_else(|| screen_tex.clone());
        textures.insert("niri_screen_prev".to_string(), screen_prev_tex);
```

(Note: `screen_tex` is moved into the map by the existing `niri_screen` insert — do the `screen_result` clone and the `screen_prev` fallback clone BEFORE that move. Order the lines so all `screen_tex.clone()` uses precede `textures.insert("niri_screen", screen_tex)`.)

- [ ] **Step 7: Pass the fields at the element-build site**

In `src/niri.rs` where `GlobalShaderElement::new(...)` is called (~line 4374), pass the new args in the matching positions:

```rust
                state.global_shader_screen_prev.clone(),
                state.global_shader_screen_result.clone(),
```

- [ ] **Step 8: Post-submit ping-pong**

In `src/backend/tty.rs` next to the existing move (~line 1946-1948):

```rust
                    let screen_result = output_state.global_shader_screen_result.borrow_mut().take();
                    if screen_result.is_some() {
                        output_state.global_shader_screen_prev = screen_result;
                    }
```

- [ ] **Step 9: Compile-check**

Run (dev shell): `cargo check --no-default-features --features dbus,systemd`
Expected: clean. Fix the `new()` call arity and any import errors.

- [ ] **Step 10: Document the sampler**

In `docs/wiki/Configuration:-Global-Shader.md` uniforms table (niri mode), add:

```
| `niri_screen_prev` | `sampler2D` | The **previous** frame's screen (no effect), via `tex2D_screen_prev(uv)`. Use `prev − tex2D_screen_prev` to recover a feedback trail without scroll-smear. |
```

- [ ] **Step 11: Commit**

```bash
git add src/render_helpers/shaders/global_prelude.frag src/render_helpers/shaders/mod.rs src/render_helpers/global_shader_element.rs src/niri.rs src/backend/tty.rs "docs/wiki/Configuration:-Global-Shader.md"
git commit -m "global-shader: add niri_screen_prev sampler (previous-frame screen capture)"
```

- [ ] **Step 12: Manual verification (sixseven)**

Patch `~/.config/niri/global-shaders/global-trail.kdl` recovery to use `tex2D_screen_prev` (replace the `prev - s` style recovery's screen term with `tex2D_screen_prev(c.xy)`), reload, then scroll a page / play video under the trail: the trail follows the cursor with **no smear behind the moving content** (today it smears). Existing unpatched shaders look unchanged.

---

## Task 2: Extend `GlobalShaderCaps` for the buffer (niri-config)

Pure, standalone-testable. Adds `uses_buffer` and folds the new feedback samplers into the animating decision.

**Files:** Modify `niri-config/src/global_shader.rs`; Test: same file `mod tests`.

**Interfaces:**
- Consumes/Produces: `GlobalShaderCaps { uses_time, uses_cursor, uses_prev, uses_buffer }`; `is_animating()` returns `uses_time || uses_prev || uses_buffer`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `niri-config/src/global_shader.rs`:

```rust
#[test]
fn caps_scan_buffer_function() {
    let c = GlobalShaderCaps::scan(
        "vec4 global_buffer(vec3 c){ return tex2D_buffer(c.xy); } vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }",
        false,
    );
    assert!(c.uses_buffer);
    assert!(c.is_animating());
}

#[test]
fn caps_scan_screen_prev_is_feedback() {
    let c = GlobalShaderCaps::scan(
        "vec4 global_color(vec3 c){ return tex2D_screen(c.xy) - tex2D_screen_prev(c.xy); }",
        false,
    );
    assert!(c.uses_prev); // screen_prev folds into the feedback/animating set
    assert!(c.is_animating());
}

#[test]
fn caps_scan_plain_filter_not_buffer() {
    let c = GlobalShaderCaps::scan("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }", false);
    assert!(!c.uses_buffer);
    assert!(!c.is_animating());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p niri-config caps_scan_buffer caps_scan_screen_prev caps_scan_plain`
Expected: FAIL — no field `uses_buffer`.

- [ ] **Step 3: Implement**

In `GlobalShaderCaps` struct add the field:

```rust
    pub uses_buffer: bool,
```

In `scan`, the niri branch (after `uses_prev`):

```rust
            GlobalShaderCaps {
                uses_time: src.contains("niri_time"),
                uses_cursor: src.contains("niri_cursor"),
                // Feedback: previous output, previous screen, or the dedicated buffer all evolve
                // frame-to-frame, so any of them counts as feedback.
                uses_prev: src.contains("niri_prev")
                    || src.contains("tex2D_prev")
                    || src.contains("niri_screen_prev")
                    || src.contains("tex2D_screen_prev"),
                uses_buffer: src.contains("global_buffer")
                    || src.contains("niri_buffer")
                    || src.contains("tex2D_buffer"),
            }
```

In the `hyprland` branch add `uses_buffer: false,`.

Update `is_animating`:

```rust
    pub fn is_animating(&self) -> bool {
        self.uses_time || self.uses_prev || self.uses_buffer
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p niri-config`
Expected: PASS (all). The existing `caps_scan_*` tests still pass (the `Default`-derived `uses_buffer:false` keeps `GlobalShaderCaps::default()` comparisons valid).

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/global_shader.rs
git commit -m "niri-config: GlobalShaderCaps.uses_buffer + screen_prev feedback detection"
```

---

## Task 3: `GlobalBuffer` program — compile scaffolding

Adds the second compiled program (display source compiled with a buffer epilogue) and the `niri_buffer` sampler, without yet rendering it. Keeps the build green and isolates the compile changes from the render changes.

**Files:** `global_prelude.frag`, `global_buffer_epilogue.frag` (create), `shaders/mod.rs`.

**Interfaces:**
- Produces: `ProgramType::GlobalBuffer`; `Shaders.custom_global_buffer: RefCell<Option<ShaderProgram>>`; `set_custom_global_program` also compiles the buffer program when the source defines `global_buffer`.

- [ ] **Step 1: Add the `niri_buffer` sampler to the prelude**

In `global_prelude.frag`, after the `niri_screen_prev` helper (from Task 1):

```glsl
uniform sampler2D niri_buffer; // dedicated feedback buffer (last frame); == niri_prev if no global_buffer
vec4 tex2D_buffer(vec2 uv) { return texture2D(niri_buffer, (uv - niri_region.xy) / niri_region.zw); }

// User MAY define: vec4 global_buffer(vec3 coord);  // returns what to store in niri_buffer
```

- [ ] **Step 2: Create the buffer epilogue**

Create `src/render_helpers/shaders/global_buffer_epilogue.frag`:

```glsl

void main() {
    vec3 coord = vec3(niri_region.xy + niri_v_coords * niri_region.zw, 1.0);
    gl_FragColor = global_buffer(coord);
}
```

- [ ] **Step 3: Add the ProgramType + registry slot**

In `src/render_helpers/shaders/mod.rs`: add to `ProgramType`:

```rust
    GlobalBuffer,
```

Add to `Shaders` struct:

```rust
    pub custom_global_buffer: RefCell<Option<ShaderProgram>>,
```

Initialize in `Shaders::compile` `Self { ... }`:

```rust
            custom_global_buffer: RefCell::new(None),
```

Add to `program()` match:

```rust
            ProgramType::GlobalBuffer => self.custom_global_buffer.borrow().clone(),
```

Add a replace helper next to `replace_custom_global_program`:

```rust
    pub fn replace_custom_global_buffer_program(
        &self,
        program: Option<ShaderProgram>,
    ) -> Option<ShaderProgram> {
        self.custom_global_buffer.replace(program)
    }
```

- [ ] **Step 4: Compile the buffer program when `global_buffer` is present**

Add a sibling compile fn in `mod.rs`:

```rust
fn compile_global_buffer_program(
    renderer: &mut GlesRenderer,
    src: &str,
) -> Result<ShaderProgram, GlesError> {
    let mut program = include_str!("global_prelude.frag").to_string();
    program.push_str(src);
    program.push_str(include_str!("global_buffer_epilogue.frag"));
    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_time", UniformType::_1f),
            UniformName::new("niri_cursor", UniformType::_2f),
            UniformName::new("niri_region", UniformType::_4f),
            UniformName::new("niri_output_size", UniformType::_2f),
        ],
        &["niri_screen", "niri_prev", "niri_screen_prev", "niri_buffer"],
    )
}
```

Also add `niri_buffer` to the **display** program's sampler list in `compile_global_program`:

```rust
        &["niri_screen", "niri_prev", "niri_screen_prev", "niri_buffer"],
```

In `set_custom_global_program` (the niri-mode path; `hyprland` has no `global_buffer`), after installing the display program, compile+install the buffer program iff the source contains `global_buffer`:

```rust
    // Dedicated feedback buffer: compile a second program only when the source defines it.
    let buffer_program = match (src, hyprland) {
        (Some(src), false) if src.contains("global_buffer") => {
            match compile_global_buffer_program(renderer, src) {
                Ok(p) => Some(p),
                Err(err) => {
                    warn!("error compiling global_buffer shader: {err:?}");
                    None
                }
            }
        }
        _ => None,
    };
    if let Some(prev) = Shaders::get(renderer).replace_custom_global_buffer_program(buffer_program) {
        if let Err(err) = prev.destroy(renderer) {
            warn!("error destroying previous global_buffer shader: {err:?}");
        }
    }
```

(Place this inside `set_custom_global_program` after the existing display-program install block, using the same `renderer`/`src`/`hyprland` it already has.)

- [ ] **Step 5: Compile-check**

Run (dev shell): `cargo check --no-default-features --features dbus,systemd`
Expected: clean. The buffer program compiles but nothing renders it yet; `niri_buffer` is registered on the display program but not yet bound — Task 4 binds it. (Until Task 4, a no-`global_buffer` shader still works because Task 4's binding defaults `niri_buffer` to `niri_prev`; to keep THIS task's build correct in the meantime, also do Task 4 Step 3's one-line default bind now — see note.) 

**Note:** to avoid an unbound `niri_buffer` between Task 3 and Task 4, add the default bind now in `global_shader_element.rs` `draw()` textures map (final line before building the display element):

```rust
        // niri_buffer defaults to the previous output until the buffer pass (Task 4) overrides it.
        let buffer_tex = self.prev.clone().unwrap_or_else(|| {
            textures.get("niri_screen").cloned().expect("niri_screen inserted above")
        });
        textures.insert("niri_buffer".to_string(), buffer_tex);
```

- [ ] **Step 6: Commit**

```bash
git add src/render_helpers/shaders/global_prelude.frag src/render_helpers/shaders/global_buffer_epilogue.frag src/render_helpers/shaders/mod.rs src/render_helpers/global_shader_element.rs
git commit -m "global-shader: compile optional GlobalBuffer program + niri_buffer sampler"
```

---

## Task 4: Buffer render pass + ping-pong (the substrate)

Renders `global_buffer()` into a clean offscreen texture each frame and feeds it back as `niri_buffer`. Reuses `OffscreenBuffer`, whose `is_unique_reference` recreate-logic yields ping-pong for free (we hold a clone of last frame's output). **Highest-risk task** — GL render integration, manual GPU verification, with a fallback.

**Files:** `global_shader_element.rs`, `src/niri.rs`, `src/backend/tty.rs`, wiki.

**Interfaces:**
- Consumes: `ProgramType::GlobalBuffer` (Task 3), `GlobalShaderCaps.uses_buffer` (Task 2, already drives `is_animating` → scheduling/region exclusion from 3.1).
- Produces: `OutputState.global_shader_buffer: Rc<OffscreenBuffer>`, `global_shader_buffer_prev: Option<GlesTexture>`, `global_shader_buffer_result: Rc<RefCell<Option<GlesTexture>>>`; element params for the same.

- [ ] **Step 1: Add OutputState fields + init + reload reset**

In `src/niri.rs` `OutputState` (after the screen_prev fields from Task 1):

```rust
    /// Offscreen target for the global shader's dedicated feedback buffer pass.
    pub global_shader_buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// Last frame's feedback buffer, bound as `niri_buffer`.
    pub global_shader_buffer_prev: Option<GlesTexture>,
    /// Sink for this frame's buffer texture; moved into `global_shader_buffer_prev` after submit.
    pub global_shader_buffer_result: Rc<RefCell<Option<GlesTexture>>>,
```

Init (in the `OutputState { ... }` literal):

```rust
            global_shader_buffer: Rc::new(crate::render_helpers::offscreen::OffscreenBuffer::default()),
            global_shader_buffer_prev: None,
            global_shader_buffer_result: Rc::new(RefCell::new(None)),
```

Reload reset (in the reset loop):

```rust
                state.global_shader_buffer_prev = None;
                *state.global_shader_buffer_result.borrow_mut() = None;
                // OffscreenBuffer recreates its texture on demand; leave it.
```

- [ ] **Step 2: Element fields + new() params**

In `global_shader_element.rs`, add fields (after `screen_result`):

```rust
    /// Offscreen target for the buffer pass (shared with OutputState; interior-mutable).
    buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// Last frame's feedback buffer, bound as `niri_buffer`.
    buffer_prev: Option<GlesTexture>,
    /// Sink for this frame's buffer texture, ping-ponged.
    buffer_result: Rc<RefCell<Option<GlesTexture>>>,
```

Add matching `new()` params (after `screen_result`, before `result`) and set them. Update the build site in `src/niri.rs` to pass `state.global_shader_buffer.clone()`, `state.global_shader_buffer_prev.clone()`, `state.global_shader_buffer_result.clone()`.

- [ ] **Step 3: Run the buffer pass in draw()**

In `global_shader_element.rs` `draw()`, replace the Task-3 default `niri_buffer` bind with the real pass. After `screen_tex`, `prev_tex`, `screen_prev_tex` are known and BEFORE building the display `element`/uniforms, insert:

```rust
        // --- Dedicated feedback buffer pass ---
        // If the source defines global_buffer, render it into a clean offscreen texture (reading
        // last frame's buffer) and feed THAT as niri_buffer. Otherwise niri_buffer = niri_prev.
        let buffer_program = Shaders::get_from_frame(frame).program(ProgramType::GlobalBuffer);
        let buffer_tex = if buffer_program.is_some() {
            let buf_prev = self
                .buffer_prev
                .clone()
                .unwrap_or_else(|| screen_prev_tex.clone());

            let mut buf_textures = HashMap::new();
            buf_textures.insert("niri_screen".to_string(), screen_tex.clone());
            buf_textures.insert("niri_prev".to_string(), prev_tex.clone());
            buf_textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
            buf_textures.insert("niri_buffer".to_string(), buf_prev);

            let buf_element = ShaderRenderElement::new(
                ProgramType::GlobalBuffer,
                self.area.size,
                None,
                self.scale,
                1.,
                uniforms.clone(),
                buf_textures,
                Kind::Unspecified,
            );

            let mut guard = frame.renderer();
            let renderer = guard.as_mut();
            let (off_elem, _sync, _data) = self
                .buffer
                .render(renderer, Scale::from(self.scale as f64), &[buf_element])
                .map_err(|_| GlesError::UnknownPixelFormat)?;
            let next = off_elem.texture().clone();
            drop(guard);

            // Hold a clone so OffscreenBuffer allocates a fresh texture next frame (ping-pong),
            // and so the owner can move it into global_shader_buffer_prev after submit.
            *self.buffer_result.borrow_mut() = Some(next.clone());
            next
        } else {
            // No buffer program: niri_buffer aliases the previous output (== niri_prev).
            prev_tex.clone()
        };
        textures.insert("niri_buffer".to_string(), buffer_tex);
```

(Build `uniforms` BEFORE this block so `uniforms.clone()` is available; the display `textures` map insert for `niri_buffer` here REPLACES the Task-3 default — remove the Task-3 default-bind lines. `Scale` is already imported in this file; `GlesError::UnknownPixelFormat` is a stand-in error variant — if it doesn't exist, use any existing `GlesError` variant the crate exposes, e.g. map via `GlesError::ShaderCompileError`-style; the exact variant is cosmetic since failure only logs/aborts the frame.)

- [ ] **Step 4: Post-submit ping-pong**

In `src/backend/tty.rs` next to the other moves:

```rust
                    let buffer_result = output_state.global_shader_buffer_result.borrow_mut().take();
                    if buffer_result.is_some() {
                        output_state.global_shader_buffer_prev = buffer_result;
                    }
```

- [ ] **Step 5: Compile-check**

Run (dev shell): `cargo check --no-default-features --features dbus,systemd`
Expected: clean. Resolve the exact `GlesError` variant and the `OffscreenBuffer::render` signature (`render(&self, &mut GlesRenderer, Scale<f64>, &[impl RenderElement<GlesRenderer>])`) — `buf_element` is `ShaderRenderElement` which already `impl RenderElement<GlesRenderer>`.

- [ ] **Step 6: Manual verification + Y-orientation check (sixseven)**

This is the risk gate. Deploy, then set `~/.config/niri/global-shaders/global-trail.kdl` to a buffer-contract version:

```kdl
global-shader {
    enable
    source "
    vec4 global_buffer(vec3 c){
        vec3 prev = tex2D_buffer(c.xy).rgb;          // pure trail, no screen
        float d = length(c.xy*niri_output_size - niri_cursor);
        float fresh = smoothstep(18.0, 0.0, d);
        return vec4(max(prev*0.90, vec3(0.2,0.8,1.0)*fresh), 1.0);
    }
    vec4 global_color(vec3 c){
        vec3 s = tex2D_screen(c.xy).rgb;
        vec4 b = tex2D_buffer(c.xy);                  // this frame's trail
        return vec4(mix(s, b.rgb, b.r*0.0 + length(b.rgb)*0.6), 1.0);
    }"
}
```

Checks: (a) cyan trail follows the cursor and **fades cleanly**; (b) **scroll a page / play video under it — the trail shows NO ghost of the scrolled content** (the smear is gone — this is the whole point); (c) **Y-orientation:** the trail is at the cursor, not vertically mirrored. If the trail is flipped vertically, the `OffscreenBuffer` render orientation differs from `niri_screen`; fix by flipping the sample in `tex2D_buffer` (negate the remapped `y`) in the prelude, or by adjusting the buffer-pass transform — re-verify.

- [ ] **Step 7: Descope fallback**

If the buffer pass fights the renderer (Y unfixable, ping-pong not actually alternating → trail never fades or freezes, or render errors): the feature ships **inert** — Task 1 (`niri_screen_prev`) already fixes the visible smear and stands alone. Make `set_custom_global_program` skip compiling the buffer program (warn once: "global_buffer unsupported on this build") so `niri_buffer` stays aliased to `niri_prev`, commit that, and keep Tasks 1–3.

- [ ] **Step 8: Document + commit**

In `docs/wiki/Configuration:-Global-Shader.md`, add `niri_buffer`/`tex2D_buffer` and the optional `global_buffer(vec3 c)` to the niri-mode reference, noting: it stores only what you return (no screen), reads last frame via `tex2D_buffer`, `global_color` reads this frame's buffer, and a buffer shader always runs whole-output every frame.

```bash
git add src/render_helpers/global_shader_element.rs src/niri.rs src/backend/tty.rs "docs/wiki/Configuration:-Global-Shader.md"
git commit -m "global-shader: dedicated feedback buffer pass (global_buffer / niri_buffer)"
```

---

## Self-Review

**Spec coverage:**
- §3.A `niri_screen_prev` → Task 1 (sampler, ping-pong, reset, verify). ✓
- §3.B buffer (contract, compile, two-pass render, ping-pong) → Tasks 3 (compile) + 4 (render). ✓
- §3.C integration (`uses_buffer`, `is_animating`, reload) → Task 2 (caps) + Task 4 (reset). ✓
- §4 sampler table → prelude edits in Tasks 1/3 + wiki in Tasks 1/4. ✓
- §5 testing → Task 2 unit tests, Tasks 1/4 manual GPU verify, compile checks each task. ✓
- §6 boundaries (one buffer, no MRT, hyprland untouched, no config fields) → respected; buffer program compiled only in niri mode (Task 3 Step 4). ✓
- §7 order → Tasks 1→2→3→4 match. ✓

**Placeholder scan:** no TBD/"handle errors"; the one cosmetic uncertainty (exact `GlesError` variant) is called out with a concrete resolution step, not left blank. Y-orientation has a concrete check+fix in Task 4 Step 6.

**Type consistency:** `global_shader_screen_prev`/`_result`, `global_shader_buffer`/`_prev`/`_result` named identically across `OutputState` (niri.rs), the element fields, and the tty post-submit moves; `ProgramType::GlobalBuffer` and `custom_global_buffer` consistent across Tasks 3/4; `GlobalShaderCaps.uses_buffer` consistent Tasks 2/(3-4 via scheduling). ✓
