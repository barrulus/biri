// Mercury sheen — a flowing liquid-metal / chrome surface raked with moving specular glints.
// A domain-warped noise field is treated as a molten surface; its normal fakes an environment
// reflection (dark steel in the valleys, bright chrome on the ridges) and a rotating light throws
// hot highlights that sweep across as the surface rolls. Neutral, slightly cool silver — mercury.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
//
// Tuning knobs:
//   OPACITY  -> how much the metal takes over the content (lower = more of a sheen over the app)
//   SCALE    -> size of the molten cells (bigger = finer, more turbulent mercury)
//   BUMP     -> surface relief; more = sharper light/dark banding and glints
//   SPECP    -> specular tightness (higher = smaller, harder glints)
//   flow speeds (0.05/0.06 warp, 0.4 light) -> how fast it churns / the light sweeps

float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

float vnoise(vec2 p){
    vec2 i = floor(p), f = fract(p);
    f = f*f*(3.0 - 2.0*f);
    float a = hash(i), b = hash(i + vec2(1.0,0.0));
    float d = hash(i + vec2(0.0,1.0)), e = hash(i + vec2(1.0,1.0));
    return mix(mix(a,b,f.x), mix(d,e,f.x), f.y);
}

float fbm(vec2 p){
    float v = 0.0, a = 0.5;
    for (int i = 0; i < 3; i++){ v += a*vnoise(p); p *= 2.0; a *= 0.5; }
    return v;
}

// Domain-warped fbm — warping the lookup by another fbm gives the swirly, molten mercury flow.
float flow(vec2 q){
    vec2 w = vec2(fbm(q + vec2(0.0, niri_time*0.06)),
                  fbm(q + vec2(niri_time*0.05, 0.0)));
    return fbm(q + 1.6*w);
}

vec4 global_color(vec3 c){
    vec4 s  = tex2D_screen(c.xy);
    vec2 ar = vec2(niri_size.x / max(niri_size.y, 1.0), 1.0);   // keep the cells round-ish

    const float SCALE = 3.0;
    const float BUMP  = 2.2;
    const float SPECP = 50.0;

    vec2 q = c.xy * ar * SCALE;

    // Surface normal from the flowing height field (forward differences).
    float e  = 0.06;
    float h0 = flow(q);
    float hx = flow(q + vec2(e, 0.0));
    float hy = flow(q + vec2(0.0, e));
    vec3  n  = normalize(vec3((h0 - hx)/e * BUMP, (h0 - hy)/e * BUMP, 1.0));

    // Fake environment reflection: vertical gradient -> dark steel to bright chrome.
    vec3  r   = reflect(vec3(0.0, 0.0, -1.0), n);
    float env = 0.5 + 0.5*r.y;
    vec3  chrome = mix(vec3(0.20, 0.22, 0.27), vec3(0.90, 0.93, 1.0), env);  // faint cool tint

    // Rotating light -> specular glints that sweep across the surface.
    vec3  L    = normalize(vec3(cos(niri_time*0.4), 0.4 + 0.4*sin(niri_time*0.3), 0.9));
    float spec = pow(max(dot(r, L), 0.0), SPECP) * 1.6;

    // Shade the content with the metal (silvery, light/dark banded) and add the hot glints.
    float lum    = dot(s.rgb, vec3(0.299, 0.587, 0.114));
    vec3  silver = mix(s.rgb, vec3(lum), 0.5);          // half-desaturate toward metal
    vec3  metal  = silver * mix(0.55, 1.35, env) + chrome*0.25 + spec;

    const float OPACITY = 0.55;
    return vec4(mix(s.rgb, metal, OPACITY), s.a);
}
