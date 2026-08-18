// Adaptive text legibility v4 — smooth-only operations (no per-pixel text classification,
// which speckles at glyph edges). A wide blur of the capture estimates the BACKDROP; where
// bright, it is gently dimmed (multiplicative — wallpaper hue kept, reads as the terminal's
// alpha locally increasing). The DETAIL layer (pixel minus backdrop) is passed through a
// SOFT-KNEE gain: small amplitudes — the alpha-dimmed wallpaper texture — stay at 1.0 and
// render untouched; large amplitudes — glyph strokes, the only full-contrast detail in a
// transparent terminal — get amplified. Every curve is continuous in space and luminance,
// so nothing can flip or speckle; text keeps its exact colors, wallpaper keeps its look.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the composited window capture; niri_size = window px.
//
// Tuning knobs:
//   RADIUS -> backdrop blur radius in px
//   GAIN   -> amplification for full-strength glyph detail
//   KNEE0/KNEE1 -> detail amplitude range over which gain ramps from 1.0 to GAIN;
//                  raise KNEE0 if wallpaper texture still sharpens, lower if dim text is missed
//   DIM    -> how dark a fully bright backdrop gets (1.0 = never dim)
//   DIMLO/DIMHI -> backdrop luminance range over which the dim ramps in

const float RADIUS = 10.0;
const float GAIN   = 2.2;
const float KNEE0  = 0.08;
const float KNEE1  = 0.22;
const float DIM    = 0.65;
const float DIMLO  = 0.30;
const float DIMHI  = 0.65;

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
    float dimf = mix(1.0, DIM, smoothstep(DIMLO, DIMHI, lum(m)));

    // Soft-knee detail gain: wallpaper-scale amplitudes pass at 1.0, glyph-scale amplified.
    vec3  detail = s.rgb - m;
    float amp    = max(abs(lum(detail)), length(detail) * 0.5);
    float g      = mix(1.0, GAIN, smoothstep(KNEE0, KNEE1, amp));

    vec3 outc = m * dimf + detail * g;
    return vec4(clamp(outc, 0.0, 1.0), s.a);
}
