precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

varying vec2 niri_v_coords;   // 0..1 across the output
uniform vec2 niri_size;       // output size in physical px
uniform float niri_scale;
uniform float niri_alpha;

uniform float niri_time;      // seconds since shader activation
uniform vec2 niri_cursor;     // cursor position, output coords (px)

uniform sampler2D niri_screen; // composited frame below this element
uniform sampler2D niri_prev;   // previous frame's output

vec4 tex2D_screen(vec2 uv) { return texture2D(niri_screen, uv); }
vec4 tex2D_prev(vec2 uv) { return texture2D(niri_prev, uv); }

// User defines: vec4 global_color(vec3 coord);
