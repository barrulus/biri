/// Placement of one panel in the cover-flow ring, in host-view logical
/// coordinates. Pure geometry: no texture/content concerns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlacement {
    /// Logical position of panel center on the host view.
    pub center: (f64, f64),
    /// Logical panel size before tilt.
    pub size: (f64, f64),
    /// Radians; positive recedes the RIGHT edge.
    pub yaw: f64,
    pub dim: f32,
    /// Draw order: higher = nearer the viewer.
    pub z: f64,
}

/// focal = FOCAL_FACTOR * view.w (used by the panel-quad projection consuming
/// this module's placements; kept here as the tuned constant of record).
pub const FOCAL_FACTOR: f64 = 1.5;
/// Center panel height / view height at reveal = 1.
const CENTER_FRACTION: f64 = 0.72;
/// First side panel height / view height.
const SIDE_FRACTION: f64 = 0.52;
/// Per extra depth, side panel height shrinks by this scale.
const SIDE_STEP_SCALE: f64 = 0.85;
/// Radians, side panel yaw magnitude at depth 1.
const SIDE_YAW: f64 = 0.9;
const SIDE_YAW_STEP: f64 = 0.2;
/// First side panel center offset, fraction of view.w.
const SIDE_X: f64 = 0.36;
const SIDE_X_STEP: f64 = 0.09;
const SIDE_DIM: f32 = 0.78;
const SIDE_DIM_STEP: f32 = 0.85;
/// Beyond this depth, `panel_placement` returns `None`.
const MAX_VISIBLE_DEPTH: f64 = 4.0;

/// Signed ring position per output (same order as `positions`): host gets
/// 0.0, outputs physically left of host get -1.0, -2.0... (nearest first),
/// right likewise +1.0... Sort key: (x, y) of `Output::current_location()`.
pub fn ring_positions(positions: &[(i32, i32)], host_idx: usize) -> Vec<f64> {
    let mut order: Vec<usize> = (0..positions.len()).collect();
    order.sort_by_key(|&i| positions[i]);

    let host_sorted_idx = order.iter().position(|&i| i == host_idx).unwrap();

    let mut ring = vec![0.0; positions.len()];
    for (sorted_idx, &orig_idx) in order.iter().enumerate() {
        let delta = sorted_idx as f64 - host_sorted_idx as f64;
        ring[orig_idx] = delta;
    }
    ring
}

/// One side of the interpolation parameters at an integer depth (0 = center).
struct SideParams {
    height_frac: f64,
    yaw: f64,
    x_frac: f64,
    dim: f32,
}

fn side_params_at_depth(depth: f64) -> SideParams {
    // depth is a non-negative real (may be fractional via lerp callers, but
    // here we only evaluate at integers >= 1; depth 0 is the center slot).
    debug_assert!(depth >= 1.0);
    let k = depth - 1.0; // 0-based extra steps beyond the first side slot
    let height_frac = SIDE_FRACTION * SIDE_STEP_SCALE.powf(k);
    let yaw = SIDE_YAW + SIDE_YAW_STEP * k;
    let x_frac = SIDE_X + SIDE_X_STEP * k;
    let dim = SIDE_DIM * SIDE_DIM_STEP.powf(k as f32);
    SideParams {
        height_frac,
        yaw,
        x_frac,
        dim,
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Placement for one output at signed slot-delta d = ring_pos - rotation,
/// assembled-ness `reveal` in [0,1]. `None` when fully off-screen.
pub fn panel_placement(view: (f64, f64), d: f64, reveal: f64) -> Option<PanelPlacement> {
    let ad = d.abs();
    if ad > MAX_VISIBLE_DEPTH {
        return None;
    }

    let (view_w, view_h) = view;

    if ad == 0.0 {
        // Center slot: flat, undimmed, ignores reveal (live-host rule).
        return Some(PanelPlacement {
            center: (view_w / 2.0, view_h / 2.0),
            size: (view_w * CENTER_FRACTION, view_h * CENTER_FRACTION),
            yaw: 0.0,
            dim: 1.0,
            z: 100.0,
        });
    }

    let sign = d.signum();

    // Bracket ad between two integer slots: slot 0 = center params, slot k>=1
    // = side_params_at_depth(k).
    let lo = ad.floor();
    let hi = ad.ceil();
    let t = ad - lo;

    let (lo_h, lo_yaw, lo_x, lo_dim, lo_z) = if lo == 0.0 {
        (CENTER_FRACTION, 0.0, 0.0, 1.0f32, 100.0)
    } else {
        let p = side_params_at_depth(lo);
        (p.height_frac, p.yaw, p.x_frac, p.dim, 100.0 - lo)
    };
    let (hi_h, hi_yaw, hi_x, hi_dim, hi_z) = if hi == 0.0 {
        (CENTER_FRACTION, 0.0, 0.0, 1.0f32, 100.0)
    } else {
        let p = side_params_at_depth(hi);
        (p.height_frac, p.yaw, p.x_frac, p.dim, 100.0 - hi)
    };

    let height_frac = lerp(lo_h, hi_h, t);
    let yaw_mag = lerp(lo_yaw, hi_yaw, t);
    let x_frac = lerp(lo_x, hi_x, t);
    let dim = lerp_f32(lo_dim, hi_dim, t as f32);
    let z = lerp(lo_z, hi_z, t);

    let panel_h = view_h * height_frac;
    let panel_w = panel_h * (view_w / view_h);

    // Settled (reveal = 1) placement.
    let settled_center_x = view_w / 2.0 + sign * x_frac * view_w;
    // Concave: viewer is inside the curve, so panels taper TOWARD the
    // center (outer edge tall, inner edge short/receding). Per panel_quad's
    // convention (positive yaw recedes the RIGHT edge), a right-side panel
    // (sign>0) must recede its LEFT/inner edge, i.e. get negative yaw; a
    // left-side panel (sign<0) recedes its RIGHT/inner edge, i.e. positive
    // yaw. Hence the sign is inverted relative to `sign`.
    let settled_yaw = -sign * yaw_mag;

    // Off-screen start (reveal = 0): center.x at ±(view.w/2 + size.w) from
    // the view center, yaw magnitude at its max over the visible depth range.
    let start_center_x = view_w / 2.0 + sign * (view_w / 2.0 + panel_w);
    let max_step = MAX_VISIBLE_DEPTH - 1.0;
    let start_yaw_mag = SIDE_YAW + SIDE_YAW_STEP * max_step;
    let start_yaw = -sign * start_yaw_mag;

    let r = reveal.clamp(0.0, 1.0);
    let center_x = lerp(start_center_x, settled_center_x, r);
    let yaw = lerp(start_yaw, settled_yaw, r);

    Some(PanelPlacement {
        center: (center_x, view_h / 2.0),
        size: (panel_w, panel_h),
        yaw,
        dim,
        z,
    })
}

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
        // Concave: right-side panels (inner/left edge recedes) get negative
        // yaw; left-side panels (inner/right edge recedes) get positive yaw.
        assert!(l.yaw > 0. && r.yaw < 0., "yaws mirror: {} {}", l.yaw, r.yaw);
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
        // Left side (d<0): concave yaw is positive, growing with depth.
        assert!(mid.yaw > 0. && mid.yaw < side.yaw);
        assert!(mid.center.0 < settled.center.0 && mid.center.0 > side.center.0);
    }

    #[test]
    fn beyond_max_depth_is_none() {
        assert!(panel_placement((1920., 1080.), 5.0, 1.0).is_none());
    }
}
