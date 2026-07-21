# Consolidated Carousel Overview — Phase 2b (Input Regime + Carousel Slide) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the carousel a true rotating carousel (Model A): below the activation zoom, ALL outputs (host included) render as cards with one **centered/prominent** and the rest tucked; `←`/`→` and `Shift`+scroll **slide** which output is centered, wrapping over the output set.

**Architecture:** Introduce `Layout::carousel_outputs()` (all non-isolated, non-empty monitors, host INCLUDED, stable order) and make `carousel_centered_output_idx` index it, defaulting to the host's position when the carousel opens. `slide_carousel(±1)` rotates the centered index. `render_inner` gains a centered-carousel render (a new geometry that places the centered output large-and-central, others tucked) that **replaces** the host's normal full overview while `in_carousel_regime()`. Input branches the existing left/right focus arms and the Shift+scroll overview branch to slide when in-regime. Pure logic (output set, index math, geometry) is unit-tested; render + input glue is compile-green + hardware-verified.

**Tech Stack:** Rust (nightly via devshell, no `+nightly`). Reuses `Monitor::render_active_workspace_at_zoom`, `scale_relocate_crop`, `BorderRenderElement` fades from 2a.

## Global Constraints

- Build/test only via the devshell (`direnv exec . bash -c 'cargo ...'`); nightly default — never `+nightly`.
- No AI attribution; no `Co-Authored-By`.
- **No offscreen buffers** on the render path (NVIDIA latency cliff — carried from 2a).
- The carousel output set = every monitor that is **not isolated** and **non-empty** — the **host is included** (unlike 2a's `carousel_participants`, which excludes it). Stable order = layout monitor-vector order.
- `carousel_centered_output_idx` indexes `carousel_outputs()`. When the carousel opens it defaults to the **active/host output's position** in that set. Sliding wraps with `rem_euclid`.
- Below the threshold (`in_carousel_regime() && active_output() == Some(output)`), the host's **own full overview render is suppressed** and replaced by the carousel (centered + tucks). Above the threshold, behavior is unchanged from today.
- Cards remain scale-normalized (`host_scale / sibling_scale`) and edge-faded, exactly as 2a established.
- Reference: 2a plan `docs/superpowers/plans/2026-07-21-consolidated-carousel-phase2a-rendering.md`; spike findings `docs/superpowers/specs/2026-07-20-consolidated-carousel-scale-spike.md`.

### Key code anchors (verified by research)

- Action match: `src/input/mod.rs:690` (in `do_action`). Left/right focus arms: `Action::FocusColumnLeft` `:1086-1092` (calls `self.niri.layout.focus_left()` then `queue_redraw_all()`), `Action::FocusColumnRight` `:1106-1112`. Under-mouse variants `:1093-1125`.
- Shift+scroll overview branch (wheel): `src/input/mod.rs:3377-3402` (`should_handle_in_overview && modifiers == Modifiers::SHIFT` → `FocusColumnLeft/RightUnderMouse`). This is the branch to intercept. Modifiers read at `:3272-3273`.
- Overview-zoom action arms (sibling insertion site for a new `Action` if needed): `:2294-2404`.
- `carousel_centered_output_idx`: field `src/layout/mod.rs:375` (`#[allow(dead_code)]`, no mutator/reader yet), initialized `0` at `:717`/`:743`.
- `in_carousel_regime()`: `src/layout/mod.rs:2433`. `carousel_participants()` (host-EXCLUDED): `:1809`. `active_output()`: `:1602`. `monitors()`: `:1795`. `Monitor::output()`/`is_isolated()`/`has_windows()`/`render_active_workspace_at_zoom`/`scale()`/`view_size()` all exist from Phase 1/2a.
- Render insertion: `src/niri.rs` `render_inner` — host workspaces rendered ~`:4964`; 2a carousel block ~`:4986-5060`; `scale_relocate_crop` `:7286`; `OutputRenderElements` enum (`CarouselCard`, `CarouselFade`) ~`:7360`.
- Redraw: `self.niri.queue_redraw_all()`.

---

### Task 1: `carousel_outputs()` + open-time center reset (TDD)

**Files:** Modify `src/layout/mod.rs`; test in `src/layout/tests.rs`.

**Interfaces — Produces:**
```rust
/// Every monitor eligible for the carousel — not isolated, non-empty — in layout
/// order. The HOST is included (this is the full carousel set, unlike
/// `carousel_participants` which excludes the host).
pub fn carousel_outputs(&self) -> Vec<&Monitor<W>>;

/// Set the centered index to the active output's position in `carousel_outputs()`.
/// Called when the carousel/overview opens so the carousel starts centered on
/// the output the user is looking at. No-op if the active output isn't in the set.
pub fn reset_carousel_center(&mut self);
```

- [ ] **Step 1: Write the failing test**

In `src/layout/tests.rs`, mirroring the 3-output fixture from `carousel_participants_excludes_host_isolated_and_empty`:
```rust
#[test]
fn carousel_outputs_includes_host_and_reset_centers_on_active() {
    // Outputs in order: "A" (active/host, window), "B" (window), "C" (window).
    // Consolidated mode on.
    let mut layout = /* fixture: 3 non-isolated non-empty outputs, active = index 0 */;

    let names: Vec<String> = layout.carousel_outputs().iter().map(|m| m.output_name().clone()).collect();
    assert_eq!(names, vec!["A".to_string(), "B".to_string(), "C".to_string()]); // host included

    layout.reset_carousel_center();
    assert_eq!(layout.carousel_centered_output_idx(), 0); // centered on active host "A"
}
```
Add a `#[cfg(test)]`-visible reader `carousel_centered_output_idx()` if none exists (a `pub fn carousel_centered_output_idx(&self) -> usize { self.carousel_centered_output_idx }`).

- [ ] **Step 2: Run test to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri carousel_outputs_includes_host'`
Expected: FAIL — methods not found.

- [ ] **Step 3: Implement**

```rust
pub fn carousel_outputs(&self) -> Vec<&Monitor<W>> {
    self.monitors()
        .filter(|m| !m.is_isolated())
        .filter(|m| m.has_windows())
        .collect()
}

pub fn carousel_centered_output_idx(&self) -> usize {
    self.carousel_centered_output_idx
}

pub fn reset_carousel_center(&mut self) {
    let Some(active) = self.active_output().cloned() else { return };
    if let Some(idx) = self.carousel_outputs().iter().position(|m| m.output() == &active) {
        self.carousel_centered_output_idx = idx;
    }
}
```
Remove the `#[allow(dead_code)]` on the field now that it has a reader. Call `reset_carousel_center()` from the overview-open path: in `toggle_overview` (`src/layout/mod.rs:4653`), after `self.overview_open = !self.overview_open;`, add `if self.overview_open { self.reset_carousel_center(); }`.

> Borrow note: `active_output()` returns `Option<&Output>`; clone it before calling `carousel_outputs()` (which borrows `self`) to avoid overlapping borrows, as shown.

- [ ] **Step 4: Run test to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri carousel_outputs_includes_host'`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/layout/mod.rs src/layout/tests.rs
git commit -m "layout: carousel output set (host-inclusive) + open-time center reset"
```

---

### Task 2: `slide_carousel` mutator (TDD)

**Files:** Modify `src/layout/mod.rs`; test in `src/layout/tests.rs`.

**Interfaces:**
- Consumes Task 1 `carousel_outputs`, `carousel_centered_output_idx`.
- Produces: `pub fn slide_carousel(&mut self, delta: isize)` — rotates the centered index over `carousel_outputs().len()`, wrapping. No-op if fewer than 2 outputs.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn slide_carousel_wraps_over_outputs() {
    // 3 outputs, centered idx starts at 0.
    let mut layout = /* same 3-output fixture, reset_carousel_center() -> 0 */;
    layout.reset_carousel_center();

    layout.slide_carousel(1);
    assert_eq!(layout.carousel_centered_output_idx(), 1);
    layout.slide_carousel(1);
    assert_eq!(layout.carousel_centered_output_idx(), 2);
    layout.slide_carousel(1);
    assert_eq!(layout.carousel_centered_output_idx(), 0); // wrapped
    layout.slide_carousel(-1);
    assert_eq!(layout.carousel_centered_output_idx(), 2); // wrapped backward
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri slide_carousel_wraps'`
Expected: FAIL — `no method slide_carousel`.

- [ ] **Step 3: Implement**

```rust
pub fn slide_carousel(&mut self, delta: isize) {
    let len = self.carousel_outputs().len();
    if len < 2 {
        return;
    }
    let idx = self.carousel_centered_output_idx as isize;
    self.carousel_centered_output_idx = (idx + delta).rem_euclid(len as isize) as usize;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri slide_carousel_wraps'`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/layout/mod.rs src/layout/tests.rs
git commit -m "layout: slide_carousel rotates centered output with wrap"
```

---

### Task 3: centered carousel geometry (TDD)

**Files:** Modify `src/layout/carousel.rs`; inline tests.

**Interfaces:**
- Produces:
  ```rust
  /// Placements for a rotating carousel: the output at `centered_idx` gets a large
  /// central box; the others tuck outward by distance from center (indices below
  /// centered_idx to the left, above to the right), all within the host view.
  pub fn carousel_centered_layout(
      view_size: Size<f64, Logical>,
      n_outputs: usize,
      centered_idx: usize,
  ) -> Vec<CardPlacement>;
  ```
  Returns `n_outputs` placements (index i → output i's placement). `CardPlacement { card_rect, card_scale }` reused from 2a.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn centered_layout_makes_center_prominent_and_others_tucked() {
    let view = Size::from((1920., 1080.));
    let p = carousel_centered_layout(view, 3, 1); // center on index 1
    assert_eq!(p.len(), 3);
    // Center card is the largest and horizontally centered-ish.
    assert!(p[1].card_scale > p[0].card_scale);
    assert!(p[1].card_scale > p[2].card_scale);
    let center_mid = p[1].card_rect.loc.x + p[1].card_rect.size.w / 2.;
    assert!((center_mid - view.w / 2.).abs() < view.w * 0.15);
    // Index 0 tucks left of center, index 2 tucks right.
    assert!(p[0].card_rect.loc.x < p[1].card_rect.loc.x);
    assert!(p[2].card_rect.loc.x > p[1].card_rect.loc.x);
    // All on-screen.
    for c in &p {
        assert!(c.card_rect.loc.x >= 0.);
        assert!(c.card_rect.loc.x + c.card_rect.size.w <= view.w);
        assert!(c.card_rect.loc.y >= 0.);
        assert!(c.card_rect.loc.y + c.card_rect.size.h <= view.h);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri carousel::tests::centered_layout'`
Expected: FAIL — function not found.

- [ ] **Step 3: Implement**

```rust
/// Placements for a rotating carousel. Center card is large and centered; each
/// step away from center tucks further out and (optionally) smaller, clamped
/// on-screen.
pub fn carousel_centered_layout(
    view_size: Size<f64, Logical>,
    n_outputs: usize,
    centered_idx: usize,
) -> Vec<CardPlacement> {
    if n_outputs == 0 {
        return Vec::new();
    }
    let center_scale = 0.42;
    let tuck_scale = 0.24;
    let center_w = view_size.w * center_scale;
    let center_h = view_size.h * center_scale;
    let tuck_w = view_size.w * tuck_scale;
    let tuck_h = view_size.h * tuck_scale;
    let margin = view_size.w * 0.02;

    (0..n_outputs)
        .map(|i| {
            if i == centered_idx {
                let x = (view_size.w - center_w) / 2.;
                let y = (view_size.h - center_h) / 2.;
                CardPlacement {
                    card_rect: Rectangle::new(Point::from((x, y)), Size::from((center_w, center_h))),
                    card_scale: center_scale,
                }
            } else {
                let dist = i as isize - centered_idx as isize; // <0 left, >0 right
                let step = (tuck_w + margin) * (dist.unsigned_abs() as f64 - 1.);
                let x = if dist < 0 {
                    // left of center
                    ((view_size.w - center_w) / 2. - margin - tuck_w - step).max(margin)
                } else {
                    // right of center
                    ((view_size.w + center_w) / 2. + margin + step)
                        .min(view_size.w - tuck_w - margin)
                };
                let y = (view_size.h - tuck_h) / 2.;
                CardPlacement {
                    card_rect: Rectangle::new(Point::from((x, y)), Size::from((tuck_w, tuck_h))),
                    card_scale: tuck_scale,
                }
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri carousel::tests'`
Expected: PASS (existing 2a carousel tests + this one).

- [ ] **Step 5: Commit**

```bash
git add src/layout/carousel.rs
git commit -m "layout: centered rotating-carousel card geometry"
```

---

### Task 4: render the centered carousel; suppress host overview in-regime (render glue + HARDWARE)

**Files:** Modify `src/niri.rs` (`render_inner`).

**Interfaces:** Consumes Task 1 `carousel_outputs`, `carousel_centered_output_idx`, Task 3 `carousel_centered_layout`, and the 2a card+fade rendering.

- [ ] **Step 1: Suppress the host's full overview while in the carousel regime**

In `render_inner`, the host workspaces render (`mon.render_workspaces(...)`, ~`src/niri.rs:4964`, and the surrounding layer-shell-per-workspace loop) should be **skipped** when `self.layout.in_carousel_regime() && self.layout.active_output() == Some(output)`, because the carousel (Step 2) draws the host as its centered card instead. Wrap the existing host-overview render in `if !carousel_active { ... }` where `let carousel_active = self.layout.in_carousel_regime() && self.layout.active_output() == Some(output);` is computed once near the top of the element-collection section. Leave the above-threshold path untouched.

> Be surgical: only the monitor **workspace** render is suppressed. Layer-shell overlay/top popups, the backdrop, cursor, etc. stay. If suppressing turns out to also drop something the carousel still needs, note it and report DONE_WITH_CONCERNS.

- [ ] **Step 2: Replace the 2a card block with the centered-carousel render**

Replace the 2a carousel block (the `carousel_participants` + `carousel_card_layout` loop, ~`src/niri.rs:4986-5060`) with a render over `carousel_outputs()` using `carousel_centered_layout` and the centered index:
```rust
if carousel_active {
    let host_scale = output.current_scale().fractional_scale();
    let outputs = self.layout.carousel_outputs();
    let centered = self.layout.carousel_centered_output_idx().min(outputs.len().saturating_sub(1));
    let placements = crate::layout::carousel::carousel_centered_layout(
        mon.view_size(),
        outputs.len(),
        centered,
    );
    // niri renders EARLIER-pushed elements IN FRONT, so push the centered card's
    // group FIRST (front), then the tucks in index order.
    let mut order = vec![centered];
    order.extend((0..outputs.len()).filter(|&j| j != centered));
    for &i in &order {
        let m = outputs[i];
        let place = placements[i];
        // fade strips FIRST (in front of this card), then the card — per 2a fade fix.
        let backdrop = self.config.borrow().overview.backdrop_color;
        let opaque = { let mut c = backdrop; c.a = 1.; c };
        let transparent = { let mut c = backdrop; c.a = 0.; c };
        let fade_w = place.card_rect.size.w * 0.10;
        let left = Rectangle::new(place.card_rect.loc, Size::from((fade_w, place.card_rect.size.h)));
        let right = Rectangle::new(
            Point::from((place.card_rect.loc.x + place.card_rect.size.w - fade_w, place.card_rect.loc.y)),
            Size::from((fade_w, place.card_rect.size.h)),
        );
        for (rect, from, to) in [(left, opaque, transparent), (right, transparent, opaque)] {
            let strip = BorderRenderElement::new(
                rect.size, Rectangle::from_size(rect.size), GradientInterpolation::default(),
                from, to, 0., Rectangle::from_size(rect.size), f32::MAX, CornerRadius::default(),
                host_scale as f32, 1.0,
            ).with_location(rect.loc);
            push(OutputRenderElements::CarouselFade(strip));
        }
        let sibling_scale = m.scale().fractional_scale();
        let effective_zoom = place.card_scale * (host_scale / sibling_scale);
        m.render_active_workspace_at_zoom(ctx.r(), 1.0, focus_ring, &mut |elem| {
            if let Some(wrapped) = scale_relocate_crop(elem, output_scale, effective_zoom, place.card_rect) {
                push(OutputRenderElements::CarouselCard(wrapped));
            }
        });
    }
}
```
> The precise front/back ordering of center-vs-tucks is a visual detail to confirm on hardware (Step 4). The code above pushes the centered card's group first (front). If tucks should peek *in front of* the center's edges instead, reorder there — flag it as a tuning point, not a correctness bug. Keep the per-card fade-before-card ordering from 2a intact.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean (only the pre-existing `MergeWith` warning).

- [ ] **Step 4: Hardware verification (required — user)**

Focus an output, open overview, zoom past 0.25. Verify:
- The **centered card is your own output**, large and central; siblings tuck to the sides. (Your full-screen overview is now the centered card, not the background.)
- `←`/`→` (after Task 5) slides which output is centered; the previously-centered one tucks.
- Cards stay scale-normalized (mixed-DPI) and edge-faded; no bleed; no nvtop spike.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs
git commit -m "niri: render rotating carousel (centered + tucks), suppress host overview in-regime"
```

---

### Task 5: wire slide input (`←`/`→` + Shift+scroll)

**Files:** Modify `src/input/mod.rs`.

**Interfaces:** Consumes Task 2 `slide_carousel`.

- [ ] **Step 1: Branch the left/right focus arms**

At the top of `Action::FocusColumnLeft` (`src/input/mod.rs:1086`) and `Action::FocusColumnRight` (`:1106`), before the normal body:
```rust
Action::FocusColumnLeft => {
    if self.niri.layout.in_carousel_regime() {
        self.niri.layout.slide_carousel(-1);
        self.niri.queue_redraw_all();
        return;
    }
    // ... existing body ...
}
```
Right arm mirrors with `slide_carousel(1)`. (Confirm `do_action` returns `()` so a bare `return;` is valid; if it returns a value, return that arm's normal value shape instead.)

- [ ] **Step 2: Branch the Shift+scroll overview branch**

In the wheel handler's `should_handle_in_overview && modifiers == Modifiers::SHIFT` branch (`src/input/mod.rs:3377-3402`), before it picks `FocusColumnLeft/RightUnderMouse` binds: if `self.niri.layout.in_carousel_regime()`, call `self.niri.layout.slide_carousel(delta)` (delta from wheel direction: up/left = -1, down/right = +1 — match the sign the existing `FocusColumnLeft/Right` mapping uses there), `self.niri.queue_redraw_all()`, and `return` before the `handle_bind` dispatch. Follow the existing structure so the non-regime path is unchanged.

> Read `:3377-3438` fully first; mirror how that branch currently selects and dispatches `bind_up`/`bind_down` so the regime branch returns cleanly without falling through to `handle_bind`.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean.

- [ ] **Step 4: Hardware verification (required — user)**

In the carousel regime: `←`/`→` rotates the centered output (wraps); `Shift`+scroll does the same. Above the threshold, `←`/`→` and Shift+scroll behave exactly as before (normal column focus). Confirm no regression to normal overview navigation.

- [ ] **Step 5: Commit**

```bash
git add src/input/mod.rs
git commit -m "input: slide carousel with arrows and shift-scroll in carousel regime"
```

---

## Deferred to Phase 2c

- **Lens on zoom-in + focus-jump:** zooming back above the threshold shows the **centered** output's overview on the host screen (the lens); selecting a window there activates it and moves focus to that output's real monitor; re-invoke `set_monitors_overview_state` on active-output change.
- Smooth animated transition of the host overview shrinking into its centered card at the threshold crossing (2b accepts a discrete switch).
- `carousel_participants` (host-excluded, from 2a) becomes unused once Task 4 switches to `carousel_outputs`; remove it in 2c cleanup if nothing else uses it.

## Self-Review

- **Spec coverage:** host-inclusive output set (Task 1) ✓; default-center-on-open (Task 1 `reset_carousel_center`) ✓; slide+wrap (Task 2) ✓; centered-prominent geometry (Task 3) ✓; render centered carousel + suppress host overview in-regime (Task 4) ✓; `←`/`→` + Shift+scroll slide gated on regime (Task 5) ✓; scale-normalize + fade preserved (Task 4 reuses 2a) ✓. Lens/focus-jump + animation explicitly deferred to 2c.
- **Placeholder scan:** render/input tasks carry complete starting code + named hardware-verification steps (user-run); the "confirm return type / front-back ordering" notes are precise verification pointers, not TODOs. Pure-logic tasks (1–3) are full TDD.
- **Type consistency:** `carousel_outputs()`, `carousel_centered_output_idx()`, `reset_carousel_center()`, `slide_carousel(delta: isize)`, `carousel_centered_layout(view_size, n_outputs, centered_idx)`, `CardPlacement { card_rect, card_scale }` — consistent across tasks.
