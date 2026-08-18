// CRT phosphor — scanlines, RGB aperture-grille mask, barrel bulge, vignette.
//
// biri/niri window shader (niri mode). Static: does NOT use niri_time.
//   c.xy : 0..1 across the window, c.y = 0 at the TOP
//   niri_size : window size in physical pixels
//   tex2D_screen(uv) : samples the window's own composited pixels

vec4 global_color(vec3 c) {
    vec2 uv = c.xy;

    // --- barrel distortion: bulge the picture outward like a glass tube -------
    vec2 cc = uv - 0.5;
    float r2 = dot(cc, cc);
    vec2 warp = uv + cc * r2 * 0.18;

    // Anything pushed outside the window after warping becomes black bezel.
    vec2 inb = step(vec2(0.0), warp) * step(warp, vec2(1.0));
    float mask = inb.x * inb.y;

    // --- chromatic aberration: split R/B outward, grows toward the edges -----
    float ca = 0.0016 + r2 * 0.004;
    vec3 col;
    col.r = tex2D_screen(warp + cc * ca).r;
    col.g = tex2D_screen(warp).g;
    col.b = tex2D_screen(warp - cc * ca).b;

    // --- scanlines: one dark band per ~2 physical pixel rows ------------------
    float sl = 0.5 + 0.5 * sin(warp.y * niri_size.y * 3.14159);
    col *= mix(0.72, 1.0, sl);

    // --- RGB aperture-grille mask: tint successive pixel columns R/G/B --------
    float triad = mod(floor(warp.x * niri_size.x), 3.0);
    vec3 grille = vec3(0.5);
    if (triad < 1.0) grille.r = 1.0;
    else if (triad < 2.0) grille.g = 1.0;
    else grille.b = 1.0;
    col *= mix(vec3(1.0), grille, 0.35);

    // --- vignette + a little brightness to compensate for the mask -----------
    float vig = smoothstep(0.85, 0.20, r2 * 2.0);
    col *= mix(0.45, 1.18, vig);

    col *= mask;
    return vec4(col, 1.0);
}
