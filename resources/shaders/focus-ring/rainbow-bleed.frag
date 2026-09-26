// Custom shader by Barrulus.
// Glossy rainbow/neon ripple, paired with an inner-content overlay.
// These constants are shader-specific: change them or replace the whole algorithm.
#ifndef WAX_STRENGTH
#define WAX_STRENGTH 0.75
#endif
#ifndef WAX_BRIGHTNESS
#define WAX_BRIGHTNESS 1.0
#endif
#ifndef WAX_PHASE
#define WAX_PHASE mod(niri_time / 4.0, 1.0)
#endif
const float RIPPLE_INSET = 16.0;
const float RIPPLE_OUTSET = 12.0;

// Uneven periodic fields rather than equally spaced waves. Warping the sampling
// coordinate makes the lobes bunch up and stretch; all frequencies stay integral
// so the perimeter seam and the four-second animation loop remain continuous.
float wax_field(float u, float phase) {
    const float tau = 6.28318530718;
    return 0.46 * sin(tau * (3.0 * u - phase) + 0.7)
        + 0.28 * sin(tau * (7.0 * u + 2.0 * phase) + 2.1)
        + 0.17 * sin(tau * (11.0 * u - 2.0 * phase) + 4.4)
        + 0.09 * sin(tau * (19.0 * u + 3.0 * phase) + 1.3);
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    const float tau = 6.28318530718;
    float phase = WAX_PHASE;
    float strength = clamp(WAX_STRENGTH, 0.0, 1.0);
    float width = ring_width;
    vec2 nominal_size = ring_size + vec2(width * 2.0);
    vec2 p = (coords - ring_size * 0.5) / max(nominal_size * 0.5, vec2(1.0));
    float u = atan(p.y, p.x) / tau;
    float drift = u + 0.045 * wax_field(u + 0.13, phase);
    float bend = wax_field(drift, phase);
    float fine = wax_field(drift * 2.0 + 0.31, phase + 0.21);
    float pool = smoothstep(-0.65, 0.65, wax_field(drift + 0.43, phase + 0.37));

    float distance = ring_distance(coords);
    float extent = min(ring_width + ring_padding, RIPPLE_OUTSET);
    float half_px = 0.5 / max(niri_scale, 0.01);
    if (distance <= -RIPPLE_INSET || distance >= extent) return vec4(0.0);
    // Keep the original outer contour, but let its pools roll through the client
    // boundary. The matching window pass paints the inward half of this field.
    float original_inner = width * strength * max(0.0, 0.40 + 0.34 * bend + 0.20 * fine);
    float original_thickness = width * mix(1.0, 0.28 + 1.65 * pool + 0.20 * fine, strength);
    float outer_edge = min(original_inner + original_thickness, max(extent - 2.0 * half_px, 0.0));
    float inner_edge = -min(width * strength * (0.35 + 1.55 * pool + 0.18 * bend), RIPPLE_INSET - 2.0 * half_px);
    float thickness = max(outer_edge - inner_edge, 0.01);
    float coverage = smoothstep(inner_edge - half_px, inner_edge + half_px, distance)
        * (1.0 - smoothstep(outer_edge - half_px, outer_edge + half_px, distance));

    // Swirled pastel pigment and narrow, broken highlights give the pools a waxy
    // surface. The highlight meanders across the band instead of whitening its
    // entire cross-section like a light travelling through a tube.
    float across = clamp((distance - inner_edge) / max(thickness, 0.01), 0.0, 1.0);
    float pigment = u - phase + 0.10 * bend + 0.025 * fine
        + 0.025 * sin(tau * (across * 0.65 + drift * 8.0 + phase));
    vec3 hue = clamp(abs(fract(pigment + vec3(0.0, 2.0/3.0, 1.0/3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
    float milk = clamp(0.30 + 0.18 * bend + 0.12 * pool, 0.12, 0.62);
    vec3 rgb = mix(hue, vec3(1.0), milk);
    float ridge = 0.48 + 0.18 * fine;
    float sheen = exp(-pow((across - ridge) / 0.17, 2.0))
        * smoothstep(0.30, 0.85, pool) * (0.65 + 0.35 * bend);
    rgb = mix(rgb, vec3(1.0), 0.8 * sheen);
    rgb *= 0.90 + 0.10 * sin(tau * (across * 0.5 + 0.1 * bend));
    rgb = clamp(rgb * WAX_BRIGHTNESS, 0.0, 1.0);
    float envelope = smoothstep(-RIPPLE_INSET, -RIPPLE_INSET + 2.0 * half_px, distance)
        * (1.0 - smoothstep(max(extent - 2.0 * half_px, 0.0), extent, distance));
    return vec4(rgb, coverage * envelope);
}
