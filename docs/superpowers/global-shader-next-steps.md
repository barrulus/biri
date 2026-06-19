# Global Shader — Next Steps / v2 Roadmap

Working notes for evolving the `global-shader` feature beyond the proof-of-concept v1.
Written 2026-06-19. Pick this up cold later; everything needed to resume is here.

> Status discipline: items below are tagged **[bug]** (demonstrated), **[untested]**
> (suspected from code reading, never reproduced), or **[enhancement]**. Don't treat
> untested as broken — verify first.

---

## 1. Where v1 stands (what exists today)

v1 is a deliberately lightweight, config-driven, full-output post-process shader. It
works and is in daily use. It cut every corner not needed to prove the concept.

**Contract (niri mode):** user writes `vec4 global_color(vec3 c)`.
- `c.xy` = 0..1 across output, `c.y = 0` is top.
- uniforms: `niri_time` (s), `niri_size` (physical px), `niri_scale`, `niri_cursor`
  (output-local physical px, y-top), `niri_alpha` (applied in epilogue).
- samplers: `niri_screen` (composited frame below), `niri_prev` (previous output frame).
- helpers: `tex2D_screen(uv)`, `tex2D_prev(uv)`.
- GLES2 `#version 100` (no `#version` line; `varying`/`gl_FragColor`/`texture2D`).
- second mode `hyprland`: raw shader with own `main()`, aliases `tex`/`v_texcoord`/`time`/`wl_output`; no cursor.

**Code map (all on `barrulus-custom`, merged from `feature/global-shader`):**
- Config: `niri-config/src/global_shader.rs` (`GlobalShader`/`GlobalShaderPart`/`MergeWith`),
  wired in `niri-config/src/lib.rs` (Config field, `"global-shader"` dispatch, parse snapshot).
- Shader registry: `src/render_helpers/shaders/mod.rs` — `custom_global` field,
  `ProgramType::Global`, `compile_global_program` / `set_custom_global_program` /
  `replace_custom_global_program`. Contract files: `src/render_helpers/shaders/global_prelude.frag`,
  `global_epilogue.frag`, `global_hypr_prelude.frag`.
- Element: `src/render_helpers/global_shader_element.rs` — `GlobalShaderElement`. Its `draw()`:
  (1) `create_buffer` a screen texture, (2) `capture::capture_framebuffer_region` blits the
  framebuffer below into it, (3) delegates the program pass to `ShaderRenderElement`
  (`ProgramType::Global`, textures `niri_screen`+`niri_prev`, uniforms `niri_time`+`niri_cursor`),
  (4) captures the result into a shared `Rc<RefCell<Option<GlesTexture>>>` for ping-pong.
- Shared capture helper: `src/render_helpers/capture.rs` — `capture_framebuffer_region`
  (extracted from `framebuffer_effect.rs`, which now calls it too).
- Wiring in `src/niri.rs`:
  - `render_inner` push, gated `ctx.target == RenderTarget::Output && Shaders::get(ctx.renderer).program(ProgramType::Global).is_some()`. Built once into an `Option`, pushed before the pointer if `reads_cursor` else after.
  - `OutputState` fields: `global_shader_prev: Option<GlesTexture>`, `global_shader_start: Cell<Option<Instant>>`, `global_shader_result: Rc<RefCell<Option<GlesTexture>>>`.
  - `global_shader_source(cfg)` helper (resolves inline/path, validates, warn+None on bad).
  - config-reload detection (~line 1586) diffing enable/source/path/mode; resets per-output state.
  - `preview_render` retargets `ctx.target` away from Output (~4203), which disables the shader during preview (commented).
  - `OutputRenderElements` enum gains `GlobalShader = GlobalShaderElement`.
- Backend (`src/backend/tty.rs`): startup `set_custom_global_program` apply (~845);
  post-submit ping-pong move `global_shader_prev = global_shader_result.borrow_mut().take()`
  after `render_frame` (~1940); software-cursor force in the `flags` block (~1917-1930) when
  `global_shader.enable && reads_cursor`. winit backend intentionally does NOT apply it.
- Docs: `docs/wiki/Configuration:-Global-Shader.md`. Authoring/porting skill:
  `.claude/skills/converting-global-shaders/SKILL.md` (also copied to `~/quixote/shader_skill.md`).
- v1 design + plan: `docs/superpowers/specs/2026-06-18-global-shader-design.md`,
  `docs/superpowers/plans/2026-06-18-global-shader.md`.

**Deliberate v1 scope cuts (non-goals):** TTY/DRM only (no winit, no headless); excluded
from screencast + screenshot sinks; single shader (no chaining); one feedback texture
(`niri_prev` = whole previous frame); full-output redraw while active.

**User-side cycle system (not in repo):** `~/.config/niri/global-shaders/` holds
`global-*.kdl` files (each a full `global-shader{}` block); `current.kdl` is a symlink to the
active one; `~/.config/niri/global-shader-cycle` round-robins them (Mod+4); Mod+3 links
`off.kdl`. `config.kdl` has a stable `include "global-shaders/current.kdl"`.

---

## 2. Known issues / limitations

### 2.1 Cost is not proportional to the effect  [enhancement, highest impact]
The element captures the whole output, reshades every pixel, and disables direct scanout /
overlay-plane offload **regardless of what the shader does**. A static color-grade pays the
same as an animated cursor effect.
- **[untested]** Whether v1 forces *continuous* redraws when idle, or schedules redraws for
  time-animated shaders, is unconfirmed — must check `render_inner` / the redraw-scheduling
  path (`queue_redraw`, animation-driven redraws). The demos animated while the cursor moved
  (which itself damages), so animation-when-idle may or may not work.
- Root mechanism: a full-output element + screen capture forces full damage and blocks scanout.

### 2.2 Trails smear on moving content; trails/glows invisible when additive  [bug — understood]
- **White invisibility:** additive output (`screen + glow`) clamps on white → invisible.
  *Fix already applied in the user's shaders:* blend with `mix` toward a color. This is an
  authoring fact, but a v2 contract could make it the default/ergonomic path.
- **Scroll smear:** `niri_prev` is the **entire** previous output (screen + effect mixed). A
  feedback trail recovers itself via `prev - screen`, which also picks up screen motion
  (scrolling/video) → false trail everywhere content changed. Mitigated in the user's shaders
  by projecting the recovery onto the trail color, but **not fully fixable with a single
  whole-frame feedback buffer**. This is the motivation for item 3.2.

### 2.3 Effect absent from recordings/screenshots  [enhancement / by-design]
Gated to `RenderTarget::Output` only (deliberate, to avoid cross-writing the per-output
ping-pong from capture renders). Consequence: portal/screencopy capture (OBS, `gpu-screen-recorder -w portal`,
wl-screenrec, grim) never shows the effect. **Only KMS/scanout capture** (`gpu-screen-recorder -w eDP-1`)
captures it, because it grabs the real scanout. Revisiting requires per-render-target state.

### 2.4 Output transform on non-normal outputs  [untested — NOT a confirmed bug]
The no-program path uses `frame.transformation().invert()`; the with-program path (via
`ShaderRenderElement`) does not. On `normal`-transform outputs this is a no-op — **verified
correct** by the red-band marker test (band at top, content upright). It is *only* a
suspicion for rotated/flipped outputs, which were never tested (user has none). If anyone uses
a 90°/270°/flipped output, re-run the marker shader; fix only if it actually inverts.

### 2.5 Misc smaller gaps
- `reads-cursor` forces **software cursor for the whole output** (cost), not just a region.
- Shader **file** changes (when using `path`) only reload on config reload, not on file edit.
- Single shader only — no multi-pass / chaining.
- winit/headless render without the effect (no in-session dev/test path).
- Ping-pong via shared `Rc<RefCell>` + post-submit move is slightly hacky; fine but worth
  revisiting if the render path is refactored.

---

## 3. Roadmap items (with design sketches)

### 3.1 Localised frames / redraw intelligence  [enhancement] — DO FIRST
Make cost proportional to the effect. Three sub-parts, increasing difficulty:
1. **Static-skip (easy, big battery win):** detect whether the compiled shader references
   `niri_time` / `niri_cursor` / `niri_prev` (scan the source string at compile time, store
   flags on the program). If none → the effect is a pure function of the screen below →
   redraw only on actual damage; do NOT force continuous redraws.
2. **Time-driven vsync (medium):** if the shader uses `niri_time` (and/or `niri_cursor`),
   ensure it schedules a redraw every frame so it animates even when the desktop is idle.
   Hook into niri's animation/redraw scheduling (`queue_redraw_all` / output redraw timers).
3. **Region-damage (hard):** for spatially-local effects (cursor ring/spotlight), reshade and
   damage only a bounding box around the cursor (+ trail extent), not the whole output, and
   keep scanout for the rest. Needs the shader's footprint declared (config: an effect radius)
   or inferred. Does NOT help whole-screen filters. Capture would also become region-limited.
- Risk: interacts with damage tracking and DRM plane assignment; tread carefully.

### 3.2 Dedicated feedback / history buffer  [enhancement] — fixes 2.2
Give the shader its own offscreen texture(s) it reads and writes, independent of the screen.
- Add per-output offscreen(s); expose as e.g. `niri_buffer` (read previous) + the shader's
  return value (or a second output) writes the next. The trail lives in the buffer, never
  mixed with the screen → no scroll smear; clean motion blur / accumulation.
- Design choices: how many buffers; whether the shader writes one combined output (screen +
  buffer) or two (display + buffer state, needs MRT or two passes); lifetime/reset on reload.
- This is the natural substrate for item 3.3 (multi-pass).

### 3.3 Layers / multi-pass  [enhancement]
- **Multi-pass chains:** allow N shaders run in sequence (blur → grade → vignette), each
  reading the prior pass's output. Config: a list of shader blocks/passes. Needs intermediate
  buffers (item 3.2).
- **Scoped shaders:** apply to a region / single window / a specific layer-shell layer rather
  than the whole output. Bigger model change ("output post-process" → "compositable shader
  layers"); integrates with the layout/window machinery.

### 3.4 iGPU / power  [enhancement]
The effect is always-on GPU load, so on a hybrid laptop, running the compositor's render path
on the efficient iGPU (and scanning out appropriately) is a battery lever. This is niri-wide
multi-GPU render-device selection, not shader-specific — the shader just runs on the render
GPU. Downstream of 3.1 (reduce the load first). Investigate smithay multi-GPU + niri's render
device selection.

### 3.5 Reach polish  [enhancement]
- **Screencast inclusion:** render the effect into the screencast/screencopy sinks with
  per-render-target ping-pong state (so capture renders don't corrupt the live output's
  feedback). Lets OBS/portal capture show the effect (item 2.3).
- **winit support:** apply the program + run the pass on winit for in-session dev/testing
  (was intentionally removed in v1). Add the post-submit ping-pong move to the winit path too.
- **Shader-file hot-reload:** watch the `path` file and recompile on change.
- **Richer uniforms:** mouse buttons / click state, frame counter, per-output index, and
  **audio/FFT** (would unlock the visualizer class — e.g. Shadertoy music shaders like
  "Chromatic Resonance" that need `iChannel0` = sound).

---

## 4. Suggested order
1. **3.1 Localised frames** (static-skip → time-vsync → region-damage) — makes it daily-viable.
2. **3.2 Dedicated feedback buffer** — fixes trail smear, unlocks multi-pass.
3. **3.3 Layers / multi-pass.**
4. **3.4 iGPU / power**, then **3.5 reach polish.**
5. **2.4 transform** — only if a rotated output is ever used (likely a 60-second check, maybe nothing).

Each is real compositor-rendering work — treat each like v1: brainstorm → spec → plan →
subagent build.

---

## 5. Build / test crib (this machine: "sixseven", NVIDIA 5060 + iGPU)
- Dev shell: `nix develop /home/barrulus/quixote#rust-compositor`. It does NOT export the
  bindgen env, so:
  - `export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib`
    (needed for libspa-sys/pipewire bindgen; full default build also needs clang C-include
    args for `inttypes.h` — unsolved here, only matters for the screencast feature).
  - For the final binary link: x86-64 `mesa-libgbm` on `LIBRARY_PATH`, e.g.
    `/nix/store/vqdj7h2d94f494in371j7jwz8akymryi-mesa-libgbm-26.0.3/lib` (pick a current one;
    confirm arch with `readelf -h .../libgbm.so | grep Machine` = X86-64).
- Per-task compile check (no pipewire, no link — fast, green on the feature work):
  `cargo check --no-default-features --features dbus,systemd` inside the dev shell.
- `niri-config` alone builds/test outside the dev shell. `cargo test -p niri-config` runs the
  parse snapshot + `wiki_docs_parses` (the wiki KDL examples must parse).
- Snapshot gotcha: `cargo insta accept` does NOT apply inline pending snapshots here and can
  hang. Workaround used: run the test to produce `niri-config/src/.lib.rs.pending-snap` (NDJSON),
  then patch the `@r#"..."#` in `lib.rs` from the `new.snapshot` field (8-space indent per line).
- Daily deploy: `barrulus-custom` is the flake input (`github:barrulus/biri/barrulus-custom` in
  `~/quixote/flake.nix`); push it, `nix flake update biri --flake ~/quixote`, then
  `sudo nixos-rebuild switch --flake ~/quixote#sixseven`. The Nix build provides the full
  (screencast-enabled) toolchain, sidestepping the manual env gaps above.
- Recording the effect: KMS capture only — `gpu-screen-recorder -w eDP-1 ...` (portal/screencopy
  omit it by design, see 2.3).
