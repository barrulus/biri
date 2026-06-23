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
use crate::render_helpers::offscreen::OffscreenBuffer;
use crate::render_helpers::renderer::AsGlesFrame as _;
use crate::render_helpers::shader_element::ShaderRenderElement;
use crate::render_helpers::shaders::{ProgramType, Shaders};

/// Where the scoped shader reads its initial `niri_screen` texture from.
#[derive(Debug, Clone)]
pub enum ScopedSource {
    /// Capture the composited framebuffer beneath the element (live screen region).
    Capture,
    /// Use a caller-supplied texture directly (no framebuffer capture).
    Texture(GlesTexture),
}

/// A render element that runs a cached scoped shader chain over a rectangular area.
///
/// Unlike `GlobalShaderElement` this element has NO per-pass feedback (no `niri_prev`
/// history). All five standard samplers (`niri_screen`, `niri_source`, `niri_prev`,
/// `niri_screen_prev`, `niri_buffer`) are bound to the same `input` texture every
/// pass; this keeps existing shader code compiling while v1 scoped shaders don't carry
/// history between frames.
///
/// Intermediate passes render into `offscreens[i]`; the final pass composites directly
/// into the output framebuffer.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ScopedShaderElement {
    id: Id,
    commit: CommitCounter,
    area: Rectangle<f64, Logical>,
    scale: f32,
    time: f32,
    cursor: (f32, f32),
    /// Region this element covers in output-normalised coords: [origin.x, origin.y, w, h].
    region_norm: [f32; 4],
    /// True full-output size in physical px (for the `niri_output_size` uniform).
    output_size_phys: (f32, f32),
    /// Physical size of the scope area (for the `niri_size` uniform).
    size_phys: (f32, f32),
    /// The shader program key (selects `ProgramType::Scoped(key, i)`).
    key: u64,
    /// Total number of passes in the chain.
    n_passes: usize,
    /// Source for the initial `niri_screen` texture.
    source: ScopedSource,
    /// One offscreen per intermediate pass (indices 0..n_passes-1).
    offscreens: Vec<Rc<OffscreenBuffer>>,
}

impl ScopedShaderElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        area: Rectangle<f64, Logical>,
        scale: f32,
        time: f32,
        cursor: (f32, f32),
        region_norm: [f32; 4],
        output_size_phys: (f32, f32),
        size_phys: (f32, f32),
        key: u64,
        n_passes: usize,
        source: ScopedSource,
        offscreens: Vec<Rc<OffscreenBuffer>>,
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
            size_phys,
            key,
            n_passes,
            source,
            offscreens,
        }
    }
}

impl Element for ScopedShaderElement {
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

impl RenderElement<GlesRenderer> for ScopedShaderElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        let _span = tracy_client::span!("ScopedShaderElement::draw");

        let buffer_size = dst.size.to_logical(1).to_buffer(1, Transform::Normal);

        // Establish the input texture from the source.
        let input_tex: GlesTexture = match &self.source {
            ScopedSource::Capture => {
                let tex = {
                    let mut guard = frame.renderer();
                    guard
                        .as_mut()
                        .create_buffer(Fourcc::Abgr8888, buffer_size)?
                    // guard dropped here so the GlesFrame binding is restored before the blit.
                };
                capture::capture_framebuffer_region(frame, dst, &tex)?;
                tex
            }
            ScopedSource::Texture(tex) => tex.clone(),
        };

        let n = self.n_passes;
        // Chain is only ready if every pass program exists.
        let chain_ready = n > 0
            && (0..n).all(|i| {
                Shaders::get_from_frame(frame)
                    .program(ProgramType::Scoped(self.key, i))
                    .is_some()
            });

        if !chain_ready {
            // Passthrough: blit the source texture unchanged.
            return frame.render_texture_from_to(
                &input_tex,
                Rectangle::from_size(input_tex.size().to_f64()),
                dst,
                damage,
                &[],
                frame.transformation().invert(),
                1.,
                None,
                &[],
            );
        }

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
            Uniform::new("niri_size", (self.size_phys.0, self.size_phys.1)),
        ]);

        // `input` advances through the pass chain: pass 0 reads the source, later passes read the
        // prior pass's offscreen output.
        let mut input = input_tex;

        for i in 0..n {
            // v1 scoped shaders: no feedback. All five samplers alias `input`.
            let mut textures = HashMap::new();
            textures.insert("niri_screen".to_string(), input.clone());
            textures.insert("niri_source".to_string(), input.clone());
            textures.insert("niri_prev".to_string(), input.clone());
            textures.insert("niri_screen_prev".to_string(), input.clone());
            textures.insert("niri_buffer".to_string(), input.clone());

            let element = ShaderRenderElement::new(
                ProgramType::Scoped(self.key, i),
                self.area.size,
                None,
                self.scale,
                1.,
                uniforms.clone(),
                textures,
                Kind::Unspecified,
            );

            if i + 1 < n {
                // Intermediate pass: render into this pass's offscreen; its texture feeds the next
                // pass's `input`.
                let mut guard = frame.renderer();
                let renderer = guard.as_mut();
                match self.offscreens[i].render(
                    renderer,
                    Scale::from(self.scale as f64),
                    &[element],
                ) {
                    Ok((off_elem, _sync, _data)) => {
                        let next = off_elem.texture().clone();
                        drop(guard);
                        input = next;
                    }
                    Err(err) => {
                        drop(guard);
                        warn!("scoped pass {i} failed: {err:?}");
                        // Best effort: feed the input forward unchanged.
                    }
                }
            } else {
                // Last pass: composite directly into the output framebuffer.
                // ShaderRenderElement::src() is the unit rectangle, so pass a unit src for a 1:1
                // full-area mapping of the input texture onto dst.
                RenderElement::<GlesRenderer>::draw(
                    &element,
                    frame,
                    Rectangle::from_size((1., 1.).into()),
                    dst,
                    damage,
                    &[],
                    None,
                )?;
            }
        }

        Ok(())
    }
}

impl<'render> RenderElement<TtyRenderer<'render>> for ScopedShaderElement {
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
