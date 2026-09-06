//! Per-output shaders: the `output { shader { … } }` rule and the `output-shaders` preset block.
//!
//! Both bodies share one shape — inline `source`, a `path`, a `preset` reference, or a `pass`
//! chain of those. A `preset` at the top level of an `output { shader { … } }` body may name a
//! user preset; a `preset` inside a `pass` may only name a built-in. That asymmetry is what makes
//! preset recursion structurally impossible: a user preset body can only ever reach built-ins.

use tracing::warn;

use crate::shader_presets::{builtin_preset_source, ShaderPresetRefPart};

/// One pass of an output shader.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct OutputShaderPassPart {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(child)]
    pub preset: Option<ShaderPresetRefPart>,
}

/// The body of an `output { shader { … } }` block.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct OutputShaderPart {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(child)]
    pub preset: Option<ShaderPresetRefPart>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<OutputShaderPassPart>,
}

/// One `output-shaders` block; presets from all blocks accumulate in config order.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct OutputShadersPart {
    #[knuffel(children(name = "preset"))]
    pub presets: Vec<OutputShaderPreset>,
}

/// A named preset with the same body shape as an `output { shader { … } }` block.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct OutputShaderPreset {
    #[knuffel(argument)]
    pub name: String,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(child)]
    pub preset: Option<ShaderPresetRefPart>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<OutputShaderPassPart>,
}

/// Resolve one leaf spec (a pass, or a whole body with no pass chain) to `(source, hyprland)`.
/// Built-in presets only. `None` means unresolvable, which disables the whole chain.
fn resolve_leaf(
    source: &Option<String>,
    path: &Option<String>,
    mode: &str,
    preset: &Option<ShaderPresetRefPart>,
    expand: &dyn Fn(&str) -> Option<String>,
) -> Option<(String, bool)> {
    let set =
        usize::from(source.is_some()) + usize::from(path.is_some()) + usize::from(preset.is_some());
    if set > 1 {
        warn!("output shader: set only one of 'source', 'path' or 'preset', disabling");
        return None;
    }

    if let Some(r) = preset {
        return match builtin_preset_source(r) {
            // A built-in generates niri-dialect GLSL, so `mode` does not apply to it.
            Some(src) => Some((src, false)),
            None => {
                warn!("output shader: unknown preset {:?}, disabling", r.name);
                None
            }
        };
    }
    match (source, path) {
        (Some(s), None) if !s.trim().is_empty() => Some((s.clone(), mode == "hyprland")),
        (Some(_), None) => {
            warn!("output shader: empty source, disabling");
            None
        }
        (None, Some(p)) => match expand(p) {
            Some(s) => Some((s, mode == "hyprland")),
            None => {
                warn!("output shader: could not read path {p:?}, disabling");
                None
            }
        },
        _ => {
            warn!("output shader: no 'source', 'path' or 'preset', disabling");
            None
        }
    }
}

/// Shared body resolution. `user_presets` is `Some` only for a top-level `output { shader { … } }`
/// body; `None` inside a preset body, which is what forbids user-preset recursion.
fn resolve_body(
    source: &Option<String>,
    path: &Option<String>,
    mode: &Option<String>,
    preset: &Option<ShaderPresetRefPart>,
    passes: &[OutputShaderPassPart],
    user_presets: Option<&[OutputShaderPreset]>,
    expand: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, bool)> {
    let default_mode = mode.as_deref().unwrap_or("niri");

    if !passes.is_empty() {
        if source.is_some() || path.is_some() || preset.is_some() {
            warn!(
                "output shader: both a top-level source/path/preset and pass blocks; using passes"
            );
        }
        let mut out = Vec::with_capacity(passes.len());
        for p in passes {
            let pass_mode = p.mode.as_deref().unwrap_or(default_mode);
            // Passes resolve against built-ins only — never user presets.
            match resolve_leaf(&p.source, &p.path, pass_mode, &p.preset, expand) {
                Some(pair) => out.push(pair),
                None => return Vec::new(),
            }
        }
        return out;
    }

    // A top-level `preset` may name a user preset, which shadows a built-in of the same name.
    if let (Some(r), Some(presets)) = (preset, user_presets) {
        if let Some(user) = presets.iter().find(|p| p.name == r.name) {
            return user.pass_sources(expand);
        }
    }

    match resolve_leaf(source, path, default_mode, preset, expand) {
        Some(pair) => vec![pair],
        None => Vec::new(),
    }
}

impl OutputShaderPreset {
    /// Resolve this preset's body. Built-ins only: a preset body may not name another user preset.
    pub fn pass_sources(&self, expand: &dyn Fn(&str) -> Option<String>) -> Vec<(String, bool)> {
        resolve_body(
            &self.source,
            &self.path,
            &self.mode,
            &self.preset,
            &self.passes,
            None,
            expand,
        )
    }
}

impl OutputShaderPart {
    /// Resolve this output's shader body. A top-level `preset` resolves against `user_presets`
    /// first, then the built-in table.
    pub fn pass_sources(
        &self,
        user_presets: &[OutputShaderPreset],
        expand: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<(String, bool)> {
        resolve_body(
            &self.source,
            &self.path,
            &self.mode,
            &self.preset,
            &self.passes,
            Some(user_presets),
            expand,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    fn no_files(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn output_shaders_presets_parse_and_accumulate() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "night" {
                    preset "temperature" kelvin=3200
                }
            }
            output-shaders {
                preset "mono" {
                    preset "grayscale"
                }
            }
            "##,
        )
        .unwrap();

        let names: Vec<_> = config
            .output_shaders
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["night", "mono"]);

        let night = config.output_shaders[0].pass_sources(&no_files);
        assert_eq!(night.len(), 1);
        assert!(
            night[0].0.contains("c.rgb * vec3("),
            "not a temperature filter: {night:?}"
        );
        assert!(!night[0].1, "built-ins are never Hyprland dialect");

        let mono = config.output_shaders[1].pass_sources(&no_files);
        assert_eq!(mono.len(), 1);
        assert!(
            mono[0].0.contains("float l = dot"),
            "not grayscale: {mono:?}"
        );
    }

    #[test]
    fn output_shaders_preset_accepts_inline_source_and_pass_chain() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "raw" {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).bgra; }"
                }
                preset "warm-mono" {
                    pass { preset "grayscale"; }
                    pass { preset "temperature" kelvin=3500; }
                }
            }
            "##,
        )
        .unwrap();

        let raw = config.output_shaders[0].pass_sources(&no_files);
        assert_eq!(raw.len(), 1);
        assert!(raw[0].0.contains("bgra"));

        let chain = config.output_shaders[1].pass_sources(&no_files);
        assert_eq!(chain.len(), 2, "pass chain did not resolve: {chain:?}");
        assert!(chain[0].0.contains("float l = dot"));
        assert!(chain[1].0.contains("c.rgb * vec3("));
    }

    #[test]
    fn unknown_preset_name_disables_the_chain() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "broken" {
                    preset "no-such-filter"
                }
            }
            "##,
        )
        .unwrap();
        assert!(config.output_shaders[0].pass_sources(&no_files).is_empty());
    }

    #[test]
    fn unresolvable_path_disables_the_chain() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "fromfile" {
                    pass { preset "grayscale"; }
                    pass { path "missing.frag"; }
                }
            }
            "##,
        )
        .unwrap();
        assert!(config.output_shaders[0].pass_sources(&no_files).is_empty());
    }

    #[test]
    fn hyprland_mode_is_carried_for_hand_written_sources() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "hypr" {
                    source "void main(){ gl_FragColor = vec4(1.0); }"
                    mode "hyprland"
                }
            }
            "##,
        )
        .unwrap();
        let chain = config.output_shaders[0].pass_sources(&no_files);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].1, "mode \"hyprland\" was dropped");
    }
}
