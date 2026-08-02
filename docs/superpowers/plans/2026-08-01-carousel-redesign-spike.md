# Carousel Cover-Flow Redesign — Spike Plan (Gate A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the redesign's rendering approach — a retained, damage-gated offscreen texture per output drawn as a perspective-warped (homography) quad — stays off the NVIDIA latency cliff and renders correctly under virgl, before the main build is planned.

**Architecture:** Three new units: a pure quad/homography math module, a `PanelRenderElement` (custom GLES shader element following the `BorderRenderElement` template), and a spike-only prepass that renders the output's own overview into an `OffscreenBuffer` and draws it as an animated tilted panel, gated behind `NIRI_PANEL_SPIKE=1`.

**Tech Stack:** Rust (niri fork), smithay GLES2 renderer, `ShaderRenderElement` machinery, `OffscreenBuffer` (src/render_helpers/offscreen.rs), glam.

**Spec:** `docs/superpowers/specs/2026-08-01-carousel-redesign-design.md` (section "Spike gate").

## Global Constraints

- Build/test inside the repo devshell (`nix develop`); nightly is the default toolchain — never pass `+nightly`.
- Do NOT run `cargo insta` (can hang). No config/snapshot changes are needed in this plan.
- No per-frame offscreen *allocation*: `OffscreenBuffer` is retained per output. The uniqueness trap (offscreen.rs:114 — a live clone of the buffer's texture forces recreation on the next render) is a measured risk here, not a forbidden state; Task 4 measures it and the ping-pong fallback is documented inline.
- The spike renders overview windows + workspace background colors only — NO layer-shell content, so `layer_map_for_output` is never called inside the prepass (deadlock invariant trivially holds for the spike).
- Commit after each task; commit messages in the repo's existing style (`render: ...`, `layout: ...`), no AI attribution.

---

### Task 1: Panel quad math module

**Files:**
- Create: `src/render_helpers/panel_quad.rs`
- Modify: `src/render_helpers/mod.rs` (add `pub mod panel_quad;` beside the other module decls, lines ~29-55)

**Interfaces:**
- Produces (used by Tasks 3-4 and later by Gate B):
  - `pub fn tilted_panel_corners(center: (f64, f64), size: (f64, f64), yaw_rad: f64, focal: f64) -> [[f64; 2]; 4]` — screen-space corners TL,TR,BR,BL.
  - `pub fn bounding_box(corners: &[[f64; 2]; 4]) -> (f64, f64, f64, f64)` — (x, y, w, h).
  - `pub fn unit_to_quad_homography(corners: &[[f64; 2]; 4]) -> Option<[[f64; 3]; 3]>` — row-major H mapping the unit square (u,v) to the quad; `None` for degenerate quads.
  - `pub fn invert_3x3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]>`
  - `pub fn sampling_matrix(corners: &[[f64; 2]; 4]) -> Option<glam::Mat3>` — inverse homography from *bbox-normalized* point to (u,v), as column-major f32 `Mat3` ready for the shader uniform.

- [ ] **Step 1: Write the failing tests**

Create `src/render_helpers/panel_quad.rs` containing only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }

    #[test]
    fn zero_yaw_is_flat_rect() {
        let c = tilted_panel_corners((100., 50.), (80., 40.), 0., 1000.);
        assert_close(c[0][0], 60.);  // TL x
        assert_close(c[0][1], 30.);  // TL y
        assert_close(c[2][0], 140.); // BR x
        assert_close(c[2][1], 70.);  // BR y
    }

    #[test]
    fn positive_yaw_recedes_right_edge() {
        // Positive yaw pushes the RIGHT edge away from the camera:
        // it must be shorter than the left edge and pulled toward center.
        let c = tilted_panel_corners((0., 0.), (100., 60.), 0.5, 300.);
        let left_h = c[3][1] - c[0][1];
        let right_h = c[2][1] - c[1][1];
        assert!(right_h < left_h, "right edge must be shorter: {right_h} vs {left_h}");
        assert!(c[1][0] < 50., "right edge pulled toward center: {}", c[1][0]);
        // Left edge is nearer to the camera: it magnifies (taller), though its
        // x still pulls inward because cos-foreshortening dominates at this yaw.
        assert!(c[0][0] > -50., "near edge x pulls inward: {}", c[0][0]);
        let left_h = c[3][1] - c[0][1];
        assert!(left_h > 60., "near edge magnifies vertically: {left_h}");
    }

    #[test]
    fn homography_maps_unit_corners_to_quad() {
        let quad = [[0., 0.], [1., 0.1], [0.9, 0.9], [0.05, 1.]];
        let h = unit_to_quad_homography(&quad).unwrap();
        let uv = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        for (i, [u, v]) in uv.iter().enumerate() {
            let w = h[2][0] * u + h[2][1] * v + h[2][2];
            let x = (h[0][0] * u + h[0][1] * v + h[0][2]) / w;
            let y = (h[1][0] * u + h[1][1] * v + h[1][2]) / w;
            assert_close(x, quad[i][0]);
            assert_close(y, quad[i][1]);
        }
    }

    #[test]
    fn inverse_round_trips() {
        let quad = [[0., 0.], [1., 0.1], [0.9, 0.9], [0.05, 1.]];
        let h = unit_to_quad_homography(&quad).unwrap();
        let inv = invert_3x3(&h).unwrap();
        // inv maps quad points back to unit coords; check center-ish point.
        let (u, v) = (0.3, 0.7);
        let w = h[2][0] * u + h[2][1] * v + h[2][2];
        let p = [
            (h[0][0] * u + h[0][1] * v + h[0][2]) / w,
            (h[1][0] * u + h[1][1] * v + h[1][2]) / w,
        ];
        let w2 = inv[2][0] * p[0] + inv[2][1] * p[1] + inv[2][2];
        assert_close((inv[0][0] * p[0] + inv[0][1] * p[1] + inv[0][2]) / w2, u);
        assert_close((inv[1][0] * p[0] + inv[1][1] * p[1] + inv[1][2]) / w2, v);
    }

    #[test]
    fn degenerate_quad_is_none() {
        // All four corners collinear.
        let quad = [[0., 0.], [1., 0.], [2., 0.], [3., 0.]];
        assert!(unit_to_quad_homography(&quad).is_none());
    }

    #[test]
    fn sampling_matrix_maps_bbox_corners_to_uv() {
        let quad = tilted_panel_corners((50., 50.), (60., 40.), 0.4, 200.);
        let m = sampling_matrix(&quad).unwrap();
        let (bx, by, bw, bh) = bounding_box(&quad);
        // The quad's TL corner, bbox-normalized, must map to uv (0,0).
        let p = glam::Vec3::new(
            ((quad[0][0] - bx) / bw) as f32,
            ((quad[0][1] - by) / bh) as f32,
            1.0,
        );
        let s = m * p;
        assert!((s.x / s.z).abs() < 1e-4);
        assert!((s.y / s.z).abs() < 1e-4);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail to compile (functions missing)**

Run: `cargo test -p niri --lib panel_quad 2>&1 | tail -20`
Expected: compile error — `tilted_panel_corners` etc. not found.

- [ ] **Step 3: Implement the module (above the test module)**

```rust
//! Pure geometry for perspective-tilted carousel panels.
//!
//! A panel is a rectangle rotated by `yaw` about its vertical center axis and
//! projected with a simple pinhole model (focal length `focal`, in the same
//! logical units as the rectangle). Corners are returned TL, TR, BR, BL.

/// Screen-space corners of a tilted panel. Positive yaw recedes the right edge.
pub fn tilted_panel_corners(
    center: (f64, f64),
    size: (f64, f64),
    yaw_rad: f64,
    focal: f64,
) -> [[f64; 2]; 4] {
    let (cx, cy) = center;
    let (w, h) = size;
    let (sin, cos) = yaw_rad.sin_cos();
    // sx: signed x offset from center; z: depth (positive = away from camera).
    let project = |sx: f64, sy: f64| -> [f64; 2] {
        let x3 = sx * cos;
        let z3 = sx * sin;
        let s = focal / (focal + z3);
        [cx + x3 * s, cy + sy * s]
    };
    [
        project(-w / 2., -h / 2.), // TL
        project(w / 2., -h / 2.),  // TR
        project(w / 2., h / 2.),   // BR
        project(-w / 2., h / 2.),  // BL
    ]
}

/// Axis-aligned bounding box (x, y, w, h) of a quad.
pub fn bounding_box(corners: &[[f64; 2]; 4]) -> (f64, f64, f64, f64) {
    let min_x = corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min);
    let max_x = corners.iter().map(|c| c[0]).fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min);
    let max_y = corners.iter().map(|c| c[1]).fold(f64::NEG_INFINITY, f64::max);
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Projective mapping of the unit square (0,0)-(1,1) onto `corners`
/// (Heckbert's method). Row-major. `None` when the quad is degenerate.
pub fn unit_to_quad_homography(corners: &[[f64; 2]; 4]) -> Option<[[f64; 3]; 3]> {
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = *corners;
    let sx = x0 - x1 + x2 - x3;
    let sy = y0 - y1 + y2 - y3;
    let dx1 = x1 - x2;
    let dx2 = x3 - x2;
    let dy1 = y1 - y2;
    let dy2 = y3 - y2;
    let den = dx1 * dy2 - dx2 * dy1;
    if den.abs() < 1e-12 {
        return None;
    }
    let g = (sx * dy2 - dx2 * sy) / den;
    let h = (dx1 * sy - sx * dy1) / den;
    let m = [
        [x1 - x0 + g * x1, x3 - x0 + h * x3, x0],
        [y1 - y0 + g * y1, y3 - y0 + h * y3, y0],
        [g, h, 1.],
    ];
    // Reject mirror/degenerate results (zero determinant).
    invert_3x3(&m).map(|_| m)
}

/// Inverse of a 3x3 matrix via the adjugate. `None` when singular.
pub fn invert_3x3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1. / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

/// Matrix for the fragment shader: maps a point in bbox-normalized space
/// (0..1 over the quad's bounding box) to texture uv. Column-major f32.
pub fn sampling_matrix(corners: &[[f64; 2]; 4]) -> Option<glam::Mat3> {
    let (bx, by, bw, bh) = bounding_box(corners);
    if bw <= 0. || bh <= 0. {
        return None;
    }
    let norm: [[f64; 2]; 4] = std::array::from_fn(|i| {
        [(corners[i][0] - bx) / bw, (corners[i][1] - by) / bh]
    });
    let h = unit_to_quad_homography(&norm)?;
    let inv = invert_3x3(&h)?;
    // Column-major for glam / GLSL.
    Some(glam::Mat3::from_cols_array(&[
        inv[0][0] as f32, inv[1][0] as f32, inv[2][0] as f32,
        inv[0][1] as f32, inv[1][1] as f32, inv[2][1] as f32,
        inv[0][2] as f32, inv[1][2] as f32, inv[2][2] as f32,
    ]))
}
```

Add `pub mod panel_quad;` to `src/render_helpers/mod.rs`.

- [ ] **Step 4: Run tests, verify they pass**

Run: `cargo test -p niri --lib panel_quad 2>&1 | tail -10`
Expected: `6 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/render_helpers/panel_quad.rs src/render_helpers/mod.rs
git commit -m "render: add panel quad projection and homography math"
```

---

### Task 2: Panel fragment shader + program registration

**Files:**
- Create: `src/render_helpers/shaders/panel.frag`
- Modify: `src/render_helpers/shaders/mod.rs` — `Shaders` struct (~line 18), `ProgramType` enum (~line 38), `Shaders::compile` (~line 52), `Shaders::program()` match (~line 224)

**Interfaces:**
- Produces: `ProgramType::Panel`; compiled program reachable via `Shaders::get(renderer).program(ProgramType::Panel)`. Shader uniforms: `mat3 niri_panel_inv`, `float niri_panel_dim`; sampler `niri_panel_tex`; plus the standard `niri_alpha` provided by the `ShaderRenderElement` draw path.

- [ ] **Step 1: Write the shader**

`src/render_helpers/shaders/panel.frag` — mirror the varying name used at the top of `border.frag` (it is the interpolated 0..1 coordinate over the element area from `texture.vert`; use exactly the name `border.frag` uses — `v_coords` below):

```glsl
precision highp float;

varying vec2 v_coords;

uniform mat3 niri_panel_inv;
uniform float niri_panel_dim;
uniform float niri_alpha;
uniform sampler2D niri_panel_tex;

void main() {
    vec3 s = niri_panel_inv * vec3(v_coords, 1.0);
    vec2 uv = s.xy / s.z;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || s.z <= 0.0) {
        gl_FragColor = vec4(0.0);
        return;
    }
    vec4 color = texture2D(niri_panel_tex, uv);
    // Depth dim: darken rgb only (premultiplied texture, so scale rgb).
    color.rgb *= niri_panel_dim;
    gl_FragColor = color * niri_alpha;
}
```

Note for the implementer: if the panel renders vertically flipped in Task 4's visual check, flip once here (`uv.y = 1.0 - uv.y;` before sampling) — offscreen texture orientation is a known Y-flip hazard in this fork.

- [ ] **Step 2: Register the program**

In `src/render_helpers/shaders/mod.rs`:

1. Add field to `Shaders`: `pub panel: Option<ShaderProgram>,`
2. Add variant to `ProgramType`: `Panel,`
3. In `Shaders::compile`, next to the border compilation (~line 55-79):

```rust
let panel = ShaderProgram::compile(
    renderer,
    include_str!("panel.frag"),
    &[
        UniformName::new("niri_panel_inv", UniformType::Matrix3x3),
        UniformName::new("niri_panel_dim", UniformType::_1f),
    ],
    &["niri_panel_tex"],
)
.map_err(|err| {
    warn!("error compiling panel shader: {err:?}");
})
.ok();
```

(Match the exact error-handling style of the border compilation block beside it, and add `panel` to the `Shaders { ... }` constructor.)

4. In `Shaders::program()` (~line 224) add: `ProgramType::Panel => self.panel.clone(),` — match the arm style of `ProgramType::Border` (some arms return `Option<ShaderProgram>` by clone/borrow; copy the Border arm's exact form).

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: clean check (warnings at most). If `UniformType::Matrix3x3` doesn't exist under that name, use the variant the `mat3_uniform` helper's call sites pair with (see `shaders/mod.rs:594` and its callers) — the type must be the 3x3 matrix variant of smithay's `UniformType`.

- [ ] **Step 4: Commit**

```bash
git add src/render_helpers/shaders/panel.frag src/render_helpers/shaders/mod.rs
git commit -m "render: add perspective panel shader program"
```

---

### Task 3: PanelRenderElement

**Files:**
- Create: `src/render_helpers/panel.rs`
- Modify: `src/render_helpers/mod.rs` (add `pub mod panel;`)
- Modify: `src/niri.rs` — `niri_render_elements!` block `OutputRenderElements` (~line 7521-7551): add variant

**Interfaces:**
- Consumes: `panel_quad::{tilted_panel_corners, bounding_box, sampling_matrix}` (Task 1), `ProgramType::Panel` (Task 2).
- Produces: `PanelRenderElement::new(corners: [[f64; 2]; 4], texture: GlesTexture, dim: f32, scale: f64, alpha: f32) -> Option<Self>` (None when geometry degenerate) and `PanelRenderElement::has_shader(renderer) -> bool`. `OutputRenderElements::Panel(PanelRenderElement)`.

- [ ] **Step 1: Write the element, following `border.rs` structurally**

`src/render_helpers/panel.rs` (imports mirror `border.rs`; `mat3_uniform` is at `shaders/mod.rs:594`):

```rust
use std::collections::HashMap;
use std::rc::Rc;

use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture, Uniform};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};

use super::panel_quad::{bounding_box, sampling_matrix};
use super::renderer::AsGlesFrame;
use super::shader_element::ShaderRenderElement;
use super::shaders::{mat3_uniform, ProgramType, Shaders};
use crate::backend::tty::{TtyFrame, TtyRenderer};

/// A texture drawn as a perspective-tilted quad (carousel panel).
#[derive(Debug)]
pub struct PanelRenderElement(ShaderRenderElement);

impl PanelRenderElement {
    pub fn new(
        corners: [[f64; 2]; 4],
        texture: GlesTexture,
        dim: f32,
        scale: f64,
        alpha: f32,
    ) -> Option<Self> {
        let inv = sampling_matrix(&corners)?;
        let (bx, by, bw, bh) = bounding_box(&corners);
        let mut textures = HashMap::new();
        textures.insert(String::from("niri_panel_tex"), texture);
        let elem = ShaderRenderElement::new(
            ProgramType::Panel,
            Size::from((bw, bh)),
            None,
            scale as f32,
            alpha,
            Rc::new([
                mat3_uniform("niri_panel_inv", inv),
                Uniform::new("niri_panel_dim", dim),
            ]),
            textures,
            Kind::Unspecified,
        )
        .with_location(Point::from((bx, by)));
        Some(Self(elem))
    }

    pub fn has_shader(renderer: &mut impl super::renderer::NiriRenderer) -> bool {
        Shaders::get(renderer)
            .program(ProgramType::Panel)
            .is_some()
    }
}
```

Then add the two `Element`/`RenderElement` delegation blocks copied structurally from `border.rs:239-345` (delegate every method to `self.0`; for the Tty impl convert via `frame.as_gles_frame()` exactly as border does; keep the `tracy_client::span!` + `gpu_span_location!` wrapping in `draw`). The delegation is mechanical — same method list, same signatures, `self.inner` becomes `self.0`.

Add `pub mod panel;` to `src/render_helpers/mod.rs`.

- [ ] **Step 2: Add the render element variant**

In the `niri_render_elements!` `OutputRenderElements` block in `src/niri.rs` (~7521), beside `CarouselFade = BorderRenderElement`:

```rust
Panel = PanelRenderElement,
```

with the import added at the top of `src/niri.rs` alongside the other `render_helpers` imports: `use crate::render_helpers::panel::PanelRenderElement;`

- [ ] **Step 3: Verify it compiles and existing tests pass**

Run: `cargo check 2>&1 | tail -5` then `cargo test -p niri --lib panel_quad 2>&1 | tail -5`
Expected: clean check; 6 tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src/render_helpers/panel.rs src/render_helpers/mod.rs src/niri.rs
git commit -m "render: add PanelRenderElement for perspective panels"
```

---

### Task 4: Spike prepass, wiring, and the hardware gate

**Files:**
- Modify: `src/niri.rs` — `OutputState` (~553-612) + its construction (~3063-3086); `Niri::render` (~4437); `render_inner` (~4471, push site near the top of element assembly)
- Create: `docs/superpowers/specs/2026-08-01-carousel-spike-findings.md` (final step)

**Interfaces:**
- Consumes: `OffscreenBuffer` (`offscreen.rs:72 render()`), `Monitor::render_overview_at_zoom` (`monitor.rs:1968`), `PanelRenderElement` (Task 3), `panel_quad::tilted_panel_corners` (Task 1).
- Produces: spike-only behavior behind `NIRI_PANEL_SPIKE=1`; no public interfaces consumed later (Gate B replaces this wiring).

- [ ] **Step 1: Add spike state to `OutputState`**

```rust
// Spike (NIRI_PANEL_SPIKE=1): retained offscreen + last texture for the
// perspective panel experiment. Removed in the carousel redesign Gate B.
pub panel_spike_offscreen: Rc<OffscreenBuffer>,
pub panel_spike_texture: RefCell<Option<GlesTexture>>,
```

Initialize in the `OutputState` construction (`niri.rs:3063-3086`):

```rust
panel_spike_offscreen: Rc::new(OffscreenBuffer::default()),
panel_spike_texture: RefCell::new(None),
```

(Check `OffscreenBuffer`'s constructor in `offscreen.rs:27` — if it has no `Default`, use its existing `new()`/construction pattern as used by `GlobalShaderChain::resize` at `niri.rs:494`.)

Add the env-var gate as a file-scope helper in `src/niri.rs`:

```rust
fn panel_spike_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NIRI_PANEL_SPIKE").is_some_and(|v| v == "1"))
}
```

- [ ] **Step 2: Write the prepass**

Add to `impl Niri` in `src/niri.rs` (near `fill_xray_elements`, ~5202). The prepass renders this output's own overview at a fixed zoom into the retained offscreen — windows and workspace background colors only, NO layer-shell (the deadlock invariant holds trivially):

```rust
/// Spike: render this output's overview into the retained offscreen and
/// stash the texture for the tilted-panel draw. Never touches layer maps.
fn update_panel_spike_texture(
    &self,
    ctx: &mut RenderCtx<GlesRenderer>,
    output: &Output,
) {
    let Some(mon) = self.layout.monitor_for_output(output) else {
        return;
    };
    let state = &self.output_state[output];

    let zoom = 0.5;
    let mut elements: Vec<MonitorRenderElement<GlesRenderer>> = Vec::new();
    mon.render_overview_at_zoom(ctx.r(), zoom, false, &mut |elem| {
        elements.push(elem);
    });

    let scale = Scale::from(output.current_scale().fractional_scale());
    match state.panel_spike_offscreen.render(ctx.renderer, scale, &elements) {
        Ok((elem, _sync, _data)) => {
            *state.panel_spike_texture.borrow_mut() = Some(elem.texture().clone());
        }
        Err(err) => {
            warn!("panel spike offscreen render failed: {err:?}");
        }
    }
}
```

Call it from `Niri::render` (`niri.rs:4437`), right before `fill_xray_elements` (~4459), only on the GLES path:

```rust
if panel_spike_enabled() {
    self.update_panel_spike_texture(&mut ctx.as_gles(), output);
}
```

(If `render`'s generic context makes `as_gles()` awkward at that point, mirror how `fill_xray_elements` obtains its `RenderCtx<GlesRenderer>` — its call at `niri.rs:4459` is the pattern to copy.)

- [ ] **Step 3: Draw the animated tilted panel**

In `render_inner` (`niri.rs:4471`), immediately after the hotkey-overlay/exit-confirm pushes at the very top of element assembly (earlier push = on top), add:

```rust
if panel_spike_enabled() {
    if let Some(texture) = self
        .output_state
        .get(output)
        .and_then(|s| s.panel_spike_texture.borrow().clone())
    {
        let view = output_size(output);
        // Oscillating yaw so panel motion is transform-only: content
        // unchanged => offscreen must NOT re-render while it sweeps.
        let yaw = (ctx.shader_time * 0.5).sin() as f64 * 0.9;
        let corners = crate::render_helpers::panel_quad::tilted_panel_corners(
            (view.w / 2., view.h / 2.),
            (view.w * 0.6, view.h * 0.6),
            yaw,
            view.w * 1.5,
        );
        let scale = output.current_scale().fractional_scale();
        if let Some(elem) =
            PanelRenderElement::new(corners, texture, 0.85, scale, 1.)
        {
            push(elem.into());
        }
    }
}
```

Note: `ctx.shader_time` is the existing per-frame time uniform (`RenderCtx` field, `render_helpers/mod.rs:60-67`). The continuous animation also keeps redraws queued — if the panel freezes when the desktop is idle, queue redraws while the spike flag is on (mirror how shader animation schedules redraws; acceptable for a spike).

- [ ] **Step 4: Build and verify in the VM**

```bash
cargo check 2>&1 | tail -3
cd ~/dev/biri-vm && nix build .#
```

Point the VM guest config back at the fork config or keep the branch-compatible one (either parses on barrulus-custom); set the env var for the guest niri unit by adding to the VM flake module: `systemd.user.services.niri.environment.NIRI_PANEL_SPIKE = "1";` then rebuild + launch.

Expected in the VM (virgl gate): a large tilted panel showing the output's own overview, sweeping smoothly left-right; content inside it live-updates when windows change; no journal errors from niri (`journalctl --user -u niri.service | grep -iE "error|panic"` clean); niri stays responsive (`niri msg outputs` over SSH).

- [ ] **Step 5: Verify on sixseven (NVIDIA gate)**

On the host (vanilla session), run the fork nested: `NIRI_PANEL_SPIKE=1 cargo run --release` inside the devshell — the winit backend compiles shaders too (`winit.rs:160`). Watch:

1. Sweep smoothness with a video playing inside the overview (content damage every frame → offscreen re-renders every frame): this is the worst case for the `OffscreenBuffer` uniqueness trap — the stashed texture clone forces texture recreation per damaged frame (`offscreen.rs:114`).
2. Sweep smoothness with static content: offscreen must not re-render (transform-only frames).
3. `nvtop` while sweeping: no runaway VRAM growth (allocation churn) and no >100 ms frame hitches (the prior cliff was ~1 s frames).

**Fallback (only if case 1 hitches):** double-buffer — two `Rc<OffscreenBuffer>` in `OutputState`, alternate per prepass call, display the texture of the buffer NOT being rendered this frame. This removes the uniqueness conflict at the cost of one frame of panel-content latency. Implement inside Task 4 and re-verify; do not redesign anything else.

- [ ] **Step 6: Record the verdict and commit**

Write `docs/superpowers/specs/2026-08-01-carousel-spike-findings.md` recording: pass/fail per gate criterion (NVIDIA latency, virgl correctness), whether the Y-flip correction was needed, whether the double-buffer fallback was needed, and measured behavior notes (frame feel, nvtop observations). Keep it in the style of `2026-07-20-consolidated-carousel-scale-spike.md`.

```bash
git add -A src/niri.rs docs/superpowers/specs/2026-08-01-carousel-spike-findings.md
git commit -m "niri: perspective panel spike behind NIRI_PANEL_SPIKE"
```

**STOP — Gate A verdict.** Report findings to the user. Gate B/C planning (config rename, ring model, continuous reveal state, panel render integration, pull-back choreography, input/focus, cleanup) happens in a follow-up plan written against these findings.
