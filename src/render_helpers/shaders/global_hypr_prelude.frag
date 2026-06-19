precision highp float;

varying vec2 niri_v_coords;
uniform sampler2D niri_screen;
uniform float niri_time;

#define tex niri_screen
#define v_texcoord niri_v_coords
#define time niri_time
#define wl_output 0

// Hyprland-style user shader (its own main(), writes gl_FragColor) appended below.
