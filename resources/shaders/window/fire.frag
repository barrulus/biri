// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Flames — fire licking upward from the bottom edge of the window. Domain-warped fBm noise
// scrolls upward and is cut by a rising threshold, so the sheet of fire is dense and
// white-hot at the base, breaks into separate tongues higher up, and dies out into cooling
// red tips. A soft additive halo sits under the solid flame body so the glow spills onto the
// window content without hiding it; everything above FLAME_HEIGHT is untouched.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule.
//
// Tuning knobs:
//   FLAME_HEIGHT   -> how far up the window the fire reaches (1.0 = whole window)
//   OPACITY        -> how solid the flame body is over the content (0 = glow only)
//   GLOW           -> strength of the additive halo around/below the flames
//   RISE           -> how fast the fire scrolls upward
//   DETAIL         -> flame scale (higher = more, thinner tongues)
//   SWAY           -> sideways licking; grows with height
//   0.22 in `core` -> edge hardness of the flame body (lower = crisper tongues)

float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

float vnoise(vec2 p){
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);                       // smoothstep interpolation
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p){
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 4; i++){
        v += a * vnoise(p);
        p  = p * 2.03 + vec2(1.7, 9.2);                // rotate-ish + scale each octave
        a *= 0.5;
    }
    return v;
}

vec4 global_color(vec3 c){
    vec4  s  = tex2D_screen(c.xy);
    float ar = niri_size.x / max(niri_size.y, 1.0);    // keep flames the same width everywhere
    float t  = niri_time;

    const float FLAME_HEIGHT = 0.50;
    const float OPACITY      = 0.80;
    const float GLOW         = 0.55;
    const float RISE         = 1.5;
    const float DETAIL       = 9.0;
    const float SWAY         = 0.55;

    float h = 1.0 - c.y;                               // 0 at the bottom edge, 1 at the top
    float x = c.x * ar;

    if (h > FLAME_HEIGHT * 1.6) return s;              // cheap early-out above the fire

    float hn = h / FLAME_HEIGHT;                       // 0..1 across the flame band

    // Sideways lick: a slow noise field, amplified with height so the base stays anchored.
    float sway = (vnoise(vec2(x * 3.0, h * 2.5 - t * RISE * 0.5)) - 0.5) * SWAY * hn;

    // Rising fire field: coarse body + a faster fine layer for the flickery detail.
    vec2  q = vec2(x * DETAIL + sway * 3.0, h * 1.9 - t * RISE);   // low y-freq = vertically stretched tongues
    float n = fbm(q) * 0.72 + fbm(q * 2.6 + vec2(0.0, -t * RISE * 1.9)) * 0.28;

    n *= 0.94 + 0.06 * sin(t * 11.0 + x * 6.0);        // global flicker

    // Threshold climbs with height, so the sheet of fire narrows into tongues then vanishes.
    float edge  = 0.10 + hn * 0.56 - smoothstep_any_order(0.04, 0.0, h) * 0.08;   // softly anchored at the base
    float core  = smoothstep_any_order(edge, edge + 0.22, n);    // solid flame body
    float halo  = smoothstep_any_order(edge - 0.20, edge + 0.34, n) * (1.0 - hn * 0.5);

    // Temperature: white-hot at the base, orange mid, deep red at the cooling tips.
    float temp = core * (1.0 - smoothstep_any_order(0.0, 1.0, hn) * 0.62);
    vec3  col  = mix(vec3(0.62, 0.05, 0.01), vec3(1.0, 0.34, 0.02), smoothstep_any_order(0.02, 0.40, temp));
    col        = mix(col, vec3(1.0, 0.74, 0.12),  smoothstep_any_order(0.38, 0.70, temp));
    col        = mix(col, vec3(1.0, 0.94, 0.72),  smoothstep_any_order(0.78, 1.0,  temp));

    // Halo first (additive spill), then the opaque body on top.
    vec3 rgb = s.rgb + vec3(1.0, 0.42, 0.08) * halo * GLOW * s.a;
    rgb      = mix(rgb, col, core * OPACITY * s.a);

    return vec4(rgb, s.a);                             // s.a: keep rounded corners clean
}
