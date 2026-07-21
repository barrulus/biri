# Consolidated Carousel Overview — Phase 2a (Card Rendering) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote the validated scale-mismatch spike into real, correct carousel cards: sibling monitors rendered as tucked, dimmed, edge-faded previews around the focused output's overview — gated to the focused output, at a controlled zoom, scale-normalized across mixed-DPI outputs, with no offscreen buffers.

**Architecture:** In `render_inner` (`src/niri.rs`), when the focused output is in the carousel regime, iterate the participating sibling monitors and composite each as a card. Each card renders the sibling's **active workspace** at a fixed zoom (a new `Monitor::render_active_workspace_at_zoom`, bypassing the inherited `overview_progress` double-zoom), positioned by a pure `carousel` layout module, wrapped with the existing `scale_relocate_crop` (hard crop) scaled by `sibling_scale / host_scale`, and softened with left/right gradient darken strips (`BorderRenderElement`, no offscreen). Pure logic (participation, geometry, scale factor) is unit-tested; the render glue is compiled green and hardware-verified.

**Tech Stack:** Rust (nightly via devshell, no `+nightly`). Smithay render elements: `RescaleRenderElement`, `RelocateRenderElement`, `CropRenderElement`, `BorderRenderElement`. `insta`/plain unit tests for the pure logic.

## Global Constraints

- Build/test only via the devshell (`direnv exec . bash -c 'cargo ...'`); nightly is default — never `+nightly`.
- No AI attribution in commits; no `Co-Authored-By`.
- **No offscreen buffers** on the card render path (the NVIDIA per-frame-alloc latency cliff — `[[per-frame-gpu-alloc-latency]]`). The spike proved direct compositing; keep it.
- Card **participation**: a sibling appears iff it is **not the host output**, **not `isolated`**, and **non-empty** (has ≥1 window). Empty/isolated outputs are skipped. No hard cap on count.
- Card **containment**: each card is hard-cropped to its box (`CropRenderElement`); horizontal edges then softened by a gradient darken strip. No sibling content may bleed outside its card box.
- **Scale-normalize** every card by `sibling_scale / host_scale` (fractional scales) so cards are size-consistent across mixed-DPI outputs.
- **Gate**: render cards only when `self.layout.in_carousel_regime() && self.layout.active_output() == Some(output)` — the focused output only. Other physical outputs stay live/unchanged.
- Reference: spike findings `docs/superpowers/specs/2026-07-20-consolidated-carousel-scale-spike.md`; design `docs/superpowers/specs/2026-07-20-consolidated-carousel-overview-design.md`.

### Key code anchors (verified)

- `render_inner`: `src/niri.rs:4470`; host `output_scale: Scale<f64>` at `src/niri.rs:4478`; host workspaces rendered ~`src/niri.rs:4964`; insert new card block after that, before `mon.render_workspace_shadows` (~`src/niri.rs:4984`).
- `scale_relocate_crop`: `src/niri.rs:7286` — `(elem, output_scale, zoom, ws_geo) -> Option<CropRenderElement<RelocateRenderElement<RescaleRenderElement<E>>>>`; `ws_geo` is host-logical.
- `OutputRenderElements` enum: `src/niri.rs:7312` (spike already added `CarouselCard`; this plan finalizes it and adds `CarouselFade`).
- `Monitor::active_workspace_ref()`: `src/layout/monitor.rs:390`; `Workspace::render_scrolling`/`render_floating`: `src/layout/workspace.rs:1628`/`:1642` (no zoom param; caller wraps in `RescaleRenderElement`). `XrayPos::new(pos, zoom)`: `src/render_helpers/xray.rs:46`.
- Sibling scale: `Monitor::scale()` `src/layout/monitor.rs:2198` (`smithay::output::Scale`; `.fractional_scale()` → f64). Host scale: `output.current_scale().fractional_scale()`.
- Gate: `Layout::in_carousel_regime()` `src/layout/mod.rs:2421`; `Layout::active_output()` `src/layout/mod.rs:1602`; `Layout::monitors()` `src/layout/mod.rs:1795`; `Monitor::output()` `src/layout/monitor.rs:378`.
- Fade: `BorderRenderElement::new(size, gradient_area, gradient_format, color_from, color_to, angle, geometry, border_width, corner_radius, scale, alpha)` `src/render_helpers/border.rs:51`; backdrop color from `options.overview.backdrop_color`.

---

### Task 1: `carousel` layout module — pure geometry (TDD)

A focused, pure module computing card boxes so `render_inner` stays thin and the geometry is unit-tested in isolation.

**Files:**
- Create: `src/layout/carousel.rs`
- Modify: `src/layout/mod.rs` (add `mod carousel;` and re-export)
- Test: inline `#[cfg(test)]` in `src/layout/carousel.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct CardPlacement {
      /// Destination box in HOST logical coords (also the crop box).
      pub card_rect: Rectangle<f64, Logical>,
      /// Shrink factor applied to the sibling BEFORE scale-normalization.
      pub card_scale: f64,
  }
  /// Tuck up to N sibling cards around the host overview: alternating
  /// left/right, stepping inward, vertically centered. Deterministic order.
  pub fn carousel_card_layout(view_size: Size<f64, Logical>, n_cards: usize) -> Vec<CardPlacement>;
  ```

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use smithay::utils::Size;

    #[test]
    fn zero_cards_is_empty() {
        assert!(carousel_card_layout(Size::from((1920., 1080.)), 0).is_empty());
    }

    #[test]
    fn cards_are_tucked_left_and_right_and_stay_on_screen() {
        let view = Size::from((1920., 1080.));
        let cards = carousel_card_layout(view, 2);
        assert_eq!(cards.len(), 2);
        // First card tucks left, second tucks right.
        assert!(cards[0].card_rect.loc.x < view.w / 2.);
        assert!(cards[1].card_rect.loc.x > view.w / 2.);
        // Every card box stays fully within the host view (no off-screen).
        for c in &cards {
            assert!(c.card_rect.loc.x >= 0.);
            assert!(c.card_rect.loc.y >= 0.);
            assert!(c.card_rect.loc.x + c.card_rect.size.w <= view.w);
            assert!(c.card_rect.loc.y + c.card_rect.size.h <= view.h);
            assert!(c.card_scale > 0. && c.card_scale < 1.);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri carousel::tests'`
Expected: FAIL — module/function not found (compile error).

- [ ] **Step 3: Implement the module**

```rust
use smithay::utils::{Logical, Point, Rectangle, Size};

/// A single sibling card's destination in the host overview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPlacement {
    /// Destination box in HOST logical coords (also the crop box).
    pub card_rect: Rectangle<f64, Logical>,
    /// Shrink factor applied to the sibling before scale-normalization.
    pub card_scale: f64,
}

/// Tuck up to `n_cards` sibling previews around the host overview: the first
/// tucks against the left edge, the second against the right, further cards
/// step inward, all vertically centered. Deterministic in card index.
pub fn carousel_card_layout(view_size: Size<f64, Logical>, n_cards: usize) -> Vec<CardPlacement> {
    if n_cards == 0 {
        return Vec::new();
    }
    // A card occupies ~28% of the host width, vertically centered.
    let card_scale = 0.28;
    let card_w = view_size.w * card_scale;
    let card_h = view_size.h * card_scale;
    let y = (view_size.h - card_h) / 2.;
    let margin = view_size.w * 0.02;

    (0..n_cards)
        .map(|i| {
            // Alternate sides; each further pair steps inward by one card width.
            let pair = (i / 2) as f64;
            let step = (card_w + margin) * pair;
            let x = if i % 2 == 0 {
                margin + step
            } else {
                view_size.w - margin - card_w - step
            };
            CardPlacement {
                card_rect: Rectangle::new(Point::from((x, y)), Size::from((card_w, card_h))),
                card_scale,
            }
        })
        .collect()
}
```

Add to `src/layout/mod.rs` near the other `mod` declarations:
```rust
pub mod carousel;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri carousel::tests'`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/layout/carousel.rs src/layout/mod.rs
git commit -m "layout: carousel card layout geometry module"
```

---

### Task 2: Participation selection on `Layout` (TDD)

Which sibling monitors become cards, in deterministic order.

**Files:**
- Modify: `src/layout/mod.rs` (add `carousel_participants`)
- Test: `src/layout/tests.rs`

**Interfaces:**
- Consumes: `Layout::monitors()`, `Monitor::output()`, `Monitor::isolated` (via a small helper — see below), window-count.
- Produces:
  ```rust
  /// Sibling outputs to show as cards for `host`: every monitor that is not
  /// `host`, not isolated, and has at least one window. Deterministic order
  /// (layout monitor order).
  pub fn carousel_participants(&self, host: &Output) -> Vec<&Monitor<W>>;
  ```

- [ ] **Step 1: Add a non-empty accessor if missing**

Check `src/layout/monitor.rs` for an existing "has any window" predicate (search `has_windows`, `is_empty`, `n_columns`, or iterate `workspaces`). If none is directly usable, add:
```rust
pub fn has_windows(&self) -> bool {
    self.workspaces.iter().any(|ws| ws.has_windows())
}
```
(Use the workspace's existing emptiness predicate; confirm `Workspace::has_windows` or equivalent exists — if it is named differently, use that name.)

- [ ] **Step 2: Write the failing test**

In `src/layout/tests.rs`, mirroring the two-output fixture used by `consolidated_mode_scopes_overview_to_active_output`:
```rust
#[test]
fn carousel_participants_excludes_host_isolated_and_empty() {
    // Build 3 outputs: host (with a window), a populated sibling, an isolated
    // sibling (with a window), and leave one sibling empty.
    // Use the same construction helper the neighbouring tests use.
    let mut layout = /* fixture: host "eDP-1" + "S-pop" (window) + "S-iso" (isolated, window) + "S-empty" (no window) */;
    let host = /* the host Output */;

    let names: Vec<String> = layout
        .carousel_participants(&host)
        .iter()
        .map(|m| m.output_name().clone())
        .collect();

    assert_eq!(names, vec!["S-pop".to_string()]); // host, isolated, empty all excluded
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri carousel_participants_excludes'`
Expected: FAIL — `no method carousel_participants`.

- [ ] **Step 4: Implement**

In `src/layout/mod.rs`:
```rust
pub fn carousel_participants(&self, host: &Output) -> Vec<&Monitor<W>> {
    self.monitors()
        .filter(|m| m.output() != host)
        .filter(|m| !m.is_isolated())
        .filter(|m| m.has_windows())
        .collect()
}
```
`m.is_isolated()` — if no pub accessor exists for the `isolated` field (`monitor.rs:83`), add `pub fn is_isolated(&self) -> bool { self.isolated }`.

- [ ] **Step 5: Run test to verify it passes**

Run: `direnv exec . bash -c 'cargo test -p niri carousel_participants_excludes'`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/layout/mod.rs src/layout/monitor.rs src/layout/tests.rs
git commit -m "layout: carousel participant selection (non-host, non-isolated, non-empty)"
```

---

### Task 3: `Monitor::render_active_workspace_at_zoom` — controlled-zoom card render

Render one monitor's active workspace at a fixed zoom, independent of the inherited `overview_progress` (kills the double-zoom). Render glue: compile-green + hardware-verified.

**Files:**
- Modify: `src/layout/monitor.rs` (new method near `render_workspaces` ~`:1828`)

**Interfaces:**
- Consumes: `active_workspace_ref()` (`:390`), `Workspace::render_scrolling`/`render_floating` (`workspace.rs:1628`/`:1642`), `XrayPos::new`.
- Produces:
  ```rust
  /// Render this monitor's ACTIVE workspace tiles at a fixed `zoom`, ignoring
  /// overview progress. Elements are rescaled by `zoom` about the origin; the
  /// caller positions/crops them. Used for consolidated carousel cards.
  pub fn render_active_workspace_at_zoom<R: NiriRenderer>(
      &self,
      ctx: RenderCtx<R>,
      zoom: f64,
      focus_ring: bool,
      push: &mut dyn FnMut(MonitorRenderElement<R>),
  );
  ```

- [ ] **Step 1: Implement, modeled on `render_workspaces` but zoom-fixed**

Mirror `render_workspaces` (`monitor.rs:1828-1907`), but: use `self.active_workspace_ref()` instead of `workspaces_with_render_geo()`; use the passed `zoom` instead of `self.overview_zoom()`; construct `XrayPos::new(Point::from((0., 0.)), zoom)`; keep the `scale = self.scale.fractional_scale()`, `CropRenderElement` (infinite bounds), and the `RescaleRenderElement::from_element(elem, (0,0), zoom)` + `RelocateRenderElement` wrapping exactly as the existing `scale_relocate` closure does, but relocate to the origin (the caller relocates to the card). Concretely:

```rust
pub fn render_active_workspace_at_zoom<R: NiriRenderer>(
    &self,
    mut ctx: RenderCtx<R>,
    zoom: f64,
    focus_ring: bool,
    push: &mut dyn FnMut(MonitorRenderElement<R>),
) {
    let scale = self.scale.fractional_scale();
    let height = (self.view_size.h * scale).ceil() as i32;
    let crop_bounds = Rectangle::new(
        Point::from((-i32::MAX / 2, 0)),
        Size::from((i32::MAX, height)),
    );
    let ws = self.active_workspace_ref();
    let xray_pos = XrayPos::new(Point::from((0., 0.)), zoom);

    macro_rules! push_scaled {
        () => {{
            &mut |elem| {
                let elem = CropRenderElement::from_element(elem, scale, crop_bounds);
                if let Some(elem) = elem {
                    let elem = MonitorInnerRenderElement::from(elem);
                    let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
                    push(RelocateRenderElement::from_element(
                        elem,
                        Point::from((0, 0)),
                        Relocate::Relative,
                    ));
                }
            }
        }};
    }

    ws.render_floating(ctx.r(), xray_pos, focus_ring, push_scaled!());
    ws.render_scrolling(ctx.r(), xray_pos, focus_ring, push_scaled!());
}
```
Match the exact `MonitorRenderElement` / `MonitorInnerRenderElement` wrapping the existing `render_workspaces` uses — if the concrete element types differ, follow `render_workspaces` (`:1880-1906`) verbatim, changing only the zoom source and the workspace selection.

- [ ] **Step 2: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean (only the pre-existing `MergeWith` warning).

- [ ] **Step 3: Commit**

```bash
git add src/layout/monitor.rs
git commit -m "layout: render a monitor's active workspace at a fixed zoom"
```

> Hardware verification of the visual output happens in Task 4 (the wiring), where the card is actually placed on screen.

---

### Task 4: Wire real cards into `render_inner` (replace the spike block)

Gate to focused output; iterate participants; render each card scale-normalized, positioned, hard-cropped. Render glue: compile-green + **hardware-verified**.

**Files:**
- Modify: `src/niri.rs` (`render_inner` card block; finalize `OutputRenderElements::CarouselCard`)

**Interfaces:**
- Consumes: Task 1 `carousel_card_layout`, Task 2 `carousel_participants`, Task 3 `render_active_workspace_at_zoom`, `scale_relocate_crop`.

- [ ] **Step 1: Ensure the `CarouselCard` variant exists**

In the `OutputRenderElements` enum (`src/niri.rs:7312`), confirm/add:
```rust
        // Consolidated carousel: a sibling monitor composited as a tucked card.
        CarouselCard = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            MonitorRenderElement<R>
        >>>,
```

- [ ] **Step 2: Replace the spike block with the real one**

Where the spike block is (after host workspaces, before `mon.render_workspace_shadows`, ~`src/niri.rs:4984`):
```rust
        // ===== Consolidated carousel: sibling cards on the focused output =====
        if self.layout.in_carousel_regime() && self.layout.active_output() == Some(output) {
            let host_scale = output.current_scale().fractional_scale();
            let participants = self.layout.carousel_participants(output);
            let placements = crate::layout::carousel::carousel_card_layout(
                mon.view_size(),
                participants.len(),
            );
            for (sibling, place) in participants.iter().zip(placements) {
                // Scale-normalize so mixed-DPI siblings render at a consistent size.
                let sibling_scale = sibling.scale().fractional_scale();
                let norm = (sibling_scale / host_scale) as f64;
                let effective_zoom = place.card_scale * norm;
                sibling.render_active_workspace_at_zoom(
                    ctx.r(),
                    1.0, // render sibling content at native zoom; card_scale shrinks it below
                    focus_ring,
                    &mut |elem| {
                        if let Some(wrapped) =
                            scale_relocate_crop(elem, output_scale, effective_zoom, place.card_rect)
                        {
                            push(OutputRenderElements::CarouselCard(wrapped));
                        }
                    },
                );
            }
        }
        // ===== END carousel =====
```
> `mon.view_size()` — if no pub accessor exists for `view_size` (`monitor.rs:53`), add `pub fn view_size(&self) -> Size<f64, Logical> { self.view_size }`.
> Note the two-stage scale: the sibling renders at zoom 1.0 (native active workspace), and `scale_relocate_crop` applies `effective_zoom = card_scale * (sibling_scale/host_scale)` — the single place the card shrink and the DPI normalization combine.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean.

- [ ] **Step 4: Hardware verification (required)**

Build + switch to this branch on sixseven. With `overview { consolidated-carousel { activation-zoom 0.25 } }` set, focus eDP-1, open overview, zoom past 0.25. Verify:
- Card shows the sibling's **active workspace at a normal (not double-zoomed) size**, tucked left, contained.
- Card is **hard-cropped** to its box (fade is Task 5), no bleed.
- On the **non-focused** physical output (DP-2), **no card appears** — it stays live (the focused-output gate).
- Force `output "DP-2" { scale 1.5 }`: the card size is **now consistent** with the scale-1 case (normalization works), not 1.5× larger.
- No nvtop spike / no lag.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs
git commit -m "niri: consolidated carousel sibling cards (gated, controlled-zoom, scale-normalized)"
```

---

### Task 5: Horizontal edge fade (gradient darken strips)

Soften the card's left/right hard edges by overlaying backdrop-colored gradient strips (opaque at the outer edge → transparent inward). No offscreen. Render glue: compile-green + **hardware-tuned**.

**Files:**
- Modify: `src/niri.rs` (add fade strips in the card loop; add `CarouselFade` variant)

**Interfaces:**
- Consumes: `BorderRenderElement::new` (`src/render_helpers/border.rs:51`); `options.overview.backdrop_color`.

- [ ] **Step 1: Add the `CarouselFade` output variant**

In `OutputRenderElements` (`src/niri.rs:7312`):
```rust
        // Consolidated carousel: gradient darken strip over a card edge.
        CarouselFade = BorderRenderElement,
```
(If `BorderRenderElement` is not directly `Element`-compatible as an `OutputRenderElements` variant, wrap it as the existing border consumers do — check `focus_ring.rs`/`tab_indicator.rs` for the exact push type and mirror it.)

- [ ] **Step 2: Emit left + right strips per card**

After pushing a card in the Task 4 loop, add its two fade strips. Strip width ~10% of the card width; gradient goes backdrop-opaque at the card's outer edge → transparent inward (angle 0 = horizontal). Vertical (top/bottom) edges stay hard-cropped (workspaces are vertically bounded — only the horizontal/infinite-scroll axis needs the fade, per the design's "faded horizontal edges").

```rust
let backdrop = self.options.overview.backdrop_color;
let transparent = { let mut c = backdrop; c.a = 0.; c }; // Color with alpha 0
let fade_w = place.card_rect.size.w * 0.10;
// LEFT strip: opaque at x=loc.x (outer) -> transparent at x=loc.x+fade_w.
let left = Rectangle::new(place.card_rect.loc, Size::from((fade_w, place.card_rect.size.h)));
// RIGHT strip: transparent -> opaque at the right edge.
let right = Rectangle::new(
    Point::from((place.card_rect.loc.x + place.card_rect.size.w - fade_w, place.card_rect.loc.y)),
    Size::from((fade_w, place.card_rect.size.h)),
);
for (rect, from, to) in [
    (left, backdrop, transparent),   // opaque at left edge, fading right (inward)
    (right, transparent, backdrop),  // fading to opaque at right edge
] {
    let strip = BorderRenderElement::new(
        rect.size,
        rect,                              // gradient_area
        GradientInterpolation::default(),
        from,
        to,
        0.,                                // angle: horizontal
        rect,                              // geometry
        f32::MAX,                          // border_width huge => full-fill gradient
        CornerRadius::default(),
        host_scale as f32,
        1.0,
    )
    .with_location(rect.loc);
    push(OutputRenderElements::CarouselFade(strip));
}
```
> Confirm the exact `Color` alpha field name/type (`src/render_helpers/…`/`niri-config`); if `Color` has no public `a`, build the transparent stop via its constructor. Confirm `GradientInterpolation`, `CornerRadius` import paths from `border.rs:6`.

- [ ] **Step 3: Compile green**

Run: `direnv exec . bash -c 'cargo check -p niri'`
Expected: clean.

- [ ] **Step 4: Hardware tuning (required)**

Build + switch on sixseven. Verify the card's left/right edges now **fade into the dark backdrop** instead of hard-cutting, and no bleed appears beyond the card box. Tune `fade_w` (Step 2) on hardware until the fade reads well — the wide ultrawide sibling is the stress case (its content exceeds the card box, so the horizontal fade is most visible there). Record the chosen `fade_w`.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs
git commit -m "niri: fade consolidated carousel card horizontal edges"
```

---

## Deferred to Phase 2b / 2c

- **2b — input regime + slide:** `←/→` and `Shift+scroll` drive `carousel_centered_output_idx` (slide the carousel / choose the centered output) below the threshold vs move window selection above it; the centered output's card renders larger/front (this plan renders all participants as equal tucked cards — centering is 2b).
- **2c — focus-jump:** select a window on the centered target output and close the overview → activate it and move focus to its physical output; re-invoke `set_monitors_overview_state` on active-output change (spike finding).

## Self-Review

- **Spec coverage:** gate-to-focused-output (Task 4 gate) ✓; controlled zoom / no double-zoom (Task 3) ✓; scale-normalize (Task 4 `norm`) ✓; edge fade, no offscreen (Task 5) ✓; participation non-host/non-isolated/non-empty (Task 2) ✓; no hard cap (Task 1 handles arbitrary `n_cards`) ✓; containment (Task 4 crop + Task 5 fade) ✓. Centering/slide and focus-jump explicitly deferred to 2b/2c.
- **Placeholder scan:** the render tasks (3–5) carry complete starting code plus named hardware-verification steps; the "confirm exact element/Color type" notes are precise verification pointers against cited files, not TODOs. Pure-logic tasks (1–2) are full TDD.
- **Type consistency:** `CardPlacement { card_rect, card_scale }`, `carousel_card_layout(view_size, n_cards)`, `carousel_participants(host)`, `render_active_workspace_at_zoom(ctx, zoom, focus_ring, push)`, `CarouselCard`/`CarouselFade` variants — used identically across tasks.
