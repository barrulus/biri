use knuffel::errors::DecodeError;

use crate::global_shader::{GlobalShaderPass, GlobalShaderPassPart};

/// A scalar that accepts both KDL integer and float literals and stores them as f64.
/// Used for geometry coordinate/dimension properties.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CoordF64(pub f64);

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for CoordF64 {
    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(type_name) = type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }
    }

    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        match &**val {
            knuffel::ast::Literal::Int(ref value) => match <i64 as TryFrom<_>>::try_from(value) {
                Ok(v) => Ok(CoordF64(v as f64)),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(CoordF64::default())
                }
            },
            knuffel::ast::Literal::Decimal(ref value) => match <f64 as TryFrom<_>>::try_from(value) {
                Ok(v) => Ok(CoordF64(v)),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(CoordF64::default())
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "unsupported value, only numbers are recognized",
                ));
                Ok(CoordF64::default())
            }
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
struct GeometryPart {
    #[knuffel(property)]
    x: CoordF64,
    #[knuffel(property)]
    y: CoordF64,
    #[knuffel(property)]
    width: CoordF64,
    #[knuffel(property)]
    height: CoordF64,
}

/// Resolved geometry rectangle.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<GeometryPart> for Geometry {
    fn from(p: GeometryPart) -> Self {
        Geometry {
            x: p.x.0,
            y: p.y.0,
            width: p.width.0,
            height: p.height.0,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct RegionShaderPart {
    #[knuffel(child)]
    geometry: Option<GeometryPart>,
    #[knuffel(child, unwrap(argument))]
    pub output: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionShader {
    pub geometry: Geometry,
    pub output: Option<String>,
    pub source: Option<String>,
    pub path: Option<String>,
    pub mode: String,
    pub passes: Vec<GlobalShaderPass>,
}

impl From<RegionShaderPart> for RegionShader {
    fn from(p: RegionShaderPart) -> Self {
        let mode = p.mode.clone().unwrap_or_else(|| String::from("niri"));
        let passes = p
            .passes
            .iter()
            .map(|pp| GlobalShaderPass {
                source: pp.source.clone(),
                path: pp.path.clone(),
                mode: pp.mode.clone().unwrap_or_else(|| mode.clone()),
            })
            .collect();
        RegionShader {
            geometry: p.geometry.map(Geometry::from).unwrap_or_default(),
            output: p.output,
            source: p.source,
            path: p.path,
            mode,
            passes,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn region_shader_parses() {
        let config = Config::parse_mem(
            r##"
            region-shader {
                geometry x=100 y=100 width=800 height=600
                output "DP-1"
                source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
            }
            region-shader {
                geometry x=0 y=0 width=1920 height=40
                source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy)*0.5; }"
            }
            "##,
        )
        .unwrap();
        assert_eq!(config.region_shaders.len(), 2);
        assert_eq!(config.region_shaders[0].geometry.width, 800.0);
        assert_eq!(config.region_shaders[0].output.as_deref(), Some("DP-1"));
        assert!(config.region_shaders[1].output.is_none());
        assert_eq!(config.region_shaders[1].geometry.height, 40.0);
    }
}
