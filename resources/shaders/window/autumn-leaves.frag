// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Autumn leaves — leaves gust across the window, hugging the bottom edge as they bob and
// tumble; roughly one in five breaks loose and sails higher, but never past the window's
// midline. Each leaf is a pointed oval with a darker midrib that flips edge-on as it
// tumbles, in a russet / amber / olive palette.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule / window-shaders preset.
//
// Tuning knobs:
//   LEAVES        -> how many leaves are in flight (loop count; keep <= 24)
//   OPACITY       -> how solid the leaves are over the content
//   SPEED         -> base drift speed (window-widths per second, ish)
//   MAX_RISE      -> how far up the window the strays may climb (0.5 = halfway)
//   SIZE          -> leaf size (fraction of window height)
//   0.8 in `step` -> share of high flyers (higher = fewer strays)

float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float hash1(float n){ return hash(vec2(n, 1.7)); }
vec2 rot(vec2 p, float a){ float sn = sin(a); float cs = cos(a); return vec2(p.x * cs - p.y * sn, p.x * sn + p.y * cs); }

vec4 global_color(vec3 c){
    vec4  s  = tex2D_screen(c.xy);
    float ar = niri_size.x / max(niri_size.y, 1.0);
    float t  = niri_time;

    const int   LEAVES   = 18;
    const float OPACITY  = 0.92;
    const float SPEED    = 0.12;
    const float MAX_RISE = 0.5;
    const float SIZE     = 0.020;

    // everything above the midline is leaf-free — cheap early-out
    if (c.y < 1.0 - MAX_RISE - 0.07) return s;

    vec2  a    = vec2(c.x * ar, c.y);
    float gate = smoothstep_any_order(0.0, 0.25, s.a);   // respect rounded corners

    vec3  rgb   = s.rgb;
    float alpha = s.a;

    for (int i = 0; i < LEAVES; i++){
        float fi = float(i);
        float r1 = hash1(fi * 1.61 + 0.7);
        float r2 = hash1(fi * 2.23 + 4.1);
        float r3 = hash1(fi * 3.71 + 8.9);
        float r4 = hash1(fi * 5.13 + 2.3);

        // drift across, with a wind-surge stutter
        float spd = SPEED * (0.6 + 0.9 * r2);
        float x   = fract(r1 * 7.31 + t * spd + 0.05 * sin(t * 0.7 + r1 * 6.28));
        float X   = x * (ar + 0.24) - 0.12;                 // enter/exit off both edges

        // height: hugging the bottom, except the odd stray up to MAX_RISE
        float lowH  = 0.05 + 0.16 * r3 * r3;
        float highH = 0.22 + (MAX_RISE - 0.24) * r3;
        float hgt   = mix(lowH, highH, step(0.8, r4));
        float Y     = 1.0 - hgt
                    + 0.030 * sin(t * (0.9 + r2) + r1 * 6.28)   // bob
                    + 0.015 * sin(t * 2.6 + r4 * 6.28);         // flutter

        vec2  p  = a - vec2(X, Y);
        float sz = SIZE * (0.65 + 0.7 * r2);
        if (dot(p, p) > sz * sz * 12.0) continue;           // outside this leaf's reach

        // tumble + edge-on flip
        float ang    = t * (1.2 + 2.4 * r2) * sign(r4 - 0.5) + r1 * 6.28;
        float squash = 0.30 + 0.70 * abs(sin(t * (0.8 + 1.6 * r3) + r2 * 6.28));
        vec2  q      = rot(p, ang);
        float d      = length(vec2(q.x, q.y * 1.8 / squash)) / sz;
        float m      = smoothstep_any_order(1.0, 0.80, d) * gate;

        // russet / amber / olive, shaded darker when edge-on, dark midrib
        vec3 lc = mix(vec3(0.72, 0.30, 0.10), vec3(0.87, 0.60, 0.16), smoothstep_any_order(0.25, 0.60, r3));
        lc      = mix(lc, vec3(0.46, 0.44, 0.14), smoothstep_any_order(0.70, 0.90, r1));
        lc     *= (0.80 + 0.30 * r2) * (0.62 + 0.38 * squash);
        float rib = smoothstep_any_order(0.14, 0.03, abs(q.y) / max(sz * squash, 0.0001));
        lc      = mix(lc, lc * 0.72, rib);

        // opaque element on a maybe-translucent window: push alpha with coverage
        float cov = m * OPACITY;
        rgb   = mix(rgb, lc, cov);
        alpha = mix(alpha, 1.0, cov);
    }

    return vec4(rgb, alpha);
}
