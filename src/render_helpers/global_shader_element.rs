use std::collections::HashMap;
use std::rc::Rc;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture, Uniform};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{Frame as _, FrameContext as _, Offscreen, Texture as _};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Logical, Physical, Rectangle, Scale, Transform};

use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};
use crate::render_helpers::capture;
use crate::render_helpers::renderer::AsGlesFrame as _;
use crate::render_helpers::shader_element::ShaderRenderElement;
use crate::render_helpers::shaders::{ProgramType, Shaders};

/// A render element that captures the composited framebuffer below it and re-draws it through the
/// global post-process program.
///
/// This is currently a passthrough: `niri_prev` is bound to the freshly-captured screen and
/// `niri_time`/`niri_cursor` are fixed zeros. Prev-frame ping-pong, real time and cursor come in a
/// later task.
#[derive(Debug, Clone)]
pub struct GlobalShaderElement {
    id: Id,
    commit: CommitCounter,
    area: Rectangle<f64, Logical>,
    scale: f32,
    time: f32,
    cursor: (f32, f32),
    prev: Option<GlesTexture>,
}

impl GlobalShaderElement {
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        prev: Option<GlesTexture>,
    ) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            area,
            scale,
            time,
            cursor,
            prev,
        }
    }

    /// Returns the captured-result texture for prev-frame ping-pong.
    ///
    /// For this task it always returns `None`; a later task wires the real captured result.
    pub fn into_texture(self) -> Option<GlesTexture> {
        None
    }
}

impl Element for GlobalShaderElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        let size = self.area.size.to_buffer(1., Transform::Normal);
        Rectangle::from_size(size)
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.area.to_physical_precise_round(scale)
    }

    // We intentionally do not override `opaque_regions`: the default returns empty regions, which
    // is what we want since the shader may produce translucency and reads what's below.
}

impl RenderElement<GlesRenderer> for GlobalShaderElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        let _span = tracy_client::span!("GlobalShaderElement::draw");

        // Allocate the niri_screen texture sized to the physical dst region, then capture the
        // framebuffer below into it.
        let buffer_size = dst.size.to_logical(1).to_buffer(1, Transform::Normal);
        let screen_tex = {
            let mut guard = frame.renderer();
            let renderer = guard.as_mut();
            renderer.create_buffer(Fourcc::Abgr8888, buffer_size)?
            // guard dropped here so the GlesFrame binding is restored before the blit.
        };

        capture::capture_framebuffer_region(frame, dst, &screen_tex)?;

        let program = Shaders::get_from_frame(frame).program(ProgramType::Global);
        let Some(_program) = program else {
            // No global program: draw the captured screen unchanged.
            return frame.render_texture_from_to(
                &screen_tex,
                Rectangle::from_size(screen_tex.size().to_f64()),
                dst,
                damage,
                &[],
                frame.transformation().invert(),
                1.,
                None,
                &[],
            );
        };

        // Passthrough for this task: bind the freshly captured screen as prev too.
        let prev_tex = self.prev.clone().unwrap_or_else(|| screen_tex.clone());

        let uniforms: Rc<[Uniform<'static>]> = Rc::new([
            Uniform::new("niri_time", self.time),
            Uniform::new("niri_cursor", (self.cursor.0, self.cursor.1)),
        ]);

        let mut textures = HashMap::new();
        textures.insert("niri_screen".to_string(), screen_tex);
        textures.insert("niri_prev".to_string(), prev_tex);

        // Delegate the actual program pass (named samplers + custom uniforms) to
        // ShaderRenderElement, which already implements the GL uniform/texture binding.
        let element = ShaderRenderElement::new(
            ProgramType::Global,
            self.area.size,
            None,
            self.scale,
            1.,
            uniforms,
            textures,
            Kind::Unspecified,
        );

        // ShaderRenderElement::src() is the unit rectangle, so pass a unit src for a 1:1
        // full-screen mapping of the captured texture onto dst.
        RenderElement::<GlesRenderer>::draw(
            &element,
            frame,
            Rectangle::from_size((1., 1.).into()),
            dst,
            damage,
            &[],
            None,
        )
    }
}

impl<'render> RenderElement<TtyRenderer<'render>> for GlobalShaderElement {
    fn draw(
        &self,
        frame: &mut TtyFrame<'_, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        let gles_frame = frame.as_gles_frame();
        RenderElement::<GlesRenderer>::draw(
            self,
            gles_frame,
            src,
            dst,
            damage,
            opaque_regions,
            cache,
        )?;
        Ok(())
    }
}
