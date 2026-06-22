use std::cell::RefCell;
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
/// After `draw()` completes, the rendered result is written into the shared `result` handle so the
/// caller (`OutputState`) can pick it up after the frame is submitted, for prev-frame ping-pong.
/// The handle is an `Rc<RefCell<..>>` shared with `OutputState.global_shader_result`.
#[derive(Debug, Clone)]
pub struct GlobalShaderElement {
    id: Id,
    commit: CommitCounter,
    area: Rectangle<f64, Logical>,
    scale: f32,
    time: f32,
    cursor: (f32, f32),
    /// Region this element covers in output-normalised coords: [origin.x, origin.y, w, h].
    /// `[0.0, 0.0, 1.0, 1.0]` = whole output.
    region_norm: [f32; 4],
    /// True full-output size in physical px (for the `niri_output_size` uniform).
    output_size_phys: (f32, f32),
    prev: Option<GlesTexture>,
    /// Previous frame's screen capture, bound as `niri_screen_prev`.
    screen_prev: Option<GlesTexture>,
    /// Sink for this frame's screen capture (clone of `screen_tex`), ping-ponged like `result`.
    screen_result: Rc<RefCell<Option<GlesTexture>>>,
    /// Offscreen target for the buffer pass (shared with OutputState; interior-mutable).
    buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// Last frame's feedback buffer, bound as `niri_buffer`.
    buffer_prev: Option<GlesTexture>,
    /// Sink for this frame's buffer texture, ping-ponged.
    buffer_result: Rc<RefCell<Option<GlesTexture>>>,
    /// Shared sink for the captured result of this element's draw pass, used for prev-frame
    /// ping-pong. After the frame is submitted, the owner takes the texture out of this handle and
    /// moves it into `global_shader_prev`.
    result: Rc<RefCell<Option<GlesTexture>>>,
}

impl GlobalShaderElement {
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        region_norm: [f32; 4],
        output_size_phys: (f32, f32),
        prev: Option<GlesTexture>,
        screen_prev: Option<GlesTexture>,
        screen_result: Rc<RefCell<Option<GlesTexture>>>,
        buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
        buffer_prev: Option<GlesTexture>,
        buffer_result: Rc<RefCell<Option<GlesTexture>>>,
        result: Rc<RefCell<Option<GlesTexture>>>,
    ) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            area,
            scale,
            time,
            cursor,
            region_norm,
            output_size_phys,
            prev,
            screen_prev,
            screen_result,
            buffer,
            buffer_prev,
            buffer_result,
            result,
        }
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

        // Stash this frame's screen capture for next frame's niri_screen_prev.
        *self.screen_result.borrow_mut() = Some(screen_tex.clone());

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
        // Previous frame's screen; falls back to this frame's screen on the first frame.
        let screen_prev_tex = self.screen_prev.clone().unwrap_or_else(|| screen_tex.clone());

        let uniforms: Rc<[Uniform<'static>]> = Rc::new([
            Uniform::new("niri_time", self.time),
            Uniform::new("niri_cursor", (self.cursor.0, self.cursor.1)),
            Uniform::new(
                "niri_region",
                (
                    self.region_norm[0],
                    self.region_norm[1],
                    self.region_norm[2],
                    self.region_norm[3],
                ),
            ),
            Uniform::new(
                "niri_output_size",
                (self.output_size_phys.0, self.output_size_phys.1),
            ),
        ]);

        // --- Dedicated feedback buffer pass ---
        // If the source defines global_buffer, render it into a clean offscreen texture (reading
        // last frame's buffer) and feed THAT as niri_buffer. Otherwise niri_buffer == niri_prev.
        // OffscreenBuffer recreates its texture when we hold a clone of the previous one, which
        // gives ping-pong for free (read buffer_prev, write a fresh texture).
        let buffer_tex = if Shaders::get_from_frame(frame)
            .program(ProgramType::GlobalBuffer)
            .is_some()
        {
            // First frame (or after reload) there is no prior buffer; fall back to the previous
            // screen. This briefly seeds the buffer with screen content, but it is overwritten the
            // next frame and is purely cosmetic for one frame.
            let buf_prev = self
                .buffer_prev
                .clone()
                .unwrap_or_else(|| screen_prev_tex.clone());

            let mut buf_textures = HashMap::new();
            buf_textures.insert("niri_screen".to_string(), screen_tex.clone());
            buf_textures.insert("niri_prev".to_string(), prev_tex.clone());
            buf_textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
            buf_textures.insert("niri_buffer".to_string(), buf_prev);

            let buf_element = ShaderRenderElement::new(
                ProgramType::GlobalBuffer,
                self.area.size,
                None,
                self.scale,
                1.,
                uniforms.clone(),
                buf_textures,
                Kind::Unspecified,
            );

            let mut guard = frame.renderer();
            let renderer = guard.as_mut();
            match self
                .buffer
                .render(renderer, Scale::from(self.scale as f64), &[buf_element])
            {
                Ok((off_elem, _sync, _data)) => {
                    let next = off_elem.texture().clone();
                    drop(guard);
                    // DO NOT remove this retained clone. It is what forces OffscreenBuffer to
                    // allocate a *distinct* write texture next frame (its `is_unique_reference`
                    // check): without an external clone alive, next frame's buffer pass would read
                    // and write the same texture and corrupt the feedback. The clone also lets the
                    // owner move this texture into global_shader_buffer_prev after submit.
                    *self.buffer_result.borrow_mut() = Some(next.clone());
                    next
                }
                Err(err) => {
                    drop(guard);
                    warn!("global_buffer pass failed: {err:?}");
                    prev_tex.clone()
                }
            }
        } else {
            prev_tex.clone()
        };

        let mut textures = HashMap::new();
        textures.insert("niri_screen".to_string(), screen_tex);
        textures.insert("niri_prev".to_string(), prev_tex.clone());
        textures.insert("niri_screen_prev".to_string(), screen_prev_tex);
        textures.insert("niri_buffer".to_string(), buffer_tex);

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
        )?;

        // Capture the rendered result for prev-frame ping-pong. The shared handle is also held by
        // OutputState.global_shader_result; after the frame is submitted, the owner moves this into
        // OutputState.global_shader_prev.
        let result_tex = {
            let mut guard = frame.renderer();
            let renderer = guard.as_mut();
            renderer.create_buffer(Fourcc::Abgr8888, buffer_size)?
        };
        capture::capture_framebuffer_region(frame, dst, &result_tex)?;
        *self.result.borrow_mut() = Some(result_tex);

        Ok(())
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
