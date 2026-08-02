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
        assert!(c[0][0] > -50., "near edge pulled toward center: {}", c[0][0]);
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

    #[test]
    fn negative_yaw_recedes_left_edge() {
        let c = tilted_panel_corners((0., 0.), (100., 60.), -0.5, 300.);
        let left_h = c[3][1] - c[0][1];
        let right_h = c[2][1] - c[1][1];
        assert!(left_h < right_h, "left edge must be shorter: {left_h} vs {right_h}");
        assert!(c[0][0] > -50., "left edge pulled toward center: {}", c[0][0]);
        assert!(right_h > 60., "near (right) edge magnifies vertically: {right_h}");
    }
}
