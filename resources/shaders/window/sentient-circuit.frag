// Sentient circuit: etched buses, independent junctions, light-reactive pulses.
// Umbriel postprocess contract: return premultiplied RGBA.
// Logical-pixel geometry keeps traces consistent across output scales.
const float CIRCUIT_SPACING = 144.0;
const float CIRCUIT_STRENGTH = 0.72;
const float CIRCUIT_SPEED = 1.0;
const float LIGHT_RESPONSE = 1.4;

float sc_hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Distance and position along a segment, for pulses that follow the copper.
vec2 sc_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    float t = clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0);
    return vec2(length(p - a - t * v), t * length(v));
}

float sc_luminance(vec2 px) {
    vec2 uv = clamp(px / max(niri_size, vec2(1.0)), 0.0, 1.0);
    vec4 s = tex2D_screen(uv);
    return dot(s.rgb, vec3(0.2126, 0.7152, 0.0722));
}

vec4 global_color(vec3 c) {
    vec4 source = tex2D_screen(c.xy);
    float scale = max(niri_scale, 0.01);
    vec2 p = c.xy * niri_size / scale;
    vec2 cell = floor(p / CIRCUIT_SPACING);
    vec2 local = p - cell * CIRCUIT_SPACING;
    float seed = sc_hash(cell);
    vec2 hub = CIRCUIT_SPACING * (0.36 + 0.28 * vec2(seed, sc_hash(cell + 17.3)));
    vec2 hub_px = (cell * CIRCUIT_SPACING + hub) * scale;

    // Five samples around each junction: bright text/images excite nearby buses.
    // Sampling the input, rather than the effect, avoids self-amplifying feedback.
    float light = sc_luminance(hub_px) * 0.4;
    light += sc_luminance(hub_px + vec2(18.0, 0.0) * scale) * 0.15;
    light += sc_luminance(hub_px - vec2(18.0, 0.0) * scale) * 0.15;
    light += sc_luminance(hub_px + vec2(0.0, 18.0) * scale) * 0.15;
    light += sc_luminance(hub_px - vec2(0.0, 18.0) * scale) * 0.15;
    float excitement = smoothstep(0.06, 0.72, light);
    float clock = niri_time * CIRCUIT_SPEED;
    float cycle = 2.6 + seed * 2.1;
    float age = mod(clock + seed * 19.0, cycle);
    float radius = age * 100.0;
    float aa = 0.65 / scale;
    float traces = 0.0;
    float energy = 0.0;
    float bloom = 0.0;

    // Four matched edge ports, each with three parallel traces and a 45-degree
    // bend. Neighbouring cells meet at the same ports without texture assets.
    for (int side = 0; side < 4; side++) {
        vec2 q = local - hub;
        vec2 end;
        if (side == 0) end = vec2(CIRCUIT_SPACING, CIRCUIT_SPACING * 0.5) - hub;
        else if (side == 1) { q = q.yx; end = (vec2(CIRCUIT_SPACING * 0.5, CIRCUIT_SPACING) - hub).yx; }
        else if (side == 2) { q.x = -q.x; end = vec2(hub.x, CIRCUIT_SPACING * 0.5 - hub.y); }
        else { q = vec2(-q.y, q.x); end = vec2(hub.y, CIRCUIT_SPACING * 0.5 - hub.x); }
        for (int lane = 0; lane < 3; lane++) {
            float offset = (float(lane) - 1.0) * 6.0;
            vec2 a = vec2(8.0 + abs(offset), offset);
            vec2 b = vec2(end.x - abs(end.y) - 12.0, offset);
            vec2 d = vec2(end.x - 12.0, end.y + offset);
            vec2 e = vec2(end.x, end.y + offset);
            vec2 nearest = sc_segment(q, a, b);
            vec2 middle = sc_segment(q, b, d);
            middle.y += length(b - a);
            if (middle.x < nearest.x) nearest = middle;
            vec2 last = sc_segment(q, d, e);
            last.y += length(b - a) + length(d - b);
            if (last.x < nearest.x) nearest = last;
            float trace = 1.0 - smoothstep(0.48, 0.48 + aa, nearest.x);
            float pulse = exp(-pow((nearest.y + 9.0 - radius + float(lane) * 5.0) / 13.0, 2.0));
            float shimmer = pow(0.5 + 0.5 * sin(nearest.y * 0.11 - clock * 3.0 + seed * 40.0), 8.0);
            float power = (pulse * (0.65 + LIGHT_RESPONSE * excitement) + shimmer * 0.13);
            traces = max(traces, trace);
            energy = max(energy, trace * power);
            bloom = max(bloom, exp(-nearest.x * 0.48) * power);
        }
    }

    float node_distance = length(local - hub);
    float node = 1.0 - smoothstep(0.55, 0.55 + aa, abs(node_distance - 3.5));
    float firing = exp(-age * 6.0) + 0.18 * pow(0.5 + 0.5 * sin(clock * 3.0 + seed * 50.0), 8.0);
    energy += node * (0.22 + firing * (1.0 + excitement));
    bloom += exp(-node_distance * 0.19) * firing * 0.55;
    traces = max(traces, node);

    vec3 original = source.rgb / max(source.a, 0.0001);
    float lum = dot(original, vec3(0.2126, 0.7152, 0.0722));
    // Keep glyphs readable while retaining green copper on bright surfaces.
    float protect = 1.0 - 0.65 * smoothstep(0.45, 0.95, lum);
    vec3 copper = mix(vec3(0.025, 0.23, 0.12), vec3(0.05, 0.62, 0.31), 0.5 + 0.5 * sin(seed * 30.0));
    vec3 result = mix(original, copper, traces * 0.18 * CIRCUIT_STRENGTH);
    vec3 mint = vec3(0.12, 1.0, 0.48);
    vec3 hot = vec3(0.72, 1.0, 0.86);
    result += (mint * bloom * 0.19 + mix(mint, hot, clamp(energy, 0.0, 1.0)) * energy * 0.7)
        * CIRCUIT_STRENGTH * protect;
    return vec4(clamp(result, 0.0, 1.0) * source.a, source.a);
}
