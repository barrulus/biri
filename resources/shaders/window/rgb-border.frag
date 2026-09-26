// Custom shader by Barrulus.
// Descending smoothstep edges are undefined in GLSL; preserve descending-edge falloff explicitly.
float smoothstep_any_order(float a, float b, float x) {
    return a > b ? 1.0 - smoothstep(b, a, x) : smoothstep(a, b, x);
}
// RGB-border glow — "PC RGB lighting" for a window. The window content is passed through
// untouched; a rainbow rim hugs the window edge, the hue chases AROUND the perimeter and also
// cycles over time (niri_time), like an addressable RGB fan/strip.
//
// Contract: same as the global shaders — `vec4 global_color(vec3 c)`, c.xy = 0..1 across the
// WINDOW (c.y=0 at the top), tex2D_screen(uv) samples the window, niri_time = seconds.
// Attach via a niri window-rule (I'll leave the rule to you — this is just the shader source).
//
// Tuning knobs:
//   0.070 in `glow`  -> how far the soft glow bleeds inward
//   * 0.90           -> glow strength (raise for a brighter glow, lower if it fogs edge text)
//   niri_time*0.25   -> cycle speed
//   `s.rgb + rgb*m`  -> additive glow; swap to `mix(s.rgb, rgb, m)` for an opaque painted rim
vec4 global_color(vec3 c){
        vec4 s = tex2D_screen(c.xy);

        // distance to the nearest window edge (0 at edge, grows inward)
        float edge = min(min(c.x, 1.0-c.x), min(c.y, 1.0-c.y));
        // soft inner bleed only — no crisp bright rim, so it doesn't sit over edge text
        float m    = clamp(smoothstep_any_order(0.070, 0.0, edge) * 0.99, 0.0, 1.0);

        // hue chases around the perimeter (angle) and cycles over time
        float ang = atan(c.y-0.5, c.x-0.5) * 0.1591549;      // /(2pi) -> -0.5..0.5
        float hue = fract(ang + niri_time*0.25);
        vec3  rgb = 0.5 + 0.5*cos(6.2831853*(hue + vec3(0.0,0.33,0.67)));  // IQ rainbow

        return vec4(s.rgb + rgb*m, s.a);                     // additive RGB glow
    }
