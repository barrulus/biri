//! Built-in colour filters, so accessibility users do not have to write GLSL.
//!
//! Every generated source is niri-dialect, premultiplied-alpha safe, and static (no animation
//! uniforms), which is what keeps a colour filter free of continuous-redraw cost.

use tracing::warn;

use crate::utils::FloatOrInt;

/// Names of the built-in presets, in documentation order.
pub const BUILTIN_PRESET_NAMES: [&str; 4] = ["grayscale", "invert", "saturation", "temperature"];

/// Rec.709 luma weights, matching what the compositor uses elsewhere for perceptual brightness.
const LUMA: &str = "vec3(0.2126, 0.7152, 0.0722)";

/// Reference white for the temperature filter. `temperature kelvin=6500` is exact identity.
const REFERENCE_KELVIN: f64 = 6500.0;

/// A reference to a shader preset by name, with the properties the named preset may use.
///
/// Which property applies depends on the preset: `amount` for `saturation`, `kelvin` for
/// `temperature`. Supplying one a preset does not use is warned about and ignored.
#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ShaderPresetRefPart {
    #[knuffel(argument)]
    pub name: String,
    #[knuffel(property)]
    pub amount: Option<FloatOrInt<0, 10>>,
    #[knuffel(property)]
    pub kelvin: Option<FloatOrInt<1000, 40000>>,
}

/// Warn about properties the named preset ignores, so a typo is visible rather than silent.
fn warn_unused(name: &str, r: &ShaderPresetRefPart, uses_amount: bool, uses_kelvin: bool) {
    if r.amount.is_some() && !uses_amount {
        warn!("shader preset {name:?}: 'amount' is not used by this preset, ignoring");
    }
    if r.kelvin.is_some() && !uses_kelvin {
        warn!("shader preset {name:?}: 'kelvin' is not used by this preset, ignoring");
    }
}

/// Convert a colour temperature to a linear RGB multiplier, normalised against the 6500 K
/// reference white so that 6500 K is exactly `(1, 1, 1)` and no channel ever exceeds 1.
pub fn kelvin_to_rgb(kelvin: f64) -> (f64, f64, f64) {
    let (r, g, b) = raw_kelvin_to_rgb(kelvin);
    let (rw, gw, bw) = raw_kelvin_to_rgb(REFERENCE_KELVIN);

    // Divide by the reference white so 6500 K lands on exactly 1,1,1 ...
    let (r, g, b) = (r / rw, g / gw, b / bw);
    // ... then scale down if a cool temperature pushed a channel above 1, which would clip.
    let max = r.max(g).max(b);
    if max > 1.0 {
        (r / max, g / max, b / max)
    } else {
        (r, g, b)
    }
}

/// Tanner Helland's black-body approximation, returning 0..1 per channel.
fn raw_kelvin_to_rgb(kelvin: f64) -> (f64, f64, f64) {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;

    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let g = if t <= 66.0 {
        99.470_802_586 * t.ln() - 161.119_568_166
    } else {
        288.122_169_528 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223 * (t - 10.0).ln() - 305.044_792_729
    };

    (
        r.clamp(0.0, 255.0) / 255.0,
        g.clamp(0.0, 255.0) / 255.0,
        b.clamp(0.0, 255.0) / 255.0,
    )
}

/// Generate the GLSL for a built-in preset, or `None` if `r.name` is not a built-in.
pub fn builtin_preset_source(r: &ShaderPresetRefPart) -> Option<String> {
    match r.name.as_str() {
        "grayscale" => {
            warn_unused("grayscale", r, false, false);
            // Luma is linear in rgb, so it applies to premultiplied values directly.
            Some(format!(
                "vec4 global_color(vec3 niri_coord) {{ \
                 vec4 c = tex2D_screen(niri_coord.xy); \
                 float l = dot(c.rgb, {LUMA}); \
                 return vec4(vec3(l), c.a); }}"
            ))
        }
        "invert" => {
            warn_unused("invert", r, false, false);
            // Unpremultiplied invert is 1 - rgb/a, which repremultiplies to exactly a - rgb.
            Some(String::from(
                "vec4 global_color(vec3 niri_coord) { \
                 vec4 c = tex2D_screen(niri_coord.xy); \
                 return vec4(c.a - c.rgb, c.a); }",
            ))
        }
        "saturation" => {
            warn_unused("saturation", r, true, false);
            let amount = r.amount.map_or(1.5, |a| a.0);
            // mix is linear, so premultiplied-safe; the clamp keeps amount > 1 a valid
            // premultiplied colour by bounding each channel to [0, a].
            Some(format!(
                "vec4 global_color(vec3 niri_coord) {{ \
                 vec4 c = tex2D_screen(niri_coord.xy); \
                 float l = dot(c.rgb, {LUMA}); \
                 return vec4(clamp(mix(vec3(l), c.rgb, {amount:.6}), 0.0, c.a), c.a); }}"
            ))
        }
        "temperature" => {
            warn_unused("temperature", r, false, true);
            let kelvin = r.kelvin.map_or(4000.0, |k| k.0);
            let (kr, kg, kb) = kelvin_to_rgb(kelvin);
            // A per-channel constant scale is linear, so premultiplied-safe.
            Some(format!(
                "vec4 global_color(vec3 niri_coord) {{ \
                 vec4 c = tex2D_screen(niri_coord.xy); \
                 return vec4(c.rgb * vec3({kr:.6}, {kg:.6}, {kb:.6}), c.a); }}"
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refp(name: &str) -> ShaderPresetRefPart {
        ShaderPresetRefPart {
            name: String::from(name),
            amount: None,
            kelvin: None,
        }
    }

    #[test]
    fn every_builtin_resolves() {
        for name in BUILTIN_PRESET_NAMES {
            let src = builtin_preset_source(&refp(name))
                .unwrap_or_else(|| panic!("built-in {name} did not resolve"));
            assert!(
                src.contains("vec4 global_color(vec3"),
                "built-in {name} is not niri-dialect: {src}"
            );
            assert!(
                src.contains("tex2D_screen"),
                "built-in {name} does not sample the screen: {src}"
            );
        }
    }

    #[test]
    fn unknown_name_is_none() {
        assert!(builtin_preset_source(&refp("does-not-exist")).is_none());
    }

    #[test]
    fn every_builtin_is_static() {
        // A built-in that mentioned any animation token would force continuous redraws on the
        // output forever. GlobalShaderCaps::scan is a substring match, so this is exact.
        for name in BUILTIN_PRESET_NAMES {
            let src = builtin_preset_source(&refp(name)).unwrap();
            let caps = crate::global_shader::GlobalShaderCaps::scan(&src, false);
            assert!(
                !caps.is_animating(),
                "built-in {name} scans as animating: {caps:?}"
            );
        }
    }

    #[test]
    fn saturation_uses_amount_and_defaults() {
        let mut r = refp("saturation");
        let default_src = builtin_preset_source(&r).unwrap();
        assert!(
            default_src.contains("1.500000"),
            "default amount of 1.5 missing: {default_src}"
        );

        r.amount = Some(crate::utils::FloatOrInt(0.0));
        let gray = builtin_preset_source(&r).unwrap();
        assert!(
            gray.contains("0.000000"),
            "explicit amount of 0 missing: {gray}"
        );
        assert_ne!(gray, default_src);
    }

    #[test]
    fn temperature_6500k_is_identity() {
        // The reference white is 6500 K, so it must produce an exact 1,1,1 multiplier —
        // otherwise `temperature kelvin=6500` would silently tint the screen.
        let (r, g, b) = kelvin_to_rgb(6500.0);
        assert!((r - 1.0).abs() < 1e-9, "r = {r}");
        assert!((g - 1.0).abs() < 1e-9, "g = {g}");
        assert!((b - 1.0).abs() < 1e-9, "b = {b}");
    }

    #[test]
    fn temperature_warm_is_red_dominant_and_unclipped() {
        let (r, g, b) = kelvin_to_rgb(3000.0);
        assert!(r > g && g > b, "3000 K should be warm: {r} {g} {b}");
        for c in [r, g, b] {
            assert!((0.0..=1.0).contains(&c), "channel out of range: {c}");
        }
    }

    #[test]
    fn temperature_cool_is_blue_dominant_and_unclipped() {
        let (r, g, b) = kelvin_to_rgb(10000.0);
        assert!(b > g && g > r, "10000 K should be cool: {r} {g} {b}");
        for c in [r, g, b] {
            assert!((0.0..=1.0).contains(&c), "channel out of range: {c}");
        }
    }

    #[test]
    fn temperature_uses_kelvin_property() {
        let mut r = refp("temperature");
        let default_src = builtin_preset_source(&r).unwrap();
        r.kelvin = Some(crate::utils::FloatOrInt(6500.0));
        let identity_src = builtin_preset_source(&r).unwrap();
        assert_ne!(default_src, identity_src, "kelvin property was ignored");
        assert!(identity_src.contains("1.000000"), "6500 K not identity: {identity_src}");
    }
}
