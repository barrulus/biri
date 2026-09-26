// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// Prairie wind — not grass itself, but the pattern wind makes sweeping over a grassland.
// Deliberately erratic, the way real wind works a field: the wind direction itself wanders
// in space and time, so gust fronts arrive curved and broken rather than as straight bands;
// on top of them, patchy cat's-paw bursts flare up and die away, and the whole wind surges
// and slackens. Where the wind works, it pushes a colour field along with it in prairie
// tones — sage, wheat gold, pale straw — that streams and eddies downwind. The window
// content is never resampled or displaced (no smudging); the colours ride over it. A pale
// flash marks each front's leading edge, and a fine fast ripple glitters like straw — the
// susurration. Calm patches between gusts stay completely clear.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
// Attach via a niri window-rule / window-shaders preset.
//
// Tuning knobs:
//   SWEEP    -> how fast the gust fronts travel
//   STRENGTH -> peak opacity of the wind-blown colour
//   SHEEN    -> strength of the pale flash on the leading edges
//   SHADE    -> how much the gust troughs settle darker
//   SCALE    -> gust size (higher = smaller, busier gusts)
//   WANDER   -> how far the wind direction strays from WIND
//   WIND     -> mean wind direction (unit vector; default blows right, slightly downhill)
//   pal()    -> the prairie palette (sage -> wheat gold -> pale straw)

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

// prairie palette: sage green through wheat gold to pale straw
vec3 pal(float x){
    vec3 col = mix(vec3(0.28, 0.38, 0.20), vec3(0.78, 0.64, 0.28), smoothstep_any_order(0.15, 0.55, x));
    return mix(col, vec3(0.93, 0.88, 0.66), smoothstep_any_order(0.60, 0.92, x));
}

vec4 global_color(vec3 c){
    vec4  s  = tex2D_screen(c.xy);
    float ar = niri_size.x / max(niri_size.y, 1.0);
    float t  = niri_time;

    const float SWEEP    = 0.28;
    const float STRENGTH = 0.35;
    const float SHEEN    = 0.12;
    const float SHADE    = 0.07;
    const float SCALE    = 1.0;
    const float WANDER   = 1.4;
    const vec2  WIND     = vec2(0.966, 0.259);     // ~15 degrees downhill

    vec2  a    = vec2(c.x * ar, c.y);
    float gate = smoothstep_any_order(0.0, 0.25, s.a);       // respect rounded corners

    // the wind wanders: local direction strays from the mean, in space and slowly in time
    vec2  perp0 = vec2(-WIND.y, WIND.x);
    float wob   = ((fbm(a * 0.9 + vec2(t * 0.09, 0.0)) - 0.5) * WANDER + 0.25 * sin(t * 0.40));
    vec2  wdir  = normalize(WIND + perp0 * wob);
    vec2  wperp = vec2(-wdir.y, wdir.x);
    float u = dot(a, wdir);
    float v = dot(a, wperp);

    // gust field: advects along the (curving) wind, then gets warped again so the fronts
    // arrive bent and broken instead of as straight bands
    vec2 gp = vec2(u * 1.4 * SCALE - t * SWEEP * 2.2, v * 1.9 * SCALE);
    gp += (vec2(fbm(gp * 0.9 + 3.7), fbm(gp * 0.9 + 8.1)) - 0.5) * 0.9;
    float g  = fbm(gp);
    float g2 = fbm(gp + vec2(0.14, 0.0));          // one step upwind -> leading-edge detector

    // the whole wind surges and slackens...
    float surge = 0.70 + 0.30 * vnoise(vec2(t * 0.35, 3.3));
    float gust  = smoothstep_any_order(0.32, 0.70, g) * surge;
    // ...and patchy cat's-paw bursts flare up and die away on their own
    float paw   = fbm(a * 2.4 * SCALE + vec2(-t * SWEEP * 1.2, t * 0.10));
    float burst = smoothstep_any_order(0.50, 0.76, paw) * (0.55 + 0.45 * sin(t * 2.8 + paw * 12.0));
    gust = clamp(gust + burst * 0.7, 0.0, 1.0);

    // fine fast ripple riding wherever the wind works: the susurration
    float rip = (vnoise(vec2(u * 9.0 - t * SWEEP * 7.0, v * 14.0)) - 0.5) * gust;

    // the colour field the wind pushes around: advects downwind, surges ahead where a
    // front passes, eddies with the ripple — content underneath is never resampled
    float push = t * SWEEP * 1.6 + gust * 0.35 + rip * 0.15;
    float f    = fbm(vec2((u - push) * 2.2, v * 3.6 - t * 0.22));
    vec3  wc   = pal(clamp(f * 1.5 - 0.15, 0.0, 1.0));

    // the colour only shows where the wind is working; calm patches stay clear
    float amt = smoothstep_any_order(0.10, 0.65, gust) * STRENGTH * gate;
    vec3  rgb = mix(s.rgb, wc, amt);

    // pale flash where a front's leading edge catches the light...
    float front = clamp((g - g2) * 3.0, 0.0, 1.0) * gust;
    rgb += vec3(0.90, 0.86, 0.68) * front * SHEEN * s.a * gate;
    // ...and a slightly darker settle in the flattened trough behind it
    rgb *= 1.0 - SHADE * gust * (1.0 - front) * gate;
    // the ripple glitters like dry straw catching the light
    rgb += vec3(0.88, 0.80, 0.52) * max(rip, 0.0) * 0.12 * s.a * gate;

    return vec4(rgb, s.a);                         // s.a: keep rounded corners clean
}
