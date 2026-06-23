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

/// Per-pass feedback + offscreen handles cloned from `OutputState` for one frame.
#[derive(Debug, Clone)]
pub struct GlobalPassState {
    /// This pass's output last frame (its `niri_prev`).
    pub prev: Option<GlesTexture>,
    /// Sink for this pass's output this frame.
    pub result: Rc<RefCell<Option<GlesTexture>>>,
    /// Display offscreen (intermediate passes render here; unused for the last pass).
    pub pass_offscreen: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// Dedicated `global_buffer` offscreen.
    pub buffer: Rc<crate::render_helpers::offscreen::OffscreenBuffer>,
    /// This pass's dedicated buffer last frame (its `niri_buffer` when it has a buffer program).
    pub buffer_prev: Option<GlesTexture>,
    /// Sink for this pass's dedicated buffer this frame.
    pub buffer_result: Rc<RefCell<Option<GlesTexture>>>,
}

/// A render element that captures the composited framebuffer below it and re-draws it through a
/// chain of global post-process programs (one or more passes).
///
/// Each pass reads the prior pass's output as `niri_screen` (pass 0 reads the real composited
/// screen) and the original unfiltered screen as `niri_source`. Intermediate passes render into an
/// offscreen texture that feeds the next pass; the last pass composites into the output. After
/// `draw()`, each pass's rendered texture is written into its shared `result` handle so the caller
/// (`OutputState`) can ping-pong it into that pass's `niri_prev` for the next frame.
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
    /// Previous frame's screen capture, bound as `niri_screen_prev` (frame-level, all passes).
    screen_prev: Option<GlesTexture>,
    /// Sink for this frame's screen capture, ping-ponged like a pass result.
    screen_result: Rc<RefCell<Option<GlesTexture>>>,
    /// One entry per pass, in execution order.
    passes: Vec<GlobalPassState>,
}

impl GlobalShaderElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        region_norm: [f32; 4],
        output_size_phys: (f32, f32),
        screen_prev: Option<GlesTexture>,
        screen_result: Rc<RefCell<Option<GlesTexture>>>,
        passes: Vec<GlobalPassState>,
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
            screen_prev,
            screen_result,
            passes,
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

        let buffer_size = dst.size.to_logical(1).to_buffer(1, Transform::Normal);

        // Capture the composited screen below: niri_source for all passes, niri_screen for pass 0.
        let source_tex = {
            let mut guard = frame.renderer();
            guard
                .as_mut()
                .create_buffer(Fourcc::Abgr8888, buffer_size)?
            // guard dropped here so the GlesFrame binding is restored before the blit.
        };
        capture::capture_framebuffer_region(frame, dst, &source_tex)?;
        // Stash this frame's screen capture for next frame's niri_screen_prev.
        *self.screen_result.borrow_mut() = Some(source_tex.clone());

        let n = self.passes.len();
        // No chain, or any pass program missing => passthrough the captured screen unchanged.
        let chain_ready = n > 0
            && (0..n).all(|i| {
                Shaders::get_from_frame(frame)
                    .program(ProgramType::GlobalPass(i))
                    .is_some()
            });
        if !chain_ready {
            return frame.render_texture_from_to(
                &source_tex,
                Rectangle::from_size(source_tex.size().to_f64()),
                dst,
                damage,
                &[],
                frame.transformation().invert(),
                1.,
                None,
                &[],
            );
        }

        // Previous frame's screen; falls back to this frame's screen on the first frame.
        let screen_prev_tex = self
            .screen_prev
            .clone()
            .unwrap_or_else(|| source_tex.clone());

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

        // The input to the current pass: pass 0 reads the real screen, later passes read the prior
        // pass's output.
        let mut input = source_tex.clone();

        for (i, pass) in self.passes.iter().enumerate() {
            // This pass's own previous-frame output (its niri_prev); on the first frame fall back
            // to this pass's input so feedback shaders start from a sensible image.
            let prev_tex = pass.prev.clone().unwrap_or_else(|| input.clone());

            // --- Dedicated buffer sub-pass for this pass (if it defines global_buffer) ---
            // Render the GlobalPassBuffer(i) program into a clean offscreen (reading last frame's
            // buffer) and feed THAT as niri_buffer. Otherwise niri_buffer aliases niri_prev.
            let buffer_tex = if Shaders::get_from_frame(frame)
                .program(ProgramType::GlobalPassBuffer(i))
                .is_some()
            {
                // First frame (or after reload) there is no prior buffer; fall back to the previous
                // screen. Overwritten next frame, so only cosmetic for one frame.
                let buf_prev = pass
                    .buffer_prev
                    .clone()
                    .unwrap_or_else(|| screen_prev_tex.clone());

                let mut buf_textures = HashMap::new();
                buf_textures.insert("niri_screen".to_string(), input.clone());
                buf_textures.insert("niri_source".to_string(), source_tex.clone());
                buf_textures.insert("niri_prev".to_string(), prev_tex.clone());
                buf_textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
                buf_textures.insert("niri_buffer".to_string(), buf_prev);

                let buf_element = ShaderRenderElement::new(
                    ProgramType::GlobalPassBuffer(i),
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
                match pass
                    .buffer
                    .render(renderer, Scale::from(self.scale as f64), &[buf_element])
                {
                    Ok((off_elem, _sync, _data)) => {
                        let next = off_elem.texture().clone();
                        drop(guard);
                        // DO NOT remove this retained clone. It forces OffscreenBuffer to allocate
                        // a distinct write texture next frame (its
                        // is_unique_reference check); without an external
                        // clone alive, next frame's buffer pass would read and write the
                        // same texture and corrupt the feedback. The clone also lets the owner move
                        // this texture into the pass's buffer_prev after submit.
                        *pass.buffer_result.borrow_mut() = Some(next.clone());
                        next
                    }
                    Err(err) => {
                        drop(guard);
                        warn!("global_buffer pass {i} failed: {err:?}");
                        prev_tex.clone()
                    }
                }
            } else {
                prev_tex.clone()
            };

            // --- This pass's display program ---
            let mut textures = HashMap::new();
            textures.insert("niri_screen".to_string(), input.clone());
            textures.insert("niri_source".to_string(), source_tex.clone());
            textures.insert("niri_prev".to_string(), prev_tex.clone());
            textures.insert("niri_screen_prev".to_string(), screen_prev_tex.clone());
            textures.insert("niri_buffer".to_string(), buffer_tex);

            let element = ShaderRenderElement::new(
                ProgramType::GlobalPass(i),
                self.area.size,
                None,
                self.scale,
                1.,
                uniforms.clone(),
                textures,
                Kind::Unspecified,
            );

            if i + 1 < n {
                // Intermediate pass: render into this pass's display offscreen. Its texture is the
                // next pass's input AND this pass's next-frame niri_prev (same retained-clone
                // ping-pong as the buffer sub-pass).
                let mut guard = frame.renderer();
                let renderer = guard.as_mut();
                match pass.pass_offscreen.render(
                    renderer,
                    Scale::from(self.scale as f64),
                    &[element],
                ) {
                    Ok((off_elem, _sync, _data)) => {
                        let next = off_elem.texture().clone();
                        drop(guard);
                        *pass.result.borrow_mut() = Some(next.clone());
                        input = next;
                    }
                    Err(err) => {
                        drop(guard);
                        warn!("global pass {i} failed: {err:?}");
                        // Best effort: feed the input forward unchanged.
                    }
                }
            } else {
                // Last pass: composite into the output framebuffer, then capture for niri_prev.
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
                let result_tex = {
                    let mut guard = frame.renderer();
                    guard
                        .as_mut()
                        .create_buffer(Fourcc::Abgr8888, buffer_size)?
                };
                capture::capture_framebuffer_region(frame, dst, &result_tex)?;
                *pass.result.borrow_mut() = Some(result_tex);
            }
        }

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
