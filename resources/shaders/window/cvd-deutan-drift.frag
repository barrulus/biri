// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// CVD shimmer (DEUTERANOPIA) — DRIFT variant. The lost hue is encoded as a MOTION COMPASS:
// the static scrolls, and its drift direction is the pixel's position along the red-green
// confusion axis. Strong red drifts straight UP, orange up-and-right, neutral edge cases
// right, olive down-and-right, strong green straight DOWN. The grain pattern itself is
// persistent (no re-roll) so the motion stays readable. Most legible at a glance of all the
// variants, but adds constant motion wherever affected colours appear.
// Amplitude still tracks actual colour-info loss; greys stay still.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Siblings: cvd-deutan.frag (binary baseline), -oriented, -alphabet, -combo.
//
// Tuning knobs:
//   STRENGTH    -> peak shimmer amplitude
//   0.06 / 0.45 -> loss thresholds (below 0.06 nothing shimmers; full amplitude by 0.45)
//   1.571       -> compass span in radians (90deg): red=up, green=down
//   SPEED       -> drift speed in px/s
//   2.0         -> grain cell size (px)

float grainCell(vec2 q){
    vec2 seed = fract(q * 0.01371);
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

    // Compass: axis +1 -> up, 0 -> right, -1 -> down (screen y grows downward).
    float axis = clamp(err.r - err.g, -1.0, 1.0);
    float phi  = axis * 1.571;                           // +-90deg
    vec2  dir  = vec2(cos(phi), -sin(phi));

    const float SPEED = 30.0;                            // px/s
    vec2  px = c.xy * niri_size - dir * niri_time * SPEED;
    float g  = grainCell(floor(px / 2.0));

    const float STRENGTH = 0.22;
    return vec4(s.rgb + g * amp * STRENGTH, s.a);        // luminance-only: hue untouched
}
