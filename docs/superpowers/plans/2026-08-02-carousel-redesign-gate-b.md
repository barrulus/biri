# Carousel Cover-Flow Redesign — Gate B Plan (core model + panel rendering)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the card/lens carousel rendering with the cover-flow model: continuous zoom-driven reveal, physical-placement ring, perspective panels from damage-gated retained offscreens — landing every Gate A watch-list fix.

**Architecture:** Rewrite `src/layout/carousel.rs` as the pure ring/placement module; replace `carousel_centered_output_idx` with continuous `rotation` + `reveal_progress` derived from zoom; per-output `PanelSource` (retained `OffscreenRenderElement` for real damage gating) filled by a once-per-frame prepass outside `render_inner`; center stays the LIVE host overview when settled on host — textures are only for side panels, mid-rotation, and the settled-remote lens.

**Tech Stack:** Rust, smithay, existing `panel_quad`/`PanelRenderElement`/`ProgramType::Panel` from the spike.

**Spec:** `docs/superpowers/specs/2026-08-01-carousel-redesign-design.md`. **Spike findings (binding):** `docs/superpowers/specs/2026-08-01-carousel-spike-findings.md`.

## Global Constraints

- Build/test in the repo devshell; never `+nightly`; never `cargo insta` (edit inline snapshots by hand; pending snaps land in `.lib.rs.pending-snap`). Do NOT use `--release` cargo profiles (cache-cold build scripts hit the libdisplay-info 0.4.0 incompat); runtime binaries come from the VM nix build.
- Config values: `reveal-zoom` default **0.4**, `assembled-zoom` default **0.15**; validation `0 < assembled-zoom < reveal-zoom < 1`, invalid → warn + both defaults. `expand-zoom` is REMOVED.
- HARD INVARIANT (spec): panel-content rendering (which locks other outputs' layer maps) runs ONLY in the prepass, never inside `render_inner`. Enforced by a thread-local debug assertion (Task 5).
- Damage gating must be REAL (spike finding 1): retain the `OffscreenRenderElement`, never a bare `GlesTexture` clone across frames.
- Zero rendering-behavior change while `overview.consolidated-carousel` is not configured.
- Commits in repo style (`layout:`, `render:`, `config:`, `niri:` prefixes), no AI attribution, no Co-Authored-By.

---

### Task 1: Math test extensions (spike review watch-list)

**Files:**
- Modify: `src/render_helpers/panel_quad.rs` (test module only)

**Interfaces:** none new — pins existing behavior for Gate B's consumers.

- [ ] **Step 1: Write two failing-or-passing tests (they pin behavior; TDD RED not expected here, both should pass immediately — if either fails, STOP and report, the math is wrong)**

Extend `sampling_matrix_maps_bbox_corners_to_uv` to pin ALL FOUR corners:

```rust
#[test]
fn sampling_matrix_maps_bbox_corners_to_uv() {
    let quad = tilted_panel_corners((50., 50.), (60., 40.), 0.4, 200.);
    let m = sampling_matrix(&quad).unwrap();
    let (bx, by, bw, bh) = bounding_box(&quad);
    let expected = [(0., 0.), (1., 0.), (1., 1.), (0., 1.)];
    for (i, (eu, ev)) in expected.iter().enumerate() {
        let p = glam::Vec3::new(
            ((quad[i][0] - bx) / bw) as f32,
            ((quad[i][1] - by) / bh) as f32,
            1.0,
        );
        let s = m * p;
        assert!((s.x / s.z - eu).abs() < 1e-4, "corner {i} u");
        assert!((s.y / s.z - ev).abs() < 1e-4, "corner {i} v");
    }
}
```

Add a negative-yaw test (left-stack panels use negative yaw — no coverage today):

```rust
#[test]
fn negative_yaw_recedes_left_edge() {
    let c = tilted_panel_corners((0., 0.), (100., 60.), -0.5, 300.);
    let left_h = c[3][1] - c[0][1];
    let right_h = c[2][1] - c[1][1];
    assert!(left_h < right_h, "left edge must be shorter: {left_h} vs {right_h}");
    assert!(c[0][0] > -50., "left edge pulled toward center: {}", c[0][0]);
    assert!(right_h > 60., "near (right) edge magnifies vertically: {right_h}");
}
```

- [ ] **Step 2: Run** `cargo test -p niri --lib panel_quad 2>&1 | tail -5` — expect 7 passed.
- [ ] **Step 3: Commit** — `git add src/render_helpers/panel_quad.rs && git commit -m "render: pin all panel quad corners and negative yaw in tests"`

---

### Task 2: Config — reveal-zoom / assembled-zoom

**Files:**
- Modify: `niri-config/src/misc.rs:151-193` (`ConsolidatedCarousel`, `ConsolidatedCarouselPart`, `Overview::merge_with`)
- Modify: `niri-config/src/lib.rs` — tests `consolidated_carousel_parses` (~2559), `consolidated_carousel_expand_zoom_parses` (~2585), and the big `Config` debug snapshot (~line 1760, `consolidated_carousel: None` line context)
- Modify: `resources/default-config.kdl` (commented example, if it mentions activation-zoom/expand-zoom)

**Interfaces:**
- Produces: `pub struct ConsolidatedCarousel { pub reveal_zoom: f64, pub assembled_zoom: f64 }` — consumed by Tasks 4/6 as `options.overview.consolidated_carousel`.

- [ ] **Step 1: Update the structs and merge**

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsolidatedCarousel {
    pub reveal_zoom: f64,
    pub assembled_zoom: f64,
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct ConsolidatedCarouselPart {
    #[knuffel(child, unwrap(argument))]
    pub reveal_zoom: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub assembled_zoom: Option<FloatOrInt<0, 1>>,
}
```

In `merge_with` (replacing the `if let Some(cc)` block at misc.rs:186-191):

```rust
if let Some(cc) = &part.consolidated_carousel {
    let reveal = cc.reveal_zoom.map_or(0.4, |v| v.0);
    let assembled = cc.assembled_zoom.map_or(0.15, |v| v.0);
    let (reveal_zoom, assembled_zoom) = if assembled > 0. && assembled < reveal && reveal < 1. {
        (reveal, assembled)
    } else {
        warn!(
            "overview.consolidated-carousel requires 0 < assembled-zoom < reveal-zoom < 1 \
             (got assembled={assembled}, reveal={reveal}); using defaults 0.15 / 0.4"
        );
        (0.4, 0.15)
    };
    self.consolidated_carousel = Some(ConsolidatedCarousel { reveal_zoom, assembled_zoom });
}
```

(Confirm `warn!` is in scope in misc.rs — other merge blocks in niri-config log via tracing; mirror whatever this file/crate already uses, adding the `use` if the crate exports it.)

- [ ] **Step 2: Update the two dedicated tests** — rename the second to `consolidated_carousel_assembled_zoom_parses`; KDL nodes become `reveal-zoom 0.5` / `assembled-zoom 0.2`; expected debug output uses the new field names. Add a third test `consolidated_carousel_invalid_band_falls_back` asserting that `reveal-zoom 0.1` + `assembled-zoom 0.5` yields the defaults `0.4` / `0.15`.
- [ ] **Step 3: Fix the big Config snapshot by hand** (the `consolidated_carousel: None` line is unchanged in the default config — verify no other field ordering shifted; do NOT run cargo insta).
- [ ] **Step 4: Run** `cargo test -p niri-config consolidated 2>&1 | tail -6` — 3 tests pass; then `cargo test -p niri-config parse 2>&1 | tail -4` for the snapshot test.
- [ ] **Step 5: Commit** — `config: replace carousel activation/expand-zoom with reveal/assembled-zoom`

---

### Task 3: Ring + placements module (rewrite `src/layout/carousel.rs`)

**Files:**
- Rewrite: `src/layout/carousel.rs` (delete `CardPlacement`, `carousel_card_layout`, `carousel_centered_layout` and their 5 tests; the module becomes the pure cover-flow geometry)

**Interfaces (consumed by Tasks 4/6 and Gate C):**

```rust
pub struct PanelPlacement {
    pub center: (f64, f64), // logical position of panel center on the host view
    pub size: (f64, f64),   // logical panel size before tilt
    pub yaw: f64,           // radians; positive recedes the RIGHT edge
    pub dim: f32,
    pub z: f64,             // draw order: higher = nearer the viewer
}
/// Signed ring position per output (same order as `outputs`): host gets 0.0,
/// outputs physically left of host get -1.0, -2.0... (nearest first),
/// right likewise +1.0... Sort key: (x, y) of Output::current_location().
pub fn ring_positions(positions: &[(i32, i32)], host_idx: usize) -> Vec<f64>;
/// Placement for one output at signed slot-delta d = ring_pos - rotation,
/// assembled-ness `reveal` in [0,1]. `None` when fully off-screen.
pub fn panel_placement(
    view: (f64, f64),
    d: f64,
    reveal: f64,
) -> Option<PanelPlacement>;
```

Tuned constants (module-level, NOT config — spec's YAGNI decision):

```rust
pub const FOCAL_FACTOR: f64 = 1.5;   // focal = FOCAL_FACTOR * view.w
const CENTER_FRACTION: f64 = 0.72;   // center panel height / view height at reveal=1
const SIDE_FRACTION: f64 = 0.52;     // first side panel height / view height
const SIDE_STEP_SCALE: f64 = 0.85;   // per extra depth
const SIDE_YAW: f64 = 0.9;           // radians at depth 1
const SIDE_YAW_STEP: f64 = 0.2;
const SIDE_X: f64 = 0.36;            // first side panel center offset, fraction of view.w
const SIDE_X_STEP: f64 = 0.09;
const SIDE_DIM: f32 = 0.78;
const SIDE_DIM_STEP: f32 = 0.85;
const MAX_VISIBLE_DEPTH: f64 = 4.0;  // beyond this, panel_placement returns None
```

- [ ] **Step 1: Write failing tests** (new test module; the old tests are deleted with the old code):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_orders_by_physical_x_host_zero() {
        // outputs at x: 0 (host), -1920 (left), 1920 (right), 3840 (far right)
        let pos = [(0, 0), (-1920, 0), (1920, 0), (3840, 0)];
        let ring = ring_positions(&pos, 0);
        assert_eq!(ring, vec![0.0, -1.0, 1.0, 2.0]);
    }

    #[test]
    fn ring_vertical_stack_degrades_by_y() {
        // same x, stacked: above host -> one side, below -> other, by y order
        let pos = [(0, 0), (0, -1080), (0, 1080)];
        let ring = ring_positions(&pos, 0);
        assert_eq!(ring[0], 0.0);
        assert_eq!(ring[1], -1.0); // smaller y sorts first (left stack)
        assert_eq!(ring[2], 1.0);
    }

    #[test]
    fn center_slot_is_flat_and_undimmed() {
        let p = panel_placement((1920., 1080.), 0.0, 1.0).unwrap();
        assert_eq!(p.yaw, 0.0);
        assert_eq!(p.dim, 1.0);
        assert!((p.center.0 - 960.).abs() < 1e-6);
        assert!((p.size.1 - 1080. * 0.72).abs() < 1e-6);
    }

    #[test]
    fn side_slots_mirror_and_recede() {
        let l = panel_placement((1920., 1080.), -1.0, 1.0).unwrap();
        let r = panel_placement((1920., 1080.), 1.0, 1.0).unwrap();
        assert!(l.yaw < 0. && r.yaw > 0., "yaws mirror: {} {}", l.yaw, r.yaw);
        assert!((l.yaw + r.yaw).abs() < 1e-9);
        assert!(l.center.0 < 960. && r.center.0 > 960.);
        assert!(l.z < panel_placement((1920., 1080.), 0.0, 1.0).unwrap().z);
        let r2 = panel_placement((1920., 1080.), 2.0, 1.0).unwrap();
        assert!(r2.size.1 < r.size.1 && r2.z < r.z && r2.center.0 > r.center.0);
    }

    #[test]
    fn reveal_slides_side_panels_in_from_their_edge() {
        let assembled = panel_placement((1920., 1080.), 1.0, 1.0).unwrap();
        let half = panel_placement((1920., 1080.), 1.0, 0.5).unwrap();
        let start = panel_placement((1920., 1080.), 1.0, 0.0);
        assert!(half.center.0 > assembled.center.0, "mid-reveal sits further right");
        // at reveal 0 the panel is fully off-screen (or not placed at all)
        if let Some(p) = start {
            assert!(p.center.0 - p.size.0 / 2. >= 1920., "off-screen at reveal 0");
        }
    }

    #[test]
    fn fractional_rotation_interpolates_between_slots() {
        let settled = panel_placement((1920., 1080.), 0.0, 1.0).unwrap();
        let mid = panel_placement((1920., 1080.), -0.5, 1.0).unwrap();
        let side = panel_placement((1920., 1080.), -1.0, 1.0).unwrap();
        assert!(mid.yaw < 0. && mid.yaw > side.yaw);
        assert!(mid.center.0 < settled.center.0 && mid.center.0 > side.center.0);
    }

    #[test]
    fn beyond_max_depth_is_none() {
        assert!(panel_placement((1920., 1080.), 5.0, 1.0).is_none());
    }
}
```

- [ ] **Step 2: Run** `cargo test -p niri --lib layout::carousel 2>&1 | tail -5` — compile failure (functions missing).
- [ ] **Step 3: Implement.** `ring_positions`: sort indices of `positions` by `(x, y)`; the host's sorted index splits the order — outputs before it get `-(host_sorted_idx - their_sorted_idx)` reading nearest-first, after it `+(their_sorted_idx - host_sorted_idx)`. `panel_placement`: clamp/None beyond `MAX_VISIBLE_DEPTH`; compute the two integer slots around `|d|` and lerp all five parameters (linear); slot 0 = center (yaw 0, dim 1, size `CENTER_FRACTION`·view, centered, z = 100); slot k = side (sign of d picks mirror: LEFT panels get NEGATIVE yaw so their right edge — the near edge — magnifies toward center, matching the sketch); reveal lerps each side placement from its off-screen start (center.x at `±(view.w/2 + size.w)` from center, yaw at `±(SIDE_YAW + MAX·step)`) toward the slot value; center slot ignores reveal (the live-host rule — Task 6 — means the center texture panel only draws mid-rotation or on remote, always fully revealed). Keep every helper pure; aspect ratio of panels = view aspect (content letterboxing is the texture's job, Task 6).
- [ ] **Step 4: Run** the tests — 7 passed. Then `cargo check 2>&1 | tail -5` — EXPECT ERRORS in `src/niri.rs` (the deleted `carousel_centered_layout` caller at ~5023). That breakage is Task 6's job; to keep this task compile-green, keep a temporary `pub use` shim ONLY if trivial — otherwise reorder: it is acceptable for THIS task to leave `cargo check` broken ONLY if the commit message says so and Task 4 starts by confirming scope. Preferred: keep the old `carousel_centered_layout` + `CardPlacement` in place (marked `// GATE-B-TEMP: removed in render integration task`) so check stays green, and delete them in Task 6.
- [ ] **Step 5: Commit** — `layout: add cover-flow ring and panel placement geometry`

---

### Task 4: Layout state — continuous rotation + reveal

**Files:**
- Modify: `src/layout/mod.rs` — field `carousel_centered_output_idx` (~369, plus inits ~715/741), fns `carousel_outputs` (1815), `carousel_centered_output_idx` (1825), `reset_carousel_center` (1831), `slide_carousel` (1848), `in_carousel_regime` (2475), `in_carousel_lens` (2483), `verify_invariants` (~2631), `toggle_overview` (~4754), advance_animations (~2833)
- Modify: `src/layout/tests.rs` — the carousel tests at 3996-4553

**Interfaces:**
- Consumes: `ConsolidatedCarousel { reveal_zoom, assembled_zoom }` (Task 2), `ring_positions` (Task 3).
- Produces (consumed by Task 6 + Gate C):
  - `pub fn carousel_rotation(&self) -> f64` — current animated value.
  - `pub fn carousel_rotation_target(&self) -> f64` — settled target (integer ring position).
  - `pub fn rotate_carousel(&mut self, delta: isize)` — retarget by delta ring steps (replaces `slide_carousel`; clamps to ring extent, cyclic wrap preserved from old semantics).
  - `pub fn carousel_reveal(&self) -> f64` — 0 when not configured/not open; else `((reveal_zoom - zoom) / (reveal_zoom - assembled_zoom)).clamp(0., 1.)` using `self.overview_zoom()`.
  - `pub fn carousel_ring(&self) -> Vec<(usize, f64)>` — participating-monitor index (into `carousel_outputs()` order) with ring position, computed via `Output::current_location()` of each monitor's output.
  - `pub fn in_carousel_lens(&self) -> bool` — now: overview open && configured && rotation settled on a non-host output (replaces the zoom-band definition).
- State: `carousel_rotation: f64` + `carousel_rotation_anim: Option<Animation>` replace `carousel_centered_output_idx`. Animation uses the `overview_open_close` anim config (same as `toggle_overview` at mod.rs:4623-4629) — no new config.

- [ ] **Step 1: Verify output positions are readable from layout.** `rg "change_current_state" src/niri.rs` and confirm niri passes the output location (the "putting output … at x= y=" path, niri.rs ~3393 area). Then in layout, `monitor.output().current_location()` (or `output.current_location()` — check `Monitor` field access; the output is `mon.output`/`mon.output()`). If location is NOT set on the smithay Output global, STOP — report BLOCKED with what you found; the fallback design (threading positions through `Layout::add_output`) is a controller decision.
- [ ] **Step 2: Update tests first.** In `src/layout/tests.rs`: `carousel_regime_tracks_zoom_threshold` and `carousel_regime_and_lens_are_mutually_exclusive_bands` are rewritten to test `carousel_reveal()` against the new band (0 above reveal-zoom 0.4; 0.5 at zoom 0.275; 1.0 at/below 0.15 — use `set_overview_zoom_for_test`) and `in_carousel_lens()` false while rotation is 0. `slide_carousel_wraps_over_outputs` / `slide_carousel_single_output_is_noop` become `rotate_carousel_*` (wrap preserved: rotating right past the last ring position wraps to the leftmost; single output no-op). `carousel_outputs_includes_host_and_reset_centers_on_active` / `reset_carousel_center_noop_when_active_not_in_set` update to rotation-based equivalents (`reset` = rotation snapped to 0.0 instantly; the private-field write at tests.rs:4549 becomes `layout.carousel_rotation = 1.0`). Config fixtures change `activation_zoom`/`expand_zoom` to `reveal_zoom: 0.4, assembled_zoom: 0.15` struct literals.
- [ ] **Step 3: Run** `cargo test -p niri --lib carousel 2>&1 | tail -8` — failures/compile errors expected (RED).
- [ ] **Step 4: Implement** the state swap. `rotate_carousel` retargets `carousel_rotation_anim` from the current animated value to the new integer target (same retarget pattern as `Monitor::animate_zoom_to`, monitor.rs:1521-1536, but living on Layout with `self.clock`). Advance it in the same place monitor animations advance (mod.rs ~2833-2848); expose unfinished-animation status so redraws continue while rotating (mirror how `overview_progress` animation participates). `toggle_overview` open-branch: replace `reset_carousel_center()` with instant `carousel_rotation = 0.; carousel_rotation_anim = None;`. `verify_invariants` (~2631): the `idx == active_monitor_idx` clause becomes `rotation settled at 0.0 → host monitor is the overview one`. Keep `carousel_outputs()` as-is. Leave `carousel_participants` untouched (Gate C removes it).
- [ ] **Step 5: Run** carousel tests — green; then the neighboring overview tests: `cargo test -p niri --lib overview 2>&1 | tail -5`.
- [ ] **Step 6: Commit** — `layout: continuous carousel rotation and zoom-driven reveal`

---

### Task 5: Panel sources — damage-gated offscreens + retained elements

**Files:**
- Modify: `src/niri.rs` — `OutputState` (~553-612; REPLACE the two `panel_spike_*` fields), its construction (~3063), the spike prepass `update_panel_spike_texture` (~5299, replaced), `panel_spike_enabled()` (~50, removed in Task 6)
- Modify: `src/render_helpers/panel.rs` — add `update()` + stable-Id retention support

**Interfaces:**
- Produces (consumed by Task 6):
  - `struct PanelSource { offscreen: OffscreenBuffer, elem: Option<OffscreenRenderElement>, frame: u64 }` stored as `pub panel_source: RefCell<PanelSource>` on `OutputState` — the CONTENT output's state holds its own source.
  - `Niri::update_panel_sources(&self, ctx: &mut RenderCtx<GlesRenderer>, host: &Output, frame: u64)` — for each participating output (host included), if `source.frame != frame`: collect that output's panel content elements and call `offscreen.render(...)`; store the returned `OffscreenRenderElement` in `elem` (THE retained handle — never clone the raw texture; `is_unique_reference` stays true because `OffscreenBuffer` treats its own element's snapshot correctly, and damage gating engages). On error: `elem = None` (clear stale — spike review finding 2), warn. Set `frame` regardless. `frame` comes from a `Cell<u64>` counter on `Niri` bumped once per `redraw_queued_outputs` cycle (prepass may be reached twice via screencopy — the stamp makes it idempotent; spike review finding 4).
  - Panel content elements for output O: `monitor.render_overview_at_zoom(ctx.r(), fill_zoom(O), false, push)` PLUS O's Background+Bottom layer-shell surfaces and per-workspace `render_background()` — reuse the exact element-assembly recipe of the CURRENT lens block (niri.rs:5140-5188) minus the host-centering relocate (render at origin in O's own coordinates); `fill_zoom(O) = min(host_view/O_view axes) * (host_scale / O_scale)` exactly as computed at niri.rs:5112-5113. This code MOVES from `render_inner` into the prepass — the `layer_map_for_output(O)` lock is now taken outside any host render scope, which is the whole point.
  - `PanelRenderElement::update(&mut self, corners, texture_commit: CommitCounter, dim, scale, alpha)` mirroring `BorderRenderElement::update`'s params-compare-then-`ShaderRenderElement::update` pattern (border.rs:110-218), so retained elements keep stable `Id`s and only damage when the transform or content actually changed (spike review finding 3). Texture changes are signaled by comparing the `OffscreenRenderElement`'s `Element::current_commit` (a `CommitCounter`; compare via its `distance`/equality the way smithay's damage tracking does — store the last seen value in `Parameters`).
  - `_sync` fence decision (spike review triage): the prepass offscreen render and the panel draw run on the SAME GLES context, where command ordering is guaranteed, so the returned `SyncPoint` is deliberately not threaded — record this in a one-line code comment at the `offscreen.render` call site so the decision is visible where it matters. Revisit only if panels ever render cross-context (multi-GPU screencast of panels). Add a context-id guard in `draw` mirroring `OffscreenRenderElement::draw`'s (offscreen.rs:312-315): warn-and-skip on renderer context mismatch (spike review finding 2).
  - Host-side retention: `pub panel_elems: RefCell<HashMap<usize, PanelRenderElement>>` on the HOST's `OutputState`, keyed by ring index. Entries dropped when the ring shrinks.
- Also: thread-local `IN_RENDER_INNER: Cell<bool>` (file-scope in niri.rs); set true for the duration of `render_inner` (RAII guard or set/reset at entry/exit), `debug_assert!(!IN_RENDER_INNER.get())` at the top of `update_panel_sources` — the spec's invariant, now real.

- [ ] **Step 1:** Implement `PanelRenderElement::update` + context guard; `cargo check`.
- [ ] **Step 2:** Implement `PanelSource`, the frame counter, and `update_panel_sources` (replacing `update_panel_spike_texture`'s body; keep the spike env-gate calling it for now so behavior is still observable — Task 6 swaps the gate to config). The `Rc` around `OffscreenBuffer` is dropped (spike review: unnecessary).
- [ ] **Step 3:** `cargo check 2>&1 | tail -5` clean; `cargo test -p niri --lib panel_quad 2>&1 | tail -3` still 7/7.
- [ ] **Step 4: Commit** — `render: damage-gated panel sources with retained elements`

---

### Task 6: Render integration — panels replace cards and lens

**Files:**
- Modify: `src/niri.rs` — `render_inner`: DELETE the card block (5007-5094) and lens block (5096-5191); rework the host-suppression condition (4974-5000); replace the spike push (4870-4890); remove `panel_spike_enabled()` and the spike's `unfinished_animations_remain` OR (~5487-5490); `fill_xray_elements` gating (5202+); `Niri::render` prepass call site (4473-4475)
- Modify: `src/layout/carousel.rs` — delete the `GATE-B-TEMP` remnants (`CardPlacement`, `carousel_centered_layout`)
- Modify: `src/layout/monitor.rs` — `render_active_workspace_at_zoom` (2053): now unused by niri.rs; delete it and its doc comment (verify no other caller first: `rg render_active_workspace_at_zoom`)

**Interfaces:** consumes everything above. The drawing rule (THE core of this task):

```
let reveal = layout.carousel_reveal();
let rotation = layout.carousel_rotation();
let settled_on_host = rotation == rotation.round() && rotation.round() == 0.;
let rotating = <rotation animation ongoing>;
if reveal == 0. && settled_on_host { normal overview path, NO panels, NO prepass }
else:
  - prepass ran in Niri::render (gate its call on the same condition)
  - suppress the host's live workspace strip when !settled_on_host || rotating
    (reuse/replace the existing carousel_active/in_carousel_lens suppression at 4974-5000)
  - for each ring entry (idx, ring_pos), d = ring_pos - rotation:
      skip d == 0 when settled_on_host (live center);
      placement = carousel::panel_placement(view, d, reveal)  [None → skip]
      corners = panel_quad::tilted_panel_corners(placement.center, placement.size,
                    placement.yaw, view.w * carousel::FOCAL_FACTOR)
      panel_elems entry (retained) .update(corners, source elem commit, placement.dim,
                    scale, 1.0); push in z-order: sort placements by z DESCENDING and
                    push nearer panels FIRST (niri draws earlier-pushed on top)
  - backdrop stays the existing overview backdrop (already behind everything)
```

The settled-remote case (`rotation.round() != 0`, not rotating, reveal any) draws ONLY that output's panel at the center slot filling by its placement — this IS the lens; window-level interaction on it is Gate C.

- [ ] **Step 1:** Implement, keeping the diff surgical: the deleted card/lens blocks are ~180 lines; the new panel-stack block should be < 80. `fill_xray_elements`: skip xray layer re-render when panels are active (same condition as host suppression) — one guard, matching how the host strip is gated.
- [ ] **Step 2:** Delete the spike gate: `panel_spike_enabled`, the env-var read, the spike push, the spike redraw OR. Redraws while rotating are owed by the rotation animation's unfinished status (Task 4) — verify by reading, not assuming: the condition feeding `unfinished_animations_remain` must include the layout's rotation anim.
- [ ] **Step 3:** `cargo check 2>&1 | tail -5` clean; full carousel+overview test filters green; `rg "NIRI_PANEL_SPIKE|panel_spike" src/` returns nothing.
- [ ] **Step 4: Commit** — `render: cover-flow panel stack replaces carousel cards and lens`

---

### Task 7: VM verification checkpoint (STOP — needs the human)

**Controller-side prep (not the implementer):** add a second virtio display to the VM (`-device virtio-gpu-gl,max_outputs=2` replacing the single-head device in `~/dev/biri-vm/flake.nix`, plus a second `-display`/gtk tab as QEMU exposes it); remove the `NIRI_PANEL_SPIKE` env var from the VM flake; `rm -f ~/dev/biri-vm/flake.lock && nix build` and verify the closure's `niri-<rev>` matches HEAD.

**Human checks (record results in the ledger before Gate C):**
1. Single-output regressions: overview, zoom presets, shaders all behave; carousel inert without config; with config but one output, reveal does nothing visible and nothing crashes.
2. Two outputs: zooming below `reveal-zoom` slides the sibling panel in from its physical side, continuously with zoom; the host's own overview stays live and interactive in the center; sibling panel shows the sibling's full overview (wallpaper included), dimmed and tilted per the sketch.
3. `journalctl` clean; no freeze on overview open (the old deadlock scenario — sibling prepass takes the sibling's layer-map lock, which is exactly what the debug assertion now polices; run a debug build in the VM at least once to arm it).
4. Perf eyeball: settled carousel should be near-idle (damage gating now real — this is the observable difference from the spike).

Gate C's plan (`2026-08-02-carousel-redesign-gate-c.md`) executes only after this checkpoint records PASS.
