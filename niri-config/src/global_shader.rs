use crate::utils::{Flag, MergeWith};

/// Which animation/feedback inputs a compiled global shader references, derived by scanning
/// the resolved source. A substring match: it may over-report (token in a comment) — which
/// only costs extra redraws — but cannot under-report, because in GLSL a uniform must appear
/// by its literal name to be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlobalShaderCaps {
    pub uses_time: bool,
    pub uses_cursor: bool,
    pub uses_prev: bool,
    pub uses_buffer: bool,
}

impl GlobalShaderCaps {
    pub fn scan(src: &str, hyprland: bool) -> Self {
        if hyprland {
            // Hyprland dialect aliases niri_time as `time`; it has no cursor or prev uniforms.
            GlobalShaderCaps {
                uses_time: src.contains("time"),
                uses_cursor: false,
                uses_prev: false,
                uses_buffer: false,
            }
        } else {
            // `niri_prev` is the raw sampler; `tex2D_prev` is the helper most shaders actually
            // call. Either reference means the shader depends on the feedback buffer.
            GlobalShaderCaps {
                uses_time: src.contains("niri_time"),
                uses_cursor: src.contains("niri_cursor"),
                // Feedback: previous output, previous screen, or the dedicated buffer all evolve
                // frame-to-frame, so any of them counts as feedback.
                uses_prev: src.contains("niri_prev")
                    || src.contains("tex2D_prev")
                    || src.contains("niri_screen_prev")
                    || src.contains("tex2D_screen_prev"),
                uses_buffer: src.contains("global_buffer")
                    || src.contains("niri_buffer")
                    || src.contains("tex2D_buffer"),
            }
        }
    }

    /// Capabilities for a multi-pass chain: the union of every pass's caps. The chain animates
    /// if any pass does, so the whole chain redraws every frame.
    pub fn scan_chain(passes: &[(String, bool)]) -> Self {
        passes.iter().fold(Self::default(), |acc, (src, hyprland)| {
            let c = Self::scan(src, *hyprland);
            GlobalShaderCaps {
                uses_time: acc.uses_time || c.uses_time,
                uses_cursor: acc.uses_cursor || c.uses_cursor,
                uses_prev: acc.uses_prev || c.uses_prev,
                uses_buffer: acc.uses_buffer || c.uses_buffer,
            }
        })
    }

    /// Animating shaders depend on time or the feedback buffer, so they must redraw every frame
    /// to progress even when the desktop is idle.
    pub fn is_animating(&self) -> bool {
        self.uses_time || self.uses_prev || self.uses_buffer
    }
}

/// Redraw scheduling for the global shader. `Auto` derives the decision from [`GlobalShaderCaps`];
/// the others force it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedrawMode {
    #[default]
    Auto,
    OnDamage,
    Continuous,
}

impl RedrawMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "on-damage" => RedrawMode::OnDamage,
            "continuous" => RedrawMode::Continuous,
            _ => RedrawMode::Auto,
        }
    }

    /// Whether to schedule a redraw every frame (animate while idle).
    pub fn wants_continuous_redraw(self, caps: GlobalShaderCaps) -> bool {
        match self {
            RedrawMode::Continuous => true,
            RedrawMode::OnDamage => false,
            RedrawMode::Auto => caps.is_animating(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShader {
    pub enable: bool,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub reads_cursor: bool,
    /// Effect footprint radius in logical px for cursor-local shaders; `None` = whole output.
    pub cursor_radius: Option<u32>,
    /// Redraw scheduling: "auto" | "on-damage" | "continuous". Parsed via `RedrawMode::parse`.
    pub redraw: String,
    pub passes: Vec<GlobalShaderPass>,
}

impl Default for GlobalShader {
    fn default() -> Self {
        Self {
            enable: false,
            source: None,
            path: None,
            mode: String::from("niri"),
            reads_cursor: false,
            cursor_radius: None,
            redraw: String::from("auto"),
            passes: Vec::new(),
        }
    }
}

/// One pass in a multi-pass chain. Resolved form of [`GlobalShaderPassPart`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShaderPass {
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq, Eq)]
pub struct GlobalShaderPassPart {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq, Eq)]
pub struct GlobalShaderPart {
    #[knuffel(child)]
    pub enable: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(child)]
    pub reads_cursor: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub cursor_radius: Option<u32>,
    #[knuffel(child, unwrap(argument))]
    pub redraw: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

impl MergeWith<GlobalShaderPart> for GlobalShader {
    fn merge_with(&mut self, part: &GlobalShaderPart) {
        merge!((self, part), enable, reads_cursor);
        merge_clone_opt!((self, part), source, path, cursor_radius);
        merge_clone!((self, part), mode, redraw);
        if !part.passes.is_empty() {
            self.passes = part
                .passes
                .iter()
                .map(|p| GlobalShaderPass {
                    source: p.source.clone(),
                    path: p.path.clone(),
                    // Per-pass mode defaults to the chain-level mode.
                    mode: p.mode.clone().unwrap_or_else(|| self.mode.clone()),
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GlobalShaderCaps, RedrawMode};
    use crate::Config;

    #[test]
    fn caps_scan_niri_dialect() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_color(vec3 c){ return tex2D_prev(c.xy)*niri_time + niri_cursor.x; }",
            false,
        );
        assert_eq!(
            c,
            GlobalShaderCaps {
                uses_time: true,
                uses_cursor: true,
                uses_prev: true,
                uses_buffer: false
            }
        );
    }

    #[test]
    fn caps_scan_static_filter() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }",
            false,
        );
        assert_eq!(c, GlobalShaderCaps::default());
        assert!(!c.is_animating());
    }

    #[test]
    fn caps_scan_time_only_is_animating() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_color(vec3 c){ return vec4(niri_time); }",
            false,
        );
        assert!(c.uses_time && !c.uses_cursor && !c.uses_prev);
        assert!(c.is_animating());
    }

    #[test]
    fn caps_scan_hyprland_dialect() {
        // Hyprland aliases time as `time`; no cursor/prev uniforms exist.
        let with = GlobalShaderCaps::scan("void main(){ gl_FragColor = vec4(time); }", true);
        assert!(with.uses_time && !with.uses_cursor && !with.uses_prev);
        let without = GlobalShaderCaps::scan(
            "void main(){ gl_FragColor = texture2D(tex, v_texcoord); }",
            true,
        );
        assert!(!without.is_animating());
    }

    #[test]
    fn caps_scan_buffer_function() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_buffer(vec3 c){ return tex2D_buffer(c.xy); } vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }",
            false,
        );
        assert!(c.uses_buffer);
        assert!(c.is_animating());
    }

    #[test]
    fn caps_scan_screen_prev_is_feedback() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_color(vec3 c){ return tex2D_screen(c.xy) - tex2D_screen_prev(c.xy); }",
            false,
        );
        assert!(c.uses_prev); // screen_prev folds into the feedback/animating set
        assert!(c.is_animating());
    }

    #[test]
    fn caps_scan_plain_filter_not_buffer() {
        let c = GlobalShaderCaps::scan(
            "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }",
            false,
        );
        assert!(!c.uses_buffer);
        assert!(!c.is_animating());
    }

    #[test]
    fn redraw_mode_parse_and_decision() {
        let animating = GlobalShaderCaps {
            uses_time: true,
            ..Default::default()
        };
        let static_ = GlobalShaderCaps::default();
        assert!(RedrawMode::parse("auto").wants_continuous_redraw(animating));
        assert!(!RedrawMode::parse("auto").wants_continuous_redraw(static_));
        assert!(!RedrawMode::parse("on-damage").wants_continuous_redraw(animating));
        assert!(RedrawMode::parse("continuous").wants_continuous_redraw(static_));
        assert_eq!(RedrawMode::parse("bogus"), RedrawMode::Auto);
    }

    #[test]
    fn global_shader_defaults_disabled() {
        let config = Config::parse_mem("").unwrap();
        assert!(!config.global_shader.enable);
        assert_eq!(config.global_shader.mode, "niri");
        assert!(config.global_shader.source.is_none());
        assert!(!config.global_shader.reads_cursor);
    }

    #[test]
    fn global_shader_inline_source() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
                mode "niri"
                reads-cursor
            }
            "##,
        )
        .unwrap();
        assert!(config.global_shader.enable);
        assert!(config.global_shader.reads_cursor);
        assert_eq!(config.global_shader.mode, "niri");
        assert_eq!(
            config.global_shader.source.as_deref(),
            Some("vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }")
        );
    }

    #[test]
    fn global_shader_redraw_and_cursor_radius() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
                cursor-radius 200
                redraw "continuous"
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.global_shader.cursor_radius, Some(200));
        assert_eq!(config.global_shader.redraw, "continuous");
    }

    #[test]
    fn global_shader_redraw_defaults_to_auto() {
        let config = Config::parse_mem("").unwrap();
        assert_eq!(config.global_shader.redraw, "auto");
        assert_eq!(config.global_shader.cursor_radius, None);
    }

    #[test]
    fn global_shader_path_and_hyprland_mode() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                path "/tmp/crt.frag"
                mode "hyprland"
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.global_shader.path.as_deref(), Some("/tmp/crt.frag"));
        assert_eq!(config.global_shader.mode, "hyprland");
    }

    #[test]
    fn global_shader_pass_list_parses() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                pass {
                    path "blur.frag"
                }
                pass {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
                    mode "niri"
                }
                pass {
                    path "crt.frag"
                    mode "hyprland"
                }
            }
            "##,
        )
        .unwrap();
        assert!(config.global_shader.enable);
        assert_eq!(config.global_shader.passes.len(), 3);
        assert_eq!(
            config.global_shader.passes[0].path.as_deref(),
            Some("blur.frag")
        );
        assert_eq!(
            config.global_shader.passes[1].source.as_deref(),
            Some("vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }")
        );
        assert_eq!(config.global_shader.passes[1].mode, "niri");
        assert_eq!(config.global_shader.passes[2].mode, "hyprland");
    }

    #[test]
    fn caps_scan_chain_union() {
        // A static blur pass + an animated final pass => chain is animating.
        let chain = [
            (
                "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }".to_string(),
                false,
            ),
            (
                "vec4 global_color(vec3 c){ return vec4(niri_time); }".to_string(),
                false,
            ),
        ];
        let caps = GlobalShaderCaps::scan_chain(&chain);
        assert!(caps.uses_time);
        assert!(caps.is_animating());

        // All-static chain => not animating.
        let static_chain = [
            (
                "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }".to_string(),
                false,
            ),
            (
                "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).gbra; }".to_string(),
                false,
            ),
        ];
        let caps = GlobalShaderCaps::scan_chain(&static_chain);
        assert!(!caps.is_animating());
    }

    #[test]
    fn global_shader_no_passes_back_compat() {
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
            }
            "##,
        )
        .unwrap();
        assert!(config.global_shader.passes.is_empty());
        assert!(config.global_shader.source.is_some());
    }

    #[test]
    fn global_shader_pass_mode_inherits_block_mode() {
        // A pass with no explicit `mode` inherits the block-level `mode`; an explicit pass `mode`
        // overrides it. (The merge runs `mode` before the pass list, so the block mode is the
        // default applied to each pass.)
        let config = Config::parse_mem(
            r##"
            global-shader {
                enable
                mode "hyprland"
                pass {
                    path "a.frag"
                }
                pass {
                    path "b.frag"
                    mode "niri"
                }
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.global_shader.passes[0].mode, "hyprland"); // inherited from block
        assert_eq!(config.global_shader.passes[1].mode, "niri"); // explicit pass mode wins
    }
}
