use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use knuffel::errors::DecodeError;

use crate::{BasePath, FloatOrInt, Includes};

/// A shader body resolved once per config load, never during rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct DecorationShader {
    source: Option<Arc<str>>,
    key: Option<u64>,
    pub path: Option<PathBuf>,
    pub enable: bool,
    pub animated: bool,
    pub speed: FloatOrInt<0, 10>,
    pub draw_inside: bool,
    pub padding: FloatOrInt<0, 1024>,
    pub light: Option<DecorationLight>,
}

/// Opt-in light spill derived from the shader's actual premultiplied output.
#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct DecorationLight {
    #[knuffel(property, default = true)]
    pub enable: bool,
    #[knuffel(property, default = FloatOrInt(80.))]
    pub spread: FloatOrInt<1, 256>,
    #[knuffel(property, default = FloatOrInt(1.))]
    pub intensity: FloatOrInt<0, 4>,
    #[knuffel(property, default = FloatOrInt(0.5))]
    pub threshold: FloatOrInt<0, 1>,
}

#[derive(knuffel::Decode)]
struct ShaderPart {
    #[knuffel(child, unwrap(argument))]
    source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    path: Option<PathBuf>,
    #[knuffel(child, unwrap(argument), default = true)]
    enable: bool,
    #[knuffel(child, unwrap(argument), default = true)]
    animated: bool,
    #[knuffel(child, unwrap(argument), default = FloatOrInt(1.))]
    speed: FloatOrInt<0, 10>,
    #[knuffel(child, unwrap(argument), default = FloatOrInt(0.))]
    padding: FloatOrInt<0, 1024>,
    #[knuffel(child, unwrap(argument), default = false)]
    draw_inside: bool,
    #[knuffel(child)]
    light: Option<DecorationLight>,
}

impl DecorationShader {
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn key(&self) -> Option<u64> {
        self.key.filter(|_| self.enable)
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::Decode<S> for DecorationShader {
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let part = ShaderPart::decode_node(node, ctx)?;
        if part.source.is_some() && part.path.is_some() {
            return Err(DecodeError::unexpected(
                node,
                "shader",
                "use either source or path, not both",
            ));
        }
        if part.enable && part.source.is_none() && part.path.is_none() {
            return Err(DecodeError::missing(node, "shader needs source or path"));
        }

        let mut source = part.source.map(Arc::from);
        let path = if let Some(path) = part.path {
            let path = if let Ok(rest) = path.strip_prefix("~") {
                std::env::home_dir()
                    .ok_or_else(|| DecodeError::missing(node, "cannot expand home directory"))?
                    .join(rest)
            } else {
                ctx.get::<BasePath>()
                    .map(|base| base.0.join(&path))
                    .unwrap_or(path)
            };
            // Reuse the config watcher's dependency list, including missing files so
            // creating/fixing them triggers another load. Includes resolve relative
            // to the file that contains this shader block.
            if let Some(includes) = ctx.get::<Rc<RefCell<Includes>>>() {
                includes.borrow_mut().0.push(path.clone());
            }
            if part.enable {
                match std::fs::read_to_string(&path) {
                    Ok(text) => source = Some(Arc::from(text)),
                    Err(err) => tracing::warn!(
                        ?path,
                        "cannot read decoration shader; using configured colours: {err}"
                    ),
                }
            }
            Some(path)
        } else {
            None
        };
        let key = source.as_ref().map(|source: &Arc<str>| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            source.hash(&mut hash);
            hash.finish()
        });
        Ok(Self {
            source,
            key,
            path,
            enable: part.enable,
            animated: part.animated,
            speed: part.speed,
            padding: part.padding,
            draw_inside: part.draw_inside,
            light: part.light,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::MergeWith;
    use crate::Config;

    #[test]
    fn shader_rules_replace_and_disable() {
        let config = Config::parse_mem(
            r#"
            layout { focus-ring { shader { source "global"; padding 12; light; }; }; }
            window-rule { focus-ring { width 8; }; }
            window-rule { focus-ring { shader { source "per-window"; animated false; }; }; }
            window-rule { focus-ring { shader { enable false; }; }; }
        "#,
        )
        .unwrap();
        let mut ring = config.layout.focus_ring;
        let original_key = ring.shader.as_ref().unwrap().key();
        ring.merge_with(&config.window_rules[0].focus_ring);
        assert_eq!(ring.width, 8.);
        assert_eq!(ring.shader.as_ref().unwrap().key(), original_key);
        assert!(ring.shader.as_ref().unwrap().light.is_some());
        ring.merge_with(&config.window_rules[1].focus_ring);
        let shader = ring.shader.as_ref().unwrap();
        assert_ne!(shader.key(), original_key);
        assert_eq!(shader.source.as_deref(), Some("per-window"));
        assert_eq!(shader.padding.0, 0., "a shader block replaces all settings");
        assert!(!shader.animated);
        assert!(
            shader.light.is_none(),
            "a shader block also replaces lighting"
        );
        ring.merge_with(&config.window_rules[2].focus_ring);
        assert_eq!(ring.shader.as_ref().unwrap().key(), None);
    }

    #[test]
    fn rejects_invalid_shader_settings() {
        for fields in [
            "",
            "source \"x\"; path \"y\";",
            "source \"x\"; padding -1;",
            "source \"x\"; speed 11;",
            "source \"x\"; light spread=0;",
            "source \"x\"; light spread=257;",
            "source \"x\"; light intensity=-1;",
            "source \"x\"; light intensity=5;",
            "source \"x\"; light threshold=1.1;",
        ] {
            assert!(
                Config::parse_mem(&format!(
                    "layout {{ focus-ring {{ shader {{ {fields} }}; }}; }}"
                ))
                .is_err(),
                "{fields}"
            );
        }
    }

    #[test]
    fn light_is_opt_in_and_does_not_recompile_the_shader() {
        let parse = |light| {
            Config::parse_mem(&format!(
                "layout {{ focus-ring {{ shader {{ source \"x\"; {light} }}; }}; }}"
            ))
            .unwrap()
            .layout
            .focus_ring
            .shader
            .unwrap()
        };
        let plain = parse("");
        assert!(plain.light.is_none());
        let defaults = parse("light;");
        let light = defaults.light.unwrap();
        assert!(light.enable);
        assert_eq!(
            (light.spread.0, light.intensity.0, light.threshold.0),
            (80., 1., 0.5)
        );
        let tuned = parse("light enable=false spread=120 intensity=0.75 threshold=0.25;");
        assert!(!tuned.light.unwrap().enable);
        assert_eq!(tuned.light.unwrap().spread.0, 120.);
        assert_ne!(defaults, tuned, "lighting edits must trigger config reload");
        assert_eq!(defaults.key(), tuned.key());
        assert_eq!(plain.key(), tuned.key());
    }

    #[test]
    fn paths_are_config_relative_and_watched_even_when_missing() {
        let parsed = Config::parse(
            std::path::Path::new("/tmp/biri-ring-config-test/config.kdl"),
            "layout { focus-ring { shader { path \"missing.frag\"; }; }; }",
        );
        let shader = parsed.config.unwrap().layout.focus_ring.shader.unwrap();
        assert_eq!(
            shader.path.as_deref(),
            Some(std::path::Path::new(
                "/tmp/biri-ring-config-test/missing.frag"
            ))
        );
        assert_eq!(parsed.includes, vec![shader.path.unwrap()]);
        assert_eq!(shader.key, None);
    }
}
