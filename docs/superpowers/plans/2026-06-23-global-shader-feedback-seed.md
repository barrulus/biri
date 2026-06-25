# Global Shader Black-seed Feedback — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Seed a feedback global shader's first-frame `niri_prev`/`niri_buffer` with transparent black instead of the live composited screen, so accumulator shaders (comet/trail) no longer ingest the activation screen as a frozen ghost.

**Architecture:** Add one shared 1×1 transparent-black `GlesTexture` to the `Shaders` registry (created once at renderer init). In `GlobalShaderElement::draw`, swap the two accumulator first-frame fallbacks (`niri_prev`, `niri_buffer`) from `input`/`screen_prev` to that black texture. Leave `niri_screen_prev` (a screen, not an accumulator) as-is.

**Tech Stack:** Rust, smithay GLES renderer (`GlesTexture`, `ImportMem::import_memory`), GLES2 shaders.

**Spec:** `docs/superpowers/specs/2026-06-23-global-shader-feedback-seed-design.md` — read it first.

## Global Constraints

- **Black = transparent black** (`[0,0,0,0]`), a 1×1 texture sampled with the existing clamp → returns black for every `uv` at any element size. One shared texture in the registry — not per-output, not per-element.
- **Only the two accumulator fallbacks change:** `niri_prev` (`global_shader_element.rs:196`) and `niri_buffer` (~207). `niri_screen_prev` (~166) stays falling back to the current screen.
- **Byte-identity:** non-feedback shaders never read these samplers → unaffected. A feedback shader's frame-1-onward behavior is unchanged; only frame 0's seed differs.
- **Scope:** global-shader path only. Do NOT touch scoped (region/window) shaders, the `OffscreenBuffer` format, or texture precision (8-bit stays; the slow-decay residue is handled shader-side with `-epsilon`, out of scope here).
- **Build/test crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task `cargo check --no-default-features --features dbus,systemd` inside it with `export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib`. Run `cargo fmt` (plain, NOT `+nightly`) before committing. No unit tests (GPU render path); verified by compile + manual hardware check. Commits: NO Co-Authored-By / AI-attribution lines.

---

## File Structure

- **Modify** `src/render_helpers/shaders/mod.rs` — add `black_texture: GlesTexture` to `Shaders`; create it in `Shaders::compile` via `import_memory`.
- **Modify** `src/render_helpers/global_shader_element.rs` — fetch the black texture in `draw()` and swap the two accumulator fallbacks.

Two files; the second consumes the field the first adds.

---

## Task 1: Add a shared 1×1 black texture to the Shaders registry

**Files:**
- Modify: `src/render_helpers/shaders/mod.rs`

**Interfaces:**
- Produces: `Shaders.black_texture: GlesTexture` — a 1×1 transparent-black texture, available via `Shaders::get_from_frame(frame).black_texture` / `Shaders::get(renderer).black_texture`.

- [ ] **Step 1: Add the imports**

At the top of `src/render_helpers/shaders/mod.rs`, ensure these are in scope (add whichever are missing):

```rust
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::ImportMem;
use smithay::backend::renderer::gles::GlesTexture;
use smithay::utils::Size;
```

(`GlesError`, `GlesRenderer` etc. are already imported. `Fourcc`/`ImportMem`/`Size`/`GlesTexture` may not be — add the missing ones. The compiler will flag duplicates; remove any that already exist.)

- [ ] **Step 2: Add the struct field**

In `pub struct Shaders { ... }`, add (after the existing fields, e.g. after `scoped`):

```rust
    /// 1×1 transparent-black texture used to seed a feedback shader's first-frame niri_prev /
    /// niri_buffer (an "empty feedback buffer"), instead of ingesting the live screen.
    pub black_texture: GlesTexture,
```

- [ ] **Step 3: Create it in `Shaders::compile`**

In `fn compile(renderer: &mut GlesRenderer) -> Self`, before the `Self { ... }` return, add:

```rust
    // 1×1 transparent black; sampled (clamped) it returns black for every uv at any size.
    let black_texture = renderer
        .import_memory(&[0u8, 0, 0, 0], Fourcc::Abgr8888, Size::from((1, 1)), false)
        .expect("importing a 1x1 black texture must not fail");
```

Then add `black_texture,` to the `Self { ... }` struct literal.

- [ ] **Step 4: Compile-check (dev shell)**

Run:
```
nix develop /home/barrulus/quixote#rust-compositor --command bash -c 'export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib; cd /home/barrulus/dev/biri && cargo check --no-default-features --features dbus,systemd 2>&1 | tail -20'
```
Expected: GREEN (additive field + init; no consumers break). If `import_memory` isn't resolved, confirm the `ImportMem` trait import (Step 1); if the signature differs, match it to the call at `src/render_helpers/texture.rs:64` (`renderer.import_memory(data, format, size.into(), flipped)`).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/render_helpers/shaders/mod.rs
git commit -m "global-shader: add 1x1 black texture to Shaders registry (feedback seed)"
```

---

## Task 2: Seed niri_prev / niri_buffer with black on the first frame

**Files:**
- Modify: `src/render_helpers/global_shader_element.rs`

**Interfaces:**
- Consumes: `Shaders.black_texture` (from Task 1), via `Shaders::get_from_frame(frame)`.

- [ ] **Step 1: Fetch the black texture in `draw()`**

In `impl RenderElement<GlesRenderer> for GlobalShaderElement::draw`, after the `chain_ready` early-return / before the `screen_prev_tex` line (~166), add:

```rust
        // Empty-feedback seed: on the first frame a pass's niri_prev / niri_buffer has no prior
        // value; seed with black so accumulators start empty instead of ingesting the live screen.
        let black = Shaders::get_from_frame(frame).black_texture.clone();
```

(Place it before the `for (i, pass) in self.passes.iter().enumerate()` loop. `Shaders` is already imported in this file.)

- [ ] **Step 2: Swap the `niri_prev` fallback**

Change the `prev_tex` line (currently ~196):

```rust
            let prev_tex = pass.prev.clone().unwrap_or_else(|| input.clone());
```

to:

```rust
            // First frame: seed niri_prev with black (empty trail), not the live screen.
            let prev_tex = pass.prev.clone().unwrap_or_else(|| black.clone());
```

- [ ] **Step 3: Swap the `niri_buffer` fallback**

Change the `buf_prev` line (currently ~207, inside the `if … GlobalPassBuffer(i) … is_some()` block):

```rust
                let buf_prev = pass
                    .buffer_prev
                    .clone()
                    .unwrap_or_else(|| screen_prev_tex.clone());
```

to:

```rust
                // First frame: seed niri_buffer with black (empty accumulator), not the screen.
                let buf_prev = pass.buffer_prev.clone().unwrap_or_else(|| black.clone());
```

Leave `screen_prev_tex` (the `niri_screen_prev` fallback, ~166 — `self.screen_prev.clone().unwrap_or_else(|| source_tex.clone())`) **unchanged**: it is the previous *screen*, not an accumulator.

- [ ] **Step 4: Compile-check (dev shell)**

Run:
```
nix develop /home/barrulus/quixote#rust-compositor --command bash -c 'export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib; cd /home/barrulus/dev/biri && cargo check --no-default-features --features dbus,systemd 2>&1 | tail -20'
```
Expected: GREEN. Watch for an unused-variable warning on `input` — it is still used as pass 0's `niri_screen` (`textures.insert("niri_screen", input.clone())`), so it should remain used; if the compiler warns it's unused, something else was changed by mistake — revert and re-check.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/render_helpers/global_shader_element.rs
git commit -m "global-shader: black-seed first-frame niri_prev/niri_buffer (no activation-screen ingest)"
```

- [ ] **Step 6: Manual hardware verification (owed; sixseven, KMS capture only)**

Per spec §4:
1. Comet reverted to plain `*0.98` (no `-0.004`): enable it → **no activation-screen flash**; trail builds from black. (8-bit residue may still accumulate faintly — expected; keep `-0.004` for daily use.)
2. A `mix(tex2D_prev(c.xy), tex2D_screen(c.xy), 0.05)` filter shader → at most a one-frame black flash on enable, then normal.
3. Steady-state comet (with `-0.004`) looks unchanged from today.

---

## Self-Review Notes (spec coverage)

- Spec §3.A (1×1 black texture in `Shaders`, `import_memory` with `create_buffer`+clear fallback) → Task 1. (Fallback path: if `import_memory` is somehow unavailable, the implementer uses `create_buffer((1,1))` + bind + clear to `Color32F::TRANSPARENT`; `import_memory` is confirmed available at `texture.rs:64`, so the primary path should work.)
- Spec §3.B (swap the two accumulator fallbacks, keep `niri_screen_prev`) → Task 2 Steps 2–3.
- Spec §3.C / §5 (black-as-default rationale; scope: global only, no precision change, no scoped/OffscreenBuffer change) → enforced by only editing these two files / these two lines.
- Spec §4 testing → compile checks per task + Task 2 Step 6 manual.
- Type consistency: `black_texture: GlesTexture` defined in Task 1, consumed as `Shaders::get_from_frame(frame).black_texture.clone()` in Task 2.
