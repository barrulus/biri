// Pixel mosaic — chunky blocks, posterized colour, subtle grid. 8-bit feel.
//
// biri/niri window shader (niri mode). Static: does NOT use niri_time.
//   c.xy : 0..1 across the window, c.y = 0 at the TOP
//   niri_size : window size in physical pixels
//   tex2D_screen(uv) : samples the window's own composited pixels

vec4 global_color(vec3 c) {
    float block = 10.0;     // block size in physical px — raise for chunkier blocks
    float levels = 6.0;     // colour steps per channel — lower for a more retro palette

    vec2 px = c.xy * niri_size;
    vec2 cell = floor(px / block);

    // Sample the centre of each block so the whole block takes one colour.
    vec2 uvq = (cell + 0.5) * block / niri_size;
    vec4 s = tex2D_screen(uvq);

    // Posterize to a small set of levels.
    vec3 col = floor(s.rgb * levels + 0.5) / levels;

    // Faint dark grid lines between blocks.
    vec2 g = fract(px / block);
    float line = min(smoothstep(0.0, 0.06, g.x), smoothstep(0.0, 0.06, g.y));
    col *= mix(0.82, 1.0, line);

    return vec4(col, s.a);
}
