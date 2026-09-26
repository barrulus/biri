// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Rolling smoke-clouds — smoke blown against the glass, covering the whole window. The
// field doesn't sweep across: two fast counter-advecting warp fields make it boil and roll
// over itself in place, with a gentle upward billow like smoke curling against the pane.
// Kept deliberately dim and grey — muted bodies, slate shadows, low opacity — so text
// stays readable underneath.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule / window-shaders preset.
//
// Tuning knobs:
//   DENSITY -> smoke coverage (lower = more broken)
//   BOIL    -> how hard the smoke rolls over itself
//   CHURN   -> how fast the rolling turns over
//   RISE    -> upward billow speed
//   OPACITY -> how solid the smoke is over the content (keep low for readable text)

float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

float vnoise(vec2 p){
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p){
    float v   = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 4; i++){
        v  += amp * vnoise(p);
        p   = p * 2.03 + vec2(1.7, 9.2);
        amp *= 0.5;
    }
    return v;
}

vec4 global_color(vec3 c){
    vec4  s  = tex2D_screen(c.xy);
    float ar = niri_size.x / max(niri_size.y, 1.0);
    float t  = niri_time;

    const float DENSITY = 0.50;
    const float BOIL    = 0.75;
    const float CHURN   = 0.38;
    const float RISE    = 0.09;
    const float OPACITY = 0.40;

    vec2  a    = vec2(c.x * ar, c.y);
    float gate = smoothstep_any_order(0.0, 0.25, s.a);   // respect rounded corners

    // smoke pressed against the glass: the base field stays put...
    vec2  rel = a - vec2(ar * 0.5, 0.45);
    float r   = length(rel);
    vec2  p   = rel * 2.6;

    // ...and the motion is all churn: fast counter-advecting warp fields roll the smoke
    // over itself in place, while a slow rise makes it billow upward like it's curling
    vec2 q = vec2(fbm(p * 0.8 + vec2(0.0,  t * CHURN)),
                  fbm(p * 0.8 + vec2(5.2, -t * CHURN * 0.85)));
    vec2  wp = p + (q - 0.5) * BOIL * 2.8 + vec2(0.0, t * RISE);
    float n  = fbm(wp);

    // whole-window coverage, a little denser where the smoke keeps arriving
    float dens = n * (0.45 + DENSITY) * (1.05 - 0.20 * smoothstep_any_order(0.1, 0.9, r));
    float body = smoothstep_any_order(0.40, 0.68, dens);             // solid rolls
    float wisp = smoothstep_any_order(0.28, 0.48, dens);             // thin haze around them

    // lit faintly from above: compare against the field a touch higher on screen
    float n2  = fbm(wp + vec2(0.0, -0.35));
    float lit = clamp(0.62 + (n - n2) * 2.2, 0.30, 1.0);

    // muted greys — bright enough to read as smoke, dark enough not to mask text
    vec3  cl  = mix(vec3(0.30, 0.32, 0.37), vec3(0.66, 0.68, 0.72), lit);
    float cov = clamp(body + wisp * 0.35, 0.0, 1.0) * OPACITY * gate;

    // opaque element on a maybe-translucent window: push alpha with coverage
    return vec4(mix(s.rgb, cl, cov), mix(s.a, 1.0, cov));
}
