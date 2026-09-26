// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// CVD shimmer (DEUTERANOPIA) — makes colours a red-green colour-blind viewer can't
// distinguish carry a visible TEXTURE instead of shifting their hues. Each pixel is run
// through a deuteranopia simulation (Vienot-style RGB matrix); the difference between the
// original and the simulation is the colour information that viewer LOSES. Where the loss
// is large, an animated static shimmer is laid on top, amplitude proportional to the loss —
// greys and safe colours get nothing. The two confused sides get DIFFERENT static so they
// stay tellable-apart, not just flagged: red-side = coarse slow grain, green-side = fine
// fast grain. The shimmer is luminance-only, so visible hues are never altered.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule. Animates at up to 20 Hz — pairs well with
// shader-animation-max-fps. Siblings: cvd-protan.frag, cvd-tritan.frag.
//
// Tuning knobs:
//   STRENGTH    -> peak shimmer amplitude
//   0.06 / 0.45 -> loss thresholds: below 0.06 nothing shimmers; full amplitude by 0.45
//   3.0 / 8.0   -> red-side grain: cell size (px) / refresh rate (Hz)
//   1.0 / 20.0  -> green-side grain: cell size (px) / refresh rate (Hz)

float grain(vec2 px, float cellsz, float hz, float ofs){
    vec2 t    = fract(vec2(floor(niri_time * hz)) * 0.36593 + ofs);
    vec2 q    = floor(px / cellsz);
    vec2 seed = fract(q * 0.01371 + t);
    return (fract(sin(dot(seed, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) * 2.0;  // -1..1
}

vec4 global_color(vec3 c){
    vec4 s = tex2D_screen(c.xy);

    // Deuteranopia simulation (what a viewer missing the M cone actually sees).
    vec3 sim = vec3(dot(vec3(0.625, 0.375, 0.0), s.rgb),
                    dot(vec3(0.700, 0.300, 0.0), s.rgb),
                    dot(vec3(0.0,   0.300, 0.7), s.rgb));

    // The colour information lost to that viewer; 0 for greys and unaffected colours.
    vec3  err  = s.rgb - sim;
    float loss = length(err);
    float amp  = smoothstep_any_order(0.06, 0.45, loss) * s.a;
    if (amp < 0.003) return s;

    // Which side of the confusion axis? >0 = red-ish, <0 = green-ish.
    float side    = err.r - err.g;
    float sideMix = smoothstep_any_order(-0.05, 0.05, side);       // 1 = red side

    vec2  px = c.xy * niri_size;
    float gR = grain(px, 3.0,  8.0, 0.0);                // red side: coarse + slow
    float gG = grain(px, 1.0, 20.0, 0.37);               // green side: fine + fast
    float g  = mix(gG, gR, sideMix);

    const float STRENGTH = 0.22;
    return vec4(s.rgb + g * amp * STRENGTH, s.a);        // luminance-only: hue untouched
}
