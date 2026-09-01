//! Named window-shader presets, applied to windows at runtime via the
//! `toggle-window-shader` / `cycle-window-shader` actions.

use crate::global_shader::GlobalShaderPassPart;

/// One `window-shaders` block; presets from all blocks accumulate in config order.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowShadersPart {
    #[knuffel(children(name = "preset"))]
    pub presets: Vec<WindowShaderPreset>,
}

/// A named shader preset with the same source spec as a window-rule `shader {}` block.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowShaderPreset {
    #[knuffel(argument)]
    pub name: String,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

impl WindowShaderPreset {
    pub fn pass_sources(&self, expand: impl Fn(&str) -> Option<String>) -> Vec<(String, bool)> {
        let mode = self.mode.as_deref().unwrap_or("niri");
        crate::region_shader::resolve_scoped_pass_sources(
            &self.source,
            &self.path,
            mode,
            &self.passes,
            expand,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn window_shaders_presets_parse() {
        let config = Config::parse_mem(
            r##"
            window-shaders {
                preset "fire" {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).bgra; }"
                }
                preset "crt" {
                    source "vec4 global_color(vec3 c){ return vec4(c.xy, 0.0, 1.0); }"
                    mode "hyprland"
                }
            }
            "##,
        )
        .unwrap();

        assert_eq!(config.window_shaders.len(), 2);

        let fire = &config.window_shaders[0];
        assert_eq!(fire.name, "fire");
        let chain = fire.pass_sources(|_| None);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].0.contains("bgra"));
        assert!(!chain[0].1);

        let crt = &config.window_shaders[1];
        assert_eq!(crt.name, "crt");
        let chain = crt.pass_sources(|_| None);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].1);
    }

    #[test]
    fn window_shaders_blocks_accumulate() {
        let config = Config::parse_mem(
            r##"
            window-shaders {
                preset "a" {
                    source "vec4 global_color(vec3 c){ return vec4(1.0); }"
                }
            }
            window-shaders {
                preset "b" {
                    source "vec4 global_color(vec3 c){ return vec4(0.0); }"
                }
            }
            "##,
        )
        .unwrap();

        let names: Vec<_> = config
            .window_shaders
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b"]);
    }
}
