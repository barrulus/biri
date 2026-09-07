# Per-output Colour / Shader Rule Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each output a persistent post-process shader that needs no geometry, with built-in
colour filters usable without writing GLSL, plus toggle/cycle actions.

**Architecture:** A new `output { shader { … } }` config child and an `output-shaders { preset }`
block resolve to the same `(source, hyprland)` pass-chain representation region shaders already
use. Rendering reuses `ScopedShaderElement` with the full output rect, so no new render primitive
is introduced. Built-in filters are generated GLSL strings produced in Rust at config-resolve time.

**Tech Stack:** Rust (nightly via devshell), `knuffel` KDL derive, `insta` inline snapshots,
smithay render elements, GLSL ES 1.00.

**Spec:** `docs/superpowers/specs/2026-09-06-per-output-shader-design.md`

## Global Constraints

- Build and test only via the devshell: `direnv exec . bash -c '<cargo command>'` from
  `/home/barrulus/dev/biri`. Never `nix develop …--command`, never `cargo +nightly` — the
  toolchain is already nightly.
- No AI attribution in commit messages. No `Co-Authored-By` lines.
- All generated GLSL is niri-dialect (`vec4 global_color(vec3 …)` + `tex2D_screen`), never
  Hyprland dialect, so every built-in resolves to `(source, hyprland = false)`.
- All generated GLSL must be premultiplied-alpha safe. Every operation used is linear in `rgb`,
  and any clamp bounds the result to `[0, a]`.
- Every built-in must be *static*: no `niri_time`, `niri_prev`, `tex2D_prev`, `niri_screen_prev`,
  `global_buffer`, `niri_buffer`, or `tex2D_buffer` tokens. `GlobalShaderCaps::scan` does a
  substring match, so even the word `time` inside a comment would falsely mark the chain
  animating and force continuous redraws. Do not write comments into generated sources.
- A chain that cannot be fully resolved resolves to the empty vec (shader disabled), never a
  partial chain. This matches `resolve_scoped_pass_sources`.
- KDL node terminators are newline, `;` or EOF — a closing `}` is NOT one. So
  `shader { preset "grayscale" }` on a single line does NOT parse; write the body multi-line (the
  form used throughout this plan and in all docs), or terminate the inner node with `;`.
- `cargo insta` can hang in this repo. When an inline snapshot in `niri-config/src/lib.rs` fails,
  edit the expected text by hand from the failure diff rather than running `cargo insta accept`.

---

### Task 1: Built-in shader preset table

Pure functions producing GLSL. No config wiring yet, so this task is fully unit-testable on its own.

**Files:**
- Create: `niri-config/src/shader_presets.rs`
- Modify: `niri-config/src/lib.rs:29-48` (add `pub mod shader_presets;` in the alphabetical module
  list, between `recent_windows` and `region_shader`), and `niri-config/src/lib.rs:50-72` (add the
  `pub use` re-export, keeping the existing alphabetical order)
- Test: inline `#[cfg(test)] mod tests` at the bottom of `niri-config/src/shader_presets.rs`

**Interfaces:**
- Consumes: `crate::utils::FloatOrInt` (defined at `niri-config/src/utils.rs:15` as
  `pub struct FloatOrInt<const MIN: i32, const MAX: i32>(pub f64)`).
- Produces:
  - `pub struct ShaderPresetRefPart { pub name: String, pub amount: Option<FloatOrInt<0, 10>>, pub kelvin: Option<FloatOrInt<1000, 40000>> }`, deriving `knuffel::Decode`
  - `pub fn builtin_preset_source(r: &ShaderPresetRefPart) -> Option<String>`
  - `pub fn kelvin_to_rgb(kelvin: f64) -> (f64, f64, f64)`
  - `pub const BUILTIN_PRESET_NAMES: [&str; 4]`

- [ ] **Step 1: Write the failing tests**

Create `niri-config/src/shader_presets.rs` containing only this test module for now (the file will
not compile until Step 3 — that is expected and is what Step 2 checks):

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `direnv exec . bash -c 'cargo test -p niri-config shader_presets'`

Expected: FAIL to compile — `cannot find function 'builtin_preset_source' in this scope`, plus
`unresolved module` for `shader_presets` until the `pub mod` line is added.

- [ ] **Step 3: Write the implementation**

Add `pub mod shader_presets;` to `niri-config/src/lib.rs` in the module list (line ~44, keeping
alphabetical order — after `pub mod recent_windows;`, before `pub mod region_shader;`), and the
re-export alongside the others (~line 64):

```rust
pub use crate::shader_presets::{
    builtin_preset_source, ShaderPresetRefPart, BUILTIN_PRESET_NAMES,
};
```

Then prepend this above the test module in `niri-config/src/shader_presets.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `direnv exec . bash -c 'cargo test -p niri-config shader_presets'`

Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/shader_presets.rs niri-config/src/lib.rs
git commit -m "config: add built-in colour filter shader presets"
```

---

### Task 2: `output-shaders` presets block

**Files:**
- Create: `niri-config/src/output_shader.rs`
- Modify: `niri-config/src/lib.rs` — module list, re-exports, `Config` struct field (after
  `pub window_shaders: Vec<WindowShaderPreset>,` at line ~108), parse dispatch (after the
  `"window-shaders"` arm at line ~242), and the `Config::default()` inline snapshot at line ~2657
- Test: inline `#[cfg(test)] mod tests` at the bottom of `niri-config/src/output_shader.rs`

**Interfaces:**
- Consumes: `ShaderPresetRefPart` and `builtin_preset_source` from Task 1.
- Produces:
  - `pub struct OutputShaderPassPart { pub source: Option<String>, pub path: Option<String>, pub mode: Option<String>, pub preset: Option<ShaderPresetRefPart> }`
  - `pub struct OutputShaderPart { pub source, pub path, pub mode, pub preset, pub passes: Vec<OutputShaderPassPart> }` (same four plus passes)
  - `pub struct OutputShadersPart { pub presets: Vec<OutputShaderPreset> }`
  - `pub struct OutputShaderPreset { pub name: String, pub source, pub path, pub mode, pub preset, pub passes }`
  - `pub fn OutputShaderPreset::pass_sources(&self, expand: &dyn Fn(&str) -> Option<String>) -> Vec<(String, bool)>` — built-ins only
  - `pub fn OutputShaderPart::pass_sources(&self, user_presets: &[OutputShaderPreset], expand: &dyn Fn(&str) -> Option<String>) -> Vec<(String, bool)>`
  - `Config` gains `pub output_shaders: Vec<OutputShaderPreset>`

- [ ] **Step 1: Write the failing tests**

Create `niri-config/src/output_shader.rs` with only this test module for now:

```rust
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
        assert!(night[0].0.contains("c.rgb * vec3("), "not a temperature filter: {night:?}");
        assert!(!night[0].1, "built-ins are never Hyprland dialect");

        let mono = config.output_shaders[1].pass_sources(&no_files);
        assert_eq!(mono.len(), 1);
        assert!(mono[0].0.contains("float l = dot"), "not grayscale: {mono:?}");
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
                    pass {
                        preset "grayscale"
                    }
                    pass {
                        preset "temperature" kelvin=3500
                    }
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
                    pass {
                        preset "grayscale"
                    }
                    pass {
                        path "missing.frag"
                    }
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `direnv exec . bash -c 'cargo test -p niri-config output_shader'`

Expected: FAIL to compile — `no field 'output_shaders' on type 'Config'`.

- [ ] **Step 3: Write the implementation**

Prepend to `niri-config/src/output_shader.rs`:

```rust
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
    let set = usize::from(source.is_some()) + usize::from(path.is_some())
        + usize::from(preset.is_some());
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
            warn!("output shader: both a top-level source/path/preset and pass blocks; using passes");
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
```

Then wire it into `niri-config/src/lib.rs`:

1. Module list (~line 42, alphabetical, after `pub mod output;`):

```rust
pub mod output_shader;
```

2. Re-exports (~line 61, after the `output` re-export):

```rust
pub use crate::output_shader::{
    OutputShaderPart, OutputShaderPassPart, OutputShaderPreset, OutputShadersPart,
};
```

3. `Config` struct field, immediately after `pub window_shaders: Vec<WindowShaderPreset>,`:

```rust
    pub output_shaders: Vec<OutputShaderPreset>,
```

4. Parse dispatch, in the "Multipart sections" group immediately after the `"window-shaders"` arm:

```rust
                "output-shaders" => {
                    let part = OutputShadersPart::decode_node(node, ctx)?;
                    config.borrow_mut().output_shaders.extend(part.presets);
                }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `direnv exec . bash -c 'cargo test -p niri-config output_shader'`

Expected: PASS, 5 tests.

- [ ] **Step 5: Fix the `Config::default()` inline snapshot**

Run: `direnv exec . bash -c 'cargo test -p niri-config'`

The `parse_default_config`-style inline snapshot at `niri-config/src/lib.rs:2657` now fails because
`Config` has a new field. Do **not** run `cargo insta accept` — it can hang in this repo. Instead
read the failure diff and hand-edit the expected snapshot text, adding `output_shaders: [],`
immediately after the existing `window_shaders: [],` line.

Re-run: `direnv exec . bash -c 'cargo test -p niri-config'`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add niri-config/src/output_shader.rs niri-config/src/lib.rs
git commit -m "config: add output-shaders preset block"
```

---

### Task 3: `output { shader { … } }` config child

**Files:**
- Modify: `niri-config/src/output.rs:50-81` (the `Output` struct) and `:96-115` (`Default for Output`)
- Test: inline `#[cfg(test)] mod tests` at the bottom of `niri-config/src/output.rs` (create the
  module if the file has none)

**Interfaces:**
- Consumes: `OutputShaderPart` and `OutputShaderPreset` from Task 2.
- Produces: `niri_config::Output` gains `pub shader: Option<OutputShaderPart>`.

- [ ] **Step 1: Write the failing tests**

Append to `niri-config/src/output.rs` (if a `#[cfg(test)] mod tests` already exists, add these
tests inside it instead of creating a second module):

```rust
#[cfg(test)]
mod tests {
    use crate::Config;

    fn no_files(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn output_shader_child_parses() {
        let config = Config::parse_mem(
            r##"
            output "eDP-1" {
                scale 1.5
                shader {
                    preset "grayscale"
                }
            }
            output "DP-2" {
                shader {
                    preset "temperature" kelvin=4000
                }
            }
            output "HDMI-A-1" {
                shader {
                    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy).bgra; }"
                }
            }
            output "DP-3" { }
            "##,
        )
        .unwrap();

        let shader_of = |name: &str| {
            config
                .outputs
                .0
                .iter()
                .find(|o| o.name == name)
                .unwrap()
                .shader
                .clone()
        };

        let edp = shader_of("eDP-1").expect("eDP-1 has no shader");
        let chain = edp.pass_sources(&config.output_shaders, &no_files);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].0.contains("float l = dot"));

        let dp2 = shader_of("DP-2").unwrap();
        assert!(dp2.pass_sources(&config.output_shaders, &no_files)[0]
            .0
            .contains("c.rgb * vec3("));

        let hdmi = shader_of("HDMI-A-1").unwrap();
        assert!(hdmi.pass_sources(&config.output_shaders, &no_files)[0]
            .0
            .contains("bgra"));

        assert!(shader_of("DP-3").is_none(), "an output with no shader must stay None");
    }

    #[test]
    fn output_shader_resolves_user_preset_and_shadows_builtin() {
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "night" {
                    preset "temperature" kelvin=3200
                }
                preset "grayscale" {
                    source "vec4 global_color(vec3 c){ return vec4(0.0); }"
                }
            }
            output "DP-1" {
                shader {
                    preset "night"
                }
            }
            output "DP-2" {
                shader {
                    preset "grayscale"
                }
            }
            "##,
        )
        .unwrap();

        let chain_of = |name: &str| {
            config
                .outputs
                .0
                .iter()
                .find(|o| o.name == name)
                .unwrap()
                .shader
                .as_ref()
                .unwrap()
                .pass_sources(&config.output_shaders, &no_files)
        };

        // User preset resolved by name.
        assert!(chain_of("DP-1")[0].0.contains("c.rgb * vec3("));
        // A user preset shadows the built-in of the same name.
        assert!(
            chain_of("DP-2")[0].0.contains("vec4(0.0)"),
            "user preset did not shadow the built-in"
        );
    }

    #[test]
    fn user_preset_is_not_reachable_from_inside_a_pass() {
        // Passes resolve against built-ins only. This is what makes recursion impossible.
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "night" {
                    preset "temperature" kelvin=3200
                }
            }
            output "DP-1" {
                shader {
                    pass {
                        preset "night"
                    }
                }
            }
            "##,
        )
        .unwrap();

        let chain = config.outputs.0[0]
            .shader
            .as_ref()
            .unwrap()
            .pass_sources(&config.output_shaders, &no_files);
        assert!(chain.is_empty(), "a pass must not resolve a user preset: {chain:?}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `direnv exec . bash -c 'cargo test -p niri-config output_shader_child'`

Expected: FAIL to compile — `no field 'shader' on type 'Output'`.

- [ ] **Step 3: Write the implementation**

In `niri-config/src/output.rs`, add the import at the top:

```rust
use crate::output_shader::OutputShaderPart;
```

Add the field to the `Output` struct, after `pub layout: Option<LayoutPart>,`:

```rust
    #[knuffel(child)]
    pub shader: Option<OutputShaderPart>,
```

And to `Default for Output`, after `layout: None,`:

```rust
            shader: None,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `direnv exec . bash -c 'cargo test -p niri-config'`

Expected: PASS. If the `Config::default()` inline snapshot fails again, hand-edit it as in Task 2
Step 5 — the `Output` debug output gains a `shader: None,` line.

- [ ] **Step 5: Commit**

```bash
git add niri-config/src/output.rs niri-config/src/lib.rs
git commit -m "config: add per-output shader rule"
```

---

### Task 4: Compile and render the output shader

**Files:**
- Modify: `src/niri.rs:7964-7986` (`scoped_shader_chains`)
- Modify: `src/niri.rs:5000-5084` (the region shader block in `render_for_output`) — insert the
  output shader element push immediately before the "Push region shader elements" comment
- Modify: `src/niri.rs` — add `Niri::output_shader_chain` near `read_scoped_shader_path`

**Interfaces:**
- Consumes: `OutputShaderPart::pass_sources` (Task 2), `Output::shader` (Task 3),
  `crate::niri::read_scoped_shader_path` (existing, `src/niri.rs:7951`).
- Produces: `pub(crate) fn Niri::output_shader_chain(&self, output: &smithay::output::Output) -> Vec<(String, bool)>` — later tasks call this rather than reaching into config.

- [ ] **Step 1: Add the config-only chain resolver**

Add to the `impl Niri` block, next to the other shader helpers:

```rust
    /// Resolve the configured shader chain for `output`, or an empty vec if it has none.
    ///
    /// This is the single place output shader chains are resolved, so the `scoped_key` computed
    /// at compile time always matches the one looked up at render time.
    pub(crate) fn output_shader_chain(
        &self,
        output: &smithay::output::Output,
    ) -> Vec<(String, bool)> {
        // This is the established idiom for output-config lookup in this file; see the call
        // sites at src/niri.rs:3216 and :3104. `find` matches on make/model/serial or connector.
        let name = output.user_data().get::<OutputName>().unwrap();
        let config = self.config.borrow();
        let Some(out_config) = config.outputs.find(name) else {
            return Vec::new();
        };
        let Some(shader) = &out_config.shader else {
            return Vec::new();
        };
        shader.pass_sources(&config.output_shaders, &read_scoped_shader_path)
    }
```

`OutputName` is already imported in `src/niri.rs`.

- [ ] **Step 2: Compile the chains**

In `scoped_shader_chains` (`src/niri.rs:7964`), after the `window_shaders` preset loop and before
`chains`, add:

```rust
    for out in &config.outputs.0 {
        if let Some(shader) = &out.shader {
            let chain = shader.pass_sources(&config.output_shaders, &read_scoped_shader_path);
            if !chain.is_empty() {
                chains.push(chain);
            }
        }
    }
    // Compile every preset eagerly so cycle-output-shader never stalls on a recompile.
    for preset in &config.output_shaders {
        let chain = preset.pass_sources(&read_scoped_shader_path);
        if !chain.is_empty() {
            chains.push(chain);
        }
    }
```

- [ ] **Step 3: Push the render element**

In `render_for_output`, immediately **before** the existing comment
`// Push region shader elements for regions matching this output.` (`src/niri.rs:5000`), and
**inside** the same `if crate::render_helpers::target_renders_shaders(...)` guard, add the output
shader element. Earlier push means topmost, so this lands above the region shaders and below the
global shader, which is the intended order.

Restructure minimally: move the existing `let scale = …` / `out_name` / `full` / `out_phys` /
`cursor` / `time` preamble so it is computed once and shared, then before the `regions` loop:

```rust
            // The output shader is a full-output region: same element, no geometry to configure.
            let output_chain = self.output_shader_chain(output);
            if !output_chain.is_empty() {
                let key = shaders::scoped_key(&output_chain);
                if Shaders::get(ctx.renderer)
                    .program(ProgramType::Scoped(key, 0))
                    .is_some()
                {
                    let n_passes = output_chain.len();
                    let offscreens = (0..n_passes.saturating_sub(1))
                        .map(|_| {
                            std::rc::Rc::new(
                                crate::render_helpers::offscreen::OffscreenBuffer::default(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let elem = ScopedShaderElement::new(
                        Id::new(),
                        full,
                        scale,
                        time,
                        cursor,
                        [0., 0., 1., 1.],
                        out_phys,
                        key,
                        n_passes,
                        ScopedSource::Capture,
                        offscreens,
                    );
                    push(elem.into());
                }
            }
```

Check `ScopedShaderElement::new`'s exact parameter order against the existing region call site at
`src/niri.rs:5064` before writing this, and match it — the argument list above is transcribed from
that call and must not drift from it.

- [ ] **Step 4: Verify it builds and nothing regressed**

Run: `direnv exec . bash -c 'cargo build && cargo test'`

Expected: builds clean; the existing suite still passes. There is no unit test for the render path
here — region shaders have none either, for the same reason (it needs a live renderer). Task 8
records the hardware check.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs
git commit -m "render: draw the per-output shader as a full-output scoped element"
```

---

### Task 5: Redraw gate

**Files:**
- Modify: `src/niri.rs:5898-5910` (the `region_shader_animate` block in the redraw decision)

**Interfaces:**
- Consumes: `Niri::output_shader_chain` (Task 4), `niri_config::GlobalShaderCaps::scan_chain`.
- Produces: nothing new; folds into the existing redraw decision.

- [ ] **Step 1: Write the failing test**

Add to `niri-config/src/output_shader.rs`'s test module:

```rust
    #[test]
    fn builtin_output_shaders_never_force_continuous_redraws() {
        // The whole point of a static colour filter: it must not peg the GPU. Anything that
        // scans as animating here would make the output redraw every frame forever.
        let config = Config::parse_mem(
            r##"
            output-shaders {
                preset "night" {
                    preset "temperature" kelvin=3200
                }
                preset "mono"  {
                    preset "grayscale"
                }
                preset "combo" {
                    pass {
                        preset "grayscale"
                    }
                    pass {
                        preset "saturation" amount=1.4
                    }
                    pass {
                        preset "invert"
                    }
                }
            }
            "##,
        )
        .unwrap();

        for preset in &config.output_shaders {
            let chain = preset.pass_sources(&no_files);
            assert!(!chain.is_empty(), "{} did not resolve", preset.name);
            let caps = crate::GlobalShaderCaps::scan_chain(&chain);
            assert!(
                !caps.is_animating(),
                "preset {} scans as animating: {caps:?}",
                preset.name
            );
        }
    }
```

- [ ] **Step 2: Run it to verify it passes already**

Run: `direnv exec . bash -c 'cargo test -p niri-config builtin_output_shaders_never'`

Expected: PASS immediately. This test is a *regression guard*, not a red-green driver — it locks in
the property that makes Step 3 safe, so that a future edit to the generated GLSL that introduced
the word `time` would fail loudly here instead of silently pegging a GPU.

- [ ] **Step 3: Add the redraw gate**

In `src/niri.rs`, directly after the `let region_shader_animate = { … };` block, add:

```rust
            // A hand-written animated output shader must keep redrawing, exactly like a region
            // shader. Every built-in filter is static, so it costs no extra frames.
            let output_shader_animate = {
                let chain = self.output_shader_chain(output);
                niri_config::GlobalShaderCaps::scan_chain(&chain).is_animating()
            };
```

Then find where `region_shader_animate` is consumed further down the same function and OR
`output_shader_animate` into the same expression, matching the existing style.

- [ ] **Step 4: Verify**

Run: `direnv exec . bash -c 'cargo build && cargo test'`

Expected: builds clean, suite passes.

- [ ] **Step 5: Commit**

```bash
git add src/niri.rs niri-config/src/output_shader.rs
git commit -m "render: fold output shaders into the continuous-redraw decision"
```

---

### Task 6: Runtime toggle/cycle state

**Files:**
- Create: `src/output_shader.rs` (the state type and its logic), registered in `src/main.rs`'s
  module list
- Modify: `src/niri.rs:710-750` (`OutputState` struct) and `:3271` (its construction)
- Modify: `src/niri.rs` — `output_shader_chain` consults the state
- Modify: `src/niri.rs:1924` (the reload gate)
- Test: inline `#[cfg(test)] mod tests` at the bottom of `src/output_shader.rs`

**Interfaces:**
- Consumes: `niri_config::OutputShaderPreset` (Task 2), `Niri::output_shader_chain` (Task 4).
- Produces:
  - `pub struct OutputShaderState { pub preset: Option<String>, pub disabled: bool }` with
    `Default`, `pub fn toggle(&mut self)`, `pub fn cycle(&mut self, presets: &[String])`
  - `OutputState` gains `pub shader_state: OutputShaderState`

- [ ] **Step 1: Write the failing tests**

Create `src/output_shader.rs`:

```rust
//! Runtime per-output shader override, driven by the `toggle-output-shader` /
//! `cycle-output-shader` actions. Lives on `OutputState`, which is mutated rather than rebuilt
//! across a config reload, so the user's choice survives an unrelated config edit.

/// Which shader an output is currently showing, on top of its configured rule.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OutputShaderState {
    /// Selected `output-shaders` preset name; `None` = the output's configured `shader` rule.
    pub preset: Option<String>,
    /// When true, no shader renders on this output regardless of preset or rule.
    pub disabled: bool,
}

impl OutputShaderState {
    /// Flip the shader on/off, keeping the preset selection.
    pub fn toggle(&mut self) {
        self.disabled = !self.disabled;
    }

    /// Advance to the next stop in default -> preset 1 -> … -> preset N -> default, re-enabling
    /// the shader. A preset name no longer in the config restarts at the first preset.
    pub fn cycle(&mut self, presets: &[String]) {
        self.disabled = false;
        self.preset = match &self.preset {
            None => presets.first().cloned(),
            Some(cur) => match presets.iter().position(|p| p == cur) {
                Some(i) => presets.get(i + 1).cloned(),
                None => presets.first().cloned(),
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn toggle_flips_disabled_and_keeps_preset() {
        let mut s = OutputShaderState {
            preset: Some(String::from("night")),
            disabled: false,
        };
        s.toggle();
        assert!(s.disabled);
        assert_eq!(s.preset.as_deref(), Some("night"));
        s.toggle();
        assert!(!s.disabled);
        assert_eq!(s.preset.as_deref(), Some("night"));
    }

    #[test]
    fn cycle_walks_default_then_presets_then_back() {
        let presets = names(&["night", "mono"]);
        let mut s = OutputShaderState::default();

        s.cycle(&presets);
        assert_eq!(s.preset.as_deref(), Some("night"));
        s.cycle(&presets);
        assert_eq!(s.preset.as_deref(), Some("mono"));
        s.cycle(&presets);
        assert_eq!(s.preset, None, "cycle must return to the configured rule");
        s.cycle(&presets);
        assert_eq!(s.preset.as_deref(), Some("night"));
    }

    #[test]
    fn cycle_reenables_a_disabled_shader() {
        let mut s = OutputShaderState {
            preset: None,
            disabled: true,
        };
        s.cycle(&names(&["night"]));
        assert!(!s.disabled);
        assert_eq!(s.preset.as_deref(), Some("night"));
    }

    #[test]
    fn cycle_restarts_when_the_selected_preset_vanished() {
        // The user cycled to "night", then removed it from the config and reloaded.
        let mut s = OutputShaderState {
            preset: Some(String::from("night")),
            disabled: false,
        };
        s.cycle(&names(&["mono", "warm"]));
        assert_eq!(s.preset.as_deref(), Some("mono"));
    }

    #[test]
    fn cycle_with_no_presets_configured_is_a_no_op_on_selection() {
        let mut s = OutputShaderState::default();
        s.cycle(&[]);
        assert_eq!(s.preset, None);
        assert!(!s.disabled);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `direnv exec . bash -c 'cargo test output_shader_state'`

Expected: FAIL — the module is not registered, so the tests do not run at all. Confirm with
`cargo test -- --list | grep output_shader` returning nothing.

- [ ] **Step 3: Register the module and wire the state**

Add to `src/main.rs`'s module list, in alphabetical position:

```rust
mod output_shader;
```

Add to `OutputState` (`src/niri.rs:710`), after `pub shader_throttle_timer: …`:

```rust
    /// Runtime shader override for this output: preset selection and on/off.
    pub shader_state: crate::output_shader::OutputShaderState,
```

Add to the `OutputState { … }` construction at `src/niri.rs:3271`, after `shader_throttle_timer:
None,`:

```rust
            shader_state: Default::default(),
```

- [ ] **Step 4: Make `output_shader_chain` consult the state**

Rewrite the body added in Task 4 so the runtime override wins over config. Note the borrow order:
read the state out of `output_state` **before** borrowing config, so the two borrows never overlap.

```rust
    pub(crate) fn output_shader_chain(
        &self,
        output: &smithay::output::Output,
    ) -> Vec<(String, bool)> {
        let (disabled, selected) = match self.output_state.get(output) {
            Some(state) => (
                state.shader_state.disabled,
                state.shader_state.preset.clone(),
            ),
            None => (false, None),
        };
        if disabled {
            return Vec::new();
        }

        let name = output.user_data().get::<OutputName>().unwrap();
        let config = self.config.borrow();

        // A selected preset overrides the output's configured rule. Resolved by name on every
        // call, so a config reload is picked up without any re-resolution step.
        if let Some(selected) = &selected {
            let Some(preset) = config.output_shaders.iter().find(|p| &p.name == selected) else {
                return Vec::new();
            };
            return preset.pass_sources(&read_scoped_shader_path);
        }

        let Some(out_config) = config.outputs.find(name) else {
            return Vec::new();
        };
        let Some(shader) = &out_config.shader else {
            return Vec::new();
        };
        shader.pass_sources(&config.output_shaders, &read_scoped_shader_path)
    }
```

- [ ] **Step 5: Extend the reload gate**

At `src/niri.rs:1924`, the condition is currently:

```rust
        if config.region_shaders != old_config.region_shaders
            || config.window_rules != old_config.window_rules
            || config.window_shaders != old_config.window_shaders
```

Add two more clauses. Compare **only the shader fields** of outputs, not whole `Output` structs —
otherwise changing a scale or a mode would needlessly recompile every scoped shader:

```rust
        let output_shaders_changed = {
            let shaders_of = |c: &niri_config::Config| {
                c.outputs
                    .0
                    .iter()
                    .map(|o| (o.name.clone(), o.shader.clone()))
                    .collect::<Vec<_>>()
            };
            shaders_of(&config) != shaders_of(old_config)
        };

        if config.region_shaders != old_config.region_shaders
            || config.window_rules != old_config.window_rules
            || config.window_shaders != old_config.window_shaders
            || config.output_shaders != old_config.output_shaders
            || output_shaders_changed
```

No preset re-resolution call is needed for outputs: unlike windows, `output_shader_chain` resolves
the selected preset name against config on every call, so a reload is picked up for free. Leave the
existing `update_shader_preset` window loop inside the block untouched.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `direnv exec . bash -c 'cargo build && cargo test output_shader'`

Expected: PASS, 5 new tests, and the build is clean.

- [ ] **Step 7: Commit**

```bash
git add src/output_shader.rs src/main.rs src/niri.rs
git commit -m "render: add runtime per-output shader toggle state"
```

---

### Task 7: IPC, binds, and input actions

**Files:**
- Modify: `niri-ipc/src/lib.rs` (near `CycleWindowShader` at line ~872)
- Modify: `niri-config/src/binds.rs` (the `Action` enum near line ~353, and the
  `From<niri_ipc::Action>` conversion near line ~707)
- Modify: `src/input/mod.rs` (the action match, after the `CycleWindowShaderById` arm at ~2417)
- Modify: `src/ui/hotkey_overlay.rs:507-508`
- Test: add to the existing bind test at `niri-config/src/binds.rs:1221-1245`

**Interfaces:**
- Consumes: `OutputShaderState::toggle` / `::cycle` (Task 6), `Niri::output_by_name_match`
  (existing, `src/niri.rs:4072`), `Layout::active_output` (existing, `src/layout/mod.rs:1943`).
- Produces: two config `Action` variants and two IPC variants.

- [ ] **Step 1: Write the failing test**

Add to the `#[test]` block around `niri-config/src/binds.rs:1221` (extend the existing config text
and assertions, following the shape already there):

```rust
    #[test]
    fn output_shader_binds_parse() {
        let binds = Config::parse_mem(
            r##"
            binds {
                Mod+Shift+G { toggle-output-shader; }
                Mod+Shift+H { toggle-output-shader "eDP-1"; }
                Mod+Shift+C { cycle-output-shader; }
                Mod+Shift+V { cycle-output-shader "DP-2"; }
            }
            "##,
        )
        .unwrap()
        .binds;

        let actions: Vec<_> = binds.0.iter().map(|b| b.action.clone()).collect();
        assert_eq!(
            actions,
            [
                Action::ToggleOutputShader(None),
                Action::ToggleOutputShader(Some(String::from("eDP-1"))),
                Action::CycleOutputShader(None),
                Action::CycleOutputShader(Some(String::from("DP-2"))),
            ]
        );

        assert_eq!(
            Action::from(niri_ipc::Action::ToggleOutputShader { output: None }),
            Action::ToggleOutputShader(None)
        );
        assert_eq!(
            Action::from(niri_ipc::Action::ToggleOutputShader {
                output: Some(String::from("DP-2"))
            }),
            Action::ToggleOutputShader(Some(String::from("DP-2")))
        );
        assert_eq!(
            Action::from(niri_ipc::Action::CycleOutputShader { output: None }),
            Action::CycleOutputShader(None)
        );
        assert_eq!(
            Action::from(niri_ipc::Action::CycleOutputShader {
                output: Some(String::from("eDP-1"))
            }),
            Action::CycleOutputShader(Some(String::from("eDP-1")))
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `direnv exec . bash -c 'cargo test -p niri-config output_shader_binds_parse'`

Expected: FAIL to compile — `no variant named 'ToggleOutputShader'`.

- [ ] **Step 3: Add the IPC variants**

In `niri-ipc/src/lib.rs`, immediately after the `CycleWindowShader { … }` variant:

```rust
    /// Toggle the output's shader on or off.
    ToggleOutputShader {
        /// Name of the output to toggle.
        ///
        /// If `None`, uses the focused output.
        #[cfg_attr(feature = "clap", arg(long))]
        output: Option<String>,
    },
    /// Cycle the output's shader through the configured output-shaders presets.
    CycleOutputShader {
        /// Name of the output to cycle.
        ///
        /// If `None`, uses the focused output.
        #[cfg_attr(feature = "clap", arg(long))]
        output: Option<String>,
    },
```

- [ ] **Step 4: Add the config Action variants and conversion**

In `niri-config/src/binds.rs`, after `CycleWindowShaderById(u64),` (~line 355):

An optional KDL argument, so one variant serves both `toggle-output-shader;` and
`toggle-output-shader "eDP-1";`. This follows `SetDynamicCastMonitor(Option<String>)` at
`niri-config/src/binds.rs:380` — note it must NOT be a `…On(String)` pair, because knuffel derives
the node name from the variant name, so `ToggleOutputShaderOn` would parse as the node
`toggle-output-shader-on`, not as `toggle-output-shader` with an argument.

```rust
    ToggleOutputShader(#[knuffel(argument)] Option<String>),
    CycleOutputShader(#[knuffel(argument)] Option<String>),
```

And in the `From<niri_ipc::Action>` match, after the `CycleWindowShaderById` arm (~line 707):

```rust
            niri_ipc::Action::ToggleOutputShader { output } => Self::ToggleOutputShader(output),
            niri_ipc::Action::CycleOutputShader { output } => Self::CycleOutputShader(output),
```

- [ ] **Step 5: Handle the actions**

In `src/input/mod.rs`, after the `Action::CycleWindowShaderById(id) => { … }` arm:

```rust
            Action::ToggleOutputShader(name) => {
                match self.resolve_shader_output(name.as_deref()) {
                    Some(output) => self.toggle_output_shader(&output),
                    None => warn!("toggle-output-shader: no matching output"),
                }
            }
            Action::CycleOutputShader(name) => {
                match self.resolve_shader_output(name.as_deref()) {
                    Some(output) => self.cycle_output_shader(&output),
                    None => warn!("cycle-output-shader: no matching output"),
                }
            }
```

And add the three helpers to the same `impl State` block:

```rust
    /// The output an output-shader action targets: the named one, or the focused one.
    fn resolve_shader_output(&self, name: Option<&str>) -> Option<Output> {
        match name {
            Some(name) => self.niri.output_by_name_match(name).cloned(),
            None => self.niri.layout.active_output().cloned(),
        }
    }

    fn toggle_output_shader(&mut self, output: &Output) {
        if let Some(state) = self.niri.output_state.get_mut(output) {
            state.shader_state.toggle();
        }
        self.niri.queue_redraw(output);
    }

    fn cycle_output_shader(&mut self, output: &Output) {
        let names: Vec<String> = self
            .niri
            .config
            .borrow()
            .output_shaders
            .iter()
            .map(|p| p.name.clone())
            .collect();
        if let Some(state) = self.niri.output_state.get_mut(output) {
            state.shader_state.cycle(&names);
        }
        self.niri.queue_redraw(output);
    }
```

`Niri::queue_redraw(&mut self, output: &Output)` exists at `src/niri.rs:4114`, so these can be
granular — no `// FIXME: granular` comment is needed, unlike the window-shader arms which fall back
to `queue_redraw_all`.

- [ ] **Step 6: Add the hotkey overlay strings**

In `src/ui/hotkey_overlay.rs`, next to lines 507-508:

```rust
        Action::ToggleOutputShader(_) => String::from("Toggle Output Shader"),
        Action::CycleOutputShader(_) => String::from("Cycle Output Shader Preset"),
```

The binding is described the same way whether or not an output name was given, so both cases share
one arm.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `direnv exec . bash -c 'cargo build && cargo test'`

Expected: builds clean, whole suite passes. The build will surface any non-exhaustive match on
`Action` elsewhere in the tree — fix each by following the neighbouring window-shader arm.

- [ ] **Step 8: Verify the CLI surface**

Run: `direnv exec . bash -c 'cargo run --bin niri -- msg action toggle-output-shader --help'`

Expected: help text showing the `--output <OUTPUT>` optional flag.

- [ ] **Step 9: Commit**

```bash
git add niri-ipc/src/lib.rs niri-config/src/binds.rs src/input/mod.rs src/ui/hotkey_overlay.rs
git commit -m "ipc: add toggle-output-shader and cycle-output-shader actions"
```

---

### Task 8: Documentation and hardware verification

**Files:**
- Modify: `docs/wiki/Configuration:-Global-Shader.md` (near the `toggle-window-shader` note at
  line ~591)
- Modify: `docs/wiki/Configuration:-Outputs.md`
- Modify: `docs/wiki/Configuration:-Key-Bindings.md` (near the `toggle-window-shader` section at
  line ~396)
- Modify: `resources/default-config.kdl` (near the `window-shaders` comment at line ~322)
- Modify: `README.md:43` (the window-shaders feature bullet)

**Interfaces:**
- Consumes: everything from Tasks 1-7. Produces no code.

- [ ] **Step 1: Document the output shader rule**

In `docs/wiki/Configuration:-Global-Shader.md`, add a `## Per-output shaders` section covering:

- the `output "eDP-1" { shader { … } }` body and that it takes the same `source` / `path` / `mode`
  / `pass` spec as `region-shader`, minus geometry
- the built-in table verbatim from the spec: `grayscale` (no property), `invert` (no property),
  `saturation amount=` (default 1.5), `temperature kelvin=` (default 4000, 6500 = identity)
- the resolution rule: a top-level `preset` resolves user `output-shaders` presets first then
  built-ins; a `preset` inside a `pass` resolves built-ins only, which is why preset recursion
  cannot happen
- the compositing order — global shader over output shader over region shaders over windows
- that built-in filters are static and therefore cost no continuous redraws

Worked examples to include:

```kdl
output "eDP-1" {
    shader {
        preset "saturation" amount=1.4
    }
}

output "DP-2" {
    shader {
        preset "temperature" kelvin=4000
    }
}

output-shaders {
    preset "night" {
        preset "temperature" kelvin=3200
    }
    preset "mono"  {
        preset "grayscale"
    }
    preset "warm-mono" {
        pass {
            preset "grayscale"
        }
        pass {
            preset "temperature" kelvin=3500
        }
    }
}

output "HDMI-A-1" {
    shader {
        preset "warm-mono"
    }
}
```

- [ ] **Step 2: Document the outputs child and the key bindings**

In `docs/wiki/Configuration:-Outputs.md`, add a `shader` child entry that links to the Global
Shader page for the full description.

In `docs/wiki/Configuration:-Key-Bindings.md`, add `#### toggle-output-shader` and
`#### cycle-output-shader` sections mirroring the existing `toggle-window-shader` one, including
the optional output-name argument:

```kdl
binds {
    Mod+Shift+G {
        toggle-output-shader;
    }
    Mod+Shift+H {
        toggle-output-shader "eDP-1";
    }
    Mod+Shift+C {
        cycle-output-shader;
    }
}
```

Note that cycling re-enables a shader turned off with `toggle-output-shader`, matching the wording
already used for the window pair.

- [ ] **Step 3: Add the default-config example**

In `resources/default-config.kdl`, next to the existing `window-shaders` comment block, add a
commented example:

```kdl
// Per-output colour filters. Built-ins: grayscale, invert, saturation (amount=),
// temperature (kelvin=, 6500 is neutral). No GLSL required.
//
// output "eDP-1" {
//     shader {
    preset "saturation" amount=1.4
}
// }
//
// Named presets for cycle-output-shader:
//
// output-shaders {
//     preset "night" {
    preset "temperature" kelvin=3200
}
//     preset "mono"  {
    preset "grayscale"
}
// }
```

- [ ] **Step 4: Update the README feature list**

Add a bullet next to line 43:

```markdown
- **Per-output colour filters**: `output "eDP-1" { shader { preset "grayscale"; }; }` — built-in grayscale, invert, saturation and temperature filters with no GLSL to write, plus `toggle-output-shader` and `cycle-output-shader` binds. Answers upstream niri #4355, #4303 and #4405.
```

- [ ] **Step 5: Verify the whole tree**

Run: `direnv exec . bash -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test'`

Expected: all three clean. Fix anything that is not before continuing.

- [ ] **Step 6: Commit**

```bash
git add docs/wiki resources/default-config.kdl README.md
git commit -m "docs: document per-output shaders and the built-in colour filters"
```

- [ ] **Step 7: Hardware verification**

The render path has no automated coverage, matching region shaders. Before the PR is considered
done, verify on real hardware and record the result in the issue:

1. Add `output "<your connector>" { shader { preset "grayscale"; }; }`, reload, confirm that output
   goes gray and other outputs do not.
2. Confirm `nvtop` / `intel_gpu_top` show **no** continuous GPU load on an idle desktop with the
   filter active — this is the issue's open question about the redraw scheduler.
3. Bind and press `toggle-output-shader`; confirm the filter turns off and on.
4. Define two `output-shaders` presets, bind and press `cycle-output-shader`; confirm it walks
   default -> preset 1 -> preset 2 -> default.
5. With a `region-shader` also active on that output, confirm the output filter applies over it.
6. Define a multi-pass preset (e.g. `warm-mono`: `pass { preset "grayscale"; }` then
   `pass { preset "temperature" kelvin=3500; }`) and select it. The offscreen ping-pong between
   passes is the only wholly untested render code, and `n_passes > 1` is the likeliest bug site —
   every other check above is single-pass.
7. Without `shaders-in-capture` set, confirm the filter does **not** appear in a portal screencast
   (e.g. a browser screen-share) or a `grim` capture of that output; then add `shaders-in-capture`
   and confirm it **does** appear in both. This is a privacy contract, not just a rendering detail.
8. Cycle to a preset, then reload the config with an unrelated edit (e.g. touch a comment); confirm
   the filter survives the reload and the correct one is still showing. This exercises the stale
   preset re-resolution added in the final review pass.

Do not close issue #35 until these eight checks pass on hardware.
