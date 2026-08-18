// Rorschach — a shifting inkblot in the middle of the window. The blot is a central
// shape whose radius is strongly deformed by a domain-warped, slowly drifting noise
// field, mirrored around the vertical center axis (like a folded inkblot card) — so
// there is ALWAYS a blot, and the noise only decides its lobes, tendrils and splits.
// Everything outside the ink is left untouched.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
//
// Tuning knobs:
//   OPACITY -> how dark the ink presses onto the content (1.0 = solid black blot)
//   SPREAD  -> how far from the center the blot may reach (fraction of window height)
//   SCALE   -> lobe detail (bigger = more, finer lobes; smaller = one fat blob)
//   WOBBLE  -> how violently the noise deforms the blot (0 = plain breathing circle)
//   MORPH   -> how fast the blot shifts (he never stops moving, but he's not in a hurry)
//   EDGE    -> ink edge softness (smaller = crisper boundary)

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

const float MORPH = 0.55;

// Domain-warped fbm, drifting slowly — warping the lookup by another moving fbm is what
// gives the blot its oozing, organic tendrils instead of round noise bubbles.
float blot(vec2 q){
    float t = niri_time * MORPH;
    vec2 w = vec2(fbm(q + vec2(0.0, t*0.11) + 3.7),
                  fbm(q + vec2(t*0.09, 0.0) + 1.3));
    return fbm(q + 2.2*w + vec2(0.0, t*0.03));
}

vec4 global_color(vec3 c){
    vec4 s  = tex2D_screen(c.xy);
    vec2 ar = vec2(niri_size.x / max(niri_size.y, 1.0), 1.0);   // keep the blot round-ish

    const float OPACITY = 0.55;
    const float SPREAD  = 0.65;
    const float SCALE   = 3.0;
    const float WOBBLE  = 1.1;
    const float EDGE    = 0.012;

    // Centered, aspect-corrected coords; fold the card (mirror x) for bilateral symmetry.
    vec2  p = (c.xy - 0.5) * ar;
    vec2  q = vec2(abs(p.x), p.y) * SCALE;
    float r = length(p);

    // Deform the blot radius with the morphing field (plus a fine octave that roughens
    // the edge) and a slow breath, so it swells, splits into lobes and re-forms.
    float n      = (blot(q) - 0.47) + 0.10*(vnoise(q*9.0 + niri_time*0.2) - 0.5);
    float breath = 0.05*sin(niri_time*0.21);
    float radius = SPREAD * max(0.55 + breath + WOBBLE*n, 0.15);

    float ink = smoothstep(radius + EDGE, radius - EDGE, r);

    // Press near-black ink onto the content; everything outside the blot is untouched.
    vec3 black = vec3(0.03, 0.03, 0.04);
    return vec4(mix(s.rgb, black, ink * OPACITY), s.a);
}
