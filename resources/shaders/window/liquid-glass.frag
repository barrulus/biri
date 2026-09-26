// Shader-only frosted glass: diffuse the captured composition, then add tint,
// a refractive bevel and soft reflection. No compositor backdrop blur needed.
// Some original detail is retained for lettering, but diffusion affects both
// the window and its visible background; hidden desktop pixels are unavailable.
const float GLASS_BEVEL = 24.0;       // logical pixels
const float GLASS_REFRACTION = 8.0;   // logical pixels
const float GLASS_RADIUS = 4.0;       // client radius: outer radius 10 minus border 6
const float GLASS_REFLECTION = 0.32;
const float GLASS_TINT = 0.14;
const float GLASS_FROST_RADIUS = 3.0; // logical pixels
const float GLASS_FROST = 0.55;       // 0 = clear, 1 = fully diffused

vec4 glass_sample(vec2 p, vec2 size, float scale) {
    vec2 inset = vec2(0.5 / scale);
    return tex2D_screen(clamp(p, inset, max(inset, size - inset)) / size);
}

vec4 global_color(vec3 coords) {
    vec4 source = tex2D_screen(coords.xy);
    if (source.a <= 0.0) return source;
    float scale = max(niri_scale, 0.01);
    vec2 size = max(niri_size / scale, vec2(1.0 / scale));
    vec2 p = coords.xy * size;
    vec2 halfSize = size * 0.5;
    vec2 local = p - halfSize;
    float radius = min(GLASS_RADIUS, min(halfSize.x, halfSize.y));
    vec2 q = abs(local) - halfSize + radius;
    vec2 corner = max(q, 0.0);
    float distance = length(corner) + min(max(q.x, q.y), 0.0) - radius;
    vec2 normal = length(corner) > 0.001 ? normalize(corner)
        : (q.x > q.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
    normal *= sign(local);
    float bevel = min(GLASS_BEVEL, min(halfSize.x, halfSize.y) * 0.7);
    float edge = 1.0 - smoothstep(0.0, max(bevel, 0.001), max(-distance, 0.0));
    float lens = sin(clamp(-distance / max(bevel, 0.001), 0.0, 1.0) * 3.14159265);
    float drift = sin(niri_time * 0.72 + dot(p / size, vec2(3.0, 2.0)));
    vec2 offset = normal * GLASS_REFRACTION * lens * (0.92 + 0.08 * drift);
    vec2 samplePoint = p - offset;

    // Diffuse the whole pane, with slightly softer bevels. Mix in the original
    // sample so the frost still carries detail rather than erasing lettering.
    float blur = GLASS_FROST_RADIUS + 1.25 * edge;
    vec4 clear = glass_sample(samplePoint, size, scale);
    vec4 frosted = clear * 0.25;
    frosted += glass_sample(samplePoint + vec2(blur, 0.0), size, scale) * 0.125;
    frosted += glass_sample(samplePoint - vec2(blur, 0.0), size, scale) * 0.125;
    frosted += glass_sample(samplePoint + vec2(0.0, blur), size, scale) * 0.125;
    frosted += glass_sample(samplePoint - vec2(0.0, blur), size, scale) * 0.125;
    frosted += glass_sample(samplePoint + vec2(blur, blur), size, scale) * 0.0625;
    frosted += glass_sample(samplePoint + vec2(blur, -blur), size, scale) * 0.0625;
    frosted += glass_sample(samplePoint + vec2(-blur, blur), size, scale) * 0.0625;
    frosted += glass_sample(samplePoint - vec2(blur, blur), size, scale) * 0.0625;
    vec4 refracted = mix(clear, frosted, GLASS_FROST);
    vec4 red = glass_sample(samplePoint - normal * lens * 0.65, size, scale);
    vec4 blue = glass_sample(samplePoint + normal * lens * 0.65, size, scale);
    vec3 colour = refracted.rgb / max(refracted.a, 0.0001);
    colour.r = mix(colour.r, red.r / max(red.a, 0.0001), edge * 0.65);
    colour.b = mix(colour.b, blue.b / max(blue.a, 0.0001), edge * 0.65);

    float light = dot(normal, normalize(vec2(-0.65, -0.76)));
    float lip = exp(-abs(distance + 1.2) * scale * 0.85);
    float innerLip = exp(-abs(distance + bevel * 0.7) / 1.8) * 0.12;
    float gleam = pow(abs(light), 5.0) * (0.92 + 0.08 * drift);
    vec3 reflection = mix(vec3(0.55, 0.85, 1.0), vec3(1.0, 0.72, 0.90),
        0.5 + 0.5 * sin(niri_time * 0.35 + (p.x + p.y) / 150.0));
    colour = mix(colour, vec3(0.80, 0.91, 1.0), GLASS_TINT);
    colour *= 1.0 - edge * 0.07 * max(-light, 0.0);
    colour = mix(colour, reflection, clamp((lip * (0.25 + 0.75 * gleam) + innerLip)
        * GLASS_REFLECTION, 0.0, 1.0));
    return vec4(colour * source.a, source.a);
}
