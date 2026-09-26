// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// CVD shimmer (DEUTERANOPIA) — COMBO variant. Two texture channels at once:
//   stripe ANGLE      = WHERE on the red-green axis the colour sits (red +50deg ... green -50deg)
//   grain COARSENESS  = HOW MUCH colour info is lost (bolder streaks = more invisible colour)
// i.e. the oriented variant, plus loss magnitude promoted from pure amplitude into physical
// streak size. Richest signal of the four variants, and the busiest look.
// Amplitude still tracks actual colour-info loss; greys stay still.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Siblings: cvd-deutan.frag (binary baseline), -oriented, -alphabet, -drift.
//
// Tuning knobs:
//   STRENGTH    -> peak shimmer amplitude
//   0.06 / 0.45 -> loss thresholds (below 0.06 nothing shimmers; full amplitude by 0.45)
//   0.873       -> max stripe lean in radians (50deg)
//   1.0..3.0 / 6.0..14.0 -> streak thickness / length range as loss grows
//   12.0        -> flicker rate (Hz)

float grainCell(vec2 q, vec2 t){
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

    // Channel 1: angle = position on the confusion axis.
    float axis = clamp(err.r - err.g, -1.0, 1.0);
    float th   = axis * 0.873;                           // +-50deg
    float ca = cos(th), sa = sin(th);
    vec2  px = c.xy * niri_size;
    vec2  rp = vec2(ca * px.x - sa * px.y,
                    sa * px.x + ca * px.y);

    // Channel 2: coarseness = loss magnitude (bolder streaks where more colour vanishes).
    vec2 cell = mix(vec2(1.0, 6.0), vec2(3.0, 14.0), amp);
    vec2 q    = floor(rp / cell);

    vec2  t = fract(vec2(floor(niri_time * 12.0)) * 0.36593);
    float g = grainCell(q, t);

    const float STRENGTH = 0.22;
    return vec4(s.rgb + g * amp * STRENGTH, s.a);        // luminance-only: hue untouched
}
