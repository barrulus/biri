# Global Shader — Black-seed Feedback Buffers (first-frame fix)

Design spec. Written 2026-06-23. Fixes the "frozen overlay of the activation screen" bug for
feedback global shaders (e.g. the rainbow comet). Builds on the multi-pass global-shader path
(`src/render_helpers/global_shader_element.rs`). Pick this up cold; everything needed to plan and
build is here.

> Status discipline: **[confirmed]** = verified by code reading / hardware this session;
> **[design]** = proposed here, not yet built.

---

## 1. Problem & goal

**[confirmed, hardware 2026-06-23]** When a feedback global shader (one that reads `niri_prev` /
`niri_buffer`, e.g. an accumulating comet/trail) is enabled, it shows a **frozen, faint overlay of
the screen as it was at the moment of activation**. Two factors combine:

1. On the **first frame**, a pass's `niri_prev` falls back to the pass *input* — the live composited
   screen (`global_shader_element.rs:196`, `prev = pass.prev.unwrap_or(input)`). So an accumulator
   shader ingests the **entire activation screen** into its feedback buffer. The `niri_buffer`
   fallback similarly seeds from the previous screen.
2. The feedback textures are **8-bit** (`Fourcc::Abgr8888`), so a slow per-frame decay (e.g.
   `*0.98`, a 2% drop) cannot decrement small 8-bit values to zero — the seeded screen sticks
   permanently as a faint ghost.

**Goal (this spec):** stop factor 1. Seed a pass's first-frame `niri_prev`/`niri_buffer` with
**transparent black** instead of the live screen, so accumulators start empty and never ingest the
activation screen. This fixes the reported bug for every feedback shader.

**Out of scope (factor 2):** the 8-bit slow-decay residue is *not* addressed here. It is handled
shader-side with a small decay floor (`max(prev*0.98 - 0.004, 0.0)`, proven to work), and could be
addressed engine-side later with a higher-precision (RGBA16F) feedback texture — a larger change to
the shared `OffscreenBuffer` that is deliberately deferred.

---

## 2. Ground truth (verified this session)

- **[confirmed]** First-frame fallbacks in `GlobalShaderElement::draw`
  (`src/render_helpers/global_shader_element.rs`):
  - `niri_prev`: line ~196 — `let prev_tex = pass.prev.clone().unwrap_or_else(|| input.clone());`
    (for pass 0, `input` = `source_tex` = the live composited screen capture).
  - `niri_buffer`: line ~207 — `let buf_prev = pass.buffer_prev.clone().unwrap_or_else(|| screen_prev_tex.clone());`
  - `niri_screen_prev`: line ~166 — `let screen_prev_tex = self.screen_prev.clone().unwrap_or_else(|| source_tex.clone());`
- **[confirmed]** Feedback/offscreen textures are `Fourcc::Abgr8888` (`offscreen.rs:135`,
  `global_shader_element.rs:136/317`). The steady-state feedback buffers clear to transparent black
  (`Color32F::TRANSPARENT`) where untouched, so a transparent-black seed matches steady state.
- **[confirmed]** `Shaders` registry (`src/render_helpers/shaders/mod.rs`) is created once at
  renderer init (`Shaders::compile`) and stored in the renderer's egl user_data; reachable in the
  element via `Shaders::get_from_frame(frame)`. It already holds compiled programs and a `scoped`
  cache — the natural home for a shared 1×1 black texture.
- **[confirmed]** Scoped (region/window) shaders bind all five feedback samplers to their live
  input and have no cross-frame accumulation (`scoped_shader_element.rs`), so they do NOT have this
  bug and are untouched.
- **[confirmed]** Non-feedback shaders (warmtint/grayscale/CRT/vignette) never read
  `niri_prev`/`niri_buffer`, so the changed fallbacks never fire for them — byte-identical.

---

## 3. Architecture

### 3.A — Shared 1×1 transparent-black texture

Add a cached black texture to the `Shaders` registry:

```rust
pub struct Shaders {
    // ... existing fields ...
    pub black_texture: GlesTexture,   // 1×1 transparent black; an "empty feedback buffer" sampler
}
```

- Created once in `Shaders::compile(renderer)` by importing a 1×1 RGBA pixel of `[0, 0, 0, 0]`
  (e.g. `renderer.import_memory(&[0u8; 4], Fourcc::Abgr8888, (1, 1).into(), false)`). Confirm the
  exact `import_memory` signature in the plan phase; if `import_memory` is unavailable on the bare
  `GlesRenderer`, fall back to `create_buffer((1,1))` + bind + clear to `Color32F::TRANSPARENT`.
- A 1×1 texture sampled with the existing clamp wrapping returns black for every `uv`, so it works
  as an empty feedback buffer at any element size — no per-output sizing needed.
- `GlesTexture` is a cheap clonable handle; the element clones it out for the frame.

### 3.B — Seed the accumulator fallbacks with black

In `GlobalShaderElement::draw`, fetch the black texture once (near the top, alongside the existing
`Shaders::get_from_frame` program lookups):

```rust
let black = Shaders::get_from_frame(frame).black_texture.clone();
```

Then change the two **accumulator** fallbacks:
- `niri_prev` (line ~196): `pass.prev.clone().unwrap_or_else(|| black.clone())`
- `niri_buffer` (line ~207): `pass.buffer_prev.clone().unwrap_or_else(|| black.clone())`

**Leave `niri_screen_prev` unchanged** (line ~166, still falls back to `source_tex`): it represents
the *previous screen*, not an accumulator, so "≈ current screen on frame 0" is correct and
glitch-free.

### 3.C — Rationale for black as the default

- **Accumulator (comet/trail):** starts empty — correct; no activation-screen ingest.
- **Filter (`mix(prev, screen, k)`):** sees one black frame on activation, corrected on frame 1 —
  a benign one-frame glitch, vastly better than a permanent screen ghost.
- Matches steady state (untouched feedback regions are already transparent black).

The pre-existing `prev = input` fallback (comment: "so feedback shaders start from a sensible
image") was wrong for accumulators; black is the safe general default.

---

## 4. Testing

- **Compile:** dev-shell `cargo check --no-default-features --features dbus,systemd`. Touches only
  `src/render_helpers/shaders/mod.rs` (the cached texture + its init) and
  `src/render_helpers/global_shader_element.rs` (the two fallbacks). No unit tests (GPU path).
- **Regression / byte-identity:** non-feedback shaders unaffected (never read the changed samplers);
  a feedback shader's frame-1-onward behavior is unchanged — only frame 0's seed differs.
- **Manual (sixseven), KMS capture only:**
  1. Comet reverted to plain `*0.98` (no `-epsilon`) → **no activation-screen flash** on enable;
     trail builds from black. (8-bit residue may still accumulate faintly — factor 2, expected;
     keep `-epsilon` for daily use.)
  2. A `mix(tex2D_prev, screen, 0.05)` filter shader → at most a one-frame black flash on enable,
     then normal.
  3. Confirm steady-state comet (with `-epsilon`) is unchanged from today.

---

## 5. Scope boundaries (YAGNI)

- Seeds **black** for `niri_prev` and `niri_buffer` first-frame only. No config, no per-shader
  choice of seed.
- Does **not** change texture precision (8-bit stays); factor 2 (slow-decay residue) is handled
  shader-side and deferred for any engine RGBA16F work.
- Does **not** touch `niri_screen_prev`, scoped (region/window) shaders, the `OffscreenBuffer`
  shared format, or any other render path.
- One shared 1×1 black texture in the registry (not per-output, not per-element).

---

## 6. Suggested implementation order (for the plan phase)

1. Add `black_texture` to `Shaders` + create it in `Shaders::compile` (import a 1×1 `[0,0,0,0]`
   pixel; confirm the smithay import/clear API). `cargo check`.
2. Fetch it in `GlobalShaderElement::draw` and swap the two accumulator fallbacks to `black`.
   Leave `niri_screen_prev` as-is. `cargo check`.
3. Manual hardware verification per §4.

Two tiny steps; one commit is reasonable, or one per file.

---

## 7. Build / test crib

Inherited from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, per-task `cargo
check`, KMS-only recording). Not duplicated here.
