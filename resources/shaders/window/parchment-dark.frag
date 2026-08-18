// Parchment (dark-app variant) — parchment look for dark UIs like Discord,
// tuned for READABILITY. Instead of replacing dark areas with opaque paper
// (which kills text contrast), it remaps the UI's luminance through a parchment
// palette (dark bg -> deep brown, light text -> cream). Contrast is preserved
// because the map is monotonic in luminance; coloured elements (mentions, role
// colours, emoji) keep their colour. Crackle/crumple texture is layered on as a
// gentle multiplier so it reads as paper without hurting legibility.
//
// biri/niri window shader (niri mode). Static: does NOT use niri_time.
//   c.xy : 0..1 across the window, c.y = 0 at the TOP
//   niri_size : window size in physical pixels
//   tex2D_screen(uv) : samples the window's own composited pixels

// --- static procedural noise (seeded by pixel position, no time) -------------
float hash(vec2 p) {
    p = fract(p * vec2(123.34, 345.45));
    p += dot(p, p + 34.345);
    return fract(p.x * p.y);
}

vec2 hash2(vec2 p) {
    return vec2(hash(p), hash(p + vec2(57.0, 13.0)));
}

float vnoise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p) {
    float s = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 6; i++) {
        s += amp * vnoise(p);
        p = p * 2.02 + 17.0;
        amp *= 0.5;
    }
    return s;
}

float cracks(vec2 p, float width) {
    vec2 n = floor(p);
    vec2 f = fract(p);
    float f1 = 8.0;
    float f2 = 8.0;
    for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
            vec2 g = vec2(float(i), float(j));
            vec2 o = hash2(n + g);
            vec2 r = g + o - f;
            float d = dot(r, r);
            if (d < f1) { f2 = f1; f1 = d; }
            else if (d < f2) { f2 = d; }
        }
    }
    float edge = sqrt(f2) - sqrt(f1);
    return 1.0 - smoothstep(0.0, width, edge);
}

vec4 global_color(vec3 c) {
    vec2 uv  = c.xy;
    vec4 src = tex2D_screen(uv);
    vec2 px  = uv * niri_size;

    // ---- knobs -------------------------------------------------------------
    float crackAmount   = 0.10; // crackle veins (kept subtle for readability)
    float crumpleAmount = 0.22; // cloudy tone variation
    float colorKeep     = 0.80; // how much coloured UI keeps its own hue (0..1)
    float contrastBoost = 1.15; // >1 widens text/bg separation for legibility
    float edgeBurn      = 0.45; // burnt border (gentle so it doesn't eat the UI)
    float burnWidth     = 0.78; // 0..1, higher = thinner border
    // Parchment palette: shadow -> mid -> highlight by luminance.
    vec3 shadowCol = vec3(0.15, 0.10, 0.06);
    vec3 midCol    = vec3(0.52, 0.39, 0.23);
    vec3 highCol   = vec3(0.93, 0.85, 0.66);
    // ------------------------------------------------------------------------

    // Luminance, with a little contrast expansion around mid-grey so dark UIs
    // keep crisp text/background separation after the remap.
    float lum = dot(src.rgb, vec3(0.299, 0.587, 0.114));
    lum = clamp((lum - 0.5) * contrastBoost + 0.5, 0.0, 1.0);

    // Duotone parchment map (monotonic -> preserves which pixels are lighter).
    vec3 sepia = lum < 0.5
        ? mix(shadowCol, midCol, lum * 2.0)
        : mix(midCol, highCol, (lum - 0.5) * 2.0);

    // Keep colour where the source pixel is saturated (mentions, roles, emoji),
    // warmed slightly so it sits on the paper.
    float mx = max(max(src.r, src.g), src.b);
    float mn = min(min(src.r, src.g), src.b);
    float chroma = mx - mn;
    float colorful = smoothstep(0.10, 0.32, chroma) * colorKeep;
    vec3 warmed = src.rgb * vec3(1.04, 0.98, 0.84);
    vec3 base = mix(sepia, warmed, colorful);

    // ---- paper texture as a gentle multiplier (preserves contrast) ---------
    vec2 w  = vec2(fbm(px * 0.010), fbm(px * 0.010 + 19.0)) - 0.5;
    vec2 pw = px + w * 60.0;

    float crumple = mix(fbm(pw * 0.012), fbm(pw * 0.040), 0.4);
    float cr = cracks(pw * 0.020, 0.05);
    cr = max(cr, cracks(pw * 0.045 + 5.0, 0.05) * 0.7);

    float shade = 1.0 + crumpleAmount * (crumple - 0.5) * 2.0; // ~0.78..1.22
    shade += cr * crackAmount;                                 // faint light veins
    shade *= 0.98 + 0.04 * vnoise(px * 1.7);                   // fine tooth

    vec3 col = base * shade;

    // ---- gentle burnt edge -------------------------------------------------
    vec2 e = abs(uv - 0.5) * 2.0;
    float edge = max(e.x, e.y) + (fbm(px * 0.03) - 0.5) * 0.30;
    float burn = smoothstep(burnWidth, 1.05, edge);
    col *= mix(1.0, 0.45, burn * edgeBurn);
    col = mix(col, vec3(0.20, 0.12, 0.06), burn * edgeBurn * 0.5);

    return vec4(col, src.a);
}
