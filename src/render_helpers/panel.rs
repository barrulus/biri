use std::collections::HashMap;
use std::rc::Rc;

use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture, Uniform};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::backend::renderer::{ContextId, Frame as _};
use smithay::gpu_span_location;
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform};

use super::panel_quad::{bounding_box, sampling_matrix};
use super::renderer::NiriRenderer;
use super::shader_element::ShaderRenderElement;
use super::shaders::{mat3_uniform, ProgramType, Shaders};
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};
use crate::render_helpers::renderer::AsGlesFrame as _;

/// Comparable snapshot of everything that feeds into the panel's shader uniforms + texture, so
/// `update()` can skip rebuilding (and thus skip damaging) the retained element when nothing
/// actually changed frame-to-frame (mirrors `BorderRenderElement`'s `Parameters`).
#[derive(Debug, Clone, PartialEq)]
struct Parameters {
    corners: [[f64; 2]; 4],
    // Texture *contents* are compared via the source `OffscreenRenderElement`'s commit counter
    // rather than the `GlesTexture` itself (textures aren't comparable, and a fresh clone is
    // handed in every frame regardless of whether the underlying pixels changed).
    texture_commit: CommitCounter,
    dim: f32,
    scale: f64,
    alpha: f32,
}

/// A texture drawn as a perspective-tilted quad (carousel panel).
#[derive(Debug)]
pub struct PanelRenderElement {
    inner: ShaderRenderElement,
    /// Renderer context the current texture was created against, checked in `draw` the same way
    /// `OffscreenRenderElement::draw` guards against cross-context textures (spike review finding
    /// 2). `GlesTexture` carries no such id itself, so the caller must supply it.
    context_id: ContextId<GlesTexture>,
    params: Parameters,
}

impl PanelRenderElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        corners: [[f64; 2]; 4],
        texture: GlesTexture,
        texture_commit: CommitCounter,
        context_id: ContextId<GlesTexture>,
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
        Some(Self {
            inner: elem,
            context_id,
            params: Parameters {
                corners,
                texture_commit,
                dim,
                scale,
                alpha,
            },
        })
    }

    /// Refreshes this retained element in place, keeping its stable `Id` (and thus damage
    /// tracking) across frames. Rebuilds the shader uniforms + texture map only if `corners`,
    /// the texture's commit counter, `dim`, `scale` or `alpha` actually changed since the last
    /// call — mirrors `BorderRenderElement::update`'s params-compare-then-rebuild pattern.
    /// Returns whether a rebuild happened.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        corners: [[f64; 2]; 4],
        texture: GlesTexture,
        texture_commit: CommitCounter,
        context_id: ContextId<GlesTexture>,
        dim: f32,
        scale: f64,
        alpha: f32,
    ) -> bool {
        // The context id is cheap to store and is not itself damage-relevant (a context change
        // always accompanies a fresh texture, i.e. a commit-counter change), so always refresh it
        // rather than folding it into the `Parameters` comparison.
        self.context_id = context_id;

        let params = Parameters {
            corners,
            texture_commit,
            dim,
            scale,
            alpha,
        };
        if self.params == params {
            return false;
        }

        self.params = params;

        let Some(inv) = sampling_matrix(&corners) else {
            // Degenerate quad: keep the last-good visual, but `params` is already updated so we
            // don't keep retrying this every frame while it stays degenerate.
            return false;
        };

        let (bx, by, bw, bh) = bounding_box(&corners);
        let mut textures = HashMap::new();
        textures.insert(String::from("niri_panel_tex"), texture);

        self.inner.update(
            Size::from((bw, bh)),
            None,
            scale as f32,
            alpha,
            Rc::new([
                mat3_uniform("niri_panel_inv", inv),
                Uniform::new("niri_panel_dim", dim),
            ]),
            textures,
        );

        // `ShaderRenderElement::update` doesn't touch location (only `with_location`, a
        // by-value builder, does), so swap the inner element out momentarily to reposition it.
        let inner = std::mem::replace(
            &mut self.inner,
            ShaderRenderElement::empty(ProgramType::Panel, Kind::Unspecified),
        );
        self.inner = inner.with_location(Point::from((bx, by)));

        true
    }

    pub fn has_shader(renderer: &mut impl NiriRenderer) -> bool {
        Shaders::get(renderer)
            .program(ProgramType::Panel)
            .is_some()
    }
}

impl Element for PanelRenderElement {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for PanelRenderElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        if frame.context_id() != self.context_id {
            warn!("trying to render panel texture from different renderer");
            return Ok(());
        }

        let _span = tracy_client::span!("PanelRenderElement::draw");
        frame.with_gpu_span(gpu_span_location!("PanelRenderElement::draw"), |frame| {
            RenderElement::<GlesRenderer>::draw(
                &self.inner,
                frame,
                src,
                dst,
                damage,
                opaque_regions,
                cache,
            )
        })
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

impl<'render> RenderElement<TtyRenderer<'render>> for PanelRenderElement {
    fn draw(
        &self,
        frame: &mut TtyFrame<'_, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        let frame = frame.as_gles_frame();
        RenderElement::<GlesRenderer>::draw(self, frame, src, dst, damage, opaque_regions, cache)?;
        Ok(())
    }

    fn underlying_storage(
        &self,
        renderer: &mut TtyRenderer<'render>,
    ) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}
