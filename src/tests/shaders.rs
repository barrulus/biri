use niri_config::Config;

use super::Fixture;
use crate::render_helpers::shaders::Shaders;

#[test]
fn egl_headless_shaders_startup_and_reload() {
    let config = || {
        Config::parse_mem(
            r#"
        layout {
            focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(0.1); }"; }; }
        }
        workspace "rings" {
            layout { border { shader { source "vec4 ring_color(vec2 p) { return vec4(0.2); }"; }; }; }
        }
        global-shader {
            enable true
            source "vec4 global_color(vec3 c) { return vec4(1.0); }"
        }
        region-shader {
            geometry x=0 y=0 width=100 height=100
            source "vec4 global_color(vec3 c) { return vec4(0.1); }"
        }
        window-rule {
            focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(0.3); }"; }; }
            shader {
                source "vec4 global_color(vec3 c) { return vec4(0.2); }"
            }
        }
        window-shaders {
            preset "window" {
                source "vec4 global_color(vec3 c) { return vec4(0.3); }"
            }
        }
        output "headless-1" {
            layout { focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(0.4); }"; }; }; }
            shader {
                source "vec4 global_color(vec3 c) { return vec4(0.4); }"
            }
        }
        output-shaders {
            preset "output" {
                source "vec4 global_color(vec3 c) { return vec4(0.5); }"
            }
        }
        "#,
        )
        .unwrap()
    };
    let mut f = Fixture::with_config(config());
    let state = f.niri_state();
    let programs = |renderer: &mut smithay::backend::renderer::gles::GlesRenderer| {
        let shaders = Shaders::get(renderer);
        let global = shaders.custom_global_passes.borrow().clone();
        let scoped = shaders.scoped.borrow().clone();
        let decorations = shaders.decorations.borrow().clone();
        (global, scoped, decorations)
    };
    let initial = state.backend.with_primary_renderer(programs).unwrap();
    assert_eq!(initial.0.len(), 1, "global shader must compile at startup");
    assert_eq!(
        initial.1.len(),
        5,
        "all scoped shaders must compile at startup"
    );
    assert!(initial.1.values().all(|chain| chain.len() == 1));
    assert_eq!(
        initial.2.len(),
        4,
        "global, output, workspace and window ring shaders compile at startup"
    );

    // ShaderProgram equality checks identity, so this detects recompilation as well as
    // accidentally adding or dropping programs on an unchanged/unrelated reload.
    state.reload_config(Ok(config()));
    assert_eq!(
        state.backend.with_primary_renderer(programs).unwrap(),
        initial
    );
    let mut changed = config();
    changed.shaders_in_capture = !changed.shaders_in_capture;
    // Changing geometry refreshes the scoped registry but should reuse its programs.
    changed.region_shaders[0].geometry.width = 200.0;
    state.reload_config(Ok(changed));
    assert_eq!(
        state.backend.with_primary_renderer(programs).unwrap(),
        initial
    );

    let mut changed = config();
    changed.global_shader.source =
        Some("vec4 global_color(vec3 c) { return vec4(0.6); }".to_owned());
    state.reload_config(Ok(changed));
    let reloaded = state.backend.with_primary_renderer(programs).unwrap();
    assert_eq!(reloaded.0.len(), 1);
    assert_ne!(reloaded.0, initial.0);
    assert_eq!(reloaded.1, initial.1);

    let mut changed = config();
    changed.layout.focus_ring.shader = Config::parse_mem(
        r#"layout { focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(0.8); }"; }; }; }"#,
    ).unwrap().layout.focus_ring.shader;
    state.reload_config(Ok(changed));
    let reloaded = state.backend.with_primary_renderer(programs).unwrap();
    assert_eq!(reloaded.2.len(), 4);
    assert_ne!(reloaded.2, initial.2);

    state.reload_config(Ok(Config::default()));
    let cleared = state.backend.with_primary_renderer(programs).unwrap();
    assert!(cleared.0.is_empty());
    assert!(cleared.1.is_empty());
    assert!(cleared.2.is_empty());
}

#[test]
fn egl_umbriel_shader_collection_compiles() {
    use smithay::backend::egl::native::EGLSurfacelessDisplay;
    use smithay::backend::egl::{EGLContext, EGLDisplay};
    use smithay::backend::renderer::gles::GlesRenderer;

    use crate::render_helpers::shaders;

    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
        GlesRenderer::new(EGLContext::new(&display).unwrap()).unwrap()
    };
    crate::render_helpers::resources::init(&mut renderer);
    shaders::init(&mut renderer);
    assert!(Shaders::get(&mut renderer).drag_physics.is_some());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/shaders");
    for entry in std::fs::read_dir(root.join("focus-ring")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "frag") {
            continue;
        }
        let config = Config::parse_mem(&format!(
            "layout {{ focus-ring {{ shader {{ path {:?}; draw-inside true; }}; }}; }}",
            path
        ))
        .unwrap();
        shaders::set_decoration_programs(&mut renderer, &config);
        let key = config.layout.focus_ring.shader.unwrap().key().unwrap();
        assert!(
            Shaders::get(&mut renderer)
                .decorations
                .borrow()
                .contains_key(&key),
            "{}",
            path.display()
        );
    }
    for entry in std::fs::read_dir(root.join("window")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "frag") {
            continue;
        }
        let chain = vec![(std::fs::read_to_string(&path).unwrap(), false)];
        shaders::set_scoped_programs(&mut renderer, &[chain]);
        assert_eq!(
            Shaders::get(&mut renderer)
                .scoped
                .borrow()
                .values()
                .filter(|p| !p.is_empty())
                .count(),
            1,
            "{}",
            path.display()
        );
    }
    let paper = Config::parse_mem(include_str!(
        "../../resources/shaders/close/close-paper.kdl"
    ))
    .unwrap();
    shaders::set_custom_open_program(
        &mut renderer,
        paper.animations.window_open.custom_shader.as_deref(),
    );
    shaders::set_custom_close_program(
        &mut renderer,
        paper.animations.window_close.custom_shader.as_deref(),
    );
    assert!(Shaders::get(&mut renderer).custom_open.borrow().is_some());
    assert!(Shaders::get(&mut renderer).custom_close.borrow().is_some());
    for dir in ["focus-ring", "drag", "window", "close"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "kdl") {
                let parsed = Config::parse(&path, &std::fs::read_to_string(&path).unwrap());
                assert!(
                    parsed.config.is_ok(),
                    "{}: {:?}",
                    path.display(),
                    parsed.config
                );
            }
        }
    }
}
