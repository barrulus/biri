// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Snowfall — gentle decorative snow drifting down the window. Three depth layers of soft
// fuzzy flakes: near flakes are bigger, brighter and faster; far flakes finer, fainter and
// slower. Each layer sways sideways a little as it falls. Sparse on purpose (most grid
// cells hold no flake) and low opacity, so the content stays fully readable.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule.
//
// Tuning knobs:
//   OPACITY        -> overall snow strength
//   0.72 in `on`   -> flake sparsity (higher = fewer flakes)
//   scale/speed    -> flake size + fall tempo per layer (bigger scale = smaller flakes)
//   0.012 sway     -> sideways drift amplitude
//   r / r*0.15     -> flake radius and edge fuzz (raise 0.15 toward 0.6 for harder flakes)

float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
vec2 hash2(vec2 p){ return vec2(hash(p), hash(p + 19.19)); }

vec4 global_color(vec3 c){
    vec4 s  = tex2D_screen(c.xy);
    vec2 ar = vec2(niri_size.x / max(niri_size.y, 1.0), 1.0);   // keep flakes round
    float snow = 0.0;

    for (int i = 0; i < 3; i++){
        float fi    = float(i);                    // 0 = nearest layer
        float scale = 14.0 + fi * 10.0;            // farther = finer flakes
        float speed = 0.055 - fi * 0.015;          // farther = slower fall

        vec2 p = c.xy * ar;
        p.x += sin(niri_time * 0.35 + fi * 2.1 + c.y * 4.0) * 0.012;   // gentle sway
        p.y -= niri_time * speed;                  // field scrolls -> flakes fall

        vec2  g    = p * scale;
        vec2  cell = floor(g);
        vec2  f    = fract(g);
        float on   = step(0.72, hash(cell + fi * 17.0));   // sparse: ~28% of cells
        vec2  fp   = 0.2 + 0.6 * hash2(cell + fi * 31.0);  // flake spot within its cell
        float d    = length(f - fp);
        float r    = 0.10 + 0.12 * hash(cell + 3.7);       // per-flake size
        float flake = smoothstep_any_order(r, r * 0.15, d);          // soft fuzzy disc

        snow += flake * on * (1.0 - fi * 0.3);             // nearer = brighter
    }

    const float OPACITY = 0.35;
    float m = min(snow, 1.0) * OPACITY * s.a;              // s.a: keep rounded corners clean
    return vec4(mix(s.rgb, vec3(0.95, 0.97, 1.0), m), s.a);
}
