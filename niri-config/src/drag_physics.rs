/// Parameters of Umbriel's elastic 4×4 sheet, in logical pixels and seconds.
/// A `drag-physics` block opts in; an absent block leaves movement rigid.
#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct DragPhysics {
    #[knuffel(child, unwrap(argument), default = true)]
    pub enable: bool,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(36.))]
    pub stiffness: crate::FloatOrInt<1, 1000>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(100.))]
    pub coupling: crate::FloatOrInt<0, 500>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(6.5))]
    pub damping: crate::FloatOrInt<0, 60>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(2.))]
    pub pointer_response: crate::FloatOrInt<0, 10>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(0.))]
    pub stiffness_gradient: crate::FloatOrInt<-1, 1>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(0.))]
    pub lag_gradient: crate::FloatOrInt<-1, 4>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(0.))]
    pub downward_pull: crate::FloatOrInt<0, 50>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(8.))]
    pub motion_gain: crate::FloatOrInt<0, 32>,
    #[knuffel(child, unwrap(argument), default = crate::FloatOrInt(2.8))]
    pub decay: crate::FloatOrInt<0, 30>,
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn opt_in_and_bounds() {
        assert!(Config::default()
            .animations
            .window_movement
            .drag_physics
            .is_none());
        let parse = |s| {
            Config::parse_mem(&format!(
                "animations {{ window-movement {{ drag-physics {{ {s} }}; }}; }}"
            ))
        };
        let config = parse("").unwrap();
        let physics = config.animations.window_movement.drag_physics.unwrap();
        assert!(physics.enable);
        assert_eq!(physics.stiffness.0, 36.);
        assert_eq!(physics.damping.0, 6.5);
        assert!(
            !parse("enable false;")
                .unwrap()
                .animations
                .window_movement
                .drag_physics
                .unwrap()
                .enable
        );
        for invalid in [
            "stiffness 0;",
            "coupling 501;",
            "damping 0;",
            "pointer-response 11;",
            "downward-pull -1;",
        ] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
    }
}
