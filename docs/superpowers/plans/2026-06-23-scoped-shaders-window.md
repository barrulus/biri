# Scoped Shaders — Plan 2: Per-window Shaders (static v1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply a post-process shader to a single window's own content (e.g. grayscale / CRT / colour-grade / invert on one app), selected by a `shader` child on `window-rule`, reusing the scoped-shader substrate (the `Scoped` program cache + `ScopedShaderElement`'s `Texture` source) that Plan 1 shipped and hardware-verified.

**Architecture:** A `shader { }` child on `window-rule` resolves into `ResolvedWindowRules.shader` (a compiled `scoped_key` + pass list). Its sources compile into the SAME `Shaders.scoped` cache as region shaders. At render, `Tile::render` gains a window-shader branch — mirroring the existing `ResizeAnimation` path — that renders ONLY the window's content into an `OffscreenBuffer`, then pushes a `ScopedShaderElement` (`ScopedSource::Texture(offscreen)`) over the window's geometry; border/shadow/focus-ring render normally around it.

**Tech Stack:** Rust, GLES2, smithay GLES renderer (`OffscreenBuffer`, `ScopedShaderElement`, `ShaderRenderElement`), knuffel (KDL), insta.

**Spec:** `docs/superpowers/specs/2026-06-23-scoped-shaders-design.md` — §3.D, §3.F (window half), §4 (window column). Read it. NOTE the v1 scope cut below overrides the spec's window-column promise of `niri_time`/`niri_cursor`.

## Global Constraints

- **Static v1 (scope cut, decided 2026-06-23):** window shaders get `niri_screen` = the window's own content, `niri_size` = window physical size, `c.xy` = 0..1 across the window, `niri_region` = `(0,0,1,1)`, and multi-pass `pass{}` chains. `niri_time` = 0, `niri_cursor` = `(0,0)`, `niri_output_size` = the window physical size (== `niri_size`). Animated + window-local-cursor window shaders are a deferred follow-up (they need clock/cursor threading through the layout render path). Existing static `global_color` shaders run unchanged on a window.
- **Window content only.** The shader applies to the window surface(s); border, shadow, focus-ring, and popups render normally AROUND it (mirror `ResizeAnimation`, which renders window-only into its offscreen and pushes decorations outside). Do NOT feed the full `render_inner` output to the shader offscreen.
- **Reuse the substrate.** No new program type or contract: window shaders compile into `Shaders.scoped` via the same `set_scoped_programs`, are looked up by `ProgramType::Scoped(key, i)`, and render through `ScopedShaderElement` with `ScopedSource::Texture(...)`. The `niri_size` lesson applies — NEVER push a `niri_size` uniform (it is a built-in; pushing it freezes the output). `ScopedShaderElement` already omits it.
- **Byte-identity:** a window with no `shader` rule takes today's exact render path (no offscreen), output unchanged.
- **Source dedupe:** a window shader whose source matches a region shader (or another window) shares one compiled chain (same `scoped_key`).
- **Crash-safety:** if the shader fails to resolve or its program is missing, the window renders normally (no offscreen routing) — never a frozen or blank window.
- **Build/test crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task compile `cargo check --no-default-features --features dbus,systemd` inside it with `export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib`. `niri-config` tests outside the dev shell: `cargo test -p niri-config`. Inline-snapshot gotcha: never `cargo insta accept` (hangs); patch `.lib.rs.pending-snap` by hand. Run `cargo fmt` (plain, NOT `+nightly`) before each commit. Commits: NO Co-Authored-By / AI-attribution lines.

---

## File Structure

**Config (niri-config):**
- Modify `niri-config/src/window_rule.rs` — add `ShaderRule` child struct + `shader: Option<ShaderRule>` on `WindowRule`; `ShaderRule::pass_sources`.
- Modify `niri-config/src/region_shader.rs` — factor the pass-source resolution into a shared free fn `resolve_scoped_pass_sources(...)` reused by both `RegionShader::pass_sources` and `ShaderRule::pass_sources` (avoid duplication).
- Modify `niri-config/src/lib.rs` — re-export `ShaderRule`; default snapshot unchanged (WindowRule is not in the default snapshot's printed surface — verify).

**Resolution (src):**
- Modify `src/window/mod.rs` — add `shader: Option<ResolvedShader>` to `ResolvedWindowRules` (`ResolvedShader { passes: Vec<(String,bool)>, key: u64 }`), populated in `compute()` (last-write-wins) by resolving `WindowRule.shader.pass_sources(read_scoped_shader_path)`.

**Compile wiring (src):**
- Modify `src/niri.rs` — rename/extend `region_shader_chains` → `scoped_shader_chains` to also collect every `window-rule` shader's chain; reload diff fires on `window_rules` change too.
- (`src/backend/tty.rs` startup call already routes through that fn — just follows the rename.)

**Render (src):**
- Modify `src/layout/tile.rs` — add `Scoped = ScopedShaderElement` to `TileRenderElement`; add the window-shader offscreen branch in `Tile::render`.

**Docs:**
- Modify `docs/wiki/Configuration:-Global-Shader.md` — "Per-window shaders" section + parsing example.

---

## Task Ordering

1. **T1** config (niri-config window-rule `shader`) — testable outside dev shell.
2. **T2** `ResolvedWindowRules.shader` + compute — compile-only.
3. **T3** compile wiring (`scoped_shader_chains` + reload diff) — compile-only.
4. **T4** the tile render branch — the meat; first full green + manual GPU.
5. **T5** docs.

---

## Task 1: Config — `shader` child on `window-rule`

**Files:**
- Modify: `niri-config/src/window_rule.rs`
- Modify: `niri-config/src/region_shader.rs` (factor shared resolver)
- Modify: `niri-config/src/lib.rs` (re-export)
- Test: `niri-config/src/window_rule.rs` tests

**Interfaces:**
- Produces:
  - `pub struct ShaderRule { pub source: Option<String>, pub path: Option<String>, pub mode: Option<String>, pub passes: Vec<GlobalShaderPassPart> }` (knuffel `Decode`, `Default`, `Clone`, `PartialEq`).
  - `WindowRule.shader: Option<ShaderRule>`.
  - `ShaderRule::pass_sources(&self, expand: impl Fn(&str)->Option<String>) -> Vec<(String,bool)>`.
  - `pub(crate) fn resolve_scoped_pass_sources(source: &Option<String>, path: &Option<String>, mode: &str, passes: &[GlobalShaderPassPart], expand: impl Fn(&str)->Option<String>) -> Vec<(String,bool)>` in `region_shader.rs` (shared core).

- [ ] **Step 1: Factor the shared resolver in `region_shader.rs`**

In `niri-config/src/region_shader.rs`, add (and make `RegionShader::pass_sources` delegate to it):

```rust
use crate::global_shader::GlobalShaderPassPart;

/// Resolve a scoped-shader source spec into an ordered `(source, hyprland)` pass list. Empty
/// `passes` => the top-level `source`/`path` becomes a length-1 chain. Any pass that cannot be
/// resolved => empty (disabled). `expand` maps a `path` to its file contents. Per-pass `mode`
/// defaults to `default_mode`.
pub(crate) fn resolve_scoped_pass_sources(
    source: &Option<String>,
    path: &Option<String>,
    default_mode: &str,
    passes: &[GlobalShaderPassPart],
    expand: impl Fn(&str) -> Option<String>,
) -> Vec<(String, bool)> {
    let resolve_one = |src: &Option<String>, p: &Option<String>, mode: &str| match (src, p) {
        (Some(s), None) if !s.trim().is_empty() => Some((s.clone(), mode == "hyprland")),
        (None, Some(pp)) => expand(pp).map(|s| (s, mode == "hyprland")),
        _ => None,
    };
    if passes.is_empty() {
        return match resolve_one(source, path, default_mode) {
            Some(pair) => vec![pair],
            None => Vec::new(),
        };
    }
    let mut out = Vec::with_capacity(passes.len());
    for pass in passes {
        let mode = pass.mode.as_deref().unwrap_or(default_mode);
        match resolve_one(&pass.source, &pass.path, mode) {
            Some(pair) => out.push(pair),
            None => return Vec::new(),
        }
    }
    out
}
```

Then change `RegionShader::pass_sources` to delegate:

```rust
    pub fn pass_sources(&self, expand: impl Fn(&str) -> Option<String>) -> Vec<(String, bool)> {
        // `self.passes` is already the resolved Vec<GlobalShaderPass>; pass the part-level fields.
        // RegionShader stores resolved passes — adapt: build a temporary part list. (See note.)
        crate::region_shader::resolve_scoped_pass_sources_resolved(
            &self.source, &self.path, &self.mode, &self.passes, expand,
        )
    }
```

> IMPLEMENTER NOTE: `RegionShader.passes` is the RESOLVED `Vec<GlobalShaderPass>` (each `{source,path,mode}`), whereas `ShaderRule.passes` is the PART form `Vec<GlobalShaderPassPart>`. To share cleanly, write ONE core that takes the already-resolved per-pass `(source,path,mode)` tuples. Concretely: keep `resolve_scoped_pass_sources` operating over `&[GlobalShaderPass]` (resolved) by converting `ShaderRule`'s `GlobalShaderPassPart` to `GlobalShaderPass` (defaulting mode) before calling. Pick whichever single shared signature is cleanest and make BOTH callers use it; the test in Step 4 plus the existing region tests must stay green. Do not leave two copies of the resolve logic.

- [ ] **Step 2: Write the failing test**

Add to `niri-config/src/window_rule.rs` tests (create the module if absent):

```rust
#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn window_rule_shader_parses() {
        let config = Config::parse_mem(
            r##"
            window-rule {
                match app-id="Alacritty"
                shader {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).bgra; }"
                }
            }
            window-rule {
                match app-id="foo"
            }
            "##,
        )
        .unwrap();
        let r0 = &config.window_rules[0];
        assert!(r0.shader.is_some());
        let chain = r0.shader.as_ref().unwrap().pass_sources(|_| None);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].0.contains("bgra"));
        assert!(config.window_rules[1].shader.is_none());
    }
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p niri-config window_rule_shader_parses`
Expected: FAIL — `shader` field / `ShaderRule` not defined.

- [ ] **Step 4: Add `ShaderRule` + the `shader` field + `pass_sources`**

In `niri-config/src/window_rule.rs`, add the struct (near the other rule structs):

```rust
use crate::global_shader::GlobalShaderPassPart;

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ShaderRule {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

impl ShaderRule {
    pub fn pass_sources(&self, expand: impl Fn(&str) -> Option<String>) -> Vec<(String, bool)> {
        let mode = self.mode.as_deref().unwrap_or("niri");
        crate::region_shader::resolve_scoped_pass_sources(
            &self.source, &self.path, mode, &self.passes, expand,
        )
    }
}
```

Add the field to `WindowRule` (near `shadow`/`background_effect`):

```rust
    #[knuffel(child)]
    pub shader: Option<ShaderRule>,
```

Re-export in `niri-config/src/lib.rs`: add `ShaderRule` to the `pub use crate::window_rule::{...}` line.

- [ ] **Step 5: Run the test + full suite**

Run: `cargo test -p niri-config window_rule_shader_parses && cargo test -p niri-config`
Expected: PASS. If the default-config inline snapshot in `lib.rs` changed (only if `WindowRule`/`Config` Debug surface shifted — it shouldn't, since `window_rules` defaults to `[]`), patch it by hand (no `insta accept`).

- [ ] **Step 6: Commit**

```bash
git add niri-config/src/window_rule.rs niri-config/src/region_shader.rs niri-config/src/lib.rs
git commit -m "niri-config: window-rule shader child + shared scoped resolver"
```

---

## Task 2: `ResolvedWindowRules.shader` + compute

**Files:**
- Modify: `src/window/mod.rs`

**Interfaces:**
- Consumes: `WindowRule.shader: Option<ShaderRule>`, `ShaderRule::pass_sources`, `crate::niri::read_scoped_shader_path` (exists, `pub(crate)`).
- Produces:
  - `pub struct ResolvedShader { pub passes: Vec<(String, bool)>, pub key: u64 }` (`Debug, Clone, PartialEq`).
  - `ResolvedWindowRules.shader: Option<ResolvedShader>`.

- [ ] **Step 1: Add the resolved type + field**

In `src/window/mod.rs`, add near `ResolvedWindowRules`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShader {
    /// Resolved `(source, hyprland)` pass list for this window's shader.
    pub passes: Vec<(String, bool)>,
    /// Cache key into `Shaders.scoped` (== `shaders::scoped_key(&passes)`).
    pub key: u64,
}
```

Add to `ResolvedWindowRules` (near `background_effect`):

```rust
    pub shader: Option<ResolvedShader>,
```

Add `shader: None` to wherever `ResolvedWindowRules` is default-constructed in `compute()` (the initial `resolved` value).

- [ ] **Step 2: Populate in `compute()` (last-write-wins)**

In `ResolvedWindowRules::compute()`, inside the per-matching-rule loop, alongside the other scalar overrides (e.g. `if let Some(x) = rule.opacity { ... }`), add:

```rust
            if let Some(shader_rule) = &rule.shader {
                let passes = shader_rule.pass_sources(crate::niri::read_scoped_shader_path);
                resolved.shader = if passes.is_empty() {
                    None
                } else {
                    let key = crate::render_helpers::shaders::scoped_key(&passes);
                    Some(ResolvedShader { passes, key })
                };
            }
```

> Note: `compute()` may run off the render thread / during config resolution; `read_scoped_shader_path` does a file read, which is acceptable here (rules are recomputed on window map / config change, not per frame). Confirm `scoped_key` is reachable as `crate::render_helpers::shaders::scoped_key` (it is `pub fn`).

- [ ] **Step 3: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: compiles (additive). `ResolvedShader.passes`/`key` are read by Tasks 3-4; an `unused` warning on `key`/`passes` is acceptable until then. Fix any `compute()` borrow issues.

- [ ] **Step 4: Commit**

```bash
git add src/window/mod.rs
git commit -m "window: resolve per-window shader rule into ResolvedWindowRules.shader"
```

---

## Task 3: Compile wiring — window shaders into the scoped cache

**Files:**
- Modify: `src/niri.rs` (`scoped_shader_chains` + reload diff)
- Modify: `src/backend/tty.rs` (rename follow-through)

**Interfaces:**
- Consumes: `set_scoped_programs`, `read_scoped_shader_path`, `ResolvedWindowRules`/config `WindowRule.shader`.
- Produces: `scoped_shader_chains(config) -> Vec<Vec<(String,bool)>>` collecting BOTH region and window-rule shader chains.

- [ ] **Step 1: Extend the chain collector**

In `src/niri.rs`, rename `region_shader_chains` to `scoped_shader_chains` and extend it to also collect window-rule shader chains:

```rust
/// Resolve every configured scoped shader (region shaders + window-rule shaders) into its
/// `(source, hyprland)` pass list, reading any `path` files. The list is the full set compiled
/// into `Shaders.scoped` (dedupe is by key inside `set_scoped_programs`).
pub(crate) fn scoped_shader_chains(config: &niri_config::Config) -> Vec<Vec<(String, bool)>> {
    let mut chains: Vec<Vec<(String, bool)>> = config
        .region_shaders
        .iter()
        .map(|r| r.pass_sources(read_scoped_shader_path))
        .collect();
    for rule in &config.window_rules {
        if let Some(shader) = &rule.shader {
            let chain = shader.pass_sources(read_scoped_shader_path);
            if !chain.is_empty() {
                chains.push(chain);
            }
        }
    }
    chains
}
```

Update the two call sites: `src/backend/tty.rs` (startup, was `region_shader_chains`) and `src/niri.rs` reload → `scoped_shader_chains`.

- [ ] **Step 2: Reload diff includes window rules**

In `src/niri.rs`, the reload block that currently fires on `config.region_shaders != old_config.region_shaders` must also fire on window-rule shader changes. The simplest correct condition: also recompile when `config.window_rules != old_config.window_rules`:

```rust
        if config.region_shaders != old_config.region_shaders
            || config.window_rules != old_config.window_rules
        {
            let chains = scoped_shader_chains(&config);
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_scoped_programs(renderer, &chains);
            });
            shaders_changed = true;
        }
```

> `window_rules` already triggers a rules recompute elsewhere on reload; this block additionally refreshes the scoped program cache. `set_scoped_programs` dedupes + destroys stale, so recompiling on any window-rule change is safe (no leak, no double-compile).

- [ ] **Step 3: Compile-check (dev shell)**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: green (the rename + extension are self-contained; tty.rs follows the rename). Fix any leftover `region_shader_chains` references.

- [ ] **Step 4: Commit**

```bash
git add src/niri.rs src/backend/tty.rs
git commit -m "scoped-shader: compile window-rule shaders into the scoped cache (scoped_shader_chains)"
```

---

## Task 4: Render — window-shader offscreen branch in `Tile::render`

**Files:**
- Modify: `src/layout/tile.rs`

**Interfaces:**
- Consumes: `ResolvedWindowRules.shader` (via `self.window.rules()`), `ScopedShaderElement::new`, `ScopedSource::Texture`, `ProgramType::Scoped`, `OffscreenBuffer`, `self.window.render_normal(...)`, `self.scale`, `self.window_size()`, `self.window_loc()`.
- Produces: `TileRenderElement::Scoped = ScopedShaderElement` variant; the render branch.

This is the meat and the only task that touches the layout render path. The key correctness requirement (from the exploration): render ONLY the window's content into the offscreen (mirror `ResizeAnimation` at `tile.rs:~1094-1148`), NOT the full `render_inner` output — so border/shadow/focus-ring are not shaded and the offscreen is sized to the window.

- [ ] **Step 1: Add the `Scoped` variant to `TileRenderElement`**

In the `niri_render_elements! { TileRenderElement<R> => { ... } }` macro block (`tile.rs:~123`), add:

```rust
        Scoped = crate::render_helpers::scoped_shader_element::ScopedShaderElement,
```

Add `use` imports for `ScopedShaderElement` and `ScopedSource` at the top of `tile.rs`.

- [ ] **Step 2: Add the window-shader branch in `Tile::render`**

In `Tile::render` (`tile.rs:~1337`), the branch structure is `if let Some(open) = ... { } else if let Some(alpha) = ... { }` then `if !pushed { render_inner(...) }`. Add a new `else if` for the window shader BETWEEN the alpha branch and the `if !pushed` fallback. It must:

1. read `self.window.rules().shader` → if `Some(resolved)` AND its program exists, route; else fall through.
2. render ONLY the window content into a fresh `OffscreenBuffer` (window-only, like the resize path),
3. push a `ScopedShaderElement` (`ScopedSource::Texture(offscreen_texture)`) at the window geometry,
4. push the tile's decorations (border, shadow, focus ring) normally — i.e. let the `if !pushed` path still render them, OR render them explicitly here. SIMPLEST: render the window content to offscreen + shader element here, set `pushed = true` for the WINDOW only is wrong (decorations would be skipped). Instead, do NOT set `pushed = true`; render the decorations via a dedicated decorations-only path. **Decision for v1:** render the shaded window here, then render decorations by calling the existing border/shadow/focus-ring emit helpers that `render_inner` uses. Inspect how `render_inner` emits decorations vs window content and split them (the resize path already separates these — copy its decoration-emitting tail).

```rust
        } else if let Some(resolved) = self.window.rules().shader.clone() {
            let mut gctx = ctx.as_gles();
            let scale = Scale::from(self.scale);
            // Program present? else fall through to the normal path.
            let have_program = Shaders::get(gctx.renderer)
                .program(ProgramType::Scoped(resolved.key, 0))
                .is_some();
            if have_program {
                // Render ONLY the window content into an offscreen (no decorations).
                let mut win_elements = Vec::new();
                self.window.render_normal(
                    gctx.r(),
                    Point::from((0., 0.)),
                    self.scale,
                    1.,
                    &mut |elem| win_elements.push(elem.into()),
                );
                let offscreen = OffscreenBuffer::default();
                match offscreen.render(gctx.renderer, scale, &win_elements) {
                    Ok((off_elem, _sync, _data)) => {
                        let tex = off_elem.texture().clone();
                        let win_size = self.window_size();
                        let area = Rectangle::new(location + self.window_loc(), win_size);
                        let size_phys = (
                            (win_size.w * self.scale) as f32,
                            (win_size.h * self.scale) as f32,
                        );
                        let n_passes = resolved.passes.len();
                        let offscreens = (0..n_passes.saturating_sub(1))
                            .map(|_| std::rc::Rc::new(OffscreenBuffer::default()))
                            .collect::<Vec<_>>();
                        let elem = ScopedShaderElement::new(
                            Id::new(),
                            area,
                            self.scale as f32,
                            0.0,                       // niri_time = 0 (static v1)
                            (0.0, 0.0),                // niri_cursor = (0,0) (v1)
                            [0.0, 0.0, 1.0, 1.0],      // whole window
                            size_phys,                 // niri_output_size = window size (v1)
                            resolved.key,
                            n_passes,
                            ScopedSource::Texture(tex),
                            offscreens,
                        );
                        push(elem.into());
                        // Decorations (border/shadow/focus-ring) render normally around it:
                        self.render_decorations_only(ctx, location, xray_pos, focus_ring, push);
                        pushed = true;
                    }
                    Err(err) => warn!("window shader offscreen failed: {err:?}"),
                }
            }
        }
```

> IMPLEMENTER: `ScopedShaderElement::new` takes `output_size_phys` then `key` (check the exact arg order in `scoped_shader_element.rs` — do NOT pass a `size_phys`; that field was removed. The call above passes `size_phys` AS `output_size_phys`, which is the v1 intent). Confirm `self.window.render_normal(...)`'s exact signature (the resize path at `tile.rs:~1094` calls it — copy that call shape). `render_decorations_only` does not exist yet — you must either (a) factor the decoration-emitting portion of `render_inner` into a helper both paths call, or (b) inline the border/shadow/focus-ring emission here by copying it from `render_inner`. Prefer (a): extract a `fn render_decorations(&self, ctx, location, focus_ring, push)` from `render_inner` and call it from both the normal path and this branch. Keep window-content emission and decoration emission as two helpers.

- [ ] **Step 3: Compile-check (dev shell) — first full green**

Run: `cargo check --no-default-features --features dbus,systemd`
Expected: green. This is the hardest task; iterate against the compiler (borrow/lifetime on the gles ctx, the `render_normal` signature, the decoration split). Run `cargo fmt`.

- [ ] **Step 4: niri-config regression**

Run: `cargo test -p niri-config`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/layout/tile.rs
git commit -m "scoped-shader: per-window shader render path (window-content offscreen + ScopedShaderElement)"
```

- [ ] **Step 6: MANUAL GPU verification (owed; sixseven)** — the damage/offscreen interaction can only be confirmed on hardware:
  - A `window-rule { match app-id="..."; shader { source "...bgra..." } }` on a chosen app → ONLY that window's content is channel-swapped; its **border/shadow/focus-ring are unaffected**; other windows untouched.
  - Move/resize/focus the window → it stays correctly shaded with no smearing/garbage (damage attributed correctly).
  - A second app with a different shader → both shaded independently. Same source on two apps → both shaded (dedupe).
  - Remove the `shader` rule + reload → that window renders normally (byte-identity).
  - KMS capture only.

---

## Task 5: Docs

**Files:**
- Modify: `docs/wiki/Configuration:-Global-Shader.md`

- [ ] **Step 1: Add a "Per-window shaders" section**

Document the `window-rule { shader { source/path/mode + pass{} } }` child; that it reuses the `global_color` contract scoped to the window's own content; the v1 static limits (no `niri_time`/`niri_cursor` yet — both 0; `niri_screen` = window content; decorations unaffected); multiple windows independent + dedupe. Valid ```kdl example:

````markdown
```kdl
window-rule {
    match app-id="Alacritty"
    shader {
        source "vec4 global_color(vec3 c){ vec3 s=tex2D_screen(c.xy).rgb; float g=dot(s,vec3(0.299,0.587,0.114)); return vec4(vec3(g),1.0); }"
    }
}
```
````

- [ ] **Step 2: Verify it parses**

Run: `cargo test -p niri-config wiki_docs_parses`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add "docs/wiki/Configuration:-Global-Shader.md"
git commit -m "docs: per-window shaders"
```

---

## Self-Review Notes (spec coverage)

- Spec §3.F window config → T1 (window-rule `shader`) + T2 (`ResolvedWindowRules.shader`).
- Spec §3.D window render (offscreen the tile, `ScopedSource::Texture`, window-geo, decorations outside) → T4.
- Spec §3.A substrate reuse (same `scoped` cache / `set_scoped_programs`) → T3.
- Spec §4 window column → T4 uniform binding, with the **v1 deviation** (niri_time/niri_cursor = 0, niri_output_size = window size) per the approved scope cut; documented in T5.
- Spec §6 scope (window content only; no feedback; dedupe; crash-safe) → constraints + T4.
- **Damage/offscreen for a steady-state (non-animation) window is the key risk** (per exploration) — T4 mirrors the `ResizeAnimation` window-only-offscreen pattern and is verified on hardware (T4 Step 6). If smearing/over-damage appears, the fix is to attribute offscreen damage via `OffscreenData`/`set_offscreen_data` as the alpha/resize paths do; the plan's branch must adopt that if the naive fresh-offscreen approach misbehaves.
- The `niri_size`-freeze lesson is encoded in Global Constraints: `ScopedShaderElement` already omits `niri_size`; do not reintroduce it.
