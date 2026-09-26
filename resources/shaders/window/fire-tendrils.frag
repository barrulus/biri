// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Fire — flames licking up from the bottom edge, where pieces tear off the tips and travel on.
// ONE field does all of it, the way rorschach.frag makes an inkblot split into lobes: a
// domain-warped fBm (the lookup displaced by two more drifting fBms) is advected upward and cut
// against a threshold that climbs with height. The warp is what makes tendrils rather than round
// noise bubbles, so where the threshold overtakes a narrowing tongue the tip PINCHES OFF into a
// free-floating island that keeps rising, shrinks and dies — connecting and disconnecting on its
// own. No particles, no separate ember pass; the pieces are the same fire as the sheet below.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule.
//
// Tuning knobs:
//   FLAME_HEIGHT -> height of the main sheet; torn-off pieces drift to roughly 1.5x this
//                   (safe to raise: 0.6 gives tall flames over half the window)
//   THRESH / SLOPE -> where fire starts / how fast it thins with height (raise SLOPE = shorter,
//                     more broken flames; lower = a taller solid sheet that rarely tears)
//   WARP         -> tendril strength. 0 = round noise bubbles that never pinch off; ~1.6 = fire
//   EDGE         -> flame edge crispness (small = hard-cut islands, large = soft gas)
//   XFREQ/YFREQ  -> flame width / vertical stretch (YFREQ well below XFREQ = tall tongues)
//   RISE         -> how fast the whole field climbs;  BREAK -> fine roughness that frays tips
//   OPACITY/GLOW -> solidity of the fire / strength of the additive halo around it

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
    for (int i = 0; i < 4; i++){ v += a*vnoise(p); p *= 2.0; a *= 0.5; }
    return v;
}

// White-hot -> orange -> deep red as `temp` falls from 1 to 0.
vec3 heat_color(float temp){
    vec3 col = mix(vec3(0.62, 0.05, 0.01), vec3(1.0, 0.34, 0.02), smoothstep_any_order(0.02, 0.40, temp));
    col      = mix(col, vec3(1.0, 0.74, 0.12), smoothstep_any_order(0.38, 0.70, temp));
    return     mix(col, vec3(1.0, 0.94, 0.72), smoothstep_any_order(0.78, 1.00, temp));
}

vec4 global_color(vec3 c){
    vec4  s  = tex2D_screen(c.xy);
    float ar = niri_size.x / max(niri_size.y, 1.0);
    float t  = niri_time;

    const float FLAME_HEIGHT = 0.40;
    const float THRESH       = 0.40;
    const float SLOPE        = 0.36;
    const float WARP         = 2.0;
    const float EDGE         = 0.022;
    const float XFREQ        = 10.0;
    const float YFREQ        = 3.4;
    const float RISE         = 1.6;
    const float BREAK        = 0.17;
    const float OPACITY      = 0.85;
    const float GLOW         = 0.35;

    float h = 1.0 - c.y;                                   // 0 at the bottom edge, 1 at the top
    if (h > FLAME_HEIGHT * 2.0) return s;                  // nothing burns up here

    float x  = c.x * ar;
    float hn = h / FLAME_HEIGHT;

    // The field, rising. Warping the lookup by two more drifting fBms is what grows tendrils
    // that can neck down and separate, instead of blobs that only fade.
    vec2  q = vec2(x * XFREQ, h * YFREQ - t * RISE);
    vec2  w = vec2(fbm(q * 0.6 + vec2(0.0, -t * 0.60) + 3.7),
                   fbm(q * 0.6 + vec2(t * 0.24, -t * 0.45) + 1.3));
    float n = fbm(q + WARP * w);
    n += BREAK * (vnoise(q * 3.5 + vec2(0.0, -t * 2.4)) - 0.5);   // fray the tips
    n *= 0.95 + 0.05 * sin(t * 9.0 + x * 5.0);                    // flicker

    // Threshold climbs with height: solid sheet -> tongues -> the tips pinch off -> nothing.
    float thr = THRESH + pow(max(hn, 0.0), 0.75) * SLOPE
              - smoothstep_any_order(0.05, 0.0, h) * 0.12;                  // anchor the base
    float fire = smoothstep_any_order(thr - EDGE, thr + EDGE, n);
    float halo = smoothstep_any_order(thr - 0.16, thr + 0.02, n);

    if (fire + halo <= 0.0) return s;

    // Temperature is anchored to HEIGHT, with the field only modulating it. Deriving it from
    // (n - thr) alone collapses toward 0 wherever the threshold ramp is shallow -- which is
    // exactly what raising FLAME_HEIGHT does -- and heat_color(0) is an opaque dark red, so the
    // window fills with brown mud instead of flames. Keep the height term dominant.
    float d    = clamp((n - thr) / 0.22, 0.0, 1.0);          // how deep inside the flame we are
    float temp = (0.55 + 0.45 * d) * (1.0 - smoothstep_any_order(0.0, 1.25, hn) * 0.85);

    // Composite the fire as its OWN layer (premultiplied "over"), so it reads on translucent
    // windows instead of merely tinting them; mask keeps it inside the rounded corners.
    float mask = smoothstep_any_order(0.0, 0.25, s.a);
    vec3  rgb  = s.rgb + vec3(1.0, 0.42, 0.08) * halo * GLOW * mask;
    float cov  = clamp(fire * OPACITY, 0.0, 1.0) * mask;
    rgb = mix(rgb, heat_color(temp), cov);
    return vec4(rgb, mix(s.a, 1.0, cov));
}
