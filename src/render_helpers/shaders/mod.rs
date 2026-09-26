use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use glam::Mat3;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
    UniformType, UniformValue,
};
use smithay::backend::renderer::ImportMem;
use smithay::utils::Size;

use super::renderer::NiriRenderer;
use super::shader_element::ShaderProgram;
use crate::render_helpers::blur::BlurProgram;

pub struct Shaders {
    pub border: Option<ShaderProgram>,
    pub decoration_light: Option<ShaderProgram>,
    pub decorations: RefCell<HashMap<u64, ShaderProgram>>,
    pub panel: Option<ShaderProgram>,
    pub shadow: Option<ShaderProgram>,
    pub clipped_surface: Option<GlesTexProgram>,
    pub postprocess_and_clip: Option<GlesTexProgram>,
    pub resize: Option<ShaderProgram>,
    pub drag_physics: Option<ShaderProgram>,
    pub gradient_fade: Option<GlesTexProgram>,
    pub blur: Option<BlurProgram>,
    pub custom_resize: RefCell<Option<ShaderProgram>>,
    pub custom_close: RefCell<Option<ShaderProgram>>,
    pub custom_open: RefCell<Option<ShaderProgram>>,
    pub custom_global_passes: RefCell<Vec<ShaderProgram>>,
    pub custom_global_pass_buffers: RefCell<Vec<Option<ShaderProgram>>>,
    pub scoped: RefCell<HashMap<u64, Vec<ShaderProgram>>>,
    /// 1×1 transparent-black texture used to seed a feedback shader's first-frame niri_prev /
    /// niri_buffer (an "empty feedback buffer"), instead of ingesting the live screen.
    pub black_texture: GlesTexture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramType {
    Border,
    DecorationLight,
    Decoration(u64),
    Panel,
    Shadow,
    Resize,
    DragPhysics,
    Close,
    Open,
    Global,
    GlobalBuffer,
    GlobalPass(usize),
    GlobalPassBuffer(usize),
    Scoped(u64, usize),
}

impl Shaders {
    fn compile(renderer: &mut GlesRenderer) -> Self {
        let _span = tracy_client::span!("Shaders::compile");

        let border = compile_decoration_program(renderer, None)
            .map_err(|err| warn!("error compiling border shader: {err:?}"))
            .ok();

        let decoration_light = ShaderProgram::compile(
            renderer,
            include_str!("decoration_light.frag"),
            &[
                UniformName::new("light_gain", UniformType::_1f),
                UniformName::new("light_tex_scale", UniformType::_2f),
            ],
            &["light_texture"],
        )
        .map_err(|err| warn!("error compiling decoration light shader: {err:?}"))
        .ok();

        let panel = ShaderProgram::compile(
            renderer,
            include_str!("panel.frag"),
            &[
                UniformName::new("niri_panel_inv", UniformType::Matrix3x3),
                UniformName::new("niri_panel_dim", UniformType::_1f),
            ],
            &["niri_panel_tex"],
        )
        .map_err(|err| {
            warn!("error compiling panel shader: {err:?}");
        })
        .ok();

        let shadow = ShaderProgram::compile(
            renderer,
            concat!(
                include_str!("shadow.frag"),
                include_str!("rounding_alpha.frag")
            ),
            &[
                UniformName::new("shadow_color", UniformType::_4f),
                UniformName::new("sigma", UniformType::_1f),
                UniformName::new("input_to_geo", UniformType::Matrix3x3),
                UniformName::new("geo_size", UniformType::_2f),
                UniformName::new("corner_radius", UniformType::_4f),
                UniformName::new("window_input_to_geo", UniformType::Matrix3x3),
                UniformName::new("window_geo_size", UniformType::_2f),
                UniformName::new("window_corner_radius", UniformType::_4f),
            ],
            &[],
        )
        .map_err(|err| {
            warn!("error compiling shadow shader: {err:?}");
        })
        .ok();

        let clipped_surface = renderer
            .compile_custom_texture_shader(
                concat!(
                    include_str!("clipped_surface.frag"),
                    include_str!("rounding_alpha.frag"),
                    "\nvec4 postprocess(vec4 color) { return color; }",
                ),
                &[
                    UniformName::new("niri_scale", UniformType::_1f),
                    UniformName::new("geo_size", UniformType::_2f),
                    UniformName::new("corner_radius", UniformType::_4f),
                    UniformName::new("input_to_geo", UniformType::Matrix3x3),
                ],
            )
            .map_err(|err| {
                warn!("error compiling clipped surface shader: {err:?}");
            })
            .ok();

        let postprocess_and_clip = renderer
            .compile_custom_texture_shader(
                concat!(
                    include_str!("clipped_surface.frag"),
                    include_str!("rounding_alpha.frag"),
                    include_str!("postprocess.frag"),
                ),
                &[
                    UniformName::new("niri_scale", UniformType::_1f),
                    UniformName::new("geo_size", UniformType::_2f),
                    UniformName::new("corner_radius", UniformType::_4f),
                    UniformName::new("input_to_geo", UniformType::Matrix3x3),
                    UniformName::new("noise", UniformType::_1f),
                    UniformName::new("saturation", UniformType::_1f),
                    UniformName::new("bg_color", UniformType::_4f),
                    UniformName::new("surface_to_geo", UniformType::Matrix3x3),
                    UniformName::new("alpha_mask_enabled", UniformType::_1i),
                    UniformName::new("alpha_threshold", UniformType::_1f),
                    UniformName::new("surface_tex", UniformType::_1i),
                ],
            )
            .map_err(|err| {
                warn!("error compiling postprocess_and_clip shader: {err:?}");
            })
            .ok();

        let mut drag_uniforms = vec![
            UniformName::new("area", UniformType::_4f),
            UniformName::new("window", UniformType::_4f),
            UniformName::new("source_rect", UniformType::_4f),
            UniformName::new("texture_size", UniformType::_2f),
        ];
        for i in 0..16 {
            drag_uniforms.push(UniformName::new(
                format!("deformation_{i}"),
                UniformType::_2f,
            ));
        }
        let drag_physics = ShaderProgram::compile(
            renderer,
            include_str!("drag_physics.frag"),
            &drag_uniforms,
            &["niri_tex"],
        )
        .map_err(|err| warn!("error compiling drag physics shader: {err:?}"))
        .ok();

        let resize = compile_resize_program(renderer, include_str!("resize.frag"))
            .map_err(|err| {
                warn!("error compiling resize shader: {err:?}");
            })
            .ok();

        let gradient_fade = renderer
            .compile_custom_texture_shader(
                include_str!("gradient_fade.frag"),
                &[UniformName::new("cutoff", UniformType::_2f)],
            )
            .map_err(|err| {
                warn!("error compiling gradient fade shader: {err:?}");
            })
            .ok();

        let blur = BlurProgram::compile(renderer)
            .map_err(|err| {
                warn!("error compiling blur shaders: {err:?}");
            })
            .ok();

        // 1×1 transparent black; sampled (clamped) it returns black for every uv at any size.
        let black_texture = renderer
            .import_memory(&[0u8, 0, 0, 0], Fourcc::Abgr8888, Size::from((1, 1)), false)
            .expect("importing a 1x1 black texture must not fail");

        Self {
            border,
            decoration_light,
            decorations: RefCell::new(HashMap::new()),
            panel,
            shadow,
            clipped_surface,
            postprocess_and_clip,
            resize,
            drag_physics,
            gradient_fade,
            blur,
            custom_resize: RefCell::new(None),
            custom_close: RefCell::new(None),
            custom_open: RefCell::new(None),
            custom_global_passes: RefCell::new(Vec::new()),
            custom_global_pass_buffers: RefCell::new(Vec::new()),
            scoped: RefCell::new(HashMap::new()),
            black_texture,
        }
    }

    pub fn get_from_frame<'a>(frame: &'a mut GlesFrame<'_, '_>) -> &'a Self {
        let data = frame.egl_context().user_data();
        data.get()
            .expect("shaders::init() must be called when creating the renderer")
    }

    pub fn get(renderer: &mut impl NiriRenderer) -> &Self {
        let renderer = renderer.as_gles_renderer();
        let data = renderer.egl_context().user_data();
        data.get()
            .expect("shaders::init() must be called when creating the renderer")
    }

    pub fn replace_custom_resize_program(
        &self,
        program: Option<ShaderProgram>,
    ) -> Option<ShaderProgram> {
        self.custom_resize.replace(program)
    }

    pub fn replace_custom_close_program(
        &self,
        program: Option<ShaderProgram>,
    ) -> Option<ShaderProgram> {
        self.custom_close.replace(program)
    }

    pub fn replace_custom_open_program(
        &self,
        program: Option<ShaderProgram>,
    ) -> Option<ShaderProgram> {
        self.custom_open.replace(program)
    }

    pub fn program(&self, program: ProgramType) -> Option<ShaderProgram> {
        match program {
            ProgramType::DragPhysics => self.drag_physics.clone(),
            ProgramType::Border => self.border.clone(),
            ProgramType::DecorationLight => self.decoration_light.clone(),
            ProgramType::Decoration(key) => self
                .decorations
                .borrow()
                .get(&key)
                .cloned()
                .or_else(|| self.border.clone()),
            ProgramType::Panel => self.panel.clone(),
            ProgramType::Shadow => self.shadow.clone(),
            ProgramType::Resize => self
                .custom_resize
                .borrow()
                .clone()
                .or_else(|| self.resize.clone()),
            ProgramType::Close => self.custom_close.borrow().clone(),
            ProgramType::Open => self.custom_open.borrow().clone(),
            ProgramType::Global => self.custom_global_passes.borrow().first().cloned(),
            ProgramType::GlobalBuffer => self
                .custom_global_pass_buffers
                .borrow()
                .first()
                .cloned()
                .flatten(),
            ProgramType::GlobalPass(i) => self.custom_global_passes.borrow().get(i).cloned(),
            ProgramType::GlobalPassBuffer(i) => self
                .custom_global_pass_buffers
                .borrow()
                .get(i)
                .cloned()
                .flatten(),
            ProgramType::Scoped(key, i) => self
                .scoped
                .borrow()
                .get(&key)
                .and_then(|chain| chain.get(i))
                .cloned(),
        }
    }
}

pub fn init(renderer: &mut GlesRenderer) {
    let shaders = Shaders::compile(renderer);
    let data = renderer.egl_context().user_data();
    if !data.insert_if_missing(|| shaders) {
        error!("shaders were already compiled");
    }
}

fn compile_resize_program(
    renderer: &mut GlesRenderer,
    src: &str,
) -> Result<ShaderProgram, GlesError> {
    let mut program = include_str!("resize_prelude.frag").to_string();
    program.push_str(src);
    program.push_str(include_str!("resize_epilogue.frag"));
    program.push_str(include_str!("rounding_alpha.frag"));

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_input_to_curr_geo", UniformType::Matrix3x3),
            UniformName::new("niri_curr_geo_to_prev_geo", UniformType::Matrix3x3),
            UniformName::new("niri_curr_geo_to_next_geo", UniformType::Matrix3x3),
            UniformName::new("niri_curr_geo_size", UniformType::_2f),
            UniformName::new("niri_geo_to_tex_prev", UniformType::Matrix3x3),
            UniformName::new("niri_geo_to_tex_next", UniformType::Matrix3x3),
            UniformName::new("niri_progress", UniformType::_1f),
            UniformName::new("niri_clamped_progress", UniformType::_1f),
            UniformName::new("niri_corner_radius", UniformType::_4f),
            UniformName::new("niri_clip_to_geometry", UniformType::_1f),
        ],
        &["niri_tex_prev", "niri_tex_next"],
    )
}

pub fn set_custom_resize_program(renderer: &mut GlesRenderer, src: Option<&str>) {
    let program = if let Some(src) = src {
        match compile_resize_program(renderer, src) {
            Ok(program) => Some(program),
            Err(err) => {
                warn!("error compiling custom resize shader: {err:?}");
                return;
            }
        }
    } else {
        None
    };

    if let Some(prev) = Shaders::get(renderer).replace_custom_resize_program(program) {
        if let Err(err) = prev.destroy(renderer) {
            warn!("error destroying previous custom resize shader: {err:?}");
        }
    }
}

fn compile_close_program(
    renderer: &mut GlesRenderer,
    src: &str,
) -> Result<ShaderProgram, GlesError> {
    let mut program = include_str!("close_prelude.frag").to_string();
    program.push_str(src);
    program.push_str(include_str!("close_epilogue.frag"));

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_input_to_geo", UniformType::Matrix3x3),
            UniformName::new("niri_geo_size", UniformType::_2f),
            UniformName::new("niri_geo_to_tex", UniformType::Matrix3x3),
            UniformName::new("niri_progress", UniformType::_1f),
            UniformName::new("niri_clamped_progress", UniformType::_1f),
            UniformName::new("niri_random_seed", UniformType::_1f),
        ],
        &["niri_tex"],
    )
}

pub fn set_custom_close_program(renderer: &mut GlesRenderer, src: Option<&str>) {
    let program = if let Some(src) = src {
        match compile_close_program(renderer, src) {
            Ok(program) => Some(program),
            Err(err) => {
                warn!("error compiling custom close shader: {err:?}");
                return;
            }
        }
    } else {
        None
    };

    if let Some(prev) = Shaders::get(renderer).replace_custom_close_program(program) {
        if let Err(err) = prev.destroy(renderer) {
            warn!("error destroying previous custom close shader: {err:?}");
        }
    }
}

fn compile_open_program(
    renderer: &mut GlesRenderer,
    src: &str,
) -> Result<ShaderProgram, GlesError> {
    let mut program = include_str!("open_prelude.frag").to_string();
    program.push_str(src);
    program.push_str(include_str!("open_epilogue.frag"));

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_input_to_geo", UniformType::Matrix3x3),
            UniformName::new("niri_geo_size", UniformType::_2f),
            UniformName::new("niri_geo_to_tex", UniformType::Matrix3x3),
            UniformName::new("niri_progress", UniformType::_1f),
            UniformName::new("niri_clamped_progress", UniformType::_1f),
            UniformName::new("niri_random_seed", UniformType::_1f),
        ],
        &["niri_tex"],
    )
}

pub fn set_custom_open_program(renderer: &mut GlesRenderer, src: Option<&str>) {
    let program = if let Some(src) = src {
        match compile_open_program(renderer, src) {
            Ok(program) => Some(program),
            Err(err) => {
                warn!("error compiling custom open shader: {err:?}");
                return;
            }
        }
    } else {
        None
    };

    if let Some(prev) = Shaders::get(renderer).replace_custom_open_program(program) {
        if let Err(err) = prev.destroy(renderer) {
            warn!("error destroying previous custom open shader: {err:?}");
        }
    }
}

fn compile_global_program(
    renderer: &mut GlesRenderer,
    src: &str,
    hyprland: bool,
) -> Result<ShaderProgram, GlesError> {
    let mut program = if hyprland {
        include_str!("global_hypr_prelude.frag").to_string()
    } else {
        include_str!("global_prelude.frag").to_string()
    };
    program.push_str(src);
    if !hyprland {
        program.push_str(include_str!("global_epilogue.frag"));
    }

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_time", UniformType::_1f),
            UniformName::new("niri_cursor", UniformType::_2f),
            UniformName::new("niri_region", UniformType::_4f),
            UniformName::new("niri_output_size", UniformType::_2f),
        ],
        &[
            "niri_screen",
            "niri_prev",
            "niri_screen_prev",
            "niri_buffer",
            "niri_source",
        ],
    )
}

fn compile_global_buffer_program(
    renderer: &mut GlesRenderer,
    src: &str,
) -> Result<ShaderProgram, GlesError> {
    let mut program = include_str!("global_prelude.frag").to_string();
    program.push_str(src);
    program.push_str(include_str!("global_buffer_epilogue.frag"));

    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("niri_time", UniformType::_1f),
            UniformName::new("niri_cursor", UniformType::_2f),
            UniformName::new("niri_region", UniformType::_4f),
            UniformName::new("niri_output_size", UniformType::_2f),
        ],
        &[
            "niri_screen",
            "niri_prev",
            "niri_screen_prev",
            "niri_buffer",
            "niri_source",
        ],
    )
}

/// Install a whole pass chain: for each pass compile its display program and (niri mode only,
/// when the source defines `global_buffer`) a dedicated-buffer program. Replaces both registry
/// vecs wholesale and destroys every previously-installed program. An empty slice disables the
/// chain. If any pass fails to compile, the whole chain is dropped (empty vecs) so we never run a
/// partial chain.
pub fn set_custom_global_passes(renderer: &mut GlesRenderer, passes: &[(String, bool)]) {
    let mut display = Vec::with_capacity(passes.len());
    let mut buffers = Vec::with_capacity(passes.len());
    let mut ok = true;

    for (src, hyprland) in passes {
        match compile_global_program(renderer, src, *hyprland) {
            Ok(p) => display.push(p),
            Err(err) => {
                warn!("error compiling global shader pass: {err:?}");
                ok = false;
                break;
            }
        }
        let buffer = match (src.contains("global_buffer"), *hyprland) {
            (true, false) => match compile_global_buffer_program(renderer, src) {
                Ok(p) => Some(p),
                Err(err) => {
                    warn!("error compiling global_buffer pass: {err:?}");
                    None
                }
            },
            _ => None,
        };
        buffers.push(buffer);
    }

    if !ok {
        // Destroy anything compiled this round before bailing.
        for p in display.drain(..) {
            let _ = p.destroy(renderer);
        }
        for p in buffers.drain(..).flatten() {
            let _ = p.destroy(renderer);
        }
        display = Vec::new();
        buffers = Vec::new();
    }

    let shaders = Shaders::get(renderer);
    let old_display = shaders.custom_global_passes.replace(display);
    let old_buffers = shaders.custom_global_pass_buffers.replace(buffers);
    for p in old_display {
        if let Err(err) = p.destroy(renderer) {
            warn!("error destroying previous global pass: {err:?}");
        }
    }
    for p in old_buffers.into_iter().flatten() {
        if let Err(err) = p.destroy(renderer) {
            warn!("error destroying previous global_buffer pass: {err:?}");
        }
    }
}

/// Stable hash of a resolved pass list — the cache key for a scoped shader chain. Pure function of
/// the source strings and hyprland flags (no nondeterminism).
pub fn scoped_key(passes: &[(String, bool)]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (src, hypr) in passes {
        src.hash(&mut h);
        hypr.hash(&mut h);
    }
    h.finish()
}

/// Install the set of scoped shader chains. Each entry is a resolved `(source, hyprland)` pass
/// list. Compiles any key not already cached, and destroys any cached key no longer referenced.
/// Identical pass lists share one compiled chain (same key).
pub fn set_scoped_programs(renderer: &mut GlesRenderer, chains: &[Vec<(String, bool)>]) {
    use std::collections::HashSet;
    let wanted: HashSet<u64> = chains.iter().map(|c| scoped_key(c)).collect();

    // Drop + destroy stale keys: collect the chains first (releasing the borrow), then destroy.
    let stale_chains: Vec<Vec<ShaderProgram>> = {
        let mut scoped = Shaders::get(renderer).scoped.borrow_mut();
        let stale: Vec<u64> = scoped
            .keys()
            .copied()
            .filter(|k| !wanted.contains(k))
            .collect();
        stale
            .into_iter()
            .filter_map(|k| scoped.remove(&k))
            .collect()
    };
    for chain in stale_chains {
        for p in chain {
            if let Err(err) = p.destroy(renderer) {
                warn!("error destroying scoped shader program: {err:?}");
            }
        }
    }

    // Compile missing keys.
    for passes in chains {
        if passes.is_empty() {
            continue;
        }
        let key = scoped_key(passes);
        if Shaders::get(renderer).scoped.borrow().contains_key(&key) {
            continue;
        }
        let mut compiled = Vec::with_capacity(passes.len());
        let mut ok = true;
        for (src, hyprland) in passes {
            match compile_global_program(renderer, src, *hyprland) {
                Ok(p) => compiled.push(p),
                Err(err) => {
                    warn!("error compiling scoped shader: {err:?}");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            Shaders::get(renderer)
                .scoped
                .borrow_mut()
                .insert(key, compiled);
        } else {
            for p in compiled {
                let _ = p.destroy(renderer);
            }
        }
    }
}

pub fn mat3_uniform(name: &str, mat: Mat3) -> Uniform<'_> {
    Uniform::new(
        name,
        UniformValue::Matrix3x3 {
            matrices: vec![mat.to_cols_array()],
            transpose: false,
        },
    )
}

/// Custom and legacy rings share a coordinate contract and the same editable wax source.
fn compile_decoration_program(
    renderer: &mut GlesRenderer,
    source: Option<&str>,
) -> Result<ShaderProgram, GlesError> {
    let mut program = if source.is_some() {
        "#define CUSTOM_DECORATION\n".to_owned()
    } else {
        "#define WAX_STRENGTH rainbow_ripple.y\n#define WAX_BRIGHTNESS rainbow_ripple.z\n#define WAX_PHASE rainbow_ripple.x\n".to_owned()
    };
    program.push_str(include_str!("border.frag"));
    program.push_str(include_str!("rounding_alpha.frag"));
    program.push_str(source.unwrap_or(include_str!(
        "../../../resources/shaders/focus-ring/rainbow-ripple.frag"
    )));
    ShaderProgram::compile(
        renderer,
        &program,
        &[
            UniformName::new("colorspace", UniformType::_1f),
            UniformName::new("hue_interpolation", UniformType::_1f),
            UniformName::new("color_from", UniformType::_4f),
            UniformName::new("color_to", UniformType::_4f),
            UniformName::new("grad_offset", UniformType::_2f),
            UniformName::new("grad_width", UniformType::_1f),
            UniformName::new("grad_vec", UniformType::_2f),
            UniformName::new("input_to_geo", UniformType::Matrix3x3),
            UniformName::new("geo_size", UniformType::_2f),
            UniformName::new("outer_radius", UniformType::_4f),
            UniformName::new("border_width", UniformType::_1f),
            UniformName::new("rainbow_ripple", UniformType::_4f),
            UniformName::new("ring_width", UniformType::_1f),
            UniformName::new("ring_draw_inside", UniformType::_1f),
            UniformName::new("emission_threshold", UniformType::_1f),
            UniformName::new("niri_time", UniformType::_1f),
        ],
        &[],
    )
}

/// Compile content-addressed programs once on load. Drop stale GPU resources on reload.
/// A bad program falls back to the normal border; other windows keep their own programs.
pub fn set_decoration_programs(renderer: &mut GlesRenderer, config: &niri_config::Config) {
    let layout_parts = config
        .outputs
        .0
        .iter()
        .filter_map(|o| o.layout.as_ref())
        .chain(
            config
                .workspaces
                .iter()
                .filter_map(|w| w.layout.as_ref().map(|l| &l.0)),
        );
    let layout_shaders = layout_parts
        .flat_map(|l| l.focus_ring.iter().chain(l.border.iter()))
        .filter_map(|r| r.shader.as_ref());
    let shaders = config
        .layout
        .focus_ring
        .shader
        .iter()
        .chain(config.layout.border.shader.iter())
        .chain(layout_shaders)
        .chain(
            config
                .window_rules
                .iter()
                .flat_map(|r| r.focus_ring.shader.iter().chain(r.border.shader.iter())),
        );
    let wanted: HashMap<_, _> = shaders.filter_map(|s| Some((s.key()?, s))).collect();
    let stale: Vec<_> = {
        let mut cache = Shaders::get(renderer).decorations.borrow_mut();
        let keys: Vec<_> = cache
            .keys()
            .copied()
            .filter(|k| !wanted.contains_key(k))
            .collect();
        keys.into_iter().filter_map(|k| cache.remove(&k)).collect()
    };
    for program in stale {
        let _ = program.destroy(renderer);
    }
    for (key, shader) in wanted {
        if Shaders::get(renderer)
            .decorations
            .borrow()
            .contains_key(&key)
        {
            continue;
        }
        match compile_decoration_program(renderer, shader.source()) {
            Ok(program) => {
                Shaders::get(renderer)
                    .decorations
                    .borrow_mut()
                    .insert(key, program);
            }
            Err(err) => {
                warn!(path = ?shader.path, "error compiling decoration shader; using configured colours: {err:?}")
            }
        }
    }
}
