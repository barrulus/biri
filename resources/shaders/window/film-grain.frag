// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Film grain — a gentle luminance-only grain as a visual-snow reading aid. Port of
// BanchouBoo's picom grain (github.com/BanchouBoo/dots, .config/picom/main.glsl): per-pixel
// noise is added to LUMINANCE only (the same scalar on R,G,B — chroma never shifts), the
// noise field refreshes INTERVAL times per second rather than every frame, and a gain boost
// kicks in only near pure black/white — the flat fields where visual snow is most visible.
// The overlaid grain masks the viewer's internal snow without disturbing readability.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule. Animates at GRAIN_INTERVAL Hz — pairs well with
// shader-animation-max-fps.
//
// Tuning knobs:
//   GRAIN_INTERVAL -> noise refreshes per second (original default: 15)
//   GRAIN_OPACITY  -> grain amplitude (original default: 0.035 — very subtle)
//   GRAIN_SIZE     -> grain cell size in physical px (raise to 2-3 for coarser grain on hidpi)
//   0.85/0.95      -> where the near-black/near-white gain boost engages
//   1.10           -> how strong that boost is (~2.1x total at the extremes)

const float GRAIN_INTERVAL = 15.0;
const float GRAIN_OPACITY  = 0.035;
const float GRAIN_SIZE     = 1.0;

vec4 global_color(vec3 c){
    vec4 s = tex2D_screen(c.xy);

    // Refresh the noise field INTERVAL times/sec (not every frame). .36593 is just a
    // random number so the offset never sticks on whole-number multiples.
    vec2 offset = fract(vec2(floor(niri_time * GRAIN_INTERVAL)) * 0.36593);

    // Pixel-locked seed, quantized to GRAIN_SIZE physical pixels.
    vec2 q    = floor(c.xy * niri_size / GRAIN_SIZE);
    vec2 seed = fract(q * 0.01371 + offset);
    float g   = (fract(sin(dot(seed, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) * 2.0;  // -1..1

    // Boost the grain ONLY very close to pure black or pure white (midtones untouched).
    float luma = dot(s.rgb, vec3(0.2126, 0.7152, 0.0722));
    float ext  = abs(luma * 2.0 - 1.0);                 // 0 mid -> 1 at black/white
    float gain = 1.0 + smoothstep_any_order(0.85, 0.95, ext) * 1.10;

    // Luminance-only: the same scalar on all channels leaves chroma untouched.
    // Scaled by s.a so rounded corners / transparent edges stay clean.
    return vec4(s.rgb + g * GRAIN_OPACITY * gain * s.a, s.a);
}
