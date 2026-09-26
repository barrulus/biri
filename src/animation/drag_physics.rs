//! Umbriel's elastic sheet solver, ported from umbrielfx/render/drag_physics.c.
//! Fixed 240 Hz steps and a bounded Jacobian keep inverse texture sampling stable.
use niri_config::drag_physics::DragPhysics as Parameters;

#[derive(Debug)]
pub struct DragPhysics {
    pub displacement: [[f64; 2]; 16],
    velocity: [[f64; 2]; 16],
    weights: [f64; 16],
    drag: [f64; 16],
    size: [f64; 2],
    grab_y: f64,
    stretch: f64,
    remainder: f64,
    pub parameters: Parameters,
    pub grabbed: bool,
    pub active: bool,
}

fn bernstein(t: f64) -> [f64; 4] {
    let u = 1. - t;
    [u * u * u, 3. * t * u * u, 3. * t * t * u, t * t * t]
}

impl DragPhysics {
    pub fn new(size: [f64; 2], grab: [f64; 2], parameters: Parameters) -> Self {
        let [u, v] = grab.map(|x| x.clamp(0., 1.));
        let horizontal = bernstein(u);
        let vertical = bernstein(v);
        let weights = std::array::from_fn(|i| horizontal[i % 4] * vertical[i / 4]);
        let mut drag: [f64; 16] = std::array::from_fn(|i| {
            let dx = i as f64 % 4. / 3. - u;
            let dy = (i / 4) as f64 / 3. - v;
            (-(dx * dx + dy * dy) / 0.35).exp()
        });
        let anchor: f64 = weights.iter().zip(drag).map(|(w, d)| w * d).sum();
        for d in &mut drag {
            *d = 1. - *d / anchor;
        }
        Self {
            displacement: [[0.; 2]; 16],
            velocity: [[0.; 2]; 16],
            weights,
            drag,
            size: size.map(|x| x.max(1.)),
            grab_y: v,
            stretch: 0.,
            remainder: 0.,
            parameters,
            grabbed: true,
            active: false,
        }
    }

    fn constrain(&mut self) {
        if self.grabbed {
            let sum: f64 = self.weights.iter().map(|w| w * w).sum();
            for axis in 0..2 {
                let mut position = 0.;
                let mut velocity = 0.;
                for i in 0..16 {
                    position += self.weights[i] * self.displacement[i][axis];
                    velocity += self.weights[i] * self.velocity[i][axis];
                }
                for i in 0..16 {
                    self.displacement[i][axis] -= self.weights[i] * position / sum;
                    self.velocity[i][axis] -= self.weights[i] * velocity / sum;
                }
            }
        }
        let mut ratio: f64 = 1.;
        for axis in 0..2 {
            let mut horizontal: f64 = 0.;
            let mut vertical: f64 = 0.;
            for i in 0..16 {
                if i % 4 < 3 {
                    horizontal = horizontal
                        .max((self.displacement[i + 1][axis] - self.displacement[i][axis]).abs());
                }
                if i < 12 {
                    vertical = vertical
                        .max((self.displacement[i + 4][axis] - self.displacement[i][axis]).abs());
                }
                ratio =
                    ratio.max(self.displacement[i][axis].abs() / (self.size[axis] / 5.).min(200.));
                ratio = ratio.max(self.velocity[i][axis].abs() / 4000.);
            }
            ratio = ratio.max(3. * (horizontal + vertical) / (self.size[axis] * 0.7));
        }
        for i in 0..16 {
            for axis in 0..2 {
                self.displacement[i][axis] /= ratio;
                self.velocity[i][axis] /= ratio;
            }
        }
    }

    pub fn resize(&mut self, size: [f64; 2]) {
        self.size = size.map(|x| x.max(1.));
        self.constrain();
    }

    pub fn move_by(&mut self, delta: [f64; 2]) {
        if !self.grabbed || !delta.iter().all(|d| d.is_finite()) || delta == [0., 0.] {
            return;
        }
        let p = self.parameters;
        if p.downward_pull.0 > 0. {
            self.stretch = (self.stretch
                + (delta[0] / self.size[0]).hypot(delta[1] / self.size[1]) * p.motion_gain.0)
                .min(1.);
        }
        for i in 0..16 {
            let trailing = 1. + p.lag_gradient.0 * (i / 4) as f64 / 3.;
            for (axis, delta) in delta.into_iter().enumerate() {
                self.displacement[i][axis] -=
                    p.pointer_response.0 * delta * self.drag[i] * trailing;
            }
        }
        self.constrain();
        self.active = true;
    }

    pub fn tick(&mut self, seconds: f64) {
        if !self.active || !seconds.is_finite() || seconds <= 0. {
            return;
        }
        if seconds > 0.25 {
            self.settle();
            return;
        }
        const STEP: f64 = 1. / 240.;
        self.remainder += seconds;
        let p = self.parameters;
        while self.remainder + 1e-12 >= STEP {
            self.remainder -= STEP;
            self.stretch *= (-p.decay.0.max(0.1) * STEP).exp();
            let mut acceleration = [[0.; 2]; 16];
            for (i, acceleration) in acceleration.iter_mut().enumerate() {
                let depth = (i / 4) as f64 / 3.;
                let softness = 1. + p.stiffness_gradient.0.max(-0.9) * depth;
                let neighbors = [
                    (i % 4 > 0).then(|| i - 1),
                    (i % 4 < 3).then_some(i + 1),
                    (i >= 4).then(|| i - 4),
                    (i < 12).then_some(i + 4),
                ];
                for (axis, a) in acceleration.iter_mut().enumerate() {
                    *a = -p.stiffness.0 * softness * self.displacement[i][axis]
                        - p.damping.0 * self.velocity[i][axis];
                    for n in neighbors.into_iter().flatten() {
                        *a += p.coupling.0
                            * (self.displacement[n][axis] - self.displacement[i][axis]);
                    }
                }
                let belly = if i % 4 == 1 || i % 4 == 2 { 1. } else { 0.65 };
                acceleration[1] += self.size[1]
                    * p.downward_pull.0
                    * self.stretch
                    * (depth - self.grab_y).max(0.)
                    * belly;
            }
            for (i, a) in acceleration.iter().enumerate() {
                for (axis, a) in a.iter().enumerate() {
                    self.velocity[i][axis] += a * STEP;
                    self.displacement[i][axis] += self.velocity[i][axis] * STEP;
                }
            }
            self.constrain();
        }
        self.active = p.downward_pull.0 > 0. && self.stretch > 0.001
            || (0..16).any(|i| {
                (0..2).any(|axis| {
                    self.displacement[i][axis].abs() > 0.02 || self.velocity[i][axis].abs() > 0.2
                })
            });
        if !self.active {
            self.settle();
        }
    }

    fn settle(&mut self) {
        self.displacement = [[0.; 2]; 16];
        self.velocity = [[0.; 2]; 16];
        self.remainder = 0.;
        self.stretch = 0.;
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters(taffy: bool) -> Parameters {
        let config = if taffy {
            include_str!("../../resources/shaders/drag/taffy.kdl")
        } else {
            include_str!("../../resources/shaders/drag/jelly.kdl")
        };
        niri_config::Config::parse_mem(config)
            .unwrap()
            .animations
            .window_movement
            .drag_physics
            .unwrap()
    }

    fn offset(p: &DragPhysics, uv: [f64; 2]) -> [f64; 2] {
        let x = bernstein(uv[0].clamp(0., 1.));
        let y = bernstein(uv[1].clamp(0., 1.));
        std::array::from_fn(|axis| {
            (0..16)
                .map(|i| x[i % 4] * y[i / 4] * p.displacement[i][axis] / p.size[axis])
                .sum()
        })
    }

    #[test]
    fn pins_pointer_and_inverse_remains_stable() {
        for taffy in [false, true] {
            for grab in [[0., 0.], [0.15, 0.73], [0.5, 0.5], [1., 1.]] {
                let mut p = DragPhysics::new([800., 500.], grab, parameters(taffy));
                for frame in 0..120 {
                    p.move_by([if frame % 2 == 0 { 250. } else { -190. }, 37.]);
                    p.tick(1. / 60.);
                    assert!(offset(&p, grab).iter().all(|x| x.abs() < 1e-10));
                    for y in 0..=8 {
                        for x in 0..=8 {
                            let original = [x as f64 / 8., y as f64 / 8.];
                            let d = offset(&p, original);
                            let target = [original[0] + d[0], original[1] + d[1]];
                            let mut source = target;
                            for _ in 0..28 {
                                let d = offset(&p, source);
                                source = [target[0] - d[0], target[1] - d[1]];
                            }
                            assert!((source[0] - original[0]).abs() < 0.00005);
                            assert!((source[1] - original[1]).abs() < 0.00005);
                        }
                    }
                }
                p.grabbed = false;
                for _ in 0..3600 {
                    p.tick(1. / 240.);
                }
                assert!(!p.active);
                assert_eq!(p.displacement, [[0.; 2]; 16]);
            }
        }
    }

    #[test]
    fn fixed_steps_match_across_refresh_rates_and_pause_settles() {
        let mut a = DragPhysics::new([800., 500.], [0.15, 0.15], parameters(true));
        let mut b = DragPhysics::new([800., 500.], [0.15, 0.15], parameters(true));
        a.move_by([30., 0.]);
        b.move_by([30., 0.]);
        for _ in 0..30 {
            a.tick(1. / 60.);
        }
        for _ in 0..72 {
            b.tick(1. / 144.);
        }
        for i in 0..16 {
            for axis in 0..2 {
                assert!((a.displacement[i][axis] - b.displacement[i][axis]).abs() < 1e-10);
            }
        }
        a.tick(f64::NAN);
        assert!(a.active);
        a.tick(1.);
        assert!(!a.active);
        assert_eq!(a.displacement, [[0.; 2]; 16]);
        // The grab survives a long pause and resumes on the next pointer event.
        a.move_by([5., 0.]);
        assert!(a.active);
    }
}
