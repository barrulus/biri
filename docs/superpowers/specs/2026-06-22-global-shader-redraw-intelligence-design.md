# Global Shader 3.1 — Capability-Driven Damage & Redraw Intelligence

Design spec. Written 2026-06-22. Implements roadmap item 3.1 from
`docs/superpowers/global-shader-next-steps.md` (static-skip → time-driven vsync →
region-damage), unified into one architecture. Pick this up cold; everything needed to
plan and build is here.

> Status discipline (inherited from the roadmap): claims below are tagged **[confirmed]**
> (verified by code reading this session), **[bug]** (demonstrated/understood), or
> **[design]** (proposed here, not yet built). Don't treat a design choice as fact.

---

## 1. Problem & goal

v1's global shader makes cost independent of what the shader does, and — more urgently —
**time-animated shaders freeze when the desktop is idle.** The goal of 3.1 is to make the
shader's redraw and damage behavior **proportional to what the shader actually does**, by
deriving the shader's capabilities from its source and from one new config field, and
letting that drive (a) whether a redraw is scheduled when idle and (b) what screen region
the element reports as damaged / reshades.

This is one spec covering all three roadmap sub-parts, sequenced as three implementation
phases (A enabling, B the bug fix + usable feature, C the hard scanout win).

---

## 2. Ground truth (verified this session)

These corrected the roadmap doc's assumptions and shaped the design. Citations are to
`barrulus-custom` as read on 2026-06-22.

- **[confirmed] Idle redraw is NOT forced today; the opposite is a bug.**
  - `GlobalShaderElement` is built with `commit: CommitCounter::default()`
    (`src/render_helpers/global_shader_element.rs:52`) and **nothing ever calls
    `.increment()` on it.** It does not override `damage_since`, so smithay's default
    keys off `current_commit()`: full damage on the first frame, then **empty** damage on
    every subsequent frame (the stored commit equals the constant current commit).
  - Redraw scheduling: after vblank, niri re-queues a redraw only if
    `redraw_needed || output_state.unfinished_animations_remain`
    (`src/backend/tty.rs:1788`, also `:1826`). `unfinished_animations_remain` is computed
    at `src/niri.rs:4714-4733` from layout/cursor/UI animations and **does not consider
    the global shader.**
  - **Net effect [bug]:** a `niri_time`/`niri_prev` shader only re-renders when *something
    else* damages the screen. The v1 demos appeared animated only because the moving
    cursor was constantly damaging the output. With the cursor still and the desktop idle,
    the animation **freezes.** This is the real defect inside roadmap item 2.1.

- **[confirmed] Raw shader source is available at compile time.**
  `compile_global_program(renderer, src: &str, hyprland: bool)` at
  `src/render_helpers/shaders/mod.rs:367-391` has the user source as `&str` (it concatenates
  a prelude + `src`). We can scan `src` before compiling and store flags on the program.

- **[confirmed] Scanout is decided per-output by smithay from element geometry/opacity.**
  niri passes the full element vector to `DrmCompositor::render_frame` (`src/backend/tty.rs:1939`)
  with global `FrameFlags`; it does **not** pre-filter elements for scanout. smithay decides
  per plane from each element's `geometry()`, `opaque_regions()`, `alpha()`, `transform()`.
  `GlobalShaderElement::geometry()` returns the **full output** rect
  (`global_shader_element.rs:77-79`, area = `output_size`) and `opaque_regions()` is **not
  overridden** (empty — it reads what's below and may be translucent). A full-output,
  non-opaque element on top therefore blocks scanout for everything below.
  **Consequence [confirmed]:** shrinking the element's `geometry()` to a small box lets the
  rest of the output stay scanout-eligible — region-damage can preserve scanout, not merely
  limit reshading.

- **[confirmed] Per-output state already exists.** `OutputState` (`src/niri.rs:493-499`)
  holds `global_shader_prev: Option<GlesTexture>`, `global_shader_start: Cell<Option<Instant>>`,
  `global_shader_result: Rc<RefCell<Option<GlesTexture>>>`. Ping-pong move happens
  post-submit at `src/backend/tty.rs:1946-1948`. The element is rebuilt each frame in
  `render_inner` (`src/niri.rs` ~4283-4285), with `niri_time = start.elapsed()`.

---

## 3. Architecture

### 3.1 Core idea

Compile-time **capability flags** + one config field collapse the whole feature into a
single decision per active shader: *what damage does the element report, and is a redraw
scheduled when idle?*

```
shader source ──scan──► (uses_time, uses_cursor, uses_prev)  ┐
config: cursor-radius <px>                                   ├─► shader class
config: redraw <on-damage|continuous>  (override)            ┘        │
                                                                      ▼
                                          ┌─────────────────────────────────────────┐
                                          │ scheduling: redraw-when-idle?            │
                                          │ geometry:   full output | cursor box     │
                                          │ damage:     full | underlying | box-union│
                                          └─────────────────────────────────────────┘
```

### 3.2 The behavior table (heart of the feature)

| Shader class | Condition | Redraw scheduling | Element `geometry()` | Reported damage / reshade |
|---|---|---|---|---|
| **Animating** | `uses_time` OR `uses_prev` | redraw **every frame** while active | full output | full output every frame (bump commit counter per frame) |
| **Static filter** | none of time/cursor/prev | **only on real damage** (today's default) | full output | full output on *any* underlying damage |
| **Cursor-local** | `uses_cursor` AND `cursor-radius` set AND not animating | redraw when cursor moves (already damages); none at rest | **box(cursor, radius)** | `box(cursor) ∪ box(prev_cursor)`; scanout preserved outside |

Notes:
- A shader using **both** cursor and time/prev is **Animating** (the time/prev row wins):
  full-output, redraw-every-frame. `cursor-radius` is only honored for the pure-cursor case,
  because a feedback/trail term that decays over time must reshade the whole output anyway.
- "Static filter" reshades **full output** (not just the damaged sub-rect) because the v1
  contract lets a shader call `tex2D_screen(uv)` at arbitrary `uv` (e.g. blur) — a
  non-pointwise filter would show stale halos if only the damaged sub-rect were reshaded.
  The win for static filters is purely *no idle redraw* (already the default; this phase
  makes it correct and explicit, not faster).
- The `redraw` override replaces the scheduling column: `continuous` forces redraw-every-frame
  even for a static filter; `on-damage` suppresses idle redraw even for an animating shader
  (useful to deliberately tie animation to activity).

### 3.3 Why these flags are safe

The scan is a literal substring match on the source. In GLSL a uniform/sampler must appear
by its literal name to be used, so the scan **cannot under-trigger** (no false "static"
classification of an animating shader). It can **over-trigger** (token in a comment / dead
code) → at worst extra redraws, never staleness. The optional `redraw` override exists for
the rare case an author wants to correct an over-trigger.

The hyprland dialect aliases the niri uniforms (`time`, `wl_output`, `tex`, `v_texcoord`),
so for `hyprland` mode the scan must look for the aliased names. `uses_prev` and a true
`uses_cursor` do not exist in hyprland mode (no cursor uniform) — hyprland shaders are
classified Animating iff they reference `time`.

---

## 4. Implementation phases

Each phase is independently shippable. B is the bug fix and the minimum viable result; C is
optional and ships disabled-by-default if it fights smithay.

### Phase A — Capability flags

**Where:** `src/render_helpers/shaders/mod.rs`.

- In `compile_global_program`, before compiling, scan `src` for the token set appropriate
  to the dialect (niri: `niri_time`, `niri_cursor`, `niri_prev`; hyprland: `time`, plus
  whatever the hypr prelude aliases). Derive `uses_time`, `uses_cursor`, `uses_prev`.
- Store the three bools alongside the compiled program. Determine where: either extend the
  struct that wraps the `ShaderProgram` for `ProgramType::Global` (preferred — keep flags
  with the program), or carry them in the `Shaders` registry next to `custom_global`. The
  plan picks the exact field location after reading the current `custom_global` storage.
- Expose a way to read the flags from where the element is built (`render_inner`) and from
  the scheduler (`niri.rs:4714`).
- **Tests:** unit tests over sample source strings — niri dialect (each token, all-absent,
  token-in-comment), hyprland dialect (`time` present/absent). Assert the derived flags.

### Phase B — Scheduling + full-frame damage (the bug fix)

**Where:** `src/niri.rs` (scheduler + element construction), `src/render_helpers/global_shader_element.rs`.

1. **Scheduling.** In the `unfinished_animations_remain` computation
   (`src/niri.rs:4714-4733`), OR-in a condition: the output has an active global program
   that is *Animating* (per flags) and not overridden to `on-damage`. This makes the output
   re-queue a redraw every vblank so the shader animates while idle. Honor a `continuous`
   override here too. (Confirm this is the correct single chokepoint vs. needing a
   `queue_redraw` call elsewhere; `:1788`/`:1826` both consult this flag, so it should
   suffice.)
2. **Damage.** Make the element report the damage the table requires:
   - Animating: bump the `CommitCounter` once per frame when building the element (each
     rebuild calls `.increment()`), so smithay's default `damage_since` returns full-output
     damage every frame.
   - Static filter: report full-output damage whenever there is underlying damage. Simplest
     correct approach: bump the commit counter whenever the frame is being drawn at all
     (the element is only built when a redraw is already happening), i.e. full reshade each
     redraw, no idle redraw. Verify this doesn't itself force idle redraws (it must not —
     scheduling is governed by Phase B.1, damage only governs *what* repaints once a redraw
     is already scheduled).
   - Keep `draw()` reshading the full output (it already ignores the `damage` arg) for these
     two classes.
   - Confirm element `id()` stability across frames; if the `Id` is regenerated per frame
     the tracker already sees full damage and the commit-counter bump is belt-and-suspenders
     — note which it is, keep behavior deterministic.
3. **Reload.** The flags live with the compiled program, so config reload / shader swap
   (the diff path at `src/niri.rs:~1586`) refreshes them for free; verify the per-output
   state reset still holds.

**Result after B:** a `niri_time` shader animates when idle (currently frozen); static
filters reshade correctly; no config changes required for existing shaders. This is the
"daily-viable" milestone.

### Phase C — Region-damage + scanout preservation (hard, last, descopable)

**Where:** `src/render_helpers/global_shader_element.rs`, `src/render_helpers/capture.rs`,
element construction in `src/niri.rs`, config in `niri-config`.

Active only when: `uses_cursor`, `cursor-radius` is set, and the shader is **not** Animating.

- Build the element with `area`/`geometry()` = the cursor box (`cursor ± cursor-radius`,
  clamped to output bounds) instead of the full output. Everything outside the box composites
  normally and stays scanout-eligible (per §2 ground truth).
- Capture only the box region (`capture_framebuffer_region` already takes a `dst` rect —
  pass the box, not the full output). Likewise the result re-capture for ping-pong, if
  `niri_prev` is even relevant here (it isn't for a pure-cursor shader; `niri_prev` implies
  Animating, which excludes this class — so the prev buffer can be skipped in this path).
- **Contract preservation:** the shader still expects `c.xy = 0..1 across the whole output`
  and `niri_size` = full output. Keep passing the full-output size/scale as uniforms and map
  the box's local fragment coords back to output-normalized coords (offset + scale in the
  prelude or via an added uniform giving the box origin/extent). This must be transparent to
  shader authors — the marker-band test must still place the band at the true output top.
- **Damage:** report `box(cursor) ∪ box(prev_cursor)` so the trail/movement is covered.
- **Scanout check:** verify niri isn't independently suppressing primary/overlay scanout
  flags whenever the global shader is enabled (`src/backend/tty.rs:1909-1937`). Today only
  the *cursor plane* is disabled when `reads_cursor` (`:1926`); confirm primary/overlay
  scanout is governed solely by smithay seeing the (now small) element geometry, and adjust
  flags if needed so scanout outside the box is actually allowed.
- **Risk / fallback:** this interacts with smithay damage tracking and DRM plane assignment.
  If it doesn't prove out, ship it **disabled by default** (the field present but the
  region path behind it inert / warned) and keep A+B. The spec's success does not depend on
  C landing.

---

## 5. Config surface

**Where:** `niri-config/src/global_shader.rs`, snapshot in `niri-config/src/lib.rs`, docs in
`docs/wiki/Configuration:-Global-Shader.md`.

Two new optional fields on the `global-shader {}` block:

- `cursor-radius <px>` — logical pixels; enables Phase C for cursor-only shaders. Absent →
  cursor shaders stay full-output (Phase B behavior). Square box of side `2*radius` centered
  on the cursor (circle effects bound to this box).
- `redraw <on-damage|continuous>` — scheduling override; absent → auto from flags.

Both additive and optional → **existing shader configs parse and behave unchanged**.

- Update the parse snapshot per the crib in the roadmap doc (`niri-config` builds/tests
  outside the dev shell; `cargo insta accept` may hang — patch the inline `@r#"..."#` from
  `.lib.rs.pending-snap`).
- Add a wiki example exercising both fields so `wiki_docs_parses` covers them.

---

## 6. Testing

- **niri-config:** `cargo test -p niri-config` — parse-snapshot for the two new fields +
  `wiki_docs_parses` for the new wiki example.
- **Flag scan (unit):** sample sources per dialect (niri tokens individually, all-absent,
  comment-only occurrence; hyprland `time` present/absent) → assert derived flags.
- **Compile check per task:** `cargo check --no-default-features --features dbus,systemd`
  inside the dev shell (`nix develop /home/barrulus/quixote#rust-compositor`), per the crib.
- **Behavior (manual, "sixseven", deploy via the flake per the crib):**
  - *Idle-animation (Phase B, the bug):* a `niri_time`-only shader animates with the cursor
    still and desktop idle. Pre-change it freezes; post-change it ticks every vblank.
  - *Static filter (Phase B):* a non-pointwise filter (small blur) reshades fully on window
    damage with no stale halos, and does **not** spin the GPU when idle.
  - *Override:* `redraw continuous` forces idle animation on a static shader; `redraw
    on-damage` stops idle animation on a time shader.
  - *Region (Phase C):* a cursor ring with `cursor-radius` — confirm via
    `gpu-screen-recorder -w eDP-1` (KMS capture; portal omits the effect by design) and a
    frame-time / GPU-load check that scanout/overlay survives outside the cursor box (load
    drops vs. full-output mode).

---

## 7. Scope boundaries (what this spec does NOT do)

- Does **not** fix trail smear / additive-white (roadmap 2.2 → item 3.2, dedicated feedback
  buffer). Out of scope.
- Does **not** add multi-pass/layers (3.3), screencast inclusion (2.3 / 3.5), winit support,
  shader-file hot-reload, or new uniforms (3.5). Out of scope.
- Does **not** touch output-transform handling (roadmap 2.4) — unconfirmed, separate.
- Phase C is the only part that may ship inert; A and B are the committed deliverable.

---

## 8. Build / test crib

Inherited verbatim from `docs/superpowers/global-shader-next-steps.md` §5 (dev shell, env
gaps, per-task `cargo check`, the insta-snapshot workaround, the flake deploy to "sixseven",
and KMS-only recording). See that section; not duplicated here.

---

## 9. Open questions for the plan phase

- Exact storage location of the capability flags (program wrapper vs. `Shaders` registry) —
  decide after reading current `custom_global` storage in `shaders/mod.rs`.
- Whether the element `Id` is stable per frame (affects whether the commit-counter bump is
  load-bearing or belt-and-suspenders for Phase B damage).
- Whether `unfinished_animations_remain` is the sole scheduling chokepoint or a direct
  `queue_redraw` is also needed for the very first idle frame after the shader activates.
- Phase C uv-remap mechanism: extra uniform (box origin/extent) vs. prelude-side transform —
  pick whichever keeps the author-facing contract byte-identical.
