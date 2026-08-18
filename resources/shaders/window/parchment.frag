// Parchment / vellum — aged treasure-map paper laid OVER the window.
// Builds a standalone parchment "sheet" (tan tone + crackle veins + burnt
// edges) and composites it so dark areas BECOME the paper while bright,
// coloured content (text/icons) stays its own colour, like ink on parchment.
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

// Crackle veins: Worley/Voronoi cell-border lines. ~1 on a crack, 0 off.
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
    float crackAmount   = 0.22; // brightness of the crackle veins on the paper
    float crumpleAmount = 0.40; // strength of the cloudy crumple shading
    float paperOpacity  = 0.92; // how fully dark areas turn into paper (1 = opaque)
    float inkKeep       = 0.55; // content brighter than this keeps its own colour
    float desat         = 0.45; // pull content toward neutral so it reads as ink
    float warmth        = 0.22; // warm wash over everything (0 = none)
    float edgeBurn      = 0.95; // darkness of the burnt border
    float burnWidth     = 0.50; // 0..1, lower = wider/thicker dark border
    vec3  paperTan      = vec3(0.86, 0.66, 0.42); // base parchment hue
    // ------------------------------------------------------------------------

    // Domain-warp so the texture looks organic, not regular.
    vec2 w  = vec2(fbm(px * 0.010), fbm(px * 0.010 + 19.0)) - 0.5;
    vec2 pw = px + w * 60.0;

    float crumple = mix(fbm(pw * 0.012), fbm(pw * 0.040), 0.4);

    float cr = cracks(pw * 0.020, 0.05);
    cr = max(cr, cracks(pw * 0.045 + 5.0, 0.05) * 0.7);

    // Irregular burnt edge (box-distance to border, warped by noise).
    vec2 e = abs(uv - 0.5) * 2.0;
    float edge = max(e.x, e.y) + (fbm(px * 0.03) - 0.5) * 0.35;
    float burn = smoothstep(burnWidth, 1.05, edge);

    // ---- build the standalone parchment sheet ------------------------------
    float tone = 0.72 + crumpleAmount * (crumple - 0.5) * 2.0;
    vec3 sheet = tone * paperTan;
    sheet += vec3(0.95, 0.85, 0.60) * cr * crackAmount;   // light veins
    sheet *= 0.97 + 0.05 * vnoise(px * 1.7);              // fine tooth
    sheet *= mix(1.0, 0.22, burn * edgeBurn);             // burnt darkening
    sheet = mix(sheet, vec3(0.20, 0.11, 0.05), burn * edgeBurn * 0.65); // scorch tint

    // ---- compose over the real window content ------------------------------
    float lum = dot(src.rgb, vec3(0.299, 0.587, 0.114));
    // Desaturate so a strongly-coloured window (e.g. a transparent terminal over
    // a warm wallpaper) reads as aged ink rather than glowing.
    vec3 content = mix(src.rgb, vec3(lum), desat);

    // Hue-preserving texture modulation for bright (content) areas: gentle tone
    // only (NO veins here — cracks belong to the paper, not the content, or they
    // glow over bright windows). Normalised so average paper => 1.0.
    float texMod = (tone / 0.72);
    texMod *= mix(1.0, 0.30, burn * edgeBurn);
    vec3 col = content * texMod;

    // Replace dark areas with the actual paper sheet so backgrounds become
    // parchment; bright coloured content stays itself (ink on paper).
    float paperMix = (1.0 - smoothstep(0.06, inkKeep, lum)) * paperOpacity;
    col = mix(col, sheet, paperMix);

    // Gentle warm aging wash (keeps hue).
    col *= mix(vec3(1.0), vec3(1.05, 1.00, 0.86), warmth);

    return vec4(col, src.a);
}
