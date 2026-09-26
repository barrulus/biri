// Neutral white crumpled stationery. Static, scale-independent fibres and folds.
// Content keeps its alpha; bright and dark applications both retain contrast.
const float PAPER_RELIEF = 0.24;
const float PAPER_DESATURATION = 0.85;

float paper_grain(vec2 p) {
    p = fract(p * vec2(123.34, 345.45));
    p += dot(p, p + 34.345);
    return fract(p.x * p.y);
}

float paper_relief(vec2 p) {
    float relief = 0.0;
    // Intersecting folded planes, with a narrow highlight beside each crease.
    for (int i = 0; i < 5; i++) {
        float n = float(i);
        vec2 axis = vec2(cos(n * 2.399 + 0.3), sin(n * 2.399 + 0.3));
        float phase = dot(p, axis) / (39.0 + n * 13.0)
            + 0.22 * sin(dot(p, vec2(-axis.y, axis.x)) / 67.0 + n * 4.1);
        float fold = fract(phase + n * 0.173);
        float valley = min(fold, 1.0 - fold);
        relief += (abs(fold * 2.0 - 1.0) - 0.5) * 0.26;
        relief -= exp(-valley * 100.0) * 0.33;
        relief += exp(-abs(fold - 0.032) * 90.0) * 0.23;
    }
    return relief;
}

vec4 global_color(vec3 coords) {
    vec4 source = tex2D_screen(coords.xy);
    if (source.a <= 0.0) return source;
    vec2 p = coords.xy * niri_size / max(niri_scale, 0.01);
    vec3 color = source.rgb / source.a;
    float luminance = dot(color, vec3(0.299, 0.587, 0.114));
    color = mix(color, vec3(luminance), PAPER_DESATURATION);
    // Lift black backgrounds just enough to show the paper's relief, while
    // keeping their light text light. Neutral endpoints and crease lighting
    // keep the sheet white instead of tinting it pink or cream.
    color = mix(vec3(0.095), vec3(0.985), color);
    float fibre = (paper_grain(floor(p * 1.6)) - 0.5) * 0.018;
    float relief = paper_relief(p) * PAPER_RELIEF;
    color = color * (1.0 + relief) + vec3(relief * 0.12 + fibre);
    return vec4(clamp(color, 0.0, 1.0) * source.a, source.a);
}
