precision highp float;
uniform float niri_alpha;
uniform sampler2D niri_tex;
uniform vec4 area;
uniform vec4 window;
uniform vec4 source_rect;
uniform vec2 texture_size;
varying vec2 niri_v_coords;
uniform vec2 deformation_0;
uniform vec2 deformation_1;
uniform vec2 deformation_2;
uniform vec2 deformation_3;
uniform vec2 deformation_4;
uniform vec2 deformation_5;
uniform vec2 deformation_6;
uniform vec2 deformation_7;
uniform vec2 deformation_8;
uniform vec2 deformation_9;
uniform vec2 deformation_10;
uniform vec2 deformation_11;
uniform vec2 deformation_12;
uniform vec2 deformation_13;
uniform vec2 deformation_14;
uniform vec2 deformation_15;
// One bicubic surface couples the whole window, with constant uniform indices
// for GLSL ES 1.00. The CPU pins the cursor with the same Bernstein weights.
vec2 physics_row(float t, vec2 a, vec2 b, vec2 c, vec2 d) {
    float u = 1.0 - t;
    return u * u * (u * a + 3.0 * t * b) + t * t * (3.0 * u * c + t * d);
}

vec2 physics_offset(vec2 uv) {
    vec2 p = clamp(uv, 0.0, 1.0);
    vec2 a = physics_row(p.x, deformation_0, deformation_1, deformation_2, deformation_3);
    vec2 b = physics_row(p.x, deformation_4, deformation_5, deformation_6, deformation_7);
    vec2 c = physics_row(p.x, deformation_8, deformation_9, deformation_10, deformation_11);
    vec2 d = physics_row(p.x, deformation_12, deformation_13, deformation_14, deformation_15);
    return physics_row(p.y, a, b, c, d);
}

void main() {
    vec2 target = area.xy + niri_v_coords * area.zw;
    vec2 source = target;
    for (int i = 0; i < 28; i++)
        source = target - physics_offset((source-window.xy)/window.zw);
    vec2 p = source - source_rect.xy;
    if (any(lessThan(p, vec2(0.0))) || any(greaterThan(p, source_rect.zw))) {
        gl_FragColor = vec4(0.0);
    } else {
        gl_FragColor = texture2D(niri_tex, p / texture_size) * niri_alpha;
    }
}
