precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

varying vec2 niri_v_coords;   // 0..1 across this element (remapped to output coords via niri_region)
uniform vec2 niri_size;       // element size in physical px (box size in region mode; see niri_output_size)
uniform float niri_scale;
uniform float niri_alpha;

uniform float niri_time;      // seconds since shader activation
uniform vec2 niri_cursor;     // cursor position, output coords (px)

// Region this element covers, in output-normalised coords: (origin.xy, size.xy).
// (0,0,1,1) for a whole-output shader; a sub-box when cursor-radius is set.
uniform vec4 niri_region;
uniform vec2 niri_output_size; // true full-output size in physical px

uniform sampler2D niri_screen; // composited frame below this element (covers niri_region)
uniform sampler2D niri_prev;   // previous frame's output
uniform sampler2D niri_screen_prev; // previous frame's niri_screen capture (screen only, no effect)

// uv is output-normalised (0..1 across the whole output); convert to this element's local
// texture coords. Samples outside the captured region clamp to the (transparent) border.
vec4 tex2D_screen(vec2 uv) { return texture2D(niri_screen, (uv - niri_region.xy) / niri_region.zw); }
vec4 tex2D_prev(vec2 uv) { return texture2D(niri_prev, (uv - niri_region.xy) / niri_region.zw); }
vec4 tex2D_screen_prev(vec2 uv) { return texture2D(niri_screen_prev, (uv - niri_region.xy) / niri_region.zw); }

uniform sampler2D niri_buffer; // dedicated feedback buffer (last frame); == niri_prev if no global_buffer
vec4 tex2D_buffer(vec2 uv) { return texture2D(niri_buffer, (uv - niri_region.xy) / niri_region.zw); }

// User defines: vec4 global_color(vec3 coord);
// User MAY define: vec4 global_buffer(vec3 coord);  // returns what to store in niri_buffer
