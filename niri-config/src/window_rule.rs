use knuffel::errors::DecodeError;
use niri_ipc::ColumnDisplay;

use crate::appearance::{
    BackgroundEffect, BackgroundEffectRule, BlockOutFrom, BorderRule, CornerRadius, ShadowRule,
    TabIndicatorRule,
};
use crate::global_shader::GlobalShaderPassPart;
use crate::layout::DefaultPresetSize;
use crate::utils::{MergeWith, RegexEq};
use crate::FloatOrInt;

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ShaderRule {
    #[knuffel(child, unwrap(argument))]
    pub source: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub path: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub mode: Option<String>,
    #[knuffel(children(name = "pass"))]
    pub passes: Vec<GlobalShaderPassPart>,
}

impl ShaderRule {
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

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowRule {
    #[knuffel(children(name = "match"))]
    pub matches: Vec<Match>,
    #[knuffel(children(name = "exclude"))]
    pub excludes: Vec<Match>,

    // Rules applied at initial configure.
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child)]
    pub default_window_height: Option<DefaultPresetSize>,
    #[knuffel(children(name = "open-on-output"), unwrap(argument))]
    pub open_on_output: Vec<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_on_workspace: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_maximized_to_edges: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_fullscreen: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_floating: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_sticky: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub open_focused: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub on_xdg_activate: Option<OnXdgActivate>,

    // Rules applied dynamically.
    #[knuffel(child, unwrap(argument))]
    pub min_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub min_height: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_width: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub max_height: Option<u16>,

    #[knuffel(child, default)]
    pub focus_ring: BorderRule,
    #[knuffel(child, default)]
    pub border: BorderRule,
    #[knuffel(child, default)]
    pub shadow: ShadowRule,
    #[knuffel(child, default)]
    pub tab_indicator: TabIndicatorRule,
    #[knuffel(child, unwrap(argument))]
    pub draw_border_with_background: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, unwrap(argument))]
    pub clip_to_geometry: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub baba_is_float: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub float_above_fullscreen: Option<bool>,
    #[knuffel(child)]
    pub block_out_from: Option<BlockOutFrom>,
    #[knuffel(child, unwrap(argument))]
    pub variable_refresh_rate: Option<VrrRule>,
    #[knuffel(child, unwrap(argument, str))]
    pub default_column_display: Option<ColumnDisplay>,
    #[knuffel(child)]
    pub default_floating_position: Option<FloatingPosition>,
    #[knuffel(child, unwrap(argument))]
    pub scroll_factor: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub pinch_sensitivity: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub tiled_state: Option<bool>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
    #[knuffel(child)]
    pub shader: Option<ShaderRule>,
    #[knuffel(child, default)]
    pub popups: PopupsRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VrrRule {
    /// No effect on VRR.
    Off,
    /// Enable VRR when the window is visible.
    On,
    /// Enable VRR when the window is the current fullscreen window.
    FullscreenOnly,
}

/// Rules for popup surfaces.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct PopupsRule {
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knuffel(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knuffel(child, default)]
    pub background_effect: BackgroundEffectRule,
}

/// Resolved popup-specific rules.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ResolvedPopupsRules {
    /// Extra opacity to draw popups with.
    pub opacity: Option<f32>,

    /// Corner radius to assume the popups have.
    pub geometry_corner_radius: Option<CornerRadius>,

    /// Background effect configuration for popups.
    pub background_effect: BackgroundEffect,
}

impl MergeWith<PopupsRule> for ResolvedPopupsRules {
    fn merge_with(&mut self, part: &PopupsRule) {
        if let Some(x) = part.opacity {
            self.opacity = Some(x);
        }
        if let Some(x) = part.geometry_corner_radius {
            self.geometry_corner_radius = Some(x);
        }
        self.background_effect.merge_with(&part.background_effect);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Match {
    #[knuffel(property, str)]
    pub app_id: Option<RegexEq>,
    #[knuffel(property, str)]
    pub title: Option<RegexEq>,
    #[knuffel(property)]
    pub is_active: Option<bool>,
    #[knuffel(property)]
    pub is_focused: Option<bool>,
    #[knuffel(property)]
    pub is_active_in_column: Option<bool>,
    #[knuffel(property)]
    pub is_floating: Option<bool>,
    #[knuffel(property)]
    pub is_sticky: Option<bool>,
    #[knuffel(property)]
    pub is_window_cast_target: Option<bool>,
    #[knuffel(property)]
    pub is_urgent: Option<bool>,
    #[knuffel(property)]
    pub at_startup: Option<bool>,
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct FloatingPosition {
    #[knuffel(property)]
    pub x: FloatOrInt<-65535, 65535>,
    #[knuffel(property)]
    pub y: FloatOrInt<-65535, 65535>,
    #[knuffel(property, default)]
    pub relative_to: RelativeTo,
}

#[derive(knuffel::DecodeScalar, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnXdgActivate {
    Ignore,
    SetUrgent,
    Focus,
}

#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTo {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for VrrRule {
    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        _ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<VrrRule, DecodeError<S>> {
        const ERROR: &str = "expected `true`, `\"fullscreen-only\"`, or `false`";
        match &**val {
            knuffel::ast::Literal::Bool(b) => Ok(if *b { VrrRule::On } else { VrrRule::Off }),
            knuffel::ast::Literal::String(s) => match &s[..] {
                // Also accept bool strings to reduce confusion.
                "true" => Ok(VrrRule::On),
                "false" => Ok(VrrRule::Off),
                "fullscreen-only" => Ok(VrrRule::FullscreenOnly),
                _ => Err(DecodeError::conversion(val, ERROR)),
            },
            _ => Err(DecodeError::conversion(val, ERROR)),
        }
    }

    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(typ) = type_name {
            ctx.emit_error(DecodeError::TypeName {
                span: typ.span().clone(),
                found: Some((**typ).clone()),
                expected: knuffel::errors::ExpectedType::no_type(),
                rust_type: stringify!(VrrRule),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn window_rule_shader_parses() {
        let config = Config::parse_mem(
            r##"
            window-rule {
                match app-id="Alacritty"
                shader {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).bgra; }"
                }
            }
            window-rule {
                match app-id="foo"
            }
            "##,
        )
        .unwrap();
        let r0 = &config.window_rules[0];
        assert!(r0.shader.is_some());
        let chain = r0.shader.as_ref().unwrap().pass_sources(|_| None);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].0.contains("bgra"));
        assert!(config.window_rules[1].shader.is_none());
    }
}
