// Ripple drops — small water ripples spawn at RANDOM origins and flow outward across the window,
// each a refracting wavefront that bends the content and leaves a faint bright crest, then fades.
// A few independent drops, each with its own random position, tempo and phase — and a DUTY CYCLE
// so every slot goes quiet between drops. That keeps only ~1 (occasionally 2) rippling at once,
// like the odd raindrop rather than a downpour.
//
// Contract: vec4 global_color(vec3 c); c.xy = 0..1 across the window (c.y = 0 at the TOP);
// tex2D_screen(uv) samples the window; niri_size = window size in px; niri_time = seconds.
//
// Tuning knobs:
//   N        -> number of drop slots (more = busier)
//   DUTY     -> fraction of each slot's period that's active (lower = sparser, more dead time)
//   period   -> 3.0 + up to 3.5s; overall tempo of new drops (bigger = calmer)
//   0.55     -> how far each ripple expands (fraction of the window)
//   0.010    -> refraction amount (bigger = stronger distortion)
//   90.0     -> wave frequency (more = tighter concentric rings)
//   hi*0.12  -> brightness of the ripple crests

float hash11(float n){ return fract(sin(n*127.1)      * 43758.5453); }
vec2  hash21(float n){ return fract(sin(vec2(n*127.1, n*311.7)) * 43758.5453); }

vec4 global_color(vec3 c){
    vec2 uv = c.xy;
    vec2 ar = vec2(niri_size.x / max(niri_size.y, 1.0), 1.0);   // aspect correction -> round ripples

    const int   N    = 3;
    const float DUTY = 0.5;                                      // active for half the period, idle the rest
    vec2  disp = vec2(0.0);
    float hi   = 0.0;

    for (int i = 0; i < N; i++) {
        float fi     = float(i);
        float period = 3.0 + hash11(fi + 0.3) * 3.5;            // each drop its own (slower) tempo
        float t      = niri_time / period + hash11(fi + 5.7) * 7.0;
        float life   = fract(t);                                // 0..1 within the slot's period
        vec2  origin = hash21(floor(t)*3.19 + fi*11.0);         // new random spot each cycle

        float p    = life / DUTY;                               // 0..1 while active, >1 while idle
        vec2  delta = uv - origin;
        float d    = length(delta * ar);                        // round distance to the drop
        float r    = p * 0.55;                                  // wavefront expands outward
        float env  = smoothstep(0.0, 0.08, p) * smoothstep(1.0, 0.5, p);  // fade in, out, and ZERO when idle (p>1)
        float wave = sin((d - r)*90.0) * exp(-abs(d - r)*22.0) * env;      // wavelets at the front

        vec2 dir = delta / max(length(delta), 1e-4);            // radial direction (uv space)
        disp += dir * wave * 0.010;                             // refract the content
        hi   += max(wave, 0.0);                                 // crest highlight
    }

    vec4 s = tex2D_screen(uv + disp);
    return vec4(s.rgb + hi*0.12, s.a);                          // content + subtle bright crests
}
