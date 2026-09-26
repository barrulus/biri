use std::collections::HashMap;
use std::rc::Rc;

use smithay::backend::renderer::element::{Element, Kind};
use smithay::backend::renderer::gles::Uniform;
use smithay::backend::renderer::Texture;
use smithay::utils::{Logical, Point, Rectangle, Scale};

use super::offscreen::{OffscreenData, OffscreenRenderElement};
use super::shader_element::ShaderRenderElement;
use super::shaders::ProgramType;
use crate::animation::drag_physics::DragPhysics;

/// Deform a live snapshot, including its decorations, without changing layout geometry.
pub fn render(
    physics: &DragPhysics,
    source: &OffscreenRenderElement,
    data: &mut OffscreenData,
    window: Rectangle<f64, Logical>,
    location: Point<f64, Logical>,
    scale: Scale<f64>,
    alpha: f32,
) -> ShaderRenderElement {
    let offset = source.offset();
    let size = source.logical_size();
    // Bernstein interpolation is a convex combination, so this bounds all displacement,
    // including the clamped extension used for decorations and popups.
    let padding = physics.displacement.iter().fold([0_f64; 2], |mut p, d| {
        for axis in 0..2 {
            p[axis] = p[axis].max(d[axis].abs());
        }
        p
    });
    let padding: Point<f64, Logical> = (
        (padding[0] * scale.x).ceil() / scale.x + 1. / scale.x,
        (padding[1] * scale.y).ceil() / scale.y + 1. / scale.y,
    )
        .into();
    let area = Rectangle::new(offset - padding, size + padding.to_size().upscale(2.));
    let texture = source.texture();
    let mut uniforms = vec![
        Uniform::new(
            "area",
            [
                area.loc.x as f32,
                area.loc.y as f32,
                area.size.w as f32,
                area.size.h as f32,
            ],
        ),
        Uniform::new(
            "window",
            [
                window.loc.x as f32,
                window.loc.y as f32,
                window.size.w as f32,
                window.size.h as f32,
            ],
        ),
        Uniform::new(
            "source_rect",
            [
                offset.x as f32,
                offset.y as f32,
                size.w as f32,
                size.h as f32,
            ],
        ),
        Uniform::new(
            "texture_size",
            [
                texture.width() as f32 / scale.x as f32,
                texture.height() as f32 / scale.y as f32,
            ],
        ),
    ];
    for (i, d) in physics.displacement.iter().enumerate() {
        uniforms.push(Uniform::new(
            format!("deformation_{i}"),
            [d[0] as f32, d[1] as f32],
        ));
    }
    let element = ShaderRenderElement::new(
        ProgramType::DragPhysics,
        area.size,
        None,
        scale.x as f32,
        alpha,
        Rc::from(uniforms),
        HashMap::from([("niri_tex".to_owned(), texture.clone())]),
        Kind::Unspecified,
    )
    .with_location(location + area.loc);
    data.id = element.id().clone();
    element
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egl_drag_physics_renders_padded_live_texture() {
        use smithay::backend::allocator::Fourcc;
        use smithay::backend::egl::native::EGLSurfacelessDisplay;
        use smithay::backend::egl::{EGLContext, EGLDisplay};
        use smithay::backend::renderer::gles::GlesRenderer;
        use smithay::utils::Transform;

        use crate::render_helpers::offscreen::OffscreenBuffer;
        use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
        use crate::render_helpers::{render_to_vec, resources, shaders};

        let mut renderer = unsafe {
            let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
            GlesRenderer::new(EGLContext::new(&display).unwrap()).unwrap()
        };
        resources::init(&mut renderer);
        shaders::init(&mut renderer);
        let parameters =
            niri_config::Config::parse_mem(include_str!("../../resources/shaders/drag/jelly.kdl"))
                .unwrap()
                .animations
                .window_movement
                .drag_physics
                .unwrap();
        for scale in [1., 1.25, 2.] {
            let content = SolidColorBuffer::new((160., 100.), [1., 0., 0., 1.]);
            let content =
                SolidColorRenderElement::from_buffer(&content, (0., 0.), 1., Kind::Unspecified);
            let offscreen = OffscreenBuffer::default();
            let (source, _sync, mut data) = offscreen
                .render(&mut renderer, scale.into(), &[content])
                .unwrap();
            let mut physics = DragPhysics::new([160., 100.], [0.25, 0.2], parameters);
            physics.move_by([30., 10.]);
            let element = render(
                &physics,
                &source,
                &mut data,
                Rectangle::from_size((160., 100.).into()),
                (40., 40.).into(),
                scale.into(),
                1.,
            );
            let pixels = render_to_vec(
                &mut renderer,
                ((240. * scale) as i32, (180. * scale) as i32).into(),
                scale.into(),
                Transform::Normal,
                Fourcc::Abgr8888,
                [element].into_iter(),
            )
            .unwrap();
            let width = (240. * scale) as usize;
            let at = |x: f64, y: f64| {
                &pixels[(((y * scale) as usize) * width + (x * scale) as usize) * 4..][..4]
            };
            assert_eq!(
                at(80., 60.),
                &[255, 0, 0, 255],
                "grab point remains on the sheet"
            );
            assert_eq!(
                at(2., 2.),
                &[0, 0, 0, 0],
                "outside padded geometry stays transparent"
            );
            let outside = pixels.chunks_exact(4).enumerate().any(|(i, p)| {
                let x = (i % width) as f64 / scale;
                let y = (i / width) as f64 / scale;
                p[3] > 128 && !(40. ..200.).contains(&x) || p[3] > 128 && !(40. ..140.).contains(&y)
            });
            assert!(
                outside,
                "deformed edges must not be clipped to the old rectangle"
            );
        }
    }
}
