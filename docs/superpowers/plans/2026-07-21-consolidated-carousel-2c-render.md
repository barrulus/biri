# Consolidated Carousel — Phase 2c-render (Lens) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the deepest zoom regime — the **lens**: below a new `expand-zoom` threshold, the focused output renders the *centered* output's FULL overview (all workspaces/windows) filling the screen. (Clicking a window to jump is the separate 2c-jump phase.)

**Architecture:** A new `expand-zoom` config (deeper than `activation-zoom`) and an `in_carousel_lens()` predicate on the deeper band; `in_carousel_regime()` is tightened to the carousel band only, so carousel and lens are mutually exclusive. A new `Monitor::render_overview_at_zoom` renders a monitor's full overview at a caller-forced zoom (siblings zoom in lockstep with the host, so a forced fill zoom is required). `render_inner` gains a lens branch that renders the centered output's overview filling the host view, suppressing both the host's own overview and the carousel cards. Pure logic (config, predicates) unit-tested; the render is compile-green + hardware-verified.

**Tech Stack:** Rust (nightly via devshell, no `+nightly`). Reuses `scale_relocate_crop`, `OutputRenderElements::CarouselCard`. No offscreen buffers.

## Global Constraints

- Build/test only via the devshell (`direnv exec . bash -c 'cargo ...'`); nightly default — never `+nightly`.
- No AI attribution; no `Co-Authored-By`.
- **No offscreen buffers** (carried).
- `expand-zoom` default `0.1`; it must be `< activation-zoom` for the lens to be a deeper band than the carousel. `in_carousel_lens()` = consolidated + `overview_open` + `overview_zoom() <= expand_zoom`. `in_carousel_regime()` (tightened) = consolidated + `overview_open` + `expand_zoom < overview_zoom() <= activation_zoom` — **mutually exclusive with the lens.**
- The lens renders the **centered** output (`carousel_outputs()[carousel_centered_output_idx()]`), scale-normalized, no offscreen.
- Reference: 2c design `docs/superpowers/specs/2026-07-21-consolidated-carousel-2c-design.md`.

### Key code anchors (verified by research)

- Config: `ConsolidatedCarousel` struct `niri-config/src/misc.rs:151-154`; `ConsolidatedCarouselPart` `:156-160`; merge `:183-186`; `Option<ConsolidatedCarousel>` field default `None` `:131/:141`. Parse-test pattern: `consolidated_carousel_parses` in `niri-config/src/lib.rs`.
- `Layout::in_carousel_regime()` `src/layout/mod.rs:2475-2480`; `Layout::overview_zoom()` (active monitor's) `:2465-2473`. Existing test `carousel_regime_tracks_zoom_threshold` in `src/layout/tests.rs`.
- `Monitor::render_workspaces` `src/layout/monitor.rs:1838-1917` (final rescale zoom at `:1872`); `workspaces_render_geo` `:1641-1670` (bakes `self.overview_zoom()` at `:1643` into `ws_size`/`gap`/`static_offset`/`first_ws_y`); `workspaces_with_render_geo` `:1672-1681`; leaf helpers already take zoom: `workspace_size(zoom)` `:1390`, `workspace_gap(zoom)` `:1396`. Template: `render_active_workspace_at_zoom` `:1925-1965`.
- `Monitor::view_size()` `:2260-2262`, `scale()` `:2256-2258`.
- `render_inner` `src/niri.rs:4471`; `carousel_active` `:4915-4916`; host strip guard `if !carousel_active` at `:4969` (host `render_workspaces` at `:4977`); carousel block `if carousel_active` `:4999-5084`; `render_above_top_layer` branch has its own ungated `render_workspaces` at `:4930` (provably unreachable in-regime, per 2b review). `scale_relocate_crop` `:7389-7399`. `carousel_outputs()` `src/layout/mod.rs:1818`, `carousel_centered_output_idx()` `:1827`.

---

### Task 1: Config — `expand-zoom` (TDD)

**Files:** Modify `niri-config/src/misc.rs`; test in `niri-config/src/lib.rs`.

**Interfaces — Produces:** `ConsolidatedCarousel::expand_zoom: f64` (default `0.1`).

- [ ] **Step 1: Write the failing test** in the `tests` module of `niri-config/src/lib.rs`:

```rust
#[test]
fn consolidated_carousel_expand_zoom_parses() {
    let cfg = Config::parse_mem(
        "overview {\n    consolidated-carousel {\n        activation-zoom 0.25\n        expand-zoom 0.08\n    }\n}\n",
    )
    .unwrap();
    let cc = cfg.overview.consolidated_carousel.unwrap();
    assert_eq!(cc.activation_zoom, 0.25);
    assert_eq!(cc.expand_zoom, 0.08);

    // Omitted -> default 0.1.
    let d = Config::parse_mem("overview {\n    consolidated-carousel {}\n}\n").unwrap();
    assert_eq!(d.overview.consolidated_carousel.unwrap().expand_zoom, 0.1);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri-config consolidated_carousel_expand_zoom'`
Expected: FAIL — `no field expand_zoom`.

- [ ] **Step 3: Implement**

In `niri-config/src/misc.rs`: add `pub expand_zoom: f64,` to `ConsolidatedCarousel` (after `activation_zoom`); add `#[knuffel(child, unwrap(argument))] pub expand_zoom: Option<FloatOrInt<0, 1>>,` to `ConsolidatedCarouselPart`; in the merge block add `expand_zoom: cc.expand_zoom.map_or(0.1, |v| v.0),` to the struct literal.

- [ ] **Step 4: Run to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri-config'`
Expected: PASS. If a pre-existing inline snapshot gains an `expand_zoom` line, accept that one mechanical edit inline (do NOT run `cargo insta accept`).

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/misc.rs niri-config/src/lib.rs
git commit -m "config: add overview consolidated-carousel expand-zoom"
```

---

### Task 2: `in_carousel_lens()` + tighten `in_carousel_regime()` (TDD)

**Files:** Modify `src/layout/mod.rs`; test in `src/layout/tests.rs`.

**Interfaces — Produces:** `Layout::in_carousel_lens(&self) -> bool`; redefined `in_carousel_regime()` (carousel band only).

- [ ] **Step 1: Write the failing test**

Extend/add near `carousel_regime_tracks_zoom_threshold` (reuse its fixture; consolidated mode on, activation 0.25, expand 0.1):
```rust
#[test]
fn carousel_regime_and_lens_are_mutually_exclusive_bands() {
    let mut layout = /* same fixture; consolidated_carousel = Some { activation_zoom: 0.25, expand_zoom: 0.1 } */;
    layout.toggle_overview();

    layout.set_overview_zoom_for_test(0.5);  // above activation
    assert!(!layout.in_carousel_regime() && !layout.in_carousel_lens());

    layout.set_overview_zoom_for_test(0.2);  // carousel band
    assert!(layout.in_carousel_regime() && !layout.in_carousel_lens());

    layout.set_overview_zoom_for_test(0.05); // lens band
    assert!(!layout.in_carousel_regime() && layout.in_carousel_lens());
}
```
> If the existing `carousel_regime_tracks_zoom_threshold` test asserts `in_carousel_regime()` true at a zoom `<= expand_zoom`, update it to the new band semantics (it uses 0.2/0.25, both in the carousel band with expand 0.1, so it should still pass — verify).

- [ ] **Step 2: Run to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri carousel_regime_and_lens'`
Expected: FAIL — `no method in_carousel_lens`.

- [ ] **Step 3: Implement**

Replace `in_carousel_regime` and add `in_carousel_lens` (`src/layout/mod.rs`):
```rust
pub fn in_carousel_regime(&self) -> bool {
    let Some(cc) = self.options.overview.consolidated_carousel else {
        return false;
    };
    let zoom = self.overview_zoom();
    self.overview_open && zoom <= cc.activation_zoom && zoom > cc.expand_zoom
}

pub fn in_carousel_lens(&self) -> bool {
    let Some(cc) = self.options.overview.consolidated_carousel else {
        return false;
    };
    self.overview_open && self.overview_zoom() <= cc.expand_zoom
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri -- carousel_regime'`
Expected: PASS (new test + the existing one).

- [ ] **Step 5: Commit**

```bash
git add src/layout/mod.rs src/layout/tests.rs
git commit -m "layout: add in_carousel_lens, tighten in_carousel_regime to carousel band"
```

---

### Task 3: `Monitor::render_overview_at_zoom` (compile-green)

Render a monitor's FULL overview (all workspaces) at a caller-forced zoom, bypassing the inherited `overview_zoom()`. Render glue — compile-green (visual verified in Task 4).

**Files:** Modify `src/layout/monitor.rs`.

**Interfaces — Produces:**
```rust
/// Render this monitor's full overview (all workspaces stacked) at a fixed `zoom`,
/// ignoring its inherited overview zoom. Elements are positioned in this monitor's
/// own view space; the caller relocates/crops them onto the host output.
pub fn render_overview_at_zoom<R: NiriRenderer>(
    &self,
    ctx: RenderCtx<R>,
    zoom: f64,
    focus_ring: bool,
    push: &mut dyn FnMut(MonitorRenderElement<R>),
);
```

- [ ] **Step 1: Add a zoom-parameterized geo twin**

Add `fn workspaces_render_geo_at_zoom(&self, zoom: f64) -> impl Iterator<Item = Rectangle<f64, Logical>>` — a verbatim copy of `workspaces_render_geo` (`src/layout/monitor.rs:1641-1670`) with the internal `let zoom = self.overview_zoom();` (`:1643`) REMOVED and the parameter used instead (the leaf calls `self.workspace_size(zoom)`/`self.workspace_gap(zoom)` already take the arg). Add `fn workspaces_with_render_geo_at_zoom(&self, zoom: f64)` mirroring `workspaces_with_render_geo` (`:1672-1681`) but zipping against the `_at_zoom` geo.

- [ ] **Step 2: Add `render_overview_at_zoom`**

Mirror `render_workspaces` (`src/layout/monitor.rs:1838-1917`) EXACTLY, changing only: use the passed `zoom` (not `self.overview_zoom()`) for the `scale_relocate` rescale and `XrayPos`; iterate `self.workspaces_with_render_geo_at_zoom(zoom)` instead of `workspaces_with_render_geo()`. Keep the same element wrapping (`CropRenderElement` → `MonitorInnerRenderElement` → `RescaleRenderElement` → `RelocateRenderElement`), crop bounds, and per-workspace `render_floating`/`render_scrolling` calls. Do NOT alter `render_workspaces` itself.

> If any zoom-dependent detail beyond the geo + final rescale surfaces during compile, follow `render_workspaces` verbatim and only swap the zoom source + geo iterator.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean (only the pre-existing `MergeWith` warning). If the new method is flagged `dead_code` (no caller until Task 4), add `#[allow(dead_code)]` with a one-line comment (removed in Task 4).

- [ ] **Step 4: Commit**

```bash
git add src/layout/monitor.rs
git commit -m "layout: render a monitor's full overview at a fixed zoom"
```

---

### Task 4: Wire the lens into `render_inner` (compile-green + HARDWARE)

**Files:** Modify `src/niri.rs` (`render_inner`).

**Interfaces:** Consumes `in_carousel_lens`, `carousel_outputs`, `carousel_centered_output_idx`, `render_overview_at_zoom`, `scale_relocate_crop`.

- [ ] **Step 1: Compute the lens flag and gate the host strip**

Near `carousel_active` (`src/niri.rs:4915`), add:
```rust
let in_carousel_lens =
    self.layout.in_carousel_lens() && self.layout.active_output() == Some(output);
```
Change the host-overview suppression guard (`if !carousel_active` at `:4969`) to `if !carousel_active && !in_carousel_lens`. (The `render_above_top_layer` branch stays as-is — provably unreachable in-regime.) The carousel block (`if carousel_active`, `:4999`) needs no extra gate because `in_carousel_regime`/`in_carousel_lens` are now mutually exclusive bands (Task 2), so `carousel_active` is already false in the lens band — but ADD a `debug_assert!(!(carousel_active && in_carousel_lens))` before the blocks to document the invariant.

- [ ] **Step 2: Add the lens render block**

After the carousel block, add:
```rust
// ===== Consolidated carousel: LENS — centered output's full overview fills the host =====
if in_carousel_lens {
    let outputs = self.layout.carousel_outputs();
    if !outputs.is_empty() {
        let centered = self
            .layout
            .carousel_centered_output_idx()
            .min(outputs.len() - 1);
        let target = outputs[centered];
        let host_view = mon.view_size();
        let target_view = target.view_size();
        let host_scale = output.current_scale().fractional_scale();
        let target_scale = target.scale().fractional_scale();
        // Fit the target's overview into the host view (letterbox: min of the axis
        // ratios), corrected for the host/target output-scale difference. STARTING
        // value — tune on hardware (Step 4).
        let fit = (host_view.w / target_view.w)
            .min(host_view.h / target_view.h);
        let fill_zoom = fit * (host_scale / target_scale);
        let host_rect = Rectangle::from_loc_and_size((0., 0.), host_view);
        target.render_overview_at_zoom(ctx.r(), fill_zoom, focus_ring, &mut |elem| {
            // render_overview_at_zoom already baked fill_zoom into geo+rescale, so
            // wrap with zoom=1.0 (identity rescale) — just relocate+crop onto the host.
            if let Some(wrapped) = scale_relocate_crop(elem, output_scale, 1.0, host_rect) {
                push(OutputRenderElements::CarouselCard(wrapped));
            }
        });
    }
}
// ===== END lens =====
```
> Reuses the `CarouselCard` variant (same wrapped type). `Rectangle::from_loc_and_size` / `Rectangle::new` — match the constructor used elsewhere in the file. The `fill_zoom` formula is a STARTING point; the exact fit + scale-normalization is a hardware-tuning target (Step 4), mirroring how 2a's scale-normalization was tuned.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean.

- [ ] **Step 4: Hardware verification (required — user)**

With `overview { consolidated-carousel { activation-zoom 0.25; expand-zoom 0.1 } }`, open overview, zoom into the carousel, rotate to center an output, then zoom **past 0.1**. Verify:
- The centered output's **full overview** (all its workspaces/windows) fills the host screen; carousel cards and the host's own overview are gone.
- Content is **correctly sized** (fills without gross over/underscale) and centered — tune `fill_zoom` if it's too big/small (esp. the `host_scale/target_scale` term on mixed-DPI: force `DP-2 scale 1.5` and confirm the lens still fits).
- Centering on your OWN output shows your normal overview (degenerate case).
- No nvtop spike / no lag.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs
git commit -m "niri: render the carousel lens (centered output's full overview fills the host)"
```

---

## Deferred to 2c-jump / cleanup

- **2c-jump:** pointer click in the lens → hit-test (invert the lens transform → target overview coords → target window) → focus + cursor warp (`maybe_warp_cursor_to_focus`) + close overview. Prototype the hit-test first (spike).
- **Cleanup:** remove `carousel_participants`; re-center the carousel index on active-output/output-set change.

## Self-Review

- **Spec coverage (2c-render scope):** `expand-zoom` config (Task 1) ✓; `in_carousel_lens` + mutually-exclusive bands (Task 2) ✓; forced-fill full-overview render of a remote monitor (Task 3) ✓; lens wired into `render_inner`, suppressing host overview + carousel (Task 4) ✓. Click-to-jump + cleanup deferred to 2c-jump.
- **Placeholder scan:** render tasks carry complete starting code + named hardware steps; `fill_zoom` is explicitly a tune-on-hardware starting value (like 2a's scale-norm), not a TODO. Config/predicate tasks are full TDD.
- **Type consistency:** `expand_zoom: f64`, `in_carousel_lens()`, `render_overview_at_zoom(ctx, zoom, focus_ring, push)`, `in_carousel_lens` render-flag, `CarouselCard` reuse — consistent across tasks.
