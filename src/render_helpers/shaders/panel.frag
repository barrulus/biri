precision highp float;

varying vec2 niri_v_coords;

uniform mat3 niri_panel_inv;
uniform float niri_panel_dim;
uniform float niri_alpha;
uniform sampler2D niri_panel_tex;

void main() {
    vec3 s = niri_panel_inv * vec3(niri_v_coords, 1.0);
    vec2 uv = s.xy / s.z;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || s.z <= 0.0) {
        gl_FragColor = vec4(0.0);
        return;
    }
    vec4 color = texture2D(niri_panel_tex, uv);
    // Depth dim: darken rgb only (premultiplied texture, so scale rgb).
    color.rgb *= niri_panel_dim;
    gl_FragColor = color * niri_alpha;
}
