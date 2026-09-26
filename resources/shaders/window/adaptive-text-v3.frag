// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Adaptive text legibility v3 — no per-pixel text classification (that's what caused the
// speckle: hard per-pixel decisions flip at glyph edges). Instead, two smooth operations
// that cannot speckle because they are continuous in both space and luminance:
//   1. a wide blur of the capture estimates the BACKDROP (wallpaper through the alpha);
//   2. where that backdrop is bright, it is adaptively DIMMED (like the terminal's alpha
//      locally increasing — multiplicative, so the wallpaper keeps its hue);
//   3. the DETAIL layer (pixel minus backdrop) — glyphs, mostly — is amplified on top.
// Net effect: text keeps its exact rendered colors but sits on a locally calmer, darker
// backdrop with its own contrast boosted. Over dark wallpaper the dim fades to nothing.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the composited window capture; niri_size = window px.
//
// Tuning knobs:
//   RADIUS -> backdrop blur radius in px (bigger = smoother, softer response)
//   GAIN   -> detail amplification (1 = none; 2 = double the text-vs-backdrop contrast)
//   DIM    -> how dark a fully bright backdrop gets (0.55 = 55% of original)
//   DIMLO/DIMHI -> backdrop luminance range over which the dim ramps in

const float RADIUS = 10.0;
const float GAIN   = 1.9;
const float DIM    = 0.55;
const float DIMLO  = 0.25;
const float DIMHI  = 0.60;

float lum(vec3 c){ return dot(c, vec3(0.299, 0.587, 0.114)); }

vec4 global_color(vec3 c){
    vec4 s  = tex2D_screen(c.xy);
    vec2 px = 1.0 / max(niri_size, vec2(1.0));

    // Backdrop estimate: 17-tap two-ring blur (8 at RADIUS, 8 at RADIUS/2, plus center).
    vec3 m = s.rgb;
    for (int i = 0; i < 8; i++){
        float a = 0.7853982 * float(i);
        vec2  d = vec2(cos(a), sin(a)) * px;
        m += tex2D_screen(c.xy + d * RADIUS).rgb;
        m += tex2D_screen(c.xy + d * (RADIUS * 0.5)).rgb;
    }
    m /= 17.0;

    // Adaptive dim: bright backdrop pixels get pulled down, dark ones stay as they are.
    float dimf = mix(1.0, DIM, smoothstep_any_order(DIMLO, DIMHI, lum(m)));

    // Recompose: dimmed backdrop + amplified detail (glyphs ride on top, colors intact).
    vec3 outc = m * dimf + (s.rgb - m) * GAIN;
    return vec4(clamp(outc, 0.0, 1.0), s.a);
}
