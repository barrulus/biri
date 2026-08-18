// Edge fisheye + RGB split — a frozen lens that swells toward the window edges,
// with chromatic fringing that grows with radius. Centre stays crisp.
//
// biri/niri window shader (niri mode). Static: does NOT use niri_time.
//   c.xy : 0..1 across the window, c.y = 0 at the TOP
//   niri_size : window size in physical pixels
//   tex2D_screen(uv) : samples the window's own composited pixels

vec4 global_color(vec3 c) {
    vec2 uv = c.xy;
    vec2 d = uv - 0.5;
    float r = length(d);

    // Fisheye: push samples outward; strength rises with r^2 so the centre is
    // untouched and the edges warp the most.
    float k = 0.35;                       // warp strength — raise for a stronger bulge
    vec2 warp = uv + d * r * r * k;

    // Chromatic split along the radial direction; magnitude grows with radius
    // (zero fringe at the centre).
    vec2 dir = d / max(r, 1e-4);
    float ca = 0.004 + r * 0.012;
    float rC = tex2D_screen(warp + dir * ca).r;
    float gC = tex2D_screen(warp).g;
    float bC = tex2D_screen(warp - dir * ca).b;
    float a  = tex2D_screen(warp).a;

    vec3 col = vec3(rC, gC, bC);

    // Slight edge darkening so the warp reads as a lens rather than a smear.
    col *= mix(1.0, 0.70, smoothstep(0.35, 0.72, r));

    return vec4(col, a);
}
