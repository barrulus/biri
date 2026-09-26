// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// CVD shimmer (DEUTERANOPIA) — ALPHABET variant. The affected band is QUANTIZED into four
// named zones, each with a fixed, learnable static signature (grain size encodes the
// red-vs-green family, flicker rate encodes how deep into that family):
//   strong red    -> coarse + slow   (3px,  6 Hz)
//   orange/brown  -> coarse + fast   (3px, 18 Hz)
//   olive         -> fine   + slow   (1px,  6 Hz)
//   strong green  -> fine   + fast   (1px, 18 Hz)
// Steps, not a spectrum — but each zone reads as a stable label ("coarse-slow always means
// red"). Amplitude still tracks actual colour-info loss; greys stay still.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Siblings: cvd-deutan.frag (binary baseline), -oriented, -drift, -combo.
//
// Tuning knobs:
//   STRENGTH    -> peak shimmer amplitude
//   0.06 / 0.45 -> loss thresholds (below 0.06 nothing shimmers; full amplitude by 0.45)
//   0.55        -> zone boundary between "strong" and "weak" ends of the axis
//   3.0 / 1.0   -> coarse / fine grain cell size (px)
//   6.0 / 18.0  -> slow / fast flicker (Hz)

float grain(vec2 px, float cellsz, float hz){
    vec2 t    = fract(vec2(floor(niri_time * hz)) * 0.36593);
    vec2 q    = floor(px / cellsz);
    vec2 seed = fract(q * 0.01371 + t);
    return (fract(sin(dot(seed, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) * 2.0;  // -1..1
}

vec4 global_color(vec3 c){
    vec4 s = tex2D_screen(c.xy);

    // Deuteranopia simulation and per-pixel colour-info loss.
    vec3 sim = vec3(dot(vec3(0.625, 0.375, 0.0), s.rgb),
                    dot(vec3(0.700, 0.300, 0.0), s.rgb),
                    dot(vec3(0.0,   0.300, 0.7), s.rgb));
    vec3  err  = s.rgb - sim;
    float loss = length(err);
    float amp  = smoothstep_any_order(0.06, 0.45, loss) * s.a;
    if (amp < 0.003) return s;

    // Continuous axis, then quantized into the four zones.
    float axis    = clamp(err.r - err.g, -1.0, 1.0);
    float redSide = step(0.0, axis);                     // 1 = red family
    float strong  = step(0.55, abs(axis));               // 1 = deep end of the family

    float cellsz = mix(1.0, 3.0, redSide);               // size = family
    float hz     = mix(mix(6.0, 18.0, redSide),          // rate = depth (see table above)
                       mix(18.0, 6.0, redSide), strong);

    float g = grain(c.xy * niri_size, cellsz, hz);

    const float STRENGTH = 0.22;
    return vec4(s.rgb + g * amp * STRENGTH, s.a);        // luminance-only: hue untouched
}
