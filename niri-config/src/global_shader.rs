use crate::utils::{Flag, MergeWith};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShader {
    pub enable: bool,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub reads_cursor: bool,
}

impl Default for GlobalShader {
    fn default() -> Self {
        Self {
            enable: false,
            source: None,
            path: None,
            mode: String::from("niri"),
            reads_cursor: false,
        }
    }
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
}

impl MergeWith<GlobalShaderPart> for GlobalShader {
    fn merge_with(&mut self, part: &GlobalShaderPart) {
        merge!((self, part), enable, reads_cursor);
        merge_clone_opt!((self, part), source, path);
        merge_clone!((self, part), mode);
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

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
}
