// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// CVD shimmer (DEUTERANOPIA) — ORIENTED variant. The lost hue is encoded CONTINUOUSLY as
// stripe ANGLE: the shimmer is streaky (anisotropic) static, and the streaks lean with the
// pixel's position along the red-green confusion axis. Saturated red ~ +50deg, orange ~ +25,
// brown shallow, olive ~ -25, saturated green ~ -50. Orientation is the strongest continuous
// texture channel the eye has (~10-15deg discrimination -> ~6-8 readable hue steps).
// Amplitude still tracks how much colour info the pixel actually loses; greys stay still.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Siblings: cvd-deutan.frag (binary baseline), -alphabet, -drift, -combo.
//
// Tuning knobs:
//   STRENGTH    -> peak shimmer amplitude
//   0.06 / 0.45 -> loss thresholds (below 0.06 nothing shimmers; full amplitude by 0.45)
//   0.873       -> max stripe lean in radians (50deg); raise toward 1.2 for a wider dial
//   1.5 / 10.0  -> streak thickness / length in px (the anisotropy that makes it stripy)
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

    // Continuous confusion-axis coordinate: +1 = strongly red, -1 = strongly green.
    float axis = clamp(err.r - err.g, -1.0, 1.0);

    // Rotate the grain domain by the hue angle, then quantize ANISOTROPICALLY:
    // fine across the streak, long along it -> oriented streaky static.
    float th = axis * 0.873;                             // +-50deg
    float ca = cos(th), sa = sin(th);
    vec2  px = c.xy * niri_size;
    vec2  rp = vec2(ca * px.x - sa * px.y,
                    sa * px.x + ca * px.y);
    vec2  q  = floor(rp / vec2(1.5, 10.0));              // streaks along the rotated y axis

    vec2  t  = fract(vec2(floor(niri_time * 12.0)) * 0.36593);
    float g  = grainCell(q, t);

    const float STRENGTH = 0.22;
    return vec4(s.rgb + g * amp * STRENGTH, s.a);        // luminance-only: hue untouched
}
