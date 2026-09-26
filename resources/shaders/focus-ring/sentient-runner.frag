// Green-and-gold circuit runner. Umbriel decoration contract: straight RGBA.
// Three angular buses with shared junctions, branching links, and independent
// counter-running packets that accelerate, hesitate and reverse locally.
const float RUNNER_SPEED = 1.0;
const float RUNNER_STRENGTH = 1.0;
const float JUNCTION_SPACING = 76.0;

float cr_hash(float n) {
    return fract(sin(n * 127.1 + 311.7) * 43758.5453);
}

// A periodic coordinate around the client, following its rounded distance field.
float cr_perimeter(vec2 coords) {
    vec2 half_size = max(ring_size * 0.5, vec2(1.0));
    vec2 q = coords - half_size;
    q /= max(max(abs(q.x) / half_size.x, abs(q.y) / half_size.y), 0.0001);
    if (abs(q.y) / half_size.y >= abs(q.x) / half_size.x)
        return q.y < 0.0 ? q.x + half_size.x : ring_size.x + ring_size.y + half_size.x - q.x;
    return q.x > 0.0 ? ring_size.x + q.y + half_size.y
        : 2.0 * ring_size.x + ring_size.y + half_size.y - q.y;
}

float cr_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    return length(p - a - v * clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0));
}

float cr_height(float station, float lane, float cells, float band) {
    float seed = cr_hash(mod(station, cells) + lane * 137.0);
    return band * (0.17 + lane * 0.29 + (seed - 0.5) * 0.17);
}

float cr_energy(float along, float lane, float perimeter) {
    float energy = 0.0;
    float t = niri_time * RUNNER_SPEED;
    for (int packet = 0; packet < 2; packet++) {
        float seed = cr_hash(lane * 29.0 + float(packet) * 43.0 + 5.0);
        float direction = packet == 0 ? 1.0 : -1.0;
        float speed = 92.0 + seed * 96.0;
        float phase = seed * 30.0;
        // Positive average progress with local reversals and unequal pauses.
        float journey = t + sin(t * 1.6 + phase) + 0.24 * sin(t * 3.7 + phase * 1.7);
        float head = mod(seed * perimeter + direction * speed * journey, perimeter);
        float delta = mod(along - head + perimeter * 0.5, perimeter) - perimeter * 0.5;
        float tip = exp(-pow(delta / (19.0 + seed * 13.0), 2.0));
        float wake = exp(-abs(delta) / (55.0 + seed * 38.0));
        energy += tip * 1.7 + wake * 0.52;
    }
    return energy;
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float d = ring_distance(coords);
    float aa = 0.6 / max(niri_scale, 0.01);
    float extent = min(ring_width + ring_padding, ring_width * 2.5 + 5.0);
    if (d <= 0.0 || d >= extent) return vec4(0.0);
    float band = max(extent - 3.0 * aa, aa);
    float perimeter = 2.0 * (ring_size.x + ring_size.y);
    float along = cr_perimeter(coords);
    float cells = max(floor(perimeter / JUNCTION_SPACING), 4.0);
    float spacing = perimeter / cells;
    float station = floor(along / spacing);
    float local = mod(along, spacing);
    vec2 p = vec2(local, d);
    vec3 light_sum = vec3(0.0);
    float coverage_sum = 0.0;

    for (int bus = 0; bus < 3; bus++) {
        float lane = float(bus);
        float seed = cr_hash(mod(station, cells) + lane * 137.0 + 61.0);
        float left = cr_height(station, lane, cells, band);
        float right = cr_height(station + 1.0, lane, cells, band);
        float bend = spacing * (0.26 + seed * 0.42);
        float run = min(abs(right - left), spacing * 0.18);
        vec2 a = vec2(0.0, left);
        vec2 b = vec2(bend - run * 0.5, left);
        vec2 c = vec2(bend + run * 0.5, right);
        vec2 e = vec2(spacing, right);
        float track = min(cr_segment(p, a, b), min(cr_segment(p, b, c), cr_segment(p, c, e)));
        float energy = cr_energy(along, lane, perimeter);
        // Charge fattens the copper locally, making passing pulses stand out.
        float core_width = 0.45 + clamp(energy, 0.0, 1.0) * 0.40;
        float core = 1.0 - smoothstep(core_width, core_width + aa, track);
        float halo = exp(-track * 0.58);

        // Small plated pads are irregularly spaced along the copper itself.
        vec2 node = vec2(spacing * 0.12, left);
        float pad_distance = abs(length(p - node) - 1.35);
        float pad = (1.0 - smoothstep(0.25, 0.25 + aa, pad_distance)) * step(0.40, seed);
        core = max(core, pad);

        // Selected stations link adjoining buses. Passing packets light both
        // ends of a junction, making the charge appear to split or change lanes.
        float fork_core = 0.0;
        float fork_halo = 0.0;
        if (bus < 2 && seed > 0.42) {
            float other = cr_height(station + 1.0, lane + 1.0, cells, band);
            vec2 start = vec2(spacing * 0.70, right);
            vec2 elbow = vec2(spacing * 0.70 + abs(other - right), other);
            vec2 end = vec2(spacing, other);
            float fork = min(cr_segment(p, start, elbow), cr_segment(p, elbow, end));
            float split = max(energy, cr_energy(along, lane + 1.0, perimeter));
            fork_core = (1.0 - smoothstep(0.40, 0.40 + aa, fork)) * (0.30 + split * 0.95);
            fork_halo = exp(-fork * 0.65) * split * 0.16;
        }

        // The central bus is gold; the outer bus has occasional gold branches.
        float gold = bus == 1 ? 1.0 : (bus == 2 ? step(0.79, seed) : 0.0);
        vec3 copper = mix(vec3(0.10, 1.0, 0.32), vec3(1.0, 0.66, 0.10), gold);
        vec3 hot = mix(vec3(0.88, 1.0, 0.82), vec3(1.0, 0.94, 0.67), gold);
        float core_light = core * (0.43 + energy * 1.05) + fork_core;
        float glow_light = halo * (0.025 + energy * 0.25) + fork_halo;
        float coverage = core_light + glow_light;
        light_sum += mix(copper, hot, clamp(energy * 0.8, 0.0, 1.0)) * core_light + copper * glow_light;
        coverage_sum += coverage;
    }

    float envelope = smoothstep(0.0, 2.0 * aa, d)
        * (1.0 - smoothstep(max(extent - 2.0 * aa, 0.0), extent, d));
    vec3 rgb = light_sum / max(coverage_sum, 0.0001);
    float alpha = clamp(coverage_sum * RUNNER_STRENGTH * envelope, 0.0, 1.0);
    return vec4(clamp(rgb, 0.0, 1.0), alpha);
}
