# Global Shader 3.3 — Multi-pass (Pass Chains) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run N global shaders in sequence (a pass chain), each reading the prior pass's output, the last compositing into the scene — generalizing the existing single-shader path so a length-0/1 chain stays byte-identical to today.

**Architecture:** Every "global shader" singleton (registry program, per-output `prev`/`buffer` textures, post-submit ping-pong move) becomes a per-pass indexed collection. `ProgramType` gains `GlobalPass(i)`/`GlobalPassBuffer(i)`; `GlobalShaderElement::draw()` becomes a loop that pipes each pass's output into the next via `OffscreenBuffer`, last pass to `dst`. Config gains repeatable `pass{}` blocks; caps become the union over all passes.

**Tech Stack:** Rust, GLES2 fragment shaders, smithay/GLES renderer (`GlesTexture`, `OffscreenBuffer`, `ShaderRenderElement`), knuffel (KDL config), insta (snapshot tests).

**Spec:** `docs/superpowers/specs/2026-06-22-global-shader-multipass-design.md` — read it before starting; this plan implements it.

## Global Constraints

- **Byte-identity invariant:** zero `pass{}` blocks → identical to current single-shader behavior; a length-1 chain with the same source → same result. Never regress the single-shader path.
- **No new config fields beyond the `pass` list** (+ per-pass `mode`). `enable`/`reads_cursor`/`cursor_radius`/`redraw` stay chain-level.
- **GLES2, one render target per pass.** `global_buffer` stays a second sequential sub-pass (no MRT).
- **niri-mode samplers only.** New `niri_source` sampler registered for both dialects but only meaningful in niri mode (hyprland location `−1` → no-op set).
- **TTY/DRM only.** Still excluded from screencast/screenshot sinks; winit unchanged (no effect).
- **Whole-output for N ≥ 2.** Region/`cursor_radius` mode applies only to a length-1 chain.
- **Build/test crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task compile check `cargo check --no-default-features --features dbus,systemd` (inside dev shell, needs `LIBCLANG_PATH` per spec §10). `niri-config` builds/tests **outside** the dev shell: `cargo test -p niri-config`. Snapshot gotcha: `cargo insta accept` can hang — patch inline snapshots from `niri-config/src/.lib.rs.pending-snap` by hand (8-space indent).
- **Commits:** no Co-Authored-By / AI-attribution lines (user rule).

---

## File Structure

**Config (niri-config crate — pure logic, testable outside dev shell):**
- Modify `niri-config/src/global_shader.rs` — add `GlobalShaderPass`, `GlobalShaderPassPart`, `passes` fields, `scan_chain`, merge logic, tests.
- Modify `niri-config/src/lib.rs:1681` — update the inline default snapshot for the new `passes` field.

**Shader registry + contract (src — needs dev shell to compile):**
- Modify `src/render_helpers/shaders/mod.rs` — `ProgramType::GlobalPass(usize)`/`GlobalPassBuffer(usize)`, program vecs, `program()` lookup, `set_custom_global_passes`.
- Modify `src/render_helpers/shaders/global_prelude.frag` — `niri_source` sampler + `tex2D_source` helper.

**Render path (src):**
- Modify `src/render_helpers/global_shader_element.rs` — chain-aware `new()` + `draw()` pass loop.
- Modify `src/niri.rs` — `OutputState` vec fields, construction, reload diff/reset, caps union, `global_shader_pass_sources` helper, element construction.
- Modify `src/backend/tty.rs` — startup compile via `set_custom_global_passes`; per-pass post-submit ping-pong moves.

**Docs:**
- Modify `docs/wiki/Configuration:-Global-Shader.md` — multi-pass section + a parsing example.

---

## Task Ordering & Dependencies

1. **Task 1 (config)** and **Task 2 (caps)** are pure `niri-config`, testable outside the dev shell. Do first.
2. **Task 3 (registry)** and **Task 4 (prelude sampler)** are independent of each other; both compile-only.
3. **Task 5 (state + element + draw loop)** depends on 3 + 4.
4. **Task 6 (backend + reload + caps wiring)** depends on 1, 2, 3, 5.
5. **Task 7 (docs + manual verify)** depends on everything.

---

## Task 1: Config — `pass{}` blocks

**Files:**
- Modify: `niri-config/src/global_shader.rs`
- Modify: `niri-config/src/lib.rs:1681` (inline default snapshot)
- Test: `niri-config/src/global_shader.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub struct GlobalShaderPass { pub source: Option<String>, pub path: Option<String>, pub mode: String }` (resolved form, derives `Debug, Clone, PartialEq, Eq`).
  - `GlobalShaderPassPart` (knuffel `Decode`) with `source: Option<String>`, `path: Option<String>`, `mode: Option<String>`.
  - `GlobalShader.passes: Vec<GlobalShaderPass>` and `GlobalShaderPart.passes: Vec<GlobalShaderPassPart>`.

- [ ] **Step 1: Write the failing parse test**

Add to the `tests` module in `niri-config/src/global_shader.rs`:

```rust
#[test]
fn global_shader_pass_list_parses() {
    let config = Config::parse_mem(
        r##"
        global-shader {
            enable
            pass { path "blur.frag" }
            pass { source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"; mode "niri" }
            pass { path "crt.frag"; mode "hyprland" }
        }
        "##,
    )
    .unwrap();
    assert!(config.global_shader.enable);
    assert_eq!(config.global_shader.passes.len(), 3);
    assert_eq!(config.global_shader.passes[0].path.as_deref(), Some("blur.frag"));
    assert_eq!(
        config.global_shader.passes[1].source.as_deref(),
        Some("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }")
    );
    assert_eq!(config.global_shader.passes[1].mode, "niri");
    assert_eq!(config.global_shader.passes[2].mode, "hyprland");
}

#[test]
fn global_shader_no_passes_back_compat() {
    let config = Config::parse_mem(
        r##"
        global-shader {
            enable
            source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
        }
        "##,
    )
    .unwrap();
    assert!(config.global_shader.passes.is_empty());
    assert!(config.global_shader.source.is_some());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p niri-config global_shader_pass`
Expected: FAIL — `passes` field / `GlobalShaderPass` don't exist (compile error).

- [ ] **Step 3: Add the resolved + part structs**

In `niri-config/src/global_shader.rs`, after the `GlobalShader` struct, add:

```rust
/// One pass in a multi-pass chain. Resolved form of [`GlobalShaderPassPart`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShaderPass {
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq, Eq)]
pub struct GlobalShaderPassPart {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
}
```

- [ ] **Step 4: Add `passes` to both structs + default**

In `GlobalShader` add field `pub passes: Vec<GlobalShaderPass>,`. In its `Default` impl add `passes: Vec::new(),`. In `GlobalShaderPart` add:

```rust
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
```

- [ ] **Step 5: Implement the merge (passes replace wholesale, per-pass mode defaults to chain mode)**

In `impl MergeWith<GlobalShaderPart> for GlobalShader`, after the existing `merge!`/`merge_clone!` lines (so `self.mode` is already merged), add:

```rust
        if !part.passes.is_empty() {
            self.passes = part
                .passes
                .iter()
                .map(|p| GlobalShaderPass {
                    source: p.source.clone(),
                    path: p.path.clone(),
                    // Per-pass mode defaults to the chain-level mode.
                    mode: p.mode.clone().unwrap_or_else(|| self.mode.clone()),
                })
                .collect();
        }
```

- [ ] **Step 6: Run the parse tests to verify they pass**

Run: `cargo test -p niri-config global_shader_pass global_shader_no_passes`
Expected: PASS (both).

- [ ] **Step 7: Update the inline default snapshot in lib.rs**

`niri-config/src/lib.rs:1681` is an inline `Debug` snapshot of the default config. Adding the `passes` field changes that `Debug` output. Edit the `global_shader: GlobalShader { ... }` block to add `passes: [],` after `redraw: "auto",`:

```rust
            global_shader: GlobalShader {
                enable: false,
                source: None,
                path: None,
                mode: "niri",
                reads_cursor: false,
                cursor_radius: None,
                redraw: "auto",
                passes: [],
            },
```

- [ ] **Step 8: Run the full niri-config test suite**

Run: `cargo test -p niri-config`
Expected: PASS. If a snapshot mismatch remains, the failing test writes `niri-config/src/.lib.rs.pending-snap`; patch the inline `@r#"..."#` from its `new.snapshot` field (8-space indent per line) — do NOT rely on `cargo insta accept` (it hangs here).

- [ ] **Step 9: Commit**

```bash
git add niri-config/src/global_shader.rs niri-config/src/lib.rs
git commit -m "niri-config: global-shader pass{} list (multi-pass chains)"
```

---

## Task 2: Caps — union over the pass chain

**Files:**
- Modify: `niri-config/src/global_shader.rs`
- Test: `niri-config/src/global_shader.rs` (`tests` module)

**Interfaces:**
- Consumes: `GlobalShaderCaps::scan(src, hyprland)` (exists).
- Produces: `GlobalShaderCaps::scan_chain(passes: &[(String, bool)]) -> GlobalShaderCaps` — OR of `scan` over each `(source, hyprland)`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn caps_scan_chain_union() {
    // A static blur pass + an animated final pass => chain is animating.
    let chain = [
        ("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }".to_string(), false),
        ("vec4 global_color(vec3 c){ return vec4(niri_time); }".to_string(), false),
    ];
    let caps = GlobalShaderCaps::scan_chain(&chain);
    assert!(caps.uses_time);
    assert!(caps.is_animating());

    // All-static chain => not animating.
    let static_chain = [
        ("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }".to_string(), false),
        ("vec4 global_color(vec3 c){ return tex2D_screen(c.xy).gbra; }".to_string(), false),
    ];
    let caps = GlobalShaderCaps::scan_chain(&static_chain);
    assert!(!caps.is_animating());
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p niri-config caps_scan_chain_union`
Expected: FAIL — `scan_chain` not defined.

- [ ] **Step 3: Implement `scan_chain`**

In `impl GlobalShaderCaps`, add:

```rust
    /// Capabilities for a multi-pass chain: the union of every pass's caps. The chain animates
    /// if any pass does, so the whole chain redraws every frame.
    pub fn scan_chain(passes: &[(String, bool)]) -> Self {
        passes.iter().fold(Self::default(), |acc, (src, hyprland)| {
            let c = Self::scan(src, *hyprland);
            GlobalShaderCaps {
                uses_time: acc.uses_time || c.uses_time,
                uses_cursor: acc.uses_cursor || c.uses_cursor,
                uses_prev: acc.uses_prev || c.uses_prev,
                uses_buffer: acc.uses_buffer || c.uses_buffer,
            }
        })
    }
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p niri-config caps_scan_chain_union`
Expected: PASS.

- [ ] **Step 5: Run the full crate suite**

Run: `cargo test -p niri-config`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add niri-config/src/global_shader.rs
git commit -m "niri-config: GlobalShaderCaps::scan_chain (union over passes)"
```

---

## Task 3: Registry — indexed pass programs

**Files:**
- Modify: `src/render_helpers/shaders/mod.rs`

**Interfaces:**
- Consumes: `ShaderProgram::compile`, `compile_global_program`, `compile_global_buffer_program` (exist).
- Produces:
  - `ProgramType::GlobalPass(usize)`, `ProgramType::GlobalPassBuffer(usize)`.
  - `Shaders.custom_global_passes: RefCell<Vec<ShaderProgram>>`, `Shaders.custom_global_pass_buffers: RefCell<Vec<Option<ShaderProgram>>>`.
  - `pub fn set_custom_global_passes(renderer: &mut GlesRenderer, passes: &[(String, bool)])`.
- `ProgramType::Global` / `GlobalBuffer` are **retained** and alias index 0.

- [ ] **Step 1: Add the indexed `ProgramType` variants**

In `src/render_helpers/shaders/mod.rs`, extend the enum:

```rust
#[derive(Debug, Clone, Copy)]
pub enum ProgramType {
    Border,
    Shadow,
    Resize,
    Close,
    Open,
    Global,
    GlobalBuffer,
    GlobalPass(usize),
    GlobalPassBuffer(usize),
}
```

- [ ] **Step 2: Replace the singleton fields with vecs**

In `struct Shaders`, replace:

```rust
    pub custom_global: RefCell<Option<ShaderProgram>>,
    pub custom_global_buffer: RefCell<Option<ShaderProgram>>,
```

with:

```rust
    pub custom_global_passes: RefCell<Vec<ShaderProgram>>,
    pub custom_global_pass_buffers: RefCell<Vec<Option<ShaderProgram>>>,
```

In `Shaders::compile`'s returned `Self { .. }`, replace the two `custom_global*: RefCell::new(None)` initializers with:

```rust
            custom_global_passes: RefCell::new(Vec::new()),
            custom_global_pass_buffers: RefCell::new(Vec::new()),
```

- [ ] **Step 3: Update `program()` lookup**

Replace the `Global` / `GlobalBuffer` arms in `fn program`:

```rust
            ProgramType::Global => self.custom_global_passes.borrow().first().cloned(),
            ProgramType::GlobalBuffer => self
                .custom_global_pass_buffers
                .borrow()
                .first()
                .cloned()
                .flatten(),
            ProgramType::GlobalPass(i) => self.custom_global_passes.borrow().get(i).cloned(),
            ProgramType::GlobalPassBuffer(i) => self
                .custom_global_pass_buffers
                .borrow()
                .get(i)
                .cloned()
                .flatten(),
```

- [ ] **Step 4: Replace the replace-helpers + `set_custom_global_program` with the chain installer**

Delete `replace_custom_global_program`, `replace_custom_global_buffer_program`, and `set_custom_global_program`. Add:

```rust
/// Install a whole pass chain: for each pass compile its display program and (niri mode only,
/// when the source defines `global_buffer`) a dedicated-buffer program. Replaces both registry
/// vecs wholesale and destroys every previously-installed program. An empty slice disables the
/// chain. If any pass fails to compile, the whole chain is dropped (empty vecs) so we never run a
/// partial chain.
pub fn set_custom_global_passes(renderer: &mut GlesRenderer, passes: &[(String, bool)]) {
    let mut display = Vec::with_capacity(passes.len());
    let mut buffers = Vec::with_capacity(passes.len());
    let mut ok = true;

    for (src, hyprland) in passes {
        match compile_global_program(renderer, src, *hyprland) {
            Ok(p) => display.push(p),
            Err(err) => {
                warn!("error compiling global shader pass: {err:?}");
                ok = false;
                break;
            }
        }
        let buffer = match (src.contains("global_buffer"), *hyprland) {
            (true, false) => match compile_global_buffer_program(renderer, src) {
                Ok(p) => Some(p),
                Err(err) => {
                    warn!("error compiling global_buffer pass: {err:?}");
                    None
                }
            },
            _ => None,
        };
        buffers.push(buffer);
    }

    if !ok {
        // Destroy anything compiled this round before bailing.
        for p in display.drain(..) {
            let _ = p.destroy(renderer);
        }
        for p in buffers.drain(..).flatten() {
            let _ = p.destroy(renderer);
        }
        display = Vec::new();
        buffers = Vec::new();
    }

    let shaders = Shaders::get(renderer);
    let old_display = shaders.custom_global_passes.replace(display);
    let old_buffers = shaders.custom_global_pass_buffers.replace(buffers);
    for p in old_display {
        if let Err(err) = p.destroy(renderer) {
            warn!("error destroying previous global pass: {err:?}");
        }
    }
    for p in old_buffers.into_iter().flatten() {
        if let Err(err) = p.destroy(renderer) {
            warn!("error destroying previous global_buffer pass: {err:?}");
        }
    }
}
```

- [ ] **Step 5: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: FAIL only in `src/niri.rs` / `src/backend/tty.rs` (callers of the removed `set_custom_global_program` / `program(ProgramType::Global)` still exist). The `shaders/mod.rs` file itself must have no errors. (Those callers are fixed in Tasks 5–6; this step confirms the registry compiles in isolation.)

> Note: this task does not independently compile the whole crate because its consumers change in later tasks. If you prefer a green checkpoint, do Steps 1–4 here and defer the commit until Task 6's `cargo check` is green; otherwise commit now and accept that `cargo check` is red until Task 6.

- [ ] **Step 6: Commit**

```bash
git add src/render_helpers/shaders/mod.rs
git commit -m "global-shader: indexed pass program registry (ProgramType::GlobalPass)"
```

---

## Task 4: Shader contract — `niri_source` sampler

**Files:**
- Modify: `src/render_helpers/shaders/global_prelude.frag`
- Modify: `src/render_helpers/shaders/mod.rs` (sampler registration lists)

**Interfaces:**
- Produces: GLSL sampler `niri_source` + helper `tex2D_source(uv)`; registered on both global compile paths.

- [ ] **Step 1: Add the sampler + helper to the prelude**

In `src/render_helpers/shaders/global_prelude.frag`, after the `niri_screen_prev` sampler block (line ~22) and its helper, add the sampler near the others and the helper near the other `tex2D_*` helpers:

After line 22 (`uniform sampler2D niri_screen_prev; ...`) add:

```glsl
uniform sampler2D niri_source; // original composited screen, unfiltered (== niri_screen for pass 0)
```

After line 28 (`vec4 tex2D_screen_prev(...)`) add:

```glsl
vec4 tex2D_source(vec2 uv) { return texture2D(niri_source, (uv - niri_region.xy) / niri_region.zw); }
```

- [ ] **Step 2: Register `niri_source` in both compile paths**

In `src/render_helpers/shaders/mod.rs`, in BOTH `compile_global_program` and `compile_global_buffer_program`, change the sampler list argument from:

```rust
        &["niri_screen", "niri_prev", "niri_screen_prev", "niri_buffer"],
```

to:

```rust
        &["niri_screen", "niri_prev", "niri_screen_prev", "niri_buffer", "niri_source"],
```

- [ ] **Step 3: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: same caller errors as Task 3 (niri.rs/tty.rs), nothing new from these two files. The shader source string change is validated at runtime, not compile time, so a green-where-expected here is sufficient.

- [ ] **Step 4: Commit**

```bash
git add src/render_helpers/shaders/global_prelude.frag src/render_helpers/shaders/mod.rs
git commit -m "global-shader: niri_source sampler (original unfiltered screen)"
```

---

## Task 5: Per-output state + element + draw loop

**Files:**
- Modify: `src/niri.rs` (`OutputState` fields, construction, element construction, `global_shader_pass_sources` helper)
- Modify: `src/render_helpers/global_shader_element.rs` (chain-aware `new` + `draw`)

**Interfaces:**
- Consumes: `ProgramType::GlobalPass(i)`/`GlobalPassBuffer(i)`, `OffscreenBuffer::render`, `capture::capture_framebuffer_region` (exist).
- Produces:
  - `OutputState` vec fields: `global_shader_prev: Vec<Option<GlesTexture>>`, `global_shader_result: Vec<Rc<RefCell<Option<GlesTexture>>>>`, `global_shader_pass_offscreen: Vec<Rc<OffscreenBuffer>>`, `global_shader_buffer: Vec<Rc<OffscreenBuffer>>`, `global_shader_buffer_prev: Vec<Option<GlesTexture>>`, `global_shader_buffer_result: Vec<Rc<RefCell<Option<GlesTexture>>>>`. `global_shader_screen_prev`/`_screen_result` stay scalar.
  - `GlobalShaderElement::new(..., passes: Vec<GlobalPassState>)` where `GlobalPassState { prev, result, pass_offscreen, buffer, buffer_prev, buffer_result }`.
  - `global_shader_pass_sources(cfg: &GlobalShader) -> Vec<(String, bool)>` (in `src/niri.rs`).

This is the largest task. The `draw()` loop is provided in full; the executor iterates against `cargo check` for borrow/lifetime details, but no placeholders — the structure and API calls are concrete.

- [ ] **Step 1: Add the chain-source resolver helper**

In `src/niri.rs`, next to `global_shader_source` (line ~6727), add:

```rust
/// Resolve a global-shader config into an ordered list of `(source, hyprland)` passes.
/// Empty `passes` => the legacy single shader becomes a length-1 chain. If any pass cannot be
/// resolved (missing/unreadable/ambiguous), the whole chain is dropped (returns empty) so we never
/// run a partial chain.
pub(crate) fn global_shader_pass_sources(cfg: &niri_config::GlobalShader) -> Vec<(String, bool)> {
    if !cfg.enable {
        return Vec::new();
    }
    if cfg.passes.is_empty() {
        return match global_shader_source(cfg) {
            Some(src) => vec![(src, cfg.mode == "hyprland")],
            None => Vec::new(),
        };
    }
    if cfg.source.is_some() || cfg.path.is_some() {
        warn!("global-shader: both top-level source/path and pass{{}} blocks set; using passes");
    }
    let mut out = Vec::with_capacity(cfg.passes.len());
    for pass in &cfg.passes {
        let resolved = match (&pass.source, &pass.path) {
            (Some(s), None) if !s.trim().is_empty() => Some(s.clone()),
            (Some(_), None) => {
                warn!("global-shader: empty pass source, disabling chain");
                None
            }
            (None, Some(p)) => {
                let path = match expand_home(std::path::Path::new(p)) {
                    Ok(Some(e)) => e,
                    Ok(None) => std::path::PathBuf::from(p),
                    Err(err) => {
                        warn!("global-shader: cannot expand pass path {p:?}: {err}; disabling chain");
                        return Vec::new();
                    }
                };
                match std::fs::read_to_string(&path) {
                    Ok(s) => Some(s),
                    Err(err) => {
                        warn!("global-shader: cannot read pass path {p:?}: {err}; disabling chain");
                        None
                    }
                }
            }
            (Some(_), Some(_)) => {
                warn!("global-shader: pass has both source and path; disabling chain");
                None
            }
            (None, None) => {
                warn!("global-shader: pass has neither source nor path; disabling chain");
                None
            }
        };
        match resolved {
            Some(src) => out.push((src, pass.mode == "hyprland")),
            None => return Vec::new(),
        }
    }
    out
}
```

- [ ] **Step 2: Generalize the `OutputState` fields to vecs**

In `src/niri.rs` (`struct OutputState`, ~497-514), replace the scalar global-shader feedback fields with vecs (keep `global_shader_start`, `global_shader_screen_prev`, `global_shader_screen_result` as-is — screen stays scalar):

```rust
    /// Per-pass previous-frame output, bound as each pass's `niri_prev`.
    pub global_shader_prev: Vec<Option<GlesTexture>>,
    pub global_shader_start: Cell<Option<std::time::Instant>>,
    /// Per-pass sink for this frame's output; moved into `global_shader_prev` after submit.
    pub global_shader_result: Vec<Rc<RefCell<Option<GlesTexture>>>>,
    pub global_shader_screen_prev: Option<GlesTexture>,
    pub global_shader_screen_result: Rc<RefCell<Option<GlesTexture>>>,
    /// Per-pass display offscreen for intermediate passes (last pass renders to dst).
    pub global_shader_pass_offscreen: Vec<Rc<crate::render_helpers::offscreen::OffscreenBuffer>>,
    /// Per-pass dedicated `global_buffer` offscreen.
    pub global_shader_buffer: Vec<Rc<crate::render_helpers::offscreen::OffscreenBuffer>>,
    pub global_shader_buffer_prev: Vec<Option<GlesTexture>>,
    pub global_shader_buffer_result: Vec<Rc<RefCell<Option<GlesTexture>>>>,
```

- [ ] **Step 3: Initialize the vecs at `OutputState` construction**

In `src/niri.rs` (~2963-2972), replace the scalar initializers with empty vecs (the chain is sized lazily — see Step 4):

```rust
            global_shader_prev: Vec::new(),
            global_shader_start: Cell::new(None),
            global_shader_result: Vec::new(),
            global_shader_screen_prev: None,
            global_shader_screen_result: Rc::new(RefCell::new(None)),
            global_shader_pass_offscreen: Vec::new(),
            global_shader_buffer: Vec::new(),
            global_shader_buffer_prev: Vec::new(),
            global_shader_buffer_result: Vec::new(),
```

- [ ] **Step 4: Add a helper to (re)size per-output chain state**

In `src/niri.rs`, add a free function near `global_shader_pass_sources`:

```rust
/// Ensure an output's per-pass global-shader state vecs are sized to `n` passes, preserving
/// existing textures where the length is unchanged. Called each frame before building the element.
fn resize_global_shader_state(state: &mut OutputState, n: usize) {
    use crate::render_helpers::offscreen::OffscreenBuffer;
    if state.global_shader_prev.len() == n {
        return;
    }
    state.global_shader_prev = (0..n).map(|_| None).collect();
    state.global_shader_result = (0..n).map(|_| Rc::new(RefCell::new(None))).collect();
    state.global_shader_pass_offscreen =
        (0..n).map(|_| Rc::new(OffscreenBuffer::default())).collect();
    state.global_shader_buffer = (0..n).map(|_| Rc::new(OffscreenBuffer::default())).collect();
    state.global_shader_buffer_prev = (0..n).map(|_| None).collect();
    state.global_shader_buffer_result = (0..n).map(|_| Rc::new(RefCell::new(None))).collect();
}
```

> `OffscreenBuffer` implements `Default` (`src/render_helpers/offscreen.rs:204`) — this is exactly what the current single `global_shader_buffer` initializer uses (`src/niri.rs:2968-2970`), so `OffscreenBuffer::default()` is correct.

- [ ] **Step 5: Define the per-pass element state struct + rewrite `GlobalShaderElement`**

In `src/render_helpers/global_shader_element.rs`, add above `GlobalShaderElement`:

```rust
/// Per-pass feedback + offscreen handles cloned from `OutputState` for one frame.
#[derive(Debug, Clone)]
pub struct GlobalPassState {
    /// This pass's output last frame (its `niri_prev`).
    pub prev: Option<GlesTexture>,
    /// Sink for this pass's output this frame.
    pub result: Rc<RefCell<Option<GlesTexture>>>,
    /// Display offscreen (intermediate passes render here; unused for the last pass).
    pub pass_offscreen: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// Dedicated `global_buffer` offscreen.
    pub buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// This pass's dedicated buffer last frame (its `niri_buffer` when it has a buffer program).
    pub buffer_prev: Option<GlesTexture>,
    /// Sink for this pass's dedicated buffer this frame.
    pub buffer_result: Rc<RefCell<Option<GlesTexture>>>,
}
```

Replace the `prev`/`screen_*`/`buffer*`/`result` fields on `GlobalShaderElement` with the scalar screen pair plus a `passes: Vec<GlobalPassState>`:

```rust
    /// Previous frame's screen capture, bound as `niri_screen_prev` (frame-level, all passes).
    screen_prev: Option<GlesTexture>,
    /// Sink for this frame's screen capture, ping-ponged like a pass result.
    screen_result: Rc<RefCell<Option<GlesTexture>>>,
    /// One entry per pass, in execution order.
    passes: Vec<GlobalPassState>,
```

Update `GlobalShaderElement::new` to take `screen_prev`, `screen_result`, and `passes: Vec<GlobalPassState>` in place of the old per-texture params (keep `id, area, scale, time, cursor, region_norm, output_size_phys`).

- [ ] **Step 6: Rewrite `draw()` as the pass loop**

Replace the body of `impl RenderElement<GlesRenderer> for GlobalShaderElement::draw` (the GLES path) with:

```rust
        let _span = tracy_client::span!("GlobalShaderElement::draw");

        let buffer_size = dst.size.to_logical(1).to_buffer(1, Transform::Normal);

        // Capture the composited screen below: niri_source for all passes, niri_screen for pass 0.
        let source_tex = {
            let mut guard = frame.renderer();
            guard.as_mut().create_buffer(Fourcc::Abgr8888, buffer_size)?
        };
        capture::capture_framebuffer_region(frame, dst, &source_tex)?;
        *self.screen_result.borrow_mut() = Some(source_tex.clone());

        let n = self.passes.len();
        // No chain, or any pass program missing => passthrough the captured screen unchanged.
        let chain_ready = n > 0
            && (0..n).all(|i| {
                Shaders::get_from_frame(frame)
                    .program(ProgramType::GlobalPass(i))
                    .is_some()
            });
        if !chain_ready {
            return frame.render_texture_from_to(
                &source_tex,
                Rectangle::from_size(source_tex.size().to_f64()),
                dst,
                damage,
                &[],
                frame.transformation().invert(),
                1.,
                None,
                &[],
            );
        }

        let screen_prev_tex = self.screen_prev.clone().unwrap_or_else(|| source_tex.clone());

        let uniforms: Rc<[Uniform<'static>]> = Rc::new([
            Uniform::new("niri_time", self.time),
            Uniform::new("niri_cursor", (self.cursor.0, self.cursor.1)),
            Uniform::new(
                "niri_region",
                (
                    self.region_norm[0],
                    self.region_norm[1],
                    self.region_norm[2],
                    self.region_norm[3],
                ),
            ),
            Uniform::new(
                "niri_output_size",
                (self.output_size_phys.0, self.output_size_phys.1),
            ),
        ]);

        let mut input = source_tex.clone();

        for (i, pass) in self.passes.iter().enumerate() {
            let prev_tex = pass.prev.clone().unwrap_or_else(|| input.clone());

            // --- Dedicated buffer sub-pass for this pass (if it defines global_buffer) ---
            let buffer_tex = if Shaders::get_from_frame(frame)
                .program(ProgramType::GlobalPassBuffer(i))
                .is_some()
            {
                let buf_prev = pass
                    .buffer_prev
                    .clone()
                    .unwrap_or_else(|| screen_prev_tex.clone());

                let mut buf_textures = HashMap::new();
                buf_textures.insert("niri_screen".to_string(), input.clone());
                buf_textures.insert("niri_source".to_string(), source_tex.clone());
                buf_textures.insert("niri_prev".to_string(), prev_tex.clone());
                buf_textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
                buf_textures.insert("niri_buffer".to_string(), buf_prev);

                let buf_element = ShaderRenderElement::new(
                    ProgramType::GlobalPassBuffer(i),
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
                match pass
                    .buffer
                    .render(renderer, Scale::from(self.scale as f64), &[buf_element])
                {
                    Ok((off_elem, _sync, _data)) => {
                        let next = off_elem.texture().clone();
                        drop(guard);
                        *pass.buffer_result.borrow_mut() = Some(next.clone());
                        next
                    }
                    Err(err) => {
                        drop(guard);
                        warn!("global_buffer pass {i} failed: {err:?}");
                        prev_tex.clone()
                    }
                }
            } else {
                prev_tex.clone()
            };

            // --- This pass's display program ---
            let mut textures = HashMap::new();
            textures.insert("niri_screen".to_string(), input.clone());
            textures.insert("niri_source".to_string(), source_tex.clone());
            textures.insert("niri_prev".to_string(), prev_tex.clone());
            textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
            textures.insert("niri_buffer".to_string(), buffer_tex);

            let element = ShaderRenderElement::new(
                ProgramType::GlobalPass(i),
                self.area.size,
                None,
                self.scale,
                1.,
                uniforms.clone(),
                textures,
                Kind::Unspecified,
            );

            if i + 1 < n {
                // Intermediate pass: render into this pass's display offscreen; its texture is the
                // next pass's input AND this pass's next-frame niri_prev.
                let mut guard = frame.renderer();
                let renderer = guard.as_mut();
                match pass
                    .pass_offscreen
                    .render(renderer, Scale::from(self.scale as f64), &[element])
                {
                    Ok((off_elem, _sync, _data)) => {
                        let next = off_elem.texture().clone();
                        drop(guard);
                        *pass.result.borrow_mut() = Some(next.clone());
                        input = next;
                    }
                    Err(err) => {
                        drop(guard);
                        warn!("global pass {i} failed: {err:?}");
                        // Best effort: feed the input forward unchanged.
                    }
                }
            } else {
                // Last pass: composite into the output framebuffer, then capture for niri_prev.
                RenderElement::<GlesRenderer>::draw(
                    &element,
                    frame,
                    Rectangle::from_size((1., 1.).into()),
                    dst,
                    damage,
                    &[],
                    None,
                )?;
                let result_tex = {
                    let mut guard = frame.renderer();
                    guard.as_mut().create_buffer(Fourcc::Abgr8888, buffer_size)?
                };
                capture::capture_framebuffer_region(frame, dst, &result_tex)?;
                *pass.result.borrow_mut() = Some(result_tex);
            }
        }

        Ok(())
```

> Imports: `ProgramType` is already imported; ensure `GlobalPassState` is exported from this module (`pub struct`). The retained-clone ping-pong invariant (the stored clone forces `OffscreenBuffer` to allocate a fresh texture next frame) is preserved per-pass: each `pass.result`/`pass.buffer_result` holds a live clone — keep those assignments.

- [ ] **Step 7: Update element construction in `src/niri.rs`**

At `src/niri.rs:4323`, change the gate to check pass 0 of the new chain:

```rust
        let mut global_shader_elem: Option<GlobalShaderElement> = if ctx.target
            == RenderTarget::Output
            && Shaders::get(ctx.renderer)
                .program(ProgramType::GlobalPass(0))
                .is_some()
        {
```

Before constructing the element, size the per-output state to the chain length (read it from the registry — the number of installed pass programs):

```rust
            let n_passes = Shaders::get(ctx.renderer).custom_global_passes.borrow().len();
            resize_global_shader_state(state, n_passes);
            let passes = (0..n_passes)
                .map(|i| crate::render_helpers::global_shader_element::GlobalPassState {
                    prev: state.global_shader_prev[i].clone(),
                    result: state.global_shader_result[i].clone(),
                    pass_offscreen: state.global_shader_pass_offscreen[i].clone(),
                    buffer: state.global_shader_buffer[i].clone(),
                    buffer_prev: state.global_shader_buffer_prev[i].clone(),
                    buffer_result: state.global_shader_buffer_result[i].clone(),
                })
                .collect::<Vec<_>>();
```

Then replace the `GlobalShaderElement::new(...)` call (4402-4417) to pass `state.global_shader_screen_prev.clone()`, `state.global_shader_screen_result.clone()`, and `passes` instead of the old per-texture args.

> Region/`cursor_radius` mode (4359) must apply only to a length-1 chain: change the match guard `Some(r) if caps.uses_cursor && !caps.is_animating() && r > 0` to also require `n_passes <= 1` (whole-output for N ≥ 2 per Global Constraints). Compute `n_passes` before this match.

- [ ] **Step 8: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: errors now only in `src/backend/tty.rs` (the startup `set_custom_global_program` call and the post-submit ping-pong, fixed in Task 6). `niri.rs` and `global_shader_element.rs` must be green. Fix any borrow/lifetime issues the compiler flags in the draw loop (e.g. dropping `guard` before reusing `frame`).

- [ ] **Step 9: Commit**

```bash
git add src/niri.rs src/render_helpers/global_shader_element.rs
git commit -m "global-shader: per-pass state + draw() pass-chain loop"
```

---

## Task 6: Backend wiring + reload + caps union

**Files:**
- Modify: `src/backend/tty.rs` (startup compile, post-submit per-pass moves)
- Modify: `src/niri.rs` (reload diff/reset, caps helper)

**Interfaces:**
- Consumes: `shaders::set_custom_global_passes`, `global_shader_pass_sources`, `GlobalShaderCaps::scan_chain` (from Tasks 1–3, 5).

- [ ] **Step 1: Startup compile via the chain installer**

In `src/backend/tty.rs` (~846-850), replace:

```rust
            {
                let src = global_shader_source(&config.global_shader);
                let hyprland = config.global_shader.mode == "hyprland";
                shaders::set_custom_global_program(gles_renderer, src.as_deref(), hyprland);
            }
```

with:

```rust
            {
                let passes = global_shader_pass_sources(&config.global_shader);
                shaders::set_custom_global_passes(gles_renderer, &passes);
            }
```

Ensure `global_shader_pass_sources` is imported (same path as the existing `global_shader_source` import in tty.rs).

- [ ] **Step 2: Per-pass post-submit ping-pong moves**

In `src/backend/tty.rs` (~1945-1964), replace the single-texture moves with per-pass vec moves (screen stays scalar):

```rust
                if let Some(output_state) = niri.output_state.get_mut(output) {
                    for i in 0..output_state.global_shader_result.len() {
                        let result = output_state.global_shader_result[i].borrow_mut().take();
                        if result.is_some() {
                            output_state.global_shader_prev[i] = result;
                        }
                        let buffer_result =
                            output_state.global_shader_buffer_result[i].borrow_mut().take();
                        if buffer_result.is_some() {
                            output_state.global_shader_buffer_prev[i] = buffer_result;
                        }
                    }
                    let screen_result = output_state
                        .global_shader_screen_result
                        .borrow_mut()
                        .take();
                    if screen_result.is_some() {
                        output_state.global_shader_screen_prev = screen_result;
                    }
                }
```

- [ ] **Step 3: Reload diff + reset + recompile**

The reload block is `src/niri.rs:1608-1634`. First add `passes` to the change-diff condition (1608-1614) by appending a line before the closing `{`:

```rust
            || config.global_shader.passes != old_config.global_shader.passes
```

Then replace the recompile + per-output reset body (currently 1615-1630) with the chain installer and vec resets:

```rust
            let passes = global_shader_pass_sources(&config.global_shader);
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_global_passes(renderer, &passes);
            });
            // Reset per-output time origin and ALL per-pass feedback state on (re)load. The vecs
            // are re-sized next frame from the new chain length (resize_global_shader_state).
            for state in self.niri.output_state.values_mut() {
                state.global_shader_start.set(None);
                state.global_shader_prev = Vec::new();
                state.global_shader_result = Vec::new();
                state.global_shader_pass_offscreen = Vec::new();
                state.global_shader_buffer = Vec::new();
                state.global_shader_buffer_prev = Vec::new();
                state.global_shader_buffer_result = Vec::new();
                state.global_shader_screen_prev = None;
                *state.global_shader_screen_result.borrow_mut() = None;
            }
```

The existing lines after the loop (`self.niri.global_shader_caps.set(None);` and `shaders_changed = true;`) stay unchanged. `set_custom_global_program` no longer exists, so this is the only remaining call site besides the tty startup one (Step 1).

- [ ] **Step 4: Caps helper uses the chain**

In `src/niri.rs` `global_shader_caps()` (~2292-2304), replace the single-source scan with the chain union:

```rust
    pub fn global_shader_caps(&self) -> niri_config::GlobalShaderCaps {
        if let Some(caps) = self.global_shader_caps.get() {
            return caps;
        }
        let cfg = self.config.borrow();
        let passes = global_shader_pass_sources(&cfg.global_shader);
        let caps = niri_config::GlobalShaderCaps::scan_chain(&passes);
        self.global_shader_caps.set(Some(caps));
        caps
    }
```

> Remove the now-unused single-source branch (the old `match global_shader_source(...)`), keeping the cache get/set. Adjust borrows so `cfg` is dropped before `self.global_shader_caps.set` if the borrow checker complains.

- [ ] **Step 5: Full compile-check (dev shell) — first green checkpoint for the render path**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: PASS (no errors). Fix any remaining references to the removed singletons (`global_shader_result.borrow_mut()` as a scalar, `set_custom_global_program`, `ProgramType::Global` gating) the compiler flags.

- [ ] **Step 6: Run niri-config tests (regression)**

Run: `cargo test -p niri-config`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/backend/tty.rs src/niri.rs
git commit -m "global-shader: wire pass-chain compile, ping-pong, reload, caps union"
```

---

## Task 7: Docs + wiki example + manual verification

**Files:**
- Modify: `docs/wiki/Configuration:-Global-Shader.md`
- (Verification only) sixseven machine

**Interfaces:** none (docs + manual).

- [ ] **Step 1: Document multi-pass in the wiki**

Add a "Multi-pass chains" section to `docs/wiki/Configuration:-Global-Shader.md` covering: the `pass{}` block (source/path/mode), execution order, the pipe model (`niri_screen` = prior pass output, pass 0 = real screen), `niri_source` (original screen), per-pass `niri_prev` and `niri_buffer`, and that a length-0/1 chain equals today's single shader. Include a worked example:

````markdown
```kdl
global-shader {
    enable
    pass { path "blur.frag" }
    pass { path "grade.frag" }
    pass { source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }" }
}
```
````

- [ ] **Step 2: Verify the wiki example parses**

The `niri-config` test `wiki_docs_parses` extracts and parses KDL fences from the wiki. Run:

Run: `cargo test -p niri-config wiki_docs_parses`
Expected: PASS (the new fence parses; if it intentionally shows a non-parseable fragment, mark it with the language the test ignores — check how other illustrative fences in that file are tagged).

- [ ] **Step 3: Build + deploy to sixseven**

Per spec §10 / next-steps §5: push `barrulus-custom`, `nix flake update biri --flake ~/quixote`, `sudo nixos-rebuild switch --flake ~/quixote#sixseven`.

- [ ] **Step 4: Manual verification — composition + per-pass feedback**

Configure a 2–3 pass chain, e.g. a blur/grade pass followed by the existing `comet` or `trail` shader as the final pass. Verify:
- Both effects are visible and composed (the trail rides on top of the blurred/graded screen).
- The trail still accumulates across frames (proves per-pass `niri_prev` ping-pong).
- Scrolling content under the trail shows no smear (proves `niri_source`/`niri_screen_prev` are correct).
- Switching back to a single-shader config (no `pass{}`) is visually identical to before.

Record with KMS capture only: `gpu-screen-recorder -w eDP-1 ...`.

- [ ] **Step 5: Commit docs**

```bash
git add docs/wiki/Configuration:-Global-Shader.md
git commit -m "docs: global-shader multi-pass chains"
```

---

## Self-Review Notes (spec coverage)

- Spec §3.A (registry) → Task 3. §3.B (per-output vecs) → Task 5 Steps 2–4. §3.C (`niri_source`, pass-0 identity) → Task 4 + Task 5 draw loop. §3.D (draw flow) → Task 5 Step 6. §3.E (caps union, whole-output N≥2, reload) → Task 2 + Task 5 Step 7 + Task 6 Steps 3–4.
- Spec §4 (config, back-compat, per-pass mode, MergeWith) → Task 1.
- Spec §5 (sampler summary) → Task 4 (prelude) + Task 7 (docs).
- Spec §6 (testing) → Task 1/2 unit tests, Task 6 regression, Task 7 manual.
- Spec §7 (scope: no MRT, no DAG, no scoped shaders) → enforced by construction; nothing implements them.
- Spec §9 open questions resolved in-plan: offscreens allocated uniformly per slot (Task 5 Step 4, two `OffscreenBuffer`s per pass via `_pass_offscreen` + `_buffer`); `MergeWith` = wholesale replace (Task 1 Step 5).
- Byte-identity invariant: length-1 chain uses `GlobalPass(0)` (= old `Global`), `niri_source`==`niri_screen` for pass 0, last-pass path == old single-pass path. Verified manually in Task 7 Step 4.
