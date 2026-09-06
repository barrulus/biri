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
