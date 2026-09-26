use std::cell::{Cell, RefCell};
use std::iter::zip;
use std::time::Duration;

use niri_config::{CornerRadius, Gradient, GradientRelativeTo};
use smithay::backend::renderer::element::{Element as _, Kind};
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::niri_render_elements;
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};

#[derive(Debug)]
pub struct FocusRing {
    buffers: [SolidColorBuffer; 8],
    locations: [Point<f64, Logical>; 8],
    sizes: [Size<f64, Logical>; 8],
    borders: [BorderRenderElement; 8],
    full_size: Size<f64, Logical>,
    is_border: bool,
    use_border_shader: bool,
    config: niri_config::FocusRing,
    thicken_corners: bool,
    animation_time: Duration,
    animating: bool,
    custom_key: Option<u64>,
    custom_shader_available: Cell<bool>,
    light: RefCell<crate::render_helpers::decoration_light::DecorationLight>,
}

niri_render_elements! {
    FocusRingRenderElement => {
        SolidColor = SolidColorRenderElement,
        Gradient = BorderRenderElement,
    }
}

impl FocusRing {
    pub fn new(config: niri_config::FocusRing) -> Self {
        Self {
            buffers: Default::default(),
            locations: Default::default(),
            sizes: Default::default(),
            borders: Default::default(),
            full_size: Default::default(),
            is_border: false,
            use_border_shader: false,
            config,
            thicken_corners: true,
            animation_time: Duration::ZERO,
            animating: false,
            custom_key: None,
            custom_shader_available: Cell::new(true),
            light: RefCell::new(Default::default()),
        }
    }

    pub fn update_config(&mut self, config: niri_config::FocusRing) {
        self.config = config;
    }

    pub fn set_animation_time(&mut self, time: Duration) {
        self.animation_time = time;
    }

    pub fn is_animating(&self) -> bool {
        self.animating && (self.custom_key.is_none() || self.custom_shader_available.get())
    }

    pub fn update_shaders(&mut self) {
        for elem in &mut self.borders {
            elem.damage_all();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_render_elements(
        &mut self,
        win_size: Size<f64, Logical>,
        is_active: bool,
        is_border: bool,
        is_urgent: bool,
        view_rect: Rectangle<f64, Logical>,
        radius: CornerRadius,
        scale: f64,
        alpha: f32,
    ) {
        let enabled =
            is_active && !is_urgent && !self.config.off && self.config.width > 0. && alpha > 0.;
        let custom = self
            .config
            .shader
            .as_ref()
            .filter(|shader| enabled && shader.key().is_some());
        // An explicit shader block, including enable=false, overrides the legacy effect.
        let effect = self
            .config
            .rainbow_ripple
            .filter(|effect| effect.enable && enabled && self.config.shader.is_none());
        self.animating = effect.is_some_and(|effect| effect.speed.0 > 0.)
            || custom.is_some_and(|shader| shader.animated && shader.speed.0 > 0.);
        let custom_key = custom.and_then(|shader| shader.key());
        if self.custom_key != custom_key {
            self.custom_key = custom_key;
            self.custom_shader_available.set(true);
        }
        let custom_time = custom.map_or(0., |shader| {
            if shader.animated {
                (self.animation_time.as_secs_f64() * shader.speed.0) as f32
            } else {
                0.
            }
        });
        let has_effect = effect.is_some() || custom.is_some();
        let ripple = effect.map_or([0.; 4], |effect| {
            // Reduce in f64 before sending to the GPU to retain precision on long uptimes.
            let phase = (self.animation_time.as_secs_f64() * effect.speed.0 / 4.).rem_euclid(1.);
            [
                phase as f32,
                effect.strength.0 as f32,
                effect.brightness.0 as f32,
                self.config.width as f32,
            ]
        });
        // Reserve a fixed envelope for the waves; the window and its layout never move.
        let padding = custom.map_or_else(
            || {
                effect.map_or(0., |effect| {
                    (self.config.width * effect.strength.0 * 2.2 * scale + 1.).ceil() / scale
                })
            },
            |shader| (shader.padding.0 * scale + 1.).ceil() / scale,
        );
        let width = self.config.width + padding;
        let radius = radius.expanded_by(padding as f32);
        // Keep the standard ring hollow; draw-inside uses a full quad above the client.
        let is_border = is_border || has_effect;
        self.full_size = win_size + Size::from((width, width)).upscale(2.);
        self.is_border = is_border && !custom.is_some_and(|shader| shader.draw_inside);

        let color = if is_urgent {
            self.config.urgent_color
        } else if is_active {
            self.config.active_color
        } else {
            self.config.inactive_color
        };

        for buf in &mut self.buffers {
            buf.set_color(color);
        }

        let radius = radius.fit_to(self.full_size.w as f32, self.full_size.h as f32);

        let gradient = if is_urgent {
            self.config.urgent_gradient
        } else if is_active {
            self.config.active_gradient
        } else {
            self.config.inactive_gradient
        };

        self.animating &= gradient.map_or(color.a > 0., |g| g.from.a > 0. || g.to.a > 0.);

        self.use_border_shader =
            has_effect || radius != CornerRadius::default() || gradient.is_some();

        // Set the defaults for solid color + rounded corners.
        let gradient = gradient.unwrap_or_else(|| Gradient::from(color));

        let full_rect = Rectangle::new(Point::from((-width, -width)), self.full_size);
        let gradient_area = match gradient.relative_to {
            GradientRelativeTo::Window => full_rect,
            GradientRelativeTo::WorkspaceView => view_rect,
        };

        let rounded_corner_border_width = if is_border {
            // HACK: increase the border width used for the inner rounded corners a tiny bit to
            // reduce background bleed.
            let extra = if self.thicken_corners && !has_effect {
                0.5
            } else {
                0.
            };
            width as f32 + extra
        } else {
            0.
        };

        let ceil = |logical: f64| (logical * scale).ceil() / scale;

        // All of this stuff should end up aligned to physical pixels because:
        // * Window size and border width are rounded to physical pixels before being passed to this
        //   function.
        // * We will ceil the corner radii below.
        // * We do not divide anything, only add, subtract and multiply by integers.
        // * At rendering time, tile positions are rounded to physical pixels.

        if self.is_border {
            let top_left = f64::max(width, ceil(f64::from(radius.top_left)));
            let top_right = f64::min(
                self.full_size.w - top_left,
                f64::max(width, ceil(f64::from(radius.top_right))),
            );
            let bottom_left = f64::min(
                self.full_size.h - top_left,
                f64::max(width, ceil(f64::from(radius.bottom_left))),
            );
            let bottom_right = f64::min(
                self.full_size.h - top_right,
                f64::min(
                    self.full_size.w - bottom_left,
                    f64::max(width, ceil(f64::from(radius.bottom_right))),
                ),
            );

            // Top edge.
            self.sizes[0] = Size::from((win_size.w + width * 2. - top_left - top_right, width));
            self.locations[0] = Point::from((-width + top_left, -width));

            // Bottom edge.
            self.sizes[1] =
                Size::from((win_size.w + width * 2. - bottom_left - bottom_right, width));
            self.locations[1] = Point::from((-width + bottom_left, win_size.h));

            // Left edge.
            self.sizes[2] = Size::from((width, win_size.h + width * 2. - top_left - bottom_left));
            self.locations[2] = Point::from((-width, -width + top_left));

            // Right edge.
            self.sizes[3] = Size::from((width, win_size.h + width * 2. - top_right - bottom_right));
            self.locations[3] = Point::from((win_size.w, -width + top_right));

            // Top-left corner.
            self.sizes[4] = Size::from((top_left, top_left));
            self.locations[4] = Point::from((-width, -width));

            // Top-right corner.
            self.sizes[5] = Size::from((top_right, top_right));
            self.locations[5] = Point::from((win_size.w + width - top_right, -width));

            // Bottom-right corner.
            self.sizes[6] = Size::from((bottom_right, bottom_right));
            self.locations[6] = Point::from((
                win_size.w + width - bottom_right,
                win_size.h + width - bottom_right,
            ));

            // Bottom-left corner.
            self.sizes[7] = Size::from((bottom_left, bottom_left));
            self.locations[7] = Point::from((-width, win_size.h + width - bottom_left));

            for (buf, size) in zip(&mut self.buffers, self.sizes) {
                buf.resize(size);
            }

            for (border, (loc, size)) in zip(&mut self.borders, zip(self.locations, self.sizes)) {
                border.update(
                    size,
                    Rectangle::new(gradient_area.loc - loc, gradient_area.size),
                    gradient.in_,
                    gradient.from,
                    gradient.to,
                    ((gradient.angle as f32) - 90.).to_radians(),
                    Rectangle::new(full_rect.loc - loc, full_rect.size),
                    rounded_corner_border_width,
                    radius,
                    scale as f32,
                    alpha,
                );
            }
        } else {
            self.sizes[0] = self.full_size;
            self.buffers[0].resize(self.sizes[0]);
            self.locations[0] = Point::from((-width, -width));

            self.borders[0].update(
                self.sizes[0],
                Rectangle::new(gradient_area.loc - self.locations[0], gradient_area.size),
                gradient.in_,
                gradient.from,
                gradient.to,
                ((gradient.angle as f32) - 90.).to_radians(),
                Rectangle::new(full_rect.loc - self.locations[0], full_rect.size),
                rounded_corner_border_width,
                radius,
                scale as f32,
                alpha,
            );
        }
        if self.light_config().is_none() {
            self.light.borrow_mut().clear();
        }
        for border in &mut self.borders {
            border.set_rainbow_ripple(ripple);
            border.set_shader(
                custom_key,
                custom_time,
                if has_effect {
                    self.config.width as f32
                } else {
                    0.
                },
                custom.is_some_and(|shader| shader.draw_inside),
            );
        }
    }

    fn light_config(&self) -> Option<niri_config::decoration_shader::DecorationLight> {
        self.config
            .shader
            .as_ref()?
            .light
            .filter(|light| self.custom_key.is_some() && light.enable && light.intensity.0 > 0.)
    }

    pub fn light_id(&self) -> Option<smithay::backend::renderer::element::Id> {
        self.light_config()
            .map(|_| self.light.borrow().id().clone())
    }

    pub fn render_light(
        &self,
        renderer: &mut impl NiriRenderer,
        location: Point<f64, Logical>,
        alpha: f32,
    ) -> Option<crate::render_helpers::shader_element::ShaderRenderElement> {
        use crate::render_helpers::shaders::{ProgramType, Shaders};
        let Some(config) = self.light_config().filter(|_| alpha > 0.) else {
            self.light.borrow_mut().clear();
            return None;
        };
        let shaders = Shaders::get(renderer);
        if !shaders
            .decorations
            .borrow()
            .contains_key(&self.custom_key.unwrap())
            || shaders.program(ProgramType::DecorationLight).is_none()
        {
            self.light.borrow_mut().clear();
            return None;
        }
        let sources = self
            .borders
            .iter()
            .zip(self.locations)
            .take(if self.is_border { 8 } else { 1 })
            .map(|(border, loc)| {
                border
                    .clone()
                    .as_emission(config.threshold.0 as f32)
                    .with_location(loc)
                    .into()
            })
            .collect();
        match self.light.borrow_mut().render(
            renderer.as_gles_renderer(),
            sources,
            config,
            self.config.width,
            location,
            alpha,
        ) {
            Ok(element) => Some(element),
            Err(err) => {
                warn!("error rendering decoration light: {err:?}");
                None
            }
        }
    }

    pub fn draws_above_window(&self) -> bool {
        self.custom_key.is_some() && self.config.shader.as_ref().is_some_and(|s| s.draw_inside)
    }

    pub fn render(
        &self,
        renderer: &mut impl NiriRenderer,
        location: Point<f64, Logical>,
        push: &mut dyn FnMut(FocusRingRenderElement),
    ) {
        if self.config.off {
            return;
        }

        let border_width = -self.locations[0].y;

        // If drawing as a border with width = 0, then there's nothing to draw.
        if self.is_border && border_width == 0. {
            return;
        }

        if let Some(key) = self.custom_key {
            self.custom_shader_available.set(
                crate::render_helpers::shaders::Shaders::get(renderer)
                    .decorations
                    .borrow()
                    .contains_key(&key),
            );
        }
        let has_border_shader = BorderRenderElement::has_shader(renderer);

        let mut push = |buffer, border: &BorderRenderElement, location: Point<f64, Logical>| {
            let elem = if self.use_border_shader && has_border_shader {
                border.clone().with_location(location).into()
            } else {
                let alpha = border.alpha();
                SolidColorRenderElement::from_buffer(buffer, location, alpha, Kind::Unspecified)
                    .into()
            };
            push(elem);
        };

        if self.is_border {
            for ((buf, border), loc) in zip(zip(&self.buffers, &self.borders), self.locations) {
                push(buf, border, location + loc);
            }
        } else {
            push(
                &self.buffers[0],
                &self.borders[0],
                location + self.locations[0],
            );
        }
    }

    pub fn width(&self) -> f64 {
        self.config.width
    }

    /// Outward extent including the fixed envelope for animated bulges.
    pub fn render_outset(&self) -> f64 {
        -self.locations[0].y
            + self
                .light_config()
                .map_or(0., |light| light.spread.0 * 2. + 8.)
    }

    pub fn is_off(&self) -> bool {
        self.config.off
    }

    pub fn set_thicken_corners(&mut self, value: bool) {
        self.thicken_corners = value;
    }

    pub fn config(&self) -> &niri_config::FocusRing {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rainbow_ripple_state_and_damage() {
        let mut ring = FocusRing::new(niri_config::FocusRing {
            rainbow_ripple: Some(Default::default()),
            ..Default::default()
        });
        let update = |ring: &mut FocusRing, active, urgent, alpha| {
            ring.update_render_elements(
                (200., 100.).into(),
                active,
                false,
                urgent,
                Rectangle::from_size((800., 600.).into()),
                CornerRadius {
                    top_left: 12.,
                    top_right: 12.,
                    bottom_right: 12.,
                    bottom_left: 12.,
                },
                1.25,
                alpha,
            );
        };
        update(&mut ring, true, false, 1.);
        assert!(ring.is_animating());
        assert!(
            ring.is_border,
            "animated rings must stay hollow behind translucent windows"
        );
        let commit = ring.borders[0].current_commit();
        update(&mut ring, true, false, 1.);
        assert_eq!(ring.borders[0].current_commit(), commit);
        ring.set_animation_time(Duration::from_millis(100));
        update(&mut ring, true, false, 1.);
        assert_ne!(
            ring.borders[0].current_commit(),
            commit,
            "time changes must damage the ring"
        );
        for (active, urgent, alpha) in [(false, false, 1.), (true, true, 1.), (true, false, 0.)] {
            update(&mut ring, active, urgent, alpha);
            assert!(!ring.is_animating());
        }
        ring.config.active_color.a = 0.;
        update(&mut ring, true, false, 1.);
        assert!(!ring.is_animating(), "a transparent ring needs no redraws");
        ring.config.active_color.a = 1.;
        ring.config.width = 0.;
        update(&mut ring, true, false, 1.);
        assert!(!ring.is_animating());
        ring.config.width = 4.;
        ring.config.off = true;
        update(&mut ring, true, false, 1.);
        assert!(!ring.is_animating());
    }
    niri_render_elements! {
        LightTestElement => {
            Ring = FocusRingRenderElement,
            Light = crate::render_helpers::shader_element::ShaderRenderElement,
            Solid = SolidColorRenderElement,
        }
    }

    #[test]
    fn egl_decoration_light_spills_onto_window_content() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::egl::native::EGLSurfacelessDisplay;
        use smithay::backend::egl::{EGLContext, EGLDisplay};
        use smithay::backend::renderer::gles::GlesRenderer;
        use smithay::utils::Transform;

        use crate::render_helpers::{render_to_vec, shaders};

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/shaders/focus-ring/lightning.frag");
        let config = niri_config::Config::parse_mem(&format!(r#"
            layout {{ focus-ring {{ width 6; shader {{ path {:?}; padding 24; light spread=80 intensity=1.0 threshold=0.5; }}; }}; }}
        "#, path.to_str().unwrap())).unwrap();
        let mut renderer = unsafe {
            let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
            let context = EGLContext::new(&display).unwrap();
            GlesRenderer::new(context).unwrap()
        };
        crate::render_helpers::resources::init(&mut renderer);
        shaders::init(&mut renderer);
        shaders::set_decoration_programs(&mut renderer, &config);
        assert!(shaders::Shaders::get(&mut renderer)
            .decoration_light
            .is_some());
        let mut ring = FocusRing::new(config.layout.focus_ring);
        let owner = SolidColorBuffer::new((320., 180.), [0.12, 0.15, 0.18, 1.]);
        let neighbour = SolidColorBuffer::new((320., 160.), [0.24, 0.12, 0.05, 1.]);
        let side = SolidColorBuffer::new((180., 180.), [0.06, 0.16, 0.12, 1.]);
        let background = SolidColorBuffer::new((800., 600.), [0.025, 0.03, 0.04, 1.]);
        let draw = |renderer: &mut GlesRenderer,
                    ring: &mut FocusRing,
                    time,
                    scale: f64,
                    light,
                    opacity| {
            ring.set_animation_time(Duration::from_secs_f64(time));
            ring.update_render_elements(
                (320., 180.).into(),
                true,
                false,
                false,
                Rectangle::from_size((800., 600.).into()),
                CornerRadius {
                    top_left: 14.,
                    top_right: 14.,
                    bottom_right: 14.,
                    bottom_left: 14.,
                },
                scale,
                1.,
            );
            let mut elements: Vec<LightTestElement> = Vec::new();
            let mut commit = None;
            if light {
                let elem = ring
                    .render_light(renderer, (240., 240.).into(), opacity)
                    .expect("light renders");
                commit = Some(elem.current_commit());
                elements.push(elem.into());
            }
            ring.render(renderer, (240., 240.).into(), &mut |e| {
                elements.push(e.into())
            });
            for (buffer, location) in [
                (&owner, (240., 240.)),
                (&neighbour, (240., 50.)),
                (&side, (30., 240.)),
                (&side, (590., 240.)),
                (&background, (0., 0.)),
            ] {
                elements.push(
                    SolidColorRenderElement::from_buffer(buffer, location, 1., Kind::Unspecified)
                        .into(),
                );
            }
            let pixels = render_to_vec(
                renderer,
                ((800. * scale) as i32, (600. * scale) as i32).into(),
                scale.into(),
                Transform::Normal,
                Fourcc::Abgr8888,
                elements.into_iter().rev(),
            )
            .unwrap();
            (pixels, commit)
        };
        for scale in [1., 1.25, 2.] {
            let (base, _) = draw(&mut renderer, &mut ring, 0.64, scale, false, 1.);
            let (lit, commit) = draw(&mut renderer, &mut ring, 0.64, scale, true, 1.);
            let (repeat, repeat_commit) = draw(&mut renderer, &mut ring, 0.64, scale, true, 1.);
            assert_eq!(lit, repeat);
            assert_eq!(
                commit, repeat_commit,
                "static emission should reuse its blurred texture without damage"
            );
            let pixel = |bytes: &[u8], x, y| {
                let i = (((y as f64 * scale) as usize) * (800. * scale) as usize
                    + (x as f64 * scale) as usize)
                    * 4;
                [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
            };
            for (name, x, y) in [("own content", 400, 253), ("neighbour content", 400, 203)] {
                let before = pixel(&base, x, y);
                let after = pixel(&lit, x, y);
                assert!(
                    after[2] > before[2] + 5,
                    "{name}: {before:?} -> {after:?} at {scale}"
                );
            }
            assert_eq!(
                pixel(&base, 400, 60),
                pixel(&lit, 400, 60),
                "far content stays unchanged"
            );
            assert!(
                base.chunks_exact(4)
                    .zip(lit.chunks_exact(4))
                    .all(|(b, l)| (0..3).all(|c| l[c] >= b[c])),
                "light must never darken or replace underlying content"
            );
            let (moved, moved_commit) = draw(&mut renderer, &mut ring, 2.64, scale, true, 1.);
            assert_ne!(commit, moved_commit);
            assert!(
                pixel(&lit, 400, 203)[2] > pixel(&moved, 400, 203)[2] + 5,
                "light must track the travelling spot"
            );
            let (_, fade_commit) = draw(&mut renderer, &mut ring, 2.64, scale, true, 0.4);
            assert_ne!(
                moved_commit, fade_commit,
                "fading the light damages its whole extent"
            );
        }
        if let Some(dir) = std::env::var_os("NIRI_LIGHT_PREVIEW_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            for i in 0..120 {
                let (pixels, _) = draw(&mut renderer, &mut ring, i as f64 / 30., 1., true, 1.);
                let file = std::fs::File::create(
                    std::path::Path::new(&dir).join(format!("frame-{i:03}.png")),
                )
                .unwrap();
                let mut encoder = png::Encoder::new(file, 800, 600);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&pixels)
                    .unwrap();
            }
        }
        // Threshold edits must refresh emission even when the shader time has not changed.
        let (before, _) = draw(&mut renderer, &mut ring, 0.64, 1., true, 1.);
        ring.config
            .shader
            .as_mut()
            .unwrap()
            .light
            .as_mut()
            .unwrap()
            .threshold
            .0 = 1.;
        let (after, _) = draw(&mut renderer, &mut ring, 0.64, 1., true, 1.);
        assert_ne!(before, after);
        // A smaller spread reuses the larger emission allocation. Its texture coordinates
        // must still match a fresh cache, without stretching or stale light at the edge.
        {
            let light = ring.config.shader.as_mut().unwrap().light.as_mut().unwrap();
            light.threshold.0 = 0.5;
            light.spread.0 = 24.;
        }
        let (resized, _) = draw(&mut renderer, &mut ring, 0.64, 1., true, 1.);
        let mut fresh = FocusRing::new(ring.config.clone());
        let (expected, _) = draw(&mut renderer, &mut fresh, 0.64, 1., true, 1.);
        // Different mip extents can change rounding slightly, but not placement/brightness.
        let difference = resized.iter().zip(&expected).map(|(a, b)| a.abs_diff(*b));
        assert!(
            difference.max().unwrap() <= 3,
            "resizing must preserve light mapping"
        );
        ring.config
            .shader
            .as_mut()
            .unwrap()
            .light
            .as_mut()
            .unwrap()
            .enable = false;
        assert!(ring
            .render_light(&mut renderer, (0., 0.).into(), 1.)
            .is_none());
        shaders::set_decoration_programs(&mut renderer, &niri_config::Config::default());
        assert!(
            fresh
                .render_light(&mut renderer, (0., 0.).into(), 1.)
                .is_none(),
            "unavailable shaders must not emit a fallback glow"
        );
    }

    #[test]
    fn egl_decoration_files_reload_independently() {
        use niri_config::utils::MergeWith;
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::egl::native::EGLSurfacelessDisplay;
        use smithay::backend::egl::{EGLContext, EGLDisplay};
        use smithay::backend::renderer::gles::GlesRenderer;
        use smithay::utils::Transform;

        use crate::render_helpers::{render_to_vec, shaders};

        let sh = xshell::Shell::new().unwrap();
        let dir = sh.create_temp_dir().unwrap();
        let file = dir.path().join("first.frag");
        let config_path = dir.path().join("config.kdl");
        let red = "vec4 ring_color(vec2 p) { return vec4(1., 0., 0., 1.); }";
        let blue = "vec4 ring_color(vec2 p) { return vec4(0., 0., 1., 1.); }";
        sh.write_file(&file, red).unwrap();
        sh.write_file(
            dir.path().join("second.frag"),
            "vec4 ring_color(vec2 p) { return vec4(0., 1., 0., 1.); }",
        )
        .unwrap();
        sh.write_file(&config_path, r#"
            layout { focus-ring { width 8; active-color "white"; }; }
            window-rule { match app-id="first"; focus-ring { shader { path "first.frag"; padding 13.2; }; }; }
            window-rule { match app-id="second"; focus-ring { shader { path "second.frag"; }; }; }
        "#).unwrap();
        let load = || niri_config::Config::load(&config_path).config.unwrap();
        let mut renderer = unsafe {
            let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
            let context = EGLContext::new(&display).unwrap();
            GlesRenderer::new(context).unwrap()
        };
        crate::render_helpers::resources::init(&mut renderer);
        shaders::init(&mut renderer);
        let draw =
            |renderer: &mut GlesRenderer, config: niri_config::FocusRing, time, scale: f64| {
                let mut ring = FocusRing::new(config);
                ring.set_animation_time(Duration::from_secs_f64(time));
                ring.update_render_elements(
                    (320., 180.).into(),
                    true,
                    false,
                    false,
                    Rectangle::from_size((384., 244.).into()),
                    CornerRadius {
                        top_left: 24.,
                        top_right: 12.,
                        bottom_right: 0.,
                        bottom_left: 36.,
                    },
                    scale,
                    1.,
                );
                let mut elements = Vec::new();
                ring.render(renderer, (32., 32.).into(), &mut |e| elements.push(e));
                let pixels = render_to_vec(
                    renderer,
                    ((384. * scale) as i32, (244. * scale) as i32).into(),
                    scale.into(),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    elements.into_iter(),
                )
                .unwrap();
                (pixels, ring.is_animating())
            };
        let first = |config: &niri_config::Config| {
            config
                .layout
                .focus_ring
                .clone()
                .merged_with(&config.window_rules[0].focus_ring)
        };
        let second = |config: &niri_config::Config| {
            config
                .layout
                .focus_ring
                .clone()
                .merged_with(&config.window_rules[1].focus_ring)
        };
        let top_pixel =
            |pixels: &[u8]| pixels[(26 * 384 + 160) * 4..(26 * 384 + 160) * 4 + 4].to_vec();
        let config = load();
        shaders::set_decoration_programs(&mut renderer, &config);
        let (pixels, _) = draw(&mut renderer, first(&config), 0., 1.);
        assert_eq!(top_pixel(&pixels), [255, 0, 0, 255]);
        assert_eq!(
            &pixels[(100 * 384 + 160) * 4..(100 * 384 + 160) * 4 + 4],
            [0; 4],
            "even a solid user shader must leave the client hollow"
        );
        let green = draw(&mut renderer, second(&config), 0., 1.).0;
        assert_eq!(top_pixel(&green), [0, 255, 0, 255]);
        let old_key = first(&config).shader.unwrap().key().unwrap();

        // Same path and unchanged KDL: the resolved contents select a new GPU program.
        sh.write_file(&file, blue).unwrap();
        let config = load();
        shaders::set_decoration_programs(&mut renderer, &config);
        assert!(!shaders::Shaders::get(&mut renderer)
            .decorations
            .borrow()
            .contains_key(&old_key));
        assert_eq!(
            top_pixel(&draw(&mut renderer, first(&config), 0., 1.).0),
            [0, 0, 255, 255]
        );
        assert_eq!(draw(&mut renderer, second(&config), 0., 1.).0, green);

        sh.write_file(&file, "this is invalid GLSL").unwrap();
        let config = load();
        shaders::set_decoration_programs(&mut renderer, &config);
        let (fallback, animating) = draw(&mut renderer, first(&config), 0., 1.);
        assert_eq!(
            top_pixel(&fallback),
            [255; 4],
            "failed shader falls back to configured colour"
        );
        assert!(
            !animating,
            "a failed shader should not keep scheduling frames"
        );
        assert_eq!(draw(&mut renderer, second(&config), 0., 1.).0, green);
        assert_eq!(
            shaders::Shaders::get(&mut renderer)
                .decorations
                .borrow()
                .len(),
            1
        );

        // The editable file and compatibility shorthand must render the same wax effect.
        sh.write_file(
            &file,
            include_str!("../../resources/shaders/focus-ring/rainbow-ripple.frag"),
        )
        .unwrap();
        let config = load();
        shaders::set_decoration_programs(&mut renderer, &config);
        for scale in [1., 1.25, 2.] {
            let mut legacy = config.layout.focus_ring.clone();
            legacy.rainbow_ripple = Some(Default::default());
            for time in [0., 0.25, 4.] {
                assert_eq!(
                    draw(&mut renderer, first(&config), time, scale).0,
                    draw(&mut renderer, legacy.clone(), time, scale).0,
                    "external wax must match at scale {scale}, time {time}"
                );
            }
        }
        let mut frozen = first(&config);
        frozen.shader.as_mut().unwrap().animated = false;
        let (pixels, animating) = draw(&mut renderer, frozen.clone(), 0., 1.);
        assert!(!animating);
        assert_eq!(pixels, draw(&mut renderer, frozen, 9., 1.).0);

        sh.write_file(
            dir.path().join("second.frag"),
            include_str!("../../resources/shaders/focus-ring/pulse.frag"),
        )
        .unwrap();
        let config = load();
        shaders::set_decoration_programs(&mut renderer, &config);
        assert_ne!(
            draw(&mut renderer, second(&config), 0., 1.).0,
            draw(&mut renderer, second(&config), 0.5, 1.).0,
            "the second bundled shader must compile and animate too"
        );
    }

    #[test]
    fn egl_rainbow_ripple_pixels() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::egl::native::EGLSurfacelessDisplay;
        use smithay::backend::egl::{EGLContext, EGLDisplay};
        use smithay::backend::renderer::gles::GlesRenderer;
        use smithay::utils::Transform;

        use crate::render_helpers::{render_to_vec, shaders};

        let mut renderer = unsafe {
            let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
            let context = EGLContext::new(&display).unwrap();
            GlesRenderer::new(context).unwrap()
        };
        crate::render_helpers::resources::init(&mut renderer);
        shaders::init(&mut renderer);
        assert!(
            BorderRenderElement::has_shader(&mut renderer),
            "border shader must compile"
        );
        for scale in [1., 1.25, 2.] {
            let mut ring = FocusRing::new(niri_config::FocusRing {
                width: 8.,
                rainbow_ripple: Some(Default::default()),
                ..Default::default()
            });
            let size = Size::from(((384. * scale) as i32, (244. * scale) as i32));
            let mut frame = |time, strength| {
                ring.config.rainbow_ripple.as_mut().unwrap().strength.0 = strength;
                ring.set_animation_time(Duration::from_secs_f64(time));
                ring.update_render_elements(
                    (320., 180.).into(),
                    true,
                    false,
                    false,
                    Rectangle::from_size((384., 244.).into()),
                    CornerRadius {
                        top_left: 24.,
                        top_right: 12.,
                        bottom_right: 0.,
                        bottom_left: 36.,
                    },
                    scale,
                    1.,
                );
                let mut elements = Vec::new();
                ring.render(&mut renderer, (32., 32.).into(), &mut |elem| {
                    elements.push(elem)
                });
                render_to_vec(
                    &mut renderer,
                    size,
                    scale.into(),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    elements.into_iter(),
                )
                .unwrap()
            };
            let first = frame(0., 0.75);
            let next = frame(0.25, 0.75);
            assert!(first != next, "the GPU pixels must animate");
            assert!(
                first == frame(4., 0.75),
                "the animation must loop seamlessly"
            );
            let alpha = |x: usize, y: usize| first[(y * size.w as usize + x) * 4 + 3];
            assert_eq!(
                alpha(size.w as usize / 2, size.h as usize / 2),
                0,
                "hollow centre"
            );
            assert!(first.chunks_exact(4).filter(|p| p[3] > 128).count() > 1000);
            // Sample the straight top section: BOTH contours must wander, and the
            // band must have distinct thin necks and broad pools rather than a tube.
            let mut contours = Vec::new();
            for x in ((72. * scale) as usize..(312. * scale) as usize).step_by(4) {
                let rows: Vec<_> = (0..(32. * scale) as usize)
                    .filter(|&y| alpha(x, y) > 128)
                    .collect();
                if let (Some(outer), Some(inner)) = (rows.first(), rows.last()) {
                    contours.push((*outer, *inner, inner - outer + 1));
                }
            }
            assert!(!contours.is_empty());
            for (label, values) in [
                (
                    "outer contour",
                    contours.iter().map(|c| c.0).collect::<Vec<_>>(),
                ),
                ("inner contour", contours.iter().map(|c| c.1).collect()),
                ("thickness", contours.iter().map(|c| c.2).collect()),
            ] {
                let variation = values.iter().max().unwrap() - values.iter().min().unwrap();
                assert!(variation as f64 >= 2. * scale, "{label} must vary visibly");
            }
            let steady = frame(0., 0.);
            let steady_next = frame(0.25, 0.);
            assert!(
                steady
                    .chunks_exact(4)
                    .zip(steady_next.chunks_exact(4))
                    .all(|(a, b)| a[3] == b[3]),
                "zero strength keeps both contours still"
            );
            // No wave can reach the image boundary, at any tested scale.
            for x in 0..size.w as usize {
                assert_eq!(alpha(x, 0), 0);
                assert_eq!(alpha(x, size.h as usize - 1), 0);
            }
            for y in 0..size.h as usize {
                assert_eq!(alpha(0, y), 0);
                assert_eq!(alpha(size.w as usize - 1, y), 0);
            }
            // Optional frames for visual review using the actual compositor shader.
            if scale == 1. {
                if let Some(dir) = std::env::var_os("NIRI_RAINBOW_PREVIEW_DIR") {
                    std::fs::create_dir_all(&dir).unwrap();
                    for i in 0..120 {
                        let pixels = frame(f64::from(i) / 30., 0.75);
                        let path = std::path::Path::new(&dir).join(format!("frame-{i:03}.png"));
                        let file = std::fs::File::create(path).unwrap();
                        let mut encoder = png::Encoder::new(file, size.w as u32, size.h as u32);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(&pixels)
                            .unwrap();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod inside_tests {
    use super::*;

    #[test]
    fn egl_draw_inside_is_opt_in_and_tracks_focus() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::egl::native::EGLSurfacelessDisplay;
        use smithay::backend::egl::{EGLContext, EGLDisplay};
        use smithay::backend::renderer::gles::GlesRenderer;
        use smithay::utils::Transform;

        use crate::render_helpers::{render_to_vec, shaders};

        let mut renderer = unsafe {
            let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
            GlesRenderer::new(EGLContext::new(&display).unwrap()).unwrap()
        };
        crate::render_helpers::resources::init(&mut renderer);
        shaders::init(&mut renderer);
        for inside in [false, true] {
            let config = niri_config::Config::parse_mem(&format!(
                "layout {{ focus-ring {{ width 6; shader {{ draw-inside {inside}; source \"vec4 ring_color(vec2 p) {{ return vec4(1., 0., 0., 1.); }}\"; }}; }}; }}"
            )).unwrap();
            shaders::set_decoration_programs(&mut renderer, &config);
            let mut ring = FocusRing::new(config.layout.focus_ring);
            for active in [true, false] {
                for scale in [1., 1.25, 2.] {
                    ring.update_render_elements(
                        (80., 60.).into(),
                        active,
                        true,
                        false,
                        Rectangle::from_size((120., 100.).into()),
                        CornerRadius::default(),
                        scale,
                        1.,
                    );
                    assert_eq!(ring.draws_above_window(), inside && active);
                    let mut elements = Vec::new();
                    ring.render(&mut renderer, (20., 20.).into(), &mut |e| elements.push(e));
                    let pixels = render_to_vec(
                        &mut renderer,
                        ((120. * scale) as i32, (100. * scale) as i32).into(),
                        scale.into(),
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        elements.into_iter(),
                    )
                    .unwrap();
                    let idx = (((40. * scale) as usize) * ((120. * scale) as usize)
                        + (60. * scale) as usize)
                        * 4;
                    assert_eq!(pixels[idx + 3] > 0, inside && active);
                }
            }
        }
    }
}
