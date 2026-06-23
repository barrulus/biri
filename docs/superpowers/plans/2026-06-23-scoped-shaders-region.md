# Scoped Shaders — Plan 1: Shared Substrate + Region Shaders

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run multiple independent post-process shaders, each scoped to a configured screen rectangle (`region-shader`), reusing the niri-mode `global_color` contract and the multi-pass chain — plus the shared program-cache substrate that Plan 2 (window shaders) will also use.

**Architecture:** A source-keyed program cache (`Shaders.scoped: HashMap<u64, Vec<ShaderProgram>>`) compiles each distinct scoped-shader source once. A new `ScopedShaderElement` (sources: `Capture` for regions, `Texture` for windows in Plan 2) runs the cached chain over an `area`/`region_norm`, mirroring `GlobalShaderElement::draw`. Region shaders are a repeatable top-level `region-shader{}` config node; `Niri::render_inner` pushes one element per region whose `output` matches.

**Tech Stack:** Rust, GLES2 fragment shaders, smithay GLES renderer (`GlesTexture`, `ShaderRenderElement`, `capture_framebuffer_region`), knuffel (KDL config), insta.

**Spec:** `docs/superpowers/specs/2026-06-23-scoped-shaders-design.md` — read it before starting. This plan implements §3.A, §3.B, §3.C, §3.E (region only), §3.F (region only).

## Global Constraints

- **Reuse the niri-mode contract verbatim.** No new prelude/epilogue/dialect. Scoped shaders compile via the existing `compile_global_program` and run `ProgramType::Scoped(key, i)`; the host binds samplers/uniforms per target. An existing whole-output `global_color` shader must run unchanged in a region.
- **v1 has no per-scope feedback.** `niri_prev`/`niri_screen_prev`/`niri_buffer` all alias `niri_screen`; `global_buffer` ignored. No ping-pong, no result capture for scoped elements.
- **Byte-identity:** with no `region-shader` configured, the output is identical to today — no scoped elements pushed, zero overhead.
- **Source dedupe:** identical resolved source sets across regions share one compiled chain (keyed by `scoped_key`).
- **TTY/DRM only**, `RenderTarget::Output` only (same gate as global-shader); excluded from screencast/screenshot.
- **Build/test crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task compile `cargo check --no-default-features --features dbus,systemd` inside it with `export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib`. `niri-config` builds/tests outside the dev shell: `cargo test -p niri-config`. Inline-snapshot gotcha: `cargo insta accept` can hang — patch `niri-config/src/.lib.rs.pending-snap` by hand (8-space indent). Commits: NO Co-Authored-By / AI-attribution lines.

---

## File Structure

**Config (niri-config — pure logic, testable outside dev shell):**
- Create `niri-config/src/region_shader.rs` — `RegionShader`, `RegionShaderPart`, `ScopedShaderPart` (shared shader-source shape), `Geometry`.
- Modify `niri-config/src/lib.rs` — `pub mod region_shader`, re-exports, `Config.region_shaders: Vec<RegionShader>` field, `"region-shader" => m_push!(region_shaders)` dispatch, default snapshot.

**Registry (src — needs dev shell):**
- Modify `src/render_helpers/shaders/mod.rs` — `ProgramType::Scoped(u64, usize)`, `scoped: RefCell<HashMap<u64, Vec<ShaderProgram>>>`, `scoped_key`, `set_scoped_programs`, `program()` arm.

**Element (src):**
- Create `src/render_helpers/scoped_shader_element.rs` — `ScopedShaderElement`, `ScopedSource { Capture, Texture(GlesTexture) }`.
- Modify `src/render_helpers/mod.rs` — `pub mod scoped_shader_element;`.

**Wiring (src):**
- Modify `src/niri.rs` — resolve region shaders → `(area, region_norm, key, output)`; install scoped programs on startup/reload; push `ScopedShaderElement` per region in `render_inner`; animated-region redraw.
- Modify `src/backend/tty.rs` — install scoped programs at startup alongside `set_custom_global_passes`.

**Docs:**
- Modify `docs/wiki/Configuration:-Global-Shader.md` — a "Region shaders" section + a parsing example.

---

## Task Ordering & Dependencies

1. **Task 1** (config) + **Task 2** (the shared shader-source resolver in niri-config) — pure niri-config, testable outside the dev shell.
2. **Task 3** (registry) — compile-only.
3. **Task 4** (`ScopedShaderElement`) — compile-only; depends on Task 3.
4. **Task 5** (render wiring + reload + redraw) — depends on 1–4; first full green `cargo check`.
5. **Task 6** (docs + manual verify).

---

## Task 1: Config — `region-shader` node

**Files:**
- Create: `niri-config/src/region_shader.rs`
- Modify: `niri-config/src/lib.rs`
- Test: `niri-config/src/region_shader.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub struct ScopedShaderPart` (knuffel `Decode`): `source: Option<String>`, `path: Option<String>`, `mode: Option<String>`, `passes: Vec<GlobalShaderPassPart>` (reuse the existing type from `global_shader.rs`).
  - `pub struct Geometry { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }` (knuffel `Decode`, fields as KDL properties).
  - `pub struct RegionShaderPart` (knuffel `Decode`): `geometry: Option<Geometry>`, `output: Option<String>`, flattened shader source fields (`source`/`path`/`mode`/`passes`).
  - `pub struct RegionShader { pub geometry: Geometry, pub output: Option<String>, pub source: Option<String>, pub path: Option<String>, pub mode: String, pub passes: Vec<GlobalShaderPass> }` (resolved; derives `Debug, Clone, PartialEq`).
  - `Config.region_shaders: Vec<RegionShader>`.

- [ ] **Step 1: Write the failing parse test**

Create `niri-config/src/region_shader.rs` with a tests module:

```rust
use crate::global_shader::{GlobalShaderPass, GlobalShaderPassPart};

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Geometry {
    #[knuffel(property)]
    pub x: f64,
    #[knuffel(property)]
    pub y: f64,
    #[knuffel(property)]
    pub width: f64,
    #[knuffel(property)]
    pub height: f64,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct RegionShaderPart {
    #[knuffel(child)]
    pub geometry: Option<Geometry>,
    #[knuffel(child, unwrap(argument))]
    pub output: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionShader {
    pub geometry: Geometry,
    pub output: Option<String>,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub passes: Vec<GlobalShaderPass>,
}

impl From<RegionShaderPart> for RegionShader {
    fn from(p: RegionShaderPart) -> Self {
        let mode = p.mode.clone().unwrap_or_else(|| String::from("niri"));
        let passes = p
            .passes
            .iter()
            .map(|pp| GlobalShaderPass {
                source: pp.source.clone(),
                path: pp.path.clone(),
                mode: pp.mode.clone().unwrap_or_else(|| mode.clone()),
            })
            .collect();
        RegionShader {
            geometry: p.geometry.unwrap_or_default(),
            output: p.output,
            source: p.source,
            path: p.path,
            mode,
            passes,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn region_shader_parses() {
        let config = Config::parse_mem(
            r##"
            region-shader {
                geometry x=100 y=100 width=800 height=600
                output "DP-1"
                source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
            }
            region-shader {
                geometry x=0 y=0 width=1920 height=40
                source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy)*0.5; }"
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.region_shaders.len(), 2);
        assert_eq!(config.region_shaders[0].geometry.width, 800.0);
        assert_eq!(config.region_shaders[0].output.as_deref(), Some("DP-1"));
        assert!(config.region_shaders[1].output.is_none());
        assert_eq!(config.region_shaders[1].geometry.height, 40.0);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p niri-config region_shader_parses`
Expected: FAIL — module/field not wired into `Config` yet (compile error).

- [ ] **Step 3: Wire the module + Config field + dispatch**

In `niri-config/src/lib.rs`:
- Add `pub mod region_shader;` near the other `pub mod` lines (~line 37).
- Re-export: add `pub use crate::region_shader::{Geometry, RegionShader, RegionShaderPart};` near the other `pub use` lines (~line 54).
- Add the field to `Config`: `pub region_shaders: Vec<RegionShader>,` (near `window_rules`).
- In the node-dispatch match, add a repeatable-node arm next to `"window-rule" => m_push!(window_rules)`:

```rust
                "region-shader" => {
                    region_shaders.push(RegionShaderPart::decode_node(node, ctx)?.into());
                }
```

> Check how `m_push!`/`output` decode repeatable nodes at `niri-config/src/lib.rs:212-219`; mirror the `output` arm if `m_push!` does not apply cleanly to a `Part`→resolved conversion. The collected `Vec<RegionShader>` is assigned to `config.region_shaders` where the other `m_push!` vecs are assigned.

- [ ] **Step 4: Run the parse test**

Run: `cargo test -p niri-config region_shader_parses`
Expected: PASS.

- [ ] **Step 5: Update the default-config inline snapshot**

`niri-config/src/lib.rs` has an inline `assert_debug_snapshot!` of the default `Config`. Adding `region_shaders` changes the `Debug` output. Run `cargo test -p niri-config` to find the failing snapshot; add `region_shaders: [],` in the correct position in the inline `@r#"..."#` block (match the field order in the struct). Do NOT use `cargo insta accept`; patch by hand from `.lib.rs.pending-snap` if needed (8-space indent).

- [ ] **Step 6: Full niri-config suite**

Run: `cargo test -p niri-config`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add niri-config/src/region_shader.rs niri-config/src/lib.rs
git commit -m "niri-config: region-shader config node"
```

---

## Task 2: Shared scoped-source resolver (niri-config)

**Files:**
- Modify: `niri-config/src/region_shader.rs`
- Test: same file's tests module

**Interfaces:**
- Produces: `RegionShader::pass_sources(&self, expand: impl Fn(&str) -> Option<String>) -> Vec<(String, bool)>` — resolve this region's shader into the `(source, hyprland)` pass list (empty passes → single from top-level `source`/`path`; any unresolved → empty = disabled). The `expand` closure resolves a `path` to its file contents (the host passes a real reader; tests pass an inline stub) so this stays pure in `niri-config`.

> Rationale: the global resolver `global_shader_pass_sources` lives in `src/niri.rs` (it does filesystem reads). Region resolution needs the same shape but `niri-config` must not do IO. So expose a pure resolver that takes a path→contents closure; the host (`src/niri.rs`) supplies one that reads files (reusing `expand_home` + `read_to_string`).

- [ ] **Step 1: Write the failing test**

Add to the tests module:

```rust
#[test]
fn region_pass_sources_resolves() {
    let config = Config::parse_mem(
        r##"
        region-shader {
            geometry x=0 y=0 width=10 height=10
            source "A"
        }
        region-shader {
            geometry x=0 y=0 width=10 height=10
            pass { source "B" }
            pass { path "p.frag" }
        }
        "##,
    )
    .unwrap();
    // Top-level source -> length-1 chain.
    let r0 = config.region_shaders[0].pass_sources(|_| None);
    assert_eq!(r0, vec![("A".to_string(), false)]);
    // Pass chain: inline B + resolved path.
    let r1 = config.region_shaders[1].pass_sources(|p| {
        assert_eq!(p, "p.frag");
        Some("C".to_string())
    });
    assert_eq!(r1, vec![("B".to_string(), false), ("C".to_string(), false)]);
    // Unresolvable path -> empty (disabled).
    let r1_bad = config.region_shaders[1].pass_sources(|_| None);
    assert!(r1_bad.is_empty());
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p niri-config region_pass_sources_resolves`
Expected: FAIL — `pass_sources` undefined.

- [ ] **Step 3: Implement `pass_sources`**

Add to `impl RegionShader`:

```rust
    /// Resolve this region's shader into an ordered `(source, hyprland)` pass list. Empty `passes`
    /// => the top-level `source`/`path` becomes a length-1 chain. Any pass that cannot be resolved
    /// => empty (the whole region is disabled). `expand` maps a `path` to its file contents.
    pub fn pass_sources(&self, expand: impl Fn(&str) -> Option<String>) -> Vec<(String, bool)> {
        let resolve_one = |source: &Option<String>, path: &Option<String>, mode: &str| {
            match (source, path) {
                (Some(s), None) if !s.trim().is_empty() => Some((s.clone(), mode == "hyprland")),
                (None, Some(p)) => expand(p).map(|s| (s, mode == "hyprland")),
                _ => None,
            }
        };
        if self.passes.is_empty() {
            return match resolve_one(&self.source, &self.path, &self.mode) {
                Some(pair) => vec![pair],
                None => Vec::new(),
            };
        }
        let mut out = Vec::with_capacity(self.passes.len());
        for pass in &self.passes {
            match resolve_one(&pass.source, &pass.path, &pass.mode) {
                Some(pair) => out.push(pair),
                None => return Vec::new(),
            }
        }
        out
    }
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p niri-config region_pass_sources_resolves`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/region_shader.rs
git commit -m "niri-config: RegionShader::pass_sources resolver"
```

---

## Task 3: Registry — source-keyed scoped program cache

**Files:**
- Modify: `src/render_helpers/shaders/mod.rs`

**Interfaces:**
- Consumes: `compile_global_program(renderer, src, hyprland)` (exists, niri-config-independent), `scan_chain` not needed here.
- Produces:
  - `ProgramType::Scoped(u64, usize)`.
  - `Shaders.scoped: RefCell<HashMap<u64, Vec<ShaderProgram>>>`.
  - `pub fn scoped_key(passes: &[(String, bool)]) -> u64` (stable FNV/`DefaultHasher` over sources+flags).
  - `pub fn set_scoped_programs(renderer: &mut GlesRenderer, chains: &[Vec<(String, bool)>])` — compile any missing keys, drop+destroy keys not in `chains`. Dedupes by key.

- [ ] **Step 1: Add imports + enum variant + field**

In `src/render_helpers/shaders/mod.rs`:
- Add `use std::collections::HashMap;` and `use std::hash::{Hash, Hasher};` at the top.
- Add to `ProgramType`: `Scoped(u64, usize),`.
- Add to `struct Shaders`: `pub scoped: RefCell<HashMap<u64, Vec<ShaderProgram>>>,`.
- In `Shaders::compile`'s returned `Self { .. }`, add `scoped: RefCell::new(HashMap::new()),`.

- [ ] **Step 2: Add the `program()` arm**

In `fn program`, add:

```rust
            ProgramType::Scoped(key, i) => self
                .scoped
                .borrow()
                .get(&key)
                .and_then(|chain| chain.get(i))
                .cloned(),
```

- [ ] **Step 3: Add `scoped_key` + `set_scoped_programs`**

Add free functions (near `set_custom_global_passes`):

```rust
/// Stable hash of a resolved pass list — the cache key for a scoped shader chain. Pure function of
/// the source strings and hyprland flags (no nondeterminism).
pub fn scoped_key(passes: &[(String, bool)]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (src, hypr) in passes {
        src.hash(&mut h);
        hypr.hash(&mut h);
    }
    h.finish()
}

/// Install the set of scoped shader chains. Each entry is a resolved `(source, hyprland)` pass
/// list. Compiles any key not already cached, and destroys any cached key no longer referenced.
/// Identical pass lists share one compiled chain (same key).
pub fn set_scoped_programs(renderer: &mut GlesRenderer, chains: &[Vec<(String, bool)>]) {
    use std::collections::HashSet;
    let wanted: HashSet<u64> = chains.iter().map(|c| scoped_key(c)).collect();

    // Drop + destroy stale keys.
    let stale: Vec<u64> = {
        let scoped = Shaders::get(renderer).scoped.borrow();
        scoped.keys().copied().filter(|k| !wanted.contains(k)).collect()
    };
    for k in stale {
        if let Some(chain) = Shaders::get(renderer).scoped.borrow_mut().remove(&k) {
            for p in chain {
                if let Err(err) = p.destroy(renderer) {
                    warn!("error destroying scoped shader program: {err:?}");
                }
            }
        }
    }

    // Compile missing keys.
    for passes in chains {
        if passes.is_empty() {
            continue;
        }
        let key = scoped_key(passes);
        if Shaders::get(renderer).scoped.borrow().contains_key(&key) {
            continue;
        }
        let mut compiled = Vec::with_capacity(passes.len());
        let mut ok = true;
        for (src, hyprland) in passes {
            match compile_global_program(renderer, src, *hyprland) {
                Ok(p) => compiled.push(p),
                Err(err) => {
                    warn!("error compiling scoped shader: {err:?}");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            Shaders::get(renderer).scoped.borrow_mut().insert(key, compiled);
        } else {
            for p in compiled {
                let _ = p.destroy(renderer);
            }
        }
    }
}
```

- [ ] **Step 4: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: PASS for `shaders/mod.rs` (this task adds only additive items; no existing caller breaks). If `cargo check` is green overall, great; if other files are mid-flight from later tasks, ensure no error originates in `shaders/mod.rs`.

- [ ] **Step 5: Commit**

```bash
git add src/render_helpers/shaders/mod.rs
git commit -m "scoped-shader: source-keyed program cache (ProgramType::Scoped)"
```

---

## Task 4: `ScopedShaderElement`

**Files:**
- Create: `src/render_helpers/scoped_shader_element.rs`
- Modify: `src/render_helpers/mod.rs` (`pub mod scoped_shader_element;`)

**Interfaces:**
- Consumes: `ProgramType::Scoped(u64, usize)`, `Shaders::get_from_frame`, `capture::capture_framebuffer_region`, `OffscreenBuffer`, `ShaderRenderElement` (all exist).
- Produces:
  - `pub enum ScopedSource { Capture, Texture(GlesTexture) }`.
  - `pub struct ScopedShaderElement` + `new(id, area: Rectangle<f64, Logical>, scale: f32, time: f32, cursor: (f32,f32), region_norm: [f32;4], output_size_phys: (f32,f32), size_phys: (f32,f32), key: u64, n_passes: usize, source: ScopedSource, offscreens: Vec<Rc<OffscreenBuffer>>)`.
  - `impl Element` + `impl RenderElement<GlesRenderer>` + `impl RenderElement<TtyRenderer<'_>>`.

This element is a trimmed `GlobalShaderElement`: no per-pass feedback, no result capture. Model the structure on `src/render_helpers/global_shader_element.rs` (read it). `draw()`:

```rust
// 1. Establish niri_screen for pass 0:
//    - ScopedSource::Capture: create_buffer + capture_framebuffer_region(frame, dst, &tex)
//    - ScopedSource::Texture(tex): use tex directly (no capture)
// 2. chain_ready = (0..n_passes).all(|i| program(Scoped(key,i)).is_some()); else passthrough
//    (Capture: blit the captured screen; Texture: blit the source texture) and return.
// 3. uniforms: niri_time, niri_cursor, niri_region, niri_output_size, niri_size.
// 4. for i in 0..n_passes:
//      textures = { niri_screen: input, niri_source: input, niri_prev: input,
//                   niri_screen_prev: input, niri_buffer: input }  // all alias input (v1)
//      element = ShaderRenderElement::new(ProgramType::Scoped(key, i), area.size, None, scale, 1., uniforms, textures, Kind::Unspecified)
//      if i+1 < n: render into offscreens[i] (OffscreenBuffer::render) -> input = its texture
//      else: RenderElement::draw(&element, frame, unit_src, dst, damage, &[], None)
```

- [ ] **Step 1: Create the element file**

Create `src/render_helpers/scoped_shader_element.rs`. Start from a copy of `global_shader_element.rs` and transform per the pseudocode above: remove the `screen_prev`/`screen_result`/`passes: Vec<GlobalPassState>` feedback machinery; add `source: ScopedSource`, `key: u64`, `n_passes: usize`, `size_phys`, and `offscreens: Vec<Rc<OffscreenBuffer>>` (one per intermediate pass). Add the `niri_size` uniform. Bind all five samplers to `input`. No `result`/`buffer_result` writes, no post-submit ping-pong.

Add the module line to `src/render_helpers/mod.rs`:

```rust
pub mod scoped_shader_element;
```

- [ ] **Step 2: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: `scoped_shader_element.rs` compiles (it is not yet constructed anywhere; an `unused` warning for `ScopedShaderElement::new` is acceptable until Task 5). Iterate against the borrow checker for the `frame.renderer()` guard-drop discipline (drop the guard before reusing `frame`), exactly as `global_shader_element.rs` does.

- [ ] **Step 3: Commit**

```bash
git add src/render_helpers/scoped_shader_element.rs src/render_helpers/mod.rs
git commit -m "scoped-shader: ScopedShaderElement (Capture + Texture sources, no feedback)"
```

---

## Task 5: Render wiring — region shaders

**Files:**
- Modify: `src/niri.rs` (resolve regions, install programs, push elements, redraw)
- Modify: `src/backend/tty.rs` (install scoped programs at startup)

**Interfaces:**
- Consumes: `set_scoped_programs`, `scoped_key`, `ProgramType::Scoped`, `ScopedShaderElement`, `RegionShader::pass_sources`.
- Produces:
  - `global_shader_pass_sources`-style host resolver applied to regions: a helper `region_shader_chains(config) -> Vec<Vec<(String,bool)>>` (one entry per region, using `pass_sources` with a real file reader).
  - `OutputRenderElements` gains a `ScopedShader = ScopedShaderElement` variant.

- [ ] **Step 1: Host resolver for region chains**

In `src/niri.rs`, near `global_shader_pass_sources`, add:

```rust
/// Resolve every configured region shader into its `(source, hyprland)` pass list, reading any
/// `path` files (reusing the same file resolution as the global shader). Index-aligned with
/// `config.region_shaders`.
pub(crate) fn region_shader_chains(config: &niri_config::Config) -> Vec<Vec<(String, bool)>> {
    config
        .region_shaders
        .iter()
        .map(|r| {
            r.pass_sources(|p| {
                let path = match expand_home(std::path::Path::new(p)) {
                    Ok(Some(e)) => e,
                    Ok(None) => std::path::PathBuf::from(p),
                    Err(_) => return None,
                };
                std::fs::read_to_string(&path).ok()
            })
        })
        .collect()
}
```

- [ ] **Step 2: Install scoped programs at startup (tty.rs)**

In `src/backend/tty.rs`, next to the `set_custom_global_passes` startup call (~line 846), add:

```rust
            {
                let chains = crate::niri::region_shader_chains(&config);
                shaders::set_scoped_programs(gles_renderer, &chains);
            }
```

Ensure `region_shader_chains` is importable (it is `pub(crate)`).

- [ ] **Step 3: Install + reset on config reload (niri.rs)**

In the reload handler in `src/niri.rs`, add a block (near the global-shader reload diff) that fires when `config.region_shaders != old_config.region_shaders`:

```rust
        if config.region_shaders != old_config.region_shaders {
            let chains = region_shader_chains(&config);
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_scoped_programs(renderer, &chains);
            });
            shaders_changed = true;
        }
```

- [ ] **Step 4: Add the `OutputRenderElements` variant**

Find the `OutputRenderElements` enum (the one that already has `GlobalShader = GlobalShaderElement`) and add:

```rust
    ScopedShader = ScopedShaderElement,
```

Import `ScopedShaderElement` and `ScopedSource` at the top of `src/niri.rs`.

- [ ] **Step 5: Push region elements in `render_inner`**

In `Niri::render_inner`, in the global-shader insertion zone (after the global-shader element push, still gated `ctx.target == RenderTarget::Output`), add: for each region whose `output` matches this output (or `None`), build and push a `ScopedShaderElement`:

```rust
        if ctx.target == RenderTarget::Output {
            let config = self.config.borrow();
            let scale = output.current_scale().fractional_scale() as f32;
            let out_name = output.name(); // confirm the accessor; used to match region.output
            let full = Rectangle::from_size(output_size(output));
            let out_phys = ((full.size.w * scale as f64) as f32, (full.size.h * scale as f64) as f32);
            for region in &config.region_shaders {
                if let Some(want) = &region.output {
                    if &out_name != want { continue; }
                }
                let chain = region.pass_sources(|p| {
                    let path = match expand_home(std::path::Path::new(p)) {
                        Ok(Some(e)) => e, Ok(None) => std::path::PathBuf::from(p), Err(_) => return None,
                    };
                    std::fs::read_to_string(&path).ok()
                });
                if chain.is_empty() { continue; }
                let key = shaders::scoped_key(&chain);
                if Shaders::get(ctx.renderer).program(ProgramType::Scoped(key, 0)).is_none() { continue; }
                let g = region.geometry;
                let area = Rectangle::new((g.x, g.y).into(), (g.width, g.height).into());
                let region_norm = [
                    ((area.loc.x - full.loc.x) / full.size.w) as f32,
                    ((area.loc.y - full.loc.y) / full.size.h) as f32,
                    (area.size.w / full.size.w) as f32,
                    (area.size.h / full.size.h) as f32,
                ];
                let size_phys = ((g.width * scale as f64) as f32, (g.height * scale as f64) as f32);
                let n_passes = chain.len();
                let offscreens = (0..n_passes.saturating_sub(1))
                    .map(|_| std::rc::Rc::new(crate::render_helpers::offscreen::OffscreenBuffer::default()))
                    .collect::<Vec<_>>();
                let elem = ScopedShaderElement::new(
                    Id::new(), area, scale, /*time*/ 0.0, /*cursor*/ (0.0, 0.0),
                    region_norm, out_phys, size_phys, key, n_passes,
                    ScopedSource::Capture, offscreens,
                );
                push(elem.into());
            }
        }
```

> Notes for the implementer: (a) confirm the output-name accessor (`output.name()` vs a smithay user-data lookup — grep how `region.output`-style matching is done for `open_on_output`/window-rule output matching and reuse it). (b) The fresh per-frame `OffscreenBuffer`s for intermediate passes are acceptable for v1 (no feedback ⇒ no cross-frame texture identity needed); a single-pass region needs none. (c) `time`/`cursor` are wired in Step 6.

- [ ] **Step 6: Animated regions — time origin + redraw**

For region shaders that use `niri_time`, supply a real time and force redraw:
- Add a per-`Niri` time origin `Cell<Option<Instant>>` (or reuse the global-shader `Instant`); compute `time = origin.elapsed().as_secs_f32()` and pass it to `ScopedShaderElement::new` in place of `0.0`.
- Reuse `GlobalShaderCaps::scan_chain(&chain).is_animating()`; if any matching region animates, OR it into the per-output continuous-redraw decision where the global shader already does so (grep `wants_continuous_redraw` in `src/niri.rs` and add the region check beside it).
- `cursor`: pass output-local physical cursor (the same value the global-shader element computes — factor it or recompute).

- [ ] **Step 7: Full compile-check (dev shell) — first green checkpoint**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: PASS (no errors). Fix borrow/lifetime issues (e.g. drop the `self.config.borrow()` before `push` if it conflicts; clone the small region data out first).

- [ ] **Step 8: niri-config regression**

Run: `cargo test -p niri-config`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/niri.rs src/backend/tty.rs
git commit -m "scoped-shader: wire region shaders (compile, push per region, reload, redraw)"
```

---

## Task 6: Docs + manual verification

**Files:**
- Modify: `docs/wiki/Configuration:-Global-Shader.md`

- [ ] **Step 1: Document region shaders**

Add a "Region shaders" section: the `region-shader{}` node, `geometry x= y= width= height=` (logical px), optional `output`, that it reuses the same `global_color` contract and `pass{}` chains as `global-shader`, multiple-independent + config-order draw, and the v1 limits (no per-scope feedback; `niri_screen` = composited pixels in the rect). Include a parsing example:

````markdown
```kdl
region-shader {
    geometry x=100 y=100 width=800 height=600
    source "vec4 global_color(vec3 c){ vec3 s=tex2D_screen(c.xy).rgb; return vec4(s.bgr,1.0); }"
}
```
````

- [ ] **Step 2: Verify the wiki example parses**

Run: `cargo test -p niri-config wiki_docs_parses`
Expected: PASS.

- [ ] **Step 3: Build + deploy to sixseven**

Per crib: push, `nix flake update biri --flake ~/quixote`, `sudo nixos-rebuild switch --flake ~/quixote#sixseven`.

- [ ] **Step 4: Manual verification**

- A `region-shader` rect over part of the screen running a visible filter (e.g. channel-swap or vignette) → only that rectangle is affected; the rest of the output is untouched and still scans out.
- Two non-overlapping regions with different shaders simultaneously → both apply (proves multiple-independent + per-key cache).
- Two regions with the *same* source → both apply (proves dedupe; one compiled chain).
- A time-animated region shader → animates while idle.
- Remove all `region-shader` blocks → output identical to before (byte-identity).
- KMS capture only.

- [ ] **Step 5: Commit docs**

```bash
git add "docs/wiki/Configuration:-Global-Shader.md"
git commit -m "docs: region shaders"
```

---

## Self-Review Notes (spec coverage)

- Spec §3.A (source-keyed cache) → Task 3. §3.B (`ScopedShaderElement`, both sources, samplers alias `niri_screen`, no ping-pong) → Task 4. §3.C (region render insertion) → Task 5. §3.E region redraw → Task 5 Step 6. §3.F region config → Tasks 1–2.
- Spec §4 contract (region column) → Task 4 sampler/uniform binding + Task 5 region_norm/size/cursor.
- Spec §5 testing → Task 1/2 unit tests, Task 5 Step 8 regression, Task 6 manual.
- Spec §6 scope (no feedback, no layer/backdrop, region-only here) → enforced by construction.
- Spec §8 open questions resolved in-plan: `scoped_key` = `DefaultHasher` over sources+flags (Task 3); region rect → `region_norm` mirrors the cursor-radius normalisation (Task 5 Step 5); per-frame intermediate offscreens acceptable without feedback (Task 5 note b).
- **Window shaders (spec §3.D, §3.F window half) are NOT in this plan** — they are Plan 2, built on this substrate (Tasks 3–4 are shared). Byte-identity guard: with no `region-shader`, nothing is pushed and `set_scoped_programs(&[])` clears the cache.
- The `Texture` source variant of `ScopedShaderElement` (Task 4) is built now but only exercised by Plan 2; v1 region path uses `Capture`. This is deliberate shared-substrate work, not dead code for long.
