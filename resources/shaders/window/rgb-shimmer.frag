// RGB shimmer — a faint iridescent oil-slick sheen that drifts across the window in soft patches.
// Deliberately SUBTLE and NON-UNIFORM: most of the window shows nothing, and where the sheen does
// appear it's organic blobs of shifting rainbow (noise-driven), not regular bands. The content
// stays fully readable; this is a gentle holographic glimmer, not a colour wash.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window px; niri_time = seconds.
//
// Tuning knobs:
//   STRENGTH   -> peak opacity of the sheen (raise for more presence; still patchy)
//   COVERAGE   -> the two smoothstep edges below set how much of the window is ever touched
//   *3.0 / *2.0 -> spatial scale of the hue field / the coverage patches (bigger = finer)
//   niri_time*0.03..0.05 -> drift / hue-cycle speeds (all slow on purpose)
//   spark      -> faint sparse twinkle; drop the 0.35 to kill it entirely

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
    return v;   // ~0 .. ~0.875
}

vec4 global_color(vec3 c){
    vec4 s  = tex2D_screen(c.xy);
    vec2 ar = vec2(niri_size.x / max(niri_size.y, 1.0), 1.0);   // keep patches round-ish
    vec2 p  = c.xy * ar;

    // Organic hue field — soft blobs of colour that drift and cycle.
    float f   = fbm(p*3.0 + vec2(niri_time*0.13, niri_time*0.08));
    float hue = fract(f + niri_time*0.08);
    vec3  rb  = 0.5 + 0.5*cos(6.2831853*(hue + vec3(0.0, 0.33, 0.67)));  // IQ rainbow

    // Coverage mask — a DIFFERENT drifting noise decides WHERE the sheen shows, so it's patchy:
    // large clear areas, occasional soft iridescent patches.
    float patch = smoothstep(0.35, 0.62, fbm(p*2.0 - vec2(niri_time*0.10, 0.0)));

    // Very sparse faint twinkle sitting on the patches.
    float spark = pow(vnoise(p*38.0 + niri_time*1.5), 22.0) * 0.35;

    const float STRENGTH = 0.38;                 // subtle but visible; still patchy
    float amt = patch * STRENGTH;                // modulated -> never uniform
    return vec4(mix(s.rgb, rb + spark, amt), s.a);
}
