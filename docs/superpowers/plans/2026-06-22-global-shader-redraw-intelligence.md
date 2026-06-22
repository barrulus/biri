# Global Shader 3.1 — Capability-Driven Damage & Redraw Intelligence — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the global post-process shader's redraw and damage behavior proportional to what the shader actually does — fixing the idle-animation freeze and (optionally) shrinking cursor-local effects to a region so scanout survives elsewhere.

**Architecture:** A pure source scan derives capability flags (`uses_time`/`uses_cursor`/`uses_prev`) in `niri-config`. Those flags plus two new config fields (`cursor-radius`, `redraw`) drive (a) whether a redraw is scheduled when the desktop is idle and (b) the shader element's geometry, hence its damage and scanout eligibility. Implemented in four tasks: Task 1 (scanner) and Task 2 (config) live in the standalone-testable `niri-config` crate; Task 3 (the freeze fix) and Task 4 (region-damage, descopable) touch the main `niri` crate.

**Tech Stack:** Rust, smithay (GLES2 renderer + DRM compositor), knuffel/KDL config, insta snapshot tests, GLSL ES `#version 100`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-06-22-global-shader-redraw-intelligence-design.md`. Read it first.
- **Additive config only** — `cursor-radius` and `redraw` are optional; existing `global-shader {}` blocks must parse and behave **unchanged**. `niri_region` defaults to `(0,0,1,1)` and `niri_output_size` to the true output size, so existing shaders render byte-identically.
- **Capability scan is substring match** and may over-trigger (token in a comment) → at worst extra redraws, never a missed animation. Never under-trigger.
- **No AI attribution** in commit messages (no Co-Authored-By, no "Generated with Claude").
- **Build crib** (per spec §8 / roadmap §5): dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task main-crate compile check `cargo check --no-default-features --features dbus,systemd` inside it; `niri-config` builds/tests standalone with `cargo test -p niri-config` outside the dev shell. `cargo insta accept` can hang — patch inline `@r#"..."#` snapshots from `niri-config/src/.lib.rs.pending-snap`.
- **Phase C (Task 4) is descopable**: if region geometry fights smithay's damage tracking or DRM plane assignment, ship it inert (config field parses, region path warns once and falls back to full-output) and keep Tasks 1–3.

---

## File Structure

- `niri-config/src/global_shader.rs` — **modify**: add `GlobalShaderCaps` + `RedrawMode` (Task 1); add `cursor_radius` + `redraw` config fields (Task 2).
- `niri-config/src/lib.rs` — **modify**: parse snapshot for the new fields (Task 2).
- `docs/wiki/Configuration:-Global-Shader.md` — **modify**: document new fields; add a `wiki_docs_parses` example (Task 2).
- `src/niri.rs` — **modify**: caps cache on `Niri` + scheduler wiring (Task 3); region geometry at element-build site (Task 4).
- `src/render_helpers/shaders/mod.rs` — **modify**: register `niri_region` + `niri_output_size` uniforms (Task 4).
- `src/render_helpers/shaders/global_prelude.frag` + `global_epilogue.frag` — **modify**: region-aware coord + `tex2D_screen` remap (Task 4).
- `src/render_helpers/global_shader_element.rs` — **modify**: carry region + output size, pass the two new uniforms (Task 4).

---

## Task 1: Capability scanner + redraw mode (niri-config, pure)

Pure, standalone-testable logic. No main-crate build needed.

**Files:**
- Modify: `niri-config/src/global_shader.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub struct GlobalShaderCaps { pub uses_time: bool, pub uses_cursor: bool, pub uses_prev: bool }` (`Debug, Clone, Copy, PartialEq, Eq, Default`)
  - `impl GlobalShaderCaps { pub fn scan(src: &str, hyprland: bool) -> Self; pub fn is_animating(&self) -> bool }`
  - `pub enum RedrawMode { Auto, OnDamage, Continuous }` (`Debug, Clone, Copy, PartialEq, Eq, Default`, `#[default] Auto`)
  - `impl RedrawMode { pub fn parse(s: &str) -> Self; pub fn wants_continuous_redraw(self, caps: GlobalShaderCaps) -> bool }`

- [ ] **Step 1: Write the failing tests**

Add to the existing `mod tests` in `niri-config/src/global_shader.rs`:

```rust
use super::{GlobalShaderCaps, RedrawMode};

#[test]
fn caps_scan_niri_dialect() {
    let c = GlobalShaderCaps::scan(
        "vec4 global_color(vec3 c){ return tex2D_prev(c.xy)*niri_time + niri_cursor.x; }",
        false,
    );
    assert_eq!(c, GlobalShaderCaps { uses_time: true, uses_cursor: true, uses_prev: true });
}

#[test]
fn caps_scan_static_filter() {
    let c = GlobalShaderCaps::scan("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }", false);
    assert_eq!(c, GlobalShaderCaps::default());
    assert!(!c.is_animating());
}

#[test]
fn caps_scan_time_only_is_animating() {
    let c = GlobalShaderCaps::scan("vec4 global_color(vec3 c){ return vec4(niri_time); }", false);
    assert!(c.uses_time && !c.uses_cursor && !c.uses_prev);
    assert!(c.is_animating());
}

#[test]
fn caps_scan_hyprland_dialect() {
    // Hyprland aliases time as `time`; no cursor/prev uniforms exist.
    let with = GlobalShaderCaps::scan("void main(){ gl_FragColor = vec4(time); }", true);
    assert!(with.uses_time && !with.uses_cursor && !with.uses_prev);
    let without = GlobalShaderCaps::scan("void main(){ gl_FragColor = texture2D(tex, v_texcoord); }", true);
    assert!(!without.is_animating());
}

#[test]
fn redraw_mode_parse_and_decision() {
    let animating = GlobalShaderCaps { uses_time: true, ..Default::default() };
    let static_ = GlobalShaderCaps::default();
    assert!(RedrawMode::parse("auto").wants_continuous_redraw(animating));
    assert!(!RedrawMode::parse("auto").wants_continuous_redraw(static_));
    assert!(!RedrawMode::parse("on-damage").wants_continuous_redraw(animating));
    assert!(RedrawMode::parse("continuous").wants_continuous_redraw(static_));
    assert_eq!(RedrawMode::parse("bogus"), RedrawMode::Auto);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p niri-config caps_ redraw_mode_`
Expected: FAIL — `cannot find type GlobalShaderCaps` / `RedrawMode`.

- [ ] **Step 3: Implement the scanner and mode**

Add near the top of `niri-config/src/global_shader.rs` (after the imports, before `GlobalShader`):

```rust
/// Which animation/feedback inputs a compiled global shader references, derived by scanning
/// the resolved source. A substring match: it may over-report (token in a comment) — which
/// only costs extra redraws — but cannot under-report, because in GLSL a uniform must appear
/// by its literal name to be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlobalShaderCaps {
    pub uses_time: bool,
    pub uses_cursor: bool,
    pub uses_prev: bool,
}

impl GlobalShaderCaps {
    pub fn scan(src: &str, hyprland: bool) -> Self {
        if hyprland {
            // Hyprland dialect aliases niri_time as `time`; it has no cursor or prev uniforms.
            GlobalShaderCaps {
                uses_time: src.contains("time"),
                uses_cursor: false,
                uses_prev: false,
            }
        } else {
            // `niri_prev` is the raw sampler; `tex2D_prev` is the helper most shaders actually
            // call. Either reference means the shader depends on the feedback buffer.
            GlobalShaderCaps {
                uses_time: src.contains("niri_time"),
                uses_cursor: src.contains("niri_cursor"),
                uses_prev: src.contains("niri_prev") || src.contains("tex2D_prev"),
            }
        }
    }

    /// Animating shaders depend on time or the feedback buffer, so they must redraw every frame
    /// to progress even when the desktop is idle.
    pub fn is_animating(&self) -> bool {
        self.uses_time || self.uses_prev
    }
}

/// Redraw scheduling for the global shader. `Auto` derives the decision from [`GlobalShaderCaps`];
/// the others force it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedrawMode {
    #[default]
    Auto,
    OnDamage,
    Continuous,
}

impl RedrawMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "on-damage" => RedrawMode::OnDamage,
            "continuous" => RedrawMode::Continuous,
            _ => RedrawMode::Auto,
        }
    }

    /// Whether to schedule a redraw every frame (animate while idle).
    pub fn wants_continuous_redraw(self, caps: GlobalShaderCaps) -> bool {
        match self {
            RedrawMode::Continuous => true,
            RedrawMode::OnDamage => false,
            RedrawMode::Auto => caps.is_animating(),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p niri-config caps_ redraw_mode_`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/global_shader.rs
git commit -m "niri-config: add GlobalShaderCaps source scan + RedrawMode"
```

---

## Task 2: Config fields `cursor-radius` and `redraw` (niri-config)

**Files:**
- Modify: `niri-config/src/global_shader.rs`
- Modify: `niri-config/src/lib.rs` (parse snapshot)
- Modify: `docs/wiki/Configuration:-Global-Shader.md` (docs + parse-tested example)
- Test: `niri-config/src/global_shader.rs` `mod tests`

**Interfaces:**
- Consumes: nothing from Task 1 (independent).
- Produces: `GlobalShader { ..., cursor_radius: Option<u32>, redraw: String }` (resolved struct); `GlobalShaderPart { ..., cursor_radius: Option<u32>, redraw: Option<String> }`. `redraw` defaults to `"auto"`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `niri-config/src/global_shader.rs`:

```rust
#[test]
fn global_shader_redraw_and_cursor_radius() {
    let config = Config::parse_mem(
        r##"
        global-shader {
            enable
            source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
            cursor-radius 200
            redraw "continuous"
        }
        "##,
    )
    .unwrap();
    assert_eq!(config.global_shader.cursor_radius, Some(200));
    assert_eq!(config.global_shader.redraw, "continuous");
}

#[test]
fn global_shader_redraw_defaults_to_auto() {
    let config = Config::parse_mem("").unwrap();
    assert_eq!(config.global_shader.redraw, "auto");
    assert_eq!(config.global_shader.cursor_radius, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p niri-config global_shader_redraw`
Expected: FAIL — no field `cursor_radius` / `redraw` on `GlobalShader`.

- [ ] **Step 3: Add the fields, defaults, and merge**

In `niri-config/src/global_shader.rs`, extend the resolved struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShader {
    pub enable: bool,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub reads_cursor: bool,
    /// Effect footprint radius in logical px for cursor-local shaders; `None` = whole output.
    pub cursor_radius: Option<u32>,
    /// Redraw scheduling: "auto" | "on-damage" | "continuous". Parsed via `RedrawMode::parse`.
    pub redraw: String,
}
```

Extend `Default`:

```rust
impl Default for GlobalShader {
    fn default() -> Self {
        Self {
            enable: false,
            source: None,
            path: None,
            mode: String::from("niri"),
            reads_cursor: false,
            cursor_radius: None,
            redraw: String::from("auto"),
        }
    }
}
```

Extend the knuffel part:

```rust
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
    #[knuffel(child, unwrap(argument))]
    pub cursor_radius: Option<u32>,
    #[knuffel(child, unwrap(argument))]
    pub redraw: Option<String>,
}
```

Extend the merge:

```rust
impl MergeWith<GlobalShaderPart> for GlobalShader {
    fn merge_with(&mut self, part: &GlobalShaderPart) {
        merge!((self, part), enable, reads_cursor);
        merge_clone_opt!((self, part), source, path, cursor_radius);
        merge_clone!((self, part), mode, redraw);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p niri-config global_shader_redraw`
Expected: PASS.

- [ ] **Step 5: Update the parse snapshot**

Run: `cargo test -p niri-config 2>&1 | tail -30`
If the inline `assert_debug_snapshot!` in `niri-config/src/lib.rs` fails on the added `GlobalShader` fields, patch the inline snapshot. Because `cargo insta accept` can hang, do it by hand:
1. Run the failing snapshot test to produce `niri-config/src/.lib.rs.pending-snap`.
2. Open that NDJSON, take the `new.snapshot` field, and replace the corresponding `@r#"..."#` block in `lib.rs` (8-space indent per line). The new lines will be `cursor_radius: None,` and `redraw: "auto",` inside the `global_shader: GlobalShader {` block.

Run: `cargo test -p niri-config` → Expected: PASS (all, incl. snapshot).

- [ ] **Step 6: Document and add a parse-tested wiki example**

In `docs/wiki/Configuration:-Global-Shader.md`, document both fields:
- `cursor-radius <px>` — optional; for cursor-local shaders, reshade/damage only a box of this radius around the cursor (enables region mode, Task 4). Absent → whole output. Region-mode shaders read `niri_region`/`niri_output_size` (see Task 4).
- `redraw "auto"|"on-damage"|"continuous"` — optional; scheduling override. `auto` (default): animate every frame iff the shader uses `niri_time`/`niri_prev`. `on-damage`: never force idle redraws. `continuous`: always redraw every frame.

Add a fenced `kdl` example exercising both so `wiki_docs_parses` covers them:

````markdown
```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
    cursor-radius 200
    redraw "auto"
}
```
````

Run: `cargo test -p niri-config wiki_docs_parses`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add niri-config/src/global_shader.rs niri-config/src/lib.rs docs/wiki/Configuration:-Global-Shader.md
git commit -m "niri-config: add global-shader cursor-radius and redraw fields"
```

---

## Task 3: Phase B — idle-animation freeze fix (main crate)

The element is rebuilt every frame with `Id::new()` (`src/niri.rs:4299`), so the damage tracker already reports full-output damage on every render — the damage side is correct. The **only** missing piece is scheduling a redraw every frame for animating shaders. This task wires capability-driven scheduling into `unfinished_animations_remain`.

**Files:**
- Modify: `src/niri.rs` — add caps cache field + helper on `Niri`; invalidate on reload; wire scheduler.

**Interfaces:**
- Consumes: `niri_config::{GlobalShaderCaps, RedrawMode}` (Task 1), `global_shader_source` (`src/niri.rs:6603`).
- Produces: `Niri::global_shader_caps(&self) -> GlobalShaderCaps` (cached); a new field `Niri.global_shader_caps: Cell<Option<GlobalShaderCaps>>`.

- [ ] **Step 1: Add the cache field to `Niri`**

Find the `Niri` struct definition and add a field (place it near other global-shader-adjacent state; it is `Copy`-inner so `Cell` is fine):

```rust
    /// Cached capability scan of the active global shader, invalidated on config reload.
    /// `None` = not yet computed this load.
    pub global_shader_caps: std::cell::Cell<Option<niri_config::GlobalShaderCaps>>,
```

Initialize it where `Niri` is constructed (search for the struct literal that sets up `Niri { ... }`):

```rust
            global_shader_caps: std::cell::Cell::new(None),
```

- [ ] **Step 2: Add the cached helper**

Add an `impl Niri` method (place it near `global_shader_source` usage / other `Niri` methods):

```rust
    /// Capability flags for the active global shader, computed once per config load and cached.
    /// On a cache miss this resolves the source (reading the `path` file if used), so callers
    /// may hit one filesystem read after a reload; subsequent calls are free.
    pub fn global_shader_caps(&self) -> niri_config::GlobalShaderCaps {
        if let Some(caps) = self.global_shader_caps.get() {
            return caps;
        }
        let caps = {
            let cfg = self.config.borrow();
            let hyprland = cfg.global_shader.mode == "hyprland";
            match global_shader_source(&cfg.global_shader) {
                Some(src) => niri_config::GlobalShaderCaps::scan(&src, hyprland),
                None => niri_config::GlobalShaderCaps::default(),
            }
        };
        self.global_shader_caps.set(Some(caps));
        caps
    }
```

- [ ] **Step 3: Invalidate the cache on reload**

In the config-reload diff block at `src/niri.rs:1593-1610`, inside the `if` body (after the per-output reset loop), add:

```rust
            // Recompute capability flags on next use.
            self.niri.global_shader_caps.set(None);
```

(Note: this block is in `State`, so the field is `self.niri.global_shader_caps`. Also extend the diff condition to include `redraw` and `cursor_radius` so a change to those invalidates too — though `redraw`/`cursor_radius` don't change the compiled program, `redraw` changes scheduling: add `|| config.global_shader.redraw != old_config.global_shader.redraw || config.global_shader.cursor_radius != old_config.global_shader.cursor_radius` to the existing `if` at lines 1593-1596. The cache invalidation is harmless if the program wasn't recompiled.)

- [ ] **Step 4: Wire the scheduler**

In `redraw()` (the `if self.monitors_active` block at `src/niri.rs:4712`), compute the decision **before** taking the `&mut state` borrow (calling `self.global_shader_caps()` needs `&self`, which conflicts with the outstanding `&mut self.output_state`). Insert immediately after `if self.monitors_active {` and before `let state = self.output_state.get_mut(output).unwrap();`:

```rust
            let global_shader_animate = {
                let cfg = self.config.borrow();
                let enabled = cfg.global_shader.enable;
                let mode = niri_config::RedrawMode::parse(&cfg.global_shader.redraw);
                drop(cfg);
                enabled && mode.wants_continuous_redraw(self.global_shader_caps())
            };
```

Then, after the existing `unfinished_animations_remain` assignments (after the layer-surface block ending at line 4733), add:

```rust
            // Time/feedback-driven global shaders must keep redrawing to animate when idle.
            state.unfinished_animations_remain |= global_shader_animate;
```

- [ ] **Step 5: Compile-check**

Run (in dev shell): `cargo check --no-default-features --features dbus,systemd`
Expected: builds clean. Fix borrow/import errors (ensure `niri_config::{GlobalShaderCaps, RedrawMode}` are reachable — they're re-exported via `niri_config`).

- [ ] **Step 6: Commit**

```bash
git add src/niri.rs
git commit -m "niri: schedule continuous redraws for time/feedback global shaders (fix idle freeze)"
```

- [ ] **Step 7: Manual verification (deploy to "sixseven" per crib)**

Deploy: push `barrulus-custom`, `nix flake update biri --flake ~/quixote`, `sudo nixos-rebuild switch --flake ~/quixote#sixseven`.
- *Freeze fix:* a `niri_time`-only shader (e.g. `vec4 global_color(vec3 c){ return mix(tex2D_screen(c.xy), vec4(sin(niri_time)*0.5+0.5), 0.2); }`) must visibly animate with the cursor still and the desktop idle. Pre-change it freezes.
- *Override:* set `redraw "on-damage"` on that shader → idle animation stops (only animates on activity). Set `redraw "continuous"` on a static filter → it redraws every frame.
- *Static default:* a static filter with default `redraw` must NOT spin the GPU when idle (confirm with a frame-time/GPU-load check — no continuous redraws).

---

## Task 4: Phase C — region-damage + scanout preservation (main crate, DESCOPABLE)

Active only when `cursor-radius` is set, the shader `uses_cursor`, and it is **not** animating. Shrinks the element's geometry to a box around the cursor so damage and scanout are limited to that box. Adds two uniforms (`niri_region`, `niri_output_size`) so the author-facing coordinate contract is preserved (full-output path is identity).

> **Descope rule:** if any step's manual test shows region geometry breaks rendering, scanout, or DRM plane assignment, stop, make the region branch warn-once and fall back to full-output (keep `area = full`), and leave Tasks 1–3 as the shipped result.

**Files:**
- Modify: `src/render_helpers/shaders/mod.rs` — register `niri_region` (_4f) + `niri_output_size` (_2f) uniforms.
- Modify: `src/render_helpers/shaders/global_prelude.frag` + `global_epilogue.frag` — region-aware coord + `tex2D_screen` remap.
- Modify: `src/render_helpers/global_shader_element.rs` — carry region + output size; pass the two uniforms.
- Modify: `src/niri.rs` — compute the box at the element-build site.

**Interfaces:**
- Consumes: `Niri::global_shader_caps()` (Task 3), `cursor_radius` config (Task 2).
- Produces: `GlobalShaderElement::new(... , region_norm: [f32; 4], output_size_phys: (f32, f32))` (two new trailing params).

- [ ] **Step 1: Register the two uniforms**

In `src/render_helpers/shaders/mod.rs`, in `compile_global_program` (lines 382-390), add the uniform names so the element may set them (recall: an additional uniform not in this list triggers `GlesError::UnknownUniform` at `shader_element.rs:439`):

```rust
    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_time", UniformType::_1f),
            UniformName::new("niri_cursor", UniformType::_2f),
            UniformName::new("niri_region", UniformType::_4f),
            UniformName::new("niri_output_size", UniformType::_2f),
        ],
        &["niri_screen", "niri_prev"],
    )
```

- [ ] **Step 2: Make the niri prelude region-aware**

In `src/render_helpers/shaders/global_prelude.frag`, add the uniform declarations and remap `tex2D_screen` (and `tex2D_prev` for symmetry). Replace lines 12-19 region:

```glsl
uniform float niri_time;      // seconds since shader activation
uniform vec2 niri_cursor;     // cursor position, output coords (px)

// Region this element covers, in output-normalised coords: (origin.xy, size.xy).
// (0,0,1,1) for a whole-output shader; a sub-box when cursor-radius is set.
uniform vec4 niri_region;
uniform vec2 niri_output_size; // true full-output size in physical px

uniform sampler2D niri_screen; // composited frame below this element (covers niri_region)
uniform sampler2D niri_prev;   // previous frame's output

// uv is output-normalised (0..1 across the whole output); convert to this element's local
// texture coords. Samples outside the captured region clamp to the (transparent) border.
vec4 tex2D_screen(vec2 uv) { return texture2D(niri_screen, (uv - niri_region.xy) / niri_region.zw); }
vec4 tex2D_prev(vec2 uv) { return texture2D(niri_prev, (uv - niri_region.xy) / niri_region.zw); }
```

In `src/render_helpers/shaders/global_epilogue.frag`, change the coord so `global_color` always receives output-normalised coords (line 3):

```glsl
    vec3 coord = vec3(niri_region.xy + niri_v_coords * niri_region.zw, 1.0);
```

With `niri_region = (0,0,1,1)` every expression above is the identity, so existing whole-output shaders are unaffected.

- [ ] **Step 3: Carry region + output size on the element and pass the uniforms**

In `src/render_helpers/global_shader_element.rs`:

Add fields to the struct (after `cursor`):

```rust
    /// Region this element covers in output-normalised coords: [origin.x, origin.y, w, h].
    /// `[0.0, 0.0, 1.0, 1.0]` = whole output.
    region_norm: [f32; 4],
    /// True full-output size in physical px (for the `niri_output_size` uniform).
    output_size_phys: (f32, f32),
```

Add the two params to `new` (trailing, after `prev`/`result` — keep `result` last to match the existing call order, so insert before `result`):

```rust
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        region_norm: [f32; 4],
        output_size_phys: (f32, f32),
        prev: Option<GlesTexture>,
        result: Rc<RefCell<Option<GlesTexture>>>,
    ) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            area,
            scale,
            time,
            cursor,
            region_norm,
            output_size_phys,
            prev,
            result,
        }
    }
```

In `draw()`, extend the uniforms (lines 128-131):

```rust
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
            Uniform::new("niri_output_size", (self.output_size_phys.0, self.output_size_phys.1)),
        ]);
```

- [ ] **Step 4: Compute the box at the element-build site**

In `src/niri.rs` (lines 4272-4309), after `area` is set and `cursor` computed, derive the region. Replace the `Some(GlobalShaderElement::new(...))` construction with:

```rust
            let output_size_phys = (
                (area.size.w * scale as f64) as f32,
                (area.size.h * scale as f64) as f32,
            );

            // Region-local rendering: when cursor-radius is set on a cursor-only (non-animating)
            // shader, shrink the element to a box around the cursor so damage + scanout are
            // limited to that box. Otherwise cover the whole output.
            let caps = self.global_shader_caps();
            let cursor_radius = self.config.borrow().global_shader.cursor_radius;
            let (area, region_norm) = match cursor_radius {
                Some(r) if caps.uses_cursor && !caps.is_animating() && r > 0 => {
                    let r = r as f64; // logical px
                    let cursor_logical = (pointer_local.x, pointer_local.y);
                    let full = area;
                    let box_loc = (
                        (cursor_logical.0 - r).max(full.loc.x),
                        (cursor_logical.1 - r).max(full.loc.y),
                    );
                    let box_end = (
                        (cursor_logical.0 + r).min(full.loc.x + full.size.w),
                        (cursor_logical.1 + r).min(full.loc.y + full.size.h),
                    );
                    let box_rect = Rectangle::new(
                        (box_loc.0, box_loc.1).into(),
                        ((box_end.0 - box_loc.0).max(0.0), (box_end.1 - box_loc.1).max(0.0)).into(),
                    );
                    let region_norm = [
                        ((box_rect.loc.x - full.loc.x) / full.size.w) as f32,
                        ((box_rect.loc.y - full.loc.y) / full.size.h) as f32,
                        (box_rect.size.w / full.size.w) as f32,
                        (box_rect.size.h / full.size.h) as f32,
                    ];
                    (box_rect, region_norm)
                }
                _ => (area, [0.0, 0.0, 1.0, 1.0]),
            };

            Some(GlobalShaderElement::new(
                Id::new(),
                area,
                scale,
                time,
                cursor,
                region_norm,
                output_size_phys,
                state.global_shader_prev.clone(),
                state.global_shader_result.clone(),
            ))
```

(`pointer_local` is the `Point<f64, Logical>` already computed at lines 4292; reuse it. The box is in the same output-local logical space as `area`, whose `loc` is `(0,0)` from `Rectangle::from_size`.)

- [ ] **Step 5: Compile-check**

Run (in dev shell): `cargo check --no-default-features --features dbus,systemd`
Expected: builds clean. Fix the `GlobalShaderElement::new` call signature (now two extra args) and any `Rectangle`/`Point` type inference errors.

- [ ] **Step 6: Verify full-output path is unchanged (regression)**

Deploy and confirm an existing whole-output shader (no `cursor-radius`) renders identically — run the red-band marker shader from the v1 test and confirm the band sits at the true output top and content is upright. `niri_region=(0,0,1,1)` must make this a no-op.

- [ ] **Step 7: Commit (region plumbing)**

```bash
git add src/render_helpers/shaders/mod.rs src/render_helpers/shaders/global_prelude.frag src/render_helpers/shaders/global_epilogue.frag src/render_helpers/global_shader_element.rs src/niri.rs
git commit -m "global-shader: region-local rendering via niri_region (cursor-radius)"
```

- [ ] **Step 8: Manual verification of region mode + scanout (deploy per crib)**

- *Region effect:* a cursor-only shader using `niri_cursor` and `cursor-radius 200` (e.g. a ring: `vec4 global_color(vec3 c){ float d = distance(c.xy*niri_output_size, niri_cursor); float ring = smoothstep(180.0,200.0,d)*(1.0-smoothstep(200.0,220.0,d)); return mix(tex2D_screen(c.xy), vec4(1.0,0.3,0.0,1.0), ring); }`). The ring must follow the cursor and only the box around it should repaint.
- *Scanout preserved:* with the region shader active and the cursor parked in a corner, confirm a fullscreen client elsewhere can still scan out / GPU load drops vs. full-output mode (record with `gpu-screen-recorder -w eDP-1` — KMS capture; check frame-time / power). This is the payoff for Phase C.
- *Descope check:* if the region effect renders wrong (offset, missing, tears) or scanout/plane assignment misbehaves, apply the descope rule: make the `Some(r) if ...` arm warn once and fall back to `(area, [0.0,0.0,1.0,1.0])`, commit that, and ship Tasks 1–3.

- [ ] **Step 9: Document the region contract**

In `docs/wiki/Configuration:-Global-Shader.md`, note that region-mode (cursor-radius) shaders:
- receive `coord` in whole-output normalised coords (unchanged);
- must use `niri_output_size` (full output physical px) for absolute pixel math, not `niri_size` (which equals the box in region mode);
- may only sample `tex2D_screen`/`tex2D_prev` **within** the region; samples outside clamp to a transparent border.

```bash
git add docs/wiki/Configuration:-Global-Shader.md
git commit -m "docs: document global-shader region-mode coordinate contract"
```

---

## Self-Review

**Spec coverage:**
- §3.2 behavior table — *Animating* (Task 3 scheduler + existing per-frame full damage), *Static filter* (Task 3: no idle redraw, full reshade already via `Id::new()`), *Cursor-local* (Task 4 region). ✓
- §4 Phase A (flags) → Task 1. Phase B (scheduling + damage) → Task 3 (damage already correct, documented). Phase C (region + scanout) → Task 4. ✓
- §5 config (`cursor-radius`, `redraw`, snapshot, wiki) → Task 2. ✓
- §6 testing (flag unit tests, parse/snapshot, idle-animation manual, region/scanout manual) → Tasks 1, 2, 3.7, 4.8. ✓
- §9 open questions resolved during exploration: caps stored on `Niri` via `Cell` (not the program struct); element `Id` is per-frame `Id::new()` so full damage is already reported and the commit-counter bump is unnecessary; `unfinished_animations_remain` is the scheduling chokepoint; uv-remap done via `niri_region`/`niri_output_size` uniforms keeping the full-output path identity. ✓

**Placeholder scan:** no TBD/TODO/"handle edge cases"; every code step shows concrete code. ✓

**Type consistency:** `GlobalShaderCaps`/`RedrawMode` signatures match across Tasks 1/3; `GlobalShaderElement::new` extended consistently (region_norm `[f32;4]`, output_size_phys `(f32,f32)`) between Task 4 Steps 3 and 4; config field names `cursor_radius`/`redraw` consistent across Tasks 2/3/4. ✓
