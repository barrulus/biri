# Consolidated Carousel Overview — Phase 1 (Foundation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the config, overview-scoping, and carousel-state foundation for the consolidated carousel overview, and de-risk the rendering via a scoped spike — without yet drawing sibling cards.

**Architecture:** A config-gated mode makes the focused output a "lens." Phase 1 adds the config block, scopes the overview to the focused output only (leaving other physical monitors live), adds carousel state (which output is centered + regime derived from zoom vs a threshold), and runs a throwaway prototype to answer the one open rendering question (output-scale mismatch). Rendering the cards and the input rebinding are deliberately deferred to a Phase 2 plan written after the spike, because their exact code depends on the spike's outcome.

**Tech Stack:** Rust (nightly, via devshell — no `+nightly`), `knuffel` KDL config derive, `insta` for config tests. Smithay render elements (`RescaleRenderElement` / `RelocateRenderElement` / `CropRenderElement`) — used in Phase 2, not here.

## Global Constraints

- Build only via the devshell; nightly is default — never pass `+nightly`.
- No AI attribution in commits or PRs; no `Co-Authored-By` lines.
- Config tests must use focused `Config::parse_mem(...)` + field asserts (mirror
  `shader_animation_max_fps_parses` in `niri-config/src/lib.rs`). Do NOT add giant
  inline `assert_debug_snapshot!` blocks — `cargo insta` can hang and the inline
  snapshots in `lib.rs` are already enormous.
- Default `activation-zoom` value: `0.25`.
- Honor `isolated`: isolated outputs never host or appear in the carousel (reuse
  the existing gate; do not add a parallel exclusion path).
- Reference: design doc `docs/superpowers/specs/2026-07-20-consolidated-carousel-overview-design.md`.

---

### Task 1: Config — `consolidated-carousel` block

**Files:**
- Modify: `niri-config/src/misc.rs:121-166` (the `Overview` struct, its `Default`, `OverviewPart`, and `MergeWith`)
- Test: `niri-config/src/lib.rs` (add a focused test near `shader_animation_max_fps_parses`, ~line 2547)

**Interfaces:**
- Produces: `niri_config::Overview::consolidated_carousel: Option<ConsolidatedCarousel>`, where `pub struct ConsolidatedCarousel { pub activation_zoom: f64 }`. Later tasks read `options.overview.consolidated_carousel.is_some()` (mode enabled) and `.activation_zoom` (threshold).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `niri-config/src/lib.rs`:

```rust
#[test]
fn consolidated_carousel_parses() {
    // Enabled with explicit threshold.
    let enabled = Config::parse_mem(
        "overview {\n    consolidated-carousel {\n        activation-zoom 0.2\n    }\n}\n",
    )
    .unwrap();
    let cc = enabled
        .overview
        .consolidated_carousel
        .expect("block present => Some");
    assert_eq!(cc.activation_zoom, 0.2);

    // Enabled, threshold omitted => default 0.25.
    let defaulted =
        Config::parse_mem("overview {\n    consolidated-carousel {}\n}\n").unwrap();
    assert_eq!(
        defaulted.overview.consolidated_carousel.unwrap().activation_zoom,
        0.25
    );

    // Absent => None (default global overview unchanged).
    let disabled = Config::parse_mem("").unwrap();
    assert!(disabled.overview.consolidated_carousel.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p niri-config consolidated_carousel_parses`
Expected: FAIL — `no field consolidated_carousel on type Overview` (compile error).

- [ ] **Step 3: Add the config types and wiring**

In `niri-config/src/misc.rs`, add the `consolidated_carousel` field to `Overview` (after `zoom_presets`):

```rust
pub struct Overview {
    pub zoom: f64,
    pub backdrop_color: Color,
    pub workspace_shadow: WorkspaceShadow,
    /// Optional zoom presets for cycling. If None/empty, zoom cycling is disabled.
    pub zoom_presets: Option<Vec<f64>>,
    /// When Some, the consolidated carousel mode is enabled: the overview
    /// applies to the focused output only, and zooming past `activation_zoom`
    /// reveals sibling outputs as a carousel.
    pub consolidated_carousel: Option<ConsolidatedCarousel>,
}
```

In the `Default for Overview` impl, add `consolidated_carousel: None,`.

Add the resolved and part structs (after `ZoomPresets`, before `OverviewPart`):

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsolidatedCarousel {
    pub activation_zoom: f64,
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct ConsolidatedCarouselPart {
    #[knuffel(child, unwrap(argument))]
    pub activation_zoom: Option<FloatOrInt<0, 1>>,
}
```

Add the child to `OverviewPart` (after `zoom_presets`):

```rust
    #[knuffel(child)]
    pub consolidated_carousel: Option<ConsolidatedCarouselPart>,
```

Extend `MergeWith<OverviewPart> for Overview::merge_with` (after the `zoom_presets` block):

```rust
        if let Some(cc) = &part.consolidated_carousel {
            self.consolidated_carousel = Some(ConsolidatedCarousel {
                activation_zoom: cc.activation_zoom.map_or(0.25, |v| v.0),
            });
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p niri-config consolidated_carousel_parses`
Expected: PASS.

- [ ] **Step 5: Guard against snapshot fallout**

Run: `cargo test -p niri-config`
Expected: PASS. If a pre-existing inline snapshot test fails only because the
`Overview` debug output gained a `consolidated_carousel: None` line, accept that
one mechanical change (edit the inline snapshot text directly — do NOT run
`cargo insta accept`, which can hang). If unrelated snapshots fail, they are the
known pre-existing breakage (see project memory) — leave them.

- [ ] **Step 6: Commit**

```bash
git add niri-config/src/misc.rs niri-config/src/lib.rs
git commit -m "config: add overview consolidated-carousel block"
```

---

### Task 2: Focused-output-only overview scoping

**Files:**
- Modify: `src/layout/mod.rs:4641-4651` (`set_monitors_overview_state`)
- Modify: `src/layout/mod.rs:2546` (the overview invariant assertion)
- Test: `src/layout/mod.rs` (add a unit test in the existing layout `tests` module)

**Interfaces:**
- Consumes: `niri_config::Overview::consolidated_carousel` from Task 1.
- Produces: behavioral guarantee — when `consolidated_carousel.is_some()`, only the active monitor has `overview_open == true`; every other non-active monitor has `overview_open == false` even while the layout-level `self.overview_open == true`.

- [ ] **Step 1: Write the failing test**

Find how existing layout unit tests construct a `Layout` with multiple outputs
(search `src/layout/mod.rs` tests module for `add_output` / a test helper that
builds a `Layout<TestWindow>` with two outputs). Mirror that setup. Add:

```rust
#[test]
fn consolidated_mode_scopes_overview_to_active_output() {
    // Build a layout with two non-isolated outputs and consolidated mode on.
    // (Use the same construction helper the neighbouring tests use; set
    //  options.overview.consolidated_carousel = Some(ConsolidatedCarousel {
    //  activation_zoom: 0.25 }) before adding outputs.)
    let mut layout = /* helper building two outputs, active_monitor_idx = 0 */;

    layout.toggle_overview(); // opens overview

    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(mons[0].overview_open, "active output enters overview");
    assert!(!mons[1].overview_open, "non-active output stays live");
}
```

> Note for the implementer: if no two-output test helper exists, add a minimal
> one alongside this test rather than inflating the test with setup — but check
> first; the layout tests already build multi-output fixtures.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p niri consolidated_mode_scopes_overview_to_active_output`
Expected: FAIL — `mons[1].overview_open` is `true` (current code opens overview
on all non-isolated monitors).

- [ ] **Step 3: Scope the overview to the active output**

Replace `set_monitors_overview_state` (`src/layout/mod.rs:4641`):

```rust
    pub fn set_monitors_overview_state(&mut self) {
        let consolidated = self.options.overview.consolidated_carousel.is_some();
        let overview_open = self.overview_open;
        let progress = self.overview_progress.clone();

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return;
        };
        let active_idx = *active_monitor_idx;

        for (idx, mon) in monitors.iter_mut().enumerate() {
            // Isolated outputs never enter the overview. In consolidated-carousel
            // mode the overview is a lens on the focused output only, so every
            // other physical output stays live.
            mon.overview_open =
                overview_open && !mon.isolated && (!consolidated || idx == active_idx);
            mon.set_overview_progress(progress.as_ref());
        }
    }
```

> `OverviewProgress` must be `Clone` for `progress` above. Confirm the enum at
> `src/layout/mod.rs:548` derives `Clone`; if not, add `#[derive(Clone)]` to it
> (it wraps `Animation`/gesture values that are already `Clone`). Do this in this
> step if needed.

- [ ] **Step 4: Fix the overview invariant**

The invariant at `src/layout/mod.rs:2546` asserts every monitor mirrors the
layout's `overview_open`. That is already technically too strict for isolated
outputs and becomes wrong under consolidated mode. Replace line 2546:

```rust
            let consolidated = self.options.overview.consolidated_carousel.is_some();
            let expect_open =
                self.overview_open && !monitor.isolated && (!consolidated || idx == primary_idx);
            assert_eq!(expect_open, monitor.overview_open);
```

> Confirm `primary_idx` is the active-monitor index in this scope (it is used
> just below at the `idx == primary_idx` branch). If the loop variable name
> differs, use the active index in scope.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p niri consolidated_mode_scopes_overview_to_active_output`
Expected: PASS.
Run: `cargo test -p niri -- layout` (exercise the invariant checks in other
layout tests).
Expected: PASS — no invariant panics.

- [ ] **Step 6: Commit**

```bash
git add src/layout/mod.rs
git commit -m "layout: scope overview to focused output in consolidated mode"
```

---

### Task 3: Carousel state + regime derivation

**Files:**
- Modify: `src/layout/mod.rs` (add carousel state fields near the overview state at `:361-367`; add an accessor)
- Test: `src/layout/mod.rs` (unit test in the layout `tests` module)

**Interfaces:**
- Consumes: `options.overview.consolidated_carousel.activation_zoom`; `Layout::overview_zoom()` (`src/layout/mod.rs:2402`).
- Produces:
  - `Layout::carousel_centered_output_idx: usize` (which output the carousel is centered on; defaults to the active monitor index).
  - `Layout::in_carousel_regime(&self) -> bool` — true when consolidated mode is on, overview is open, and the current overview zoom is `<= activation_zoom`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn carousel_regime_tracks_zoom_threshold() {
    // Two outputs, consolidated mode on with activation_zoom = 0.25.
    let mut layout = /* same two-output helper as Task 2 */;
    layout.toggle_overview();

    // Above threshold: single-output overview, not carousel.
    layout.set_overview_zoom_for_test(0.5);
    assert!(!layout.in_carousel_regime());

    // At/below threshold: carousel regime.
    layout.set_overview_zoom_for_test(0.2);
    assert!(layout.in_carousel_regime());
}
```

> `set_overview_zoom_for_test` is a `#[cfg(test)]` helper you add in this step
> that forces the active monitor's `overview_zoom_target` (see
> `src/layout/monitor.rs:1417` `set_zoom_target_no_anim`) so the test controls
> the zoom deterministically. Prefer reusing an existing setter if present.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p niri carousel_regime_tracks_zoom_threshold`
Expected: FAIL — `no method in_carousel_regime` (compile error).

- [ ] **Step 3: Add the state and accessor**

Add near the overview fields (`src/layout/mod.rs:361-367`):

```rust
    /// Which output the consolidated carousel is centered on. Meaningful only
    /// while in the carousel regime; defaults to the active monitor.
    carousel_centered_output_idx: usize,
```

Initialise it to `0` in every `Layout` constructor that sets `overview_open`
(the two spots near `:707` and `:732`).

Add the accessor (near `overview_zoom` at `:2402`):

```rust
    pub fn in_carousel_regime(&self) -> bool {
        let Some(cc) = self.options.overview.consolidated_carousel else {
            return false;
        };
        self.overview_open && self.overview_zoom() <= cc.activation_zoom
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p niri carousel_regime_tracks_zoom_threshold`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/layout/mod.rs
git commit -m "layout: add carousel centered-output state and regime accessor"
```

---

### Task 4: SPIKE — output-scale-mismatch rendering prototype

This is a **research task**, not shippable code. Its deliverable is a written
findings note that unblocks the Phase 2 render plan. Timebox: one sitting.

**Files:**
- Create: `docs/superpowers/specs/2026-07-20-consolidated-carousel-scale-spike.md` (findings)
- Throwaway code on a scratch branch `spike/carousel-scale` — NOT merged.

**Question to answer:** In `render_inner` (`src/niri.rs:4849-4984`), can a sibling
monitor's workspace elements — built with the sibling's own `view_size`/scale via
the `render_workspaces` path (`src/layout/monitor.rs:1828`) — be composited into
the **host** output's element list at an arbitrary card position/scale using only
`RescaleRenderElement` + `RelocateRenderElement` + `CropRenderElement`, when the
host and sibling output scales differ (e.g. host 1.0, sibling 1.5)?

- [ ] **Step 1: Branch**

```bash
git switch -c spike/carousel-scale
```

- [ ] **Step 2: Prototype**

In `render_inner`, when the host output is in `in_carousel_regime()`, pick ONE
sibling monitor (`self.layout.monitors()` excluding host + isolated + empty) and
composite its `render_workspaces` output into the host canvas at a hardcoded
card rect (e.g. left 25% of the screen, scaled 0.3), wrapped in
`RescaleRenderElement` → `RelocateRenderElement` → `CropRenderElement`. Hardcode
everything; correctness of the transform math is the only goal.

- [ ] **Step 3: Observe on hardware**

Build via the devshell and run. Verify against the project's dual-GPU notes
(ultrawide DP-2 on dGPU; watch nvtop + intel_gpu_top together). Record:
- Does the sibling card render at the correct size/position, or is it offset/
  mis-scaled when host and sibling output scales differ?
- What coordinate space does `CropRenderElement` expect (host-logical vs
  buffer), and does culling in `workspaces_with_render_geo`
  (`src/layout/monitor.rs:1662`, culls against the *sibling's own* `view_size`)
  drop content that should appear in the card?
- Frame latency on the NVIDIA path — any regression vs normal overview?

- [ ] **Step 4: Write findings**

Fill `docs/superpowers/specs/2026-07-20-consolidated-carousel-scale-spike.md`
with: the exact transform order that worked, how output-scale is reconciled, the
crop coordinate space, whether per-card culling rects are needed, and the
measured latency. This is the input to the Phase 2 render plan.

- [ ] **Step 5: Discard the prototype code, keep the findings**

```bash
git switch barrulus-custom
git branch -D spike/carousel-scale   # after cherry-picking nothing; findings doc is on barrulus-custom
```

> Commit ONLY the findings doc to `barrulus-custom`:
> ```bash
> git add docs/superpowers/specs/2026-07-20-consolidated-carousel-scale-spike.md
> git commit -m "docs: findings from carousel scale-mismatch spike"
> ```

---

## Deferred to Phase 2 (write after Task 4 findings)

These are intentionally NOT specified with exact code here, because their
implementation is determined by the spike outcome. Phase 2 will cover:

1. **Render sibling cards (direct compositing).** Using the spike's confirmed
   transform, composite all participating siblings (non-host, non-isolated,
   non-empty) into the host's `render_inner` as tucked, dimmed, edge-faded cards
   in the carousel regime; render the centered target's full overview when zoomed
   back in above the threshold. No offscreen buffers.
2. **Dimming + edge-fade** of non-centered cards.
3. **Input regime rebinding.** `←`/`→` and `Shift`+scroll drive
   `carousel_centered_output_idx` (slide) below the threshold, and window
   selection above it — decided in `src/input/mod.rs` around the existing
   overview/zoom actions (`:2294-2400`).
4. **Focus-jump on close.** Selecting a window on the centered target output and
   closing the overview activates that window and moves focus to its physical
   output.
5. **Cross-output redraw dependency** (design risk #2): schedule host redraws
   when a visible sibling's content changes, or document the accepted v1 cadence.
6. **Slide-in animation** at the threshold crossing.

## Self-Review

- **Spec coverage (Phase 1 scope):** config block (Task 1) ✓; focused-output-only
  overview / "other outputs stay live" (Task 2) ✓; `isolated` honored (Tasks 1–2
  reuse the existing gate) ✓; participation-by-emptiness and card rendering are
  Phase 2 (Task 4 spike de-risks them) ✓; activation threshold state (Task 3) ✓.
  Render, dimming, input, focus-jump, cross-output redraw, animation — explicitly
  deferred to Phase 2 with a written handoff, not dropped.
- **Placeholder scan:** the only non-code steps are in Task 4, which is a
  declared spike whose deliverable is a findings doc — appropriate, not a
  placeholder. Tasks 1–3 contain complete code.
- **Type consistency:** `consolidated_carousel: Option<ConsolidatedCarousel>` and
  `activation_zoom: f64` used identically across Tasks 1–3;
  `carousel_centered_output_idx: usize` and `in_carousel_regime()` defined in
  Task 3 and referenced by Phase 2.
