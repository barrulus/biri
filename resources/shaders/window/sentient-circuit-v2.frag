// Sentient circuit v2: growing, pulsing and decaying colonies, copper and pollen.
// Inspired by living circuit trees. No aligned tile ports or repeated cross hubs.
// Premultiplied RGBA; all geometry is in logical pixels.
const float CIRCUIT_STRENGTH = 0.78;
const float CIRCUIT_SPEED = 1.0;
const float CLUSTER_SPACING = 165.0; // larger = more breathing room; keep >= 165
const float LIGHT_RESPONSE = 1.45;
const float GOLD_FRACTION = 0.22; // gold stays with a branch for its lifetime
const float PATH_DECAY = 1.35; // seconds to dissolve after the pulse's brief afterglow
const float REGENERATION_MIN = 6.0; // each patch regenerates every 6-9 seconds

float cv_hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float cv_noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(cv_hash(i), cv_hash(i + vec2(1.0, 0.0)), f.x),
               mix(cv_hash(i + vec2(0.0, 1.0)), cv_hash(i + vec2(1.0)), f.x), f.y);
}

vec2 cv_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    float t = clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0);
    return vec2(length(p - a - t * v), t * length(v));
}

vec2 cv_route(vec2 p, vec2 a, vec2 b, vec2 c) {
    vec2 first = cv_segment(p, a, b);
    vec2 second = cv_segment(p, b, c);
    second.y += length(b - a);
    return first.x < second.x ? first : second;
}

// Concentric narrow copper lanes follow each branching route. Width and lane
// count vary per branch, instead of every junction having three identical buses.
vec3 cv_copper(vec2 route, float travel, float speed, float seed, float activity, float scale) {
    float pitch = 2.8 + 1.5 * cv_hash(vec2(seed, 71.0));
    float lanes = floor(cv_hash(vec2(seed, 29.0)) * 3.0);
    float band = abs(mod(route.x + pitch * 0.5, pitch) - pitch * 0.5);
    float aa = 0.6 / scale;
    float width = 0.28 + 0.26 * cv_hash(vec2(seed, 11.0));
    float limit = 1.0 - smoothstep(lanes * pitch + width, lanes * pitch + width + aa, route.x);
    float trace = (1.0 - smoothstep(width, width + aa, band)) * limit;
    float packet = exp(-pow((route.y - travel) / (9.0 + seed * 6.0), 2.0));
    // Reveal each section just before its pulse, then dissolve behind it.
    // All channels disappear, including the unlit copper and its halo.
    float since_pulse = (travel - route.y) / speed;
    float life = smoothstep(-0.20, 0.06, since_pulse)
        * (1.0 - smoothstep(0.45, 0.45 + PATH_DECAY + seed * 0.4, since_pulse));
    float shimmer = pow(0.5 + 0.5 * sin(route.y * 0.16 - niri_time * 2.2 + seed * 70.0), 12.0);
    float power = packet * activity + shimmer * 0.16;
    float glow = exp(-max(route.x - lanes * pitch, 0.0) * 0.3) * power;
    return vec3(trace, trace * power, glow) * life;
}

vec3 cv_pad(vec2 p, float seed, float age, float activity, float scale) {
    float size = 1.3 + seed * 2.5;
    float distance = mix(length(p), max(abs(p.x), abs(p.y)), step(0.5, seed));
    float edge = 1.0 - smoothstep(0.3, 0.3 + 0.6 / scale, abs(distance - size));
    float fire = exp(-abs(age) * 8.0);
    float blink = pow(0.5 + 0.5 * sin(niri_time * (1.4 + seed * 2.0) + seed * 90.0), 18.0);
    float power = (fire + blink * 0.45) * activity;
    float dot = exp(-length(p) * 0.75) * power;
    float life = smoothstep(-0.18, 0.04, age)
        * (1.0 - smoothstep(0.45, 0.45 + PATH_DECAY, age));
    return vec3(edge, edge * power + dot, exp(-length(p) * 0.16) * power) * life;
}

float cv_light(vec2 logical) {
    vec2 uv = clamp(logical * max(niri_scale, 0.01) / max(niri_size, vec2(1.0)), 0.0, 1.0);
    return dot(tex2D_screen(uv).rgb, vec3(0.2126, 0.7152, 0.0722));
}

vec4 global_color(vec3 c) {
    vec4 source = tex2D_screen(c.xy);
    float scale = max(niri_scale, 0.01);
    vec2 p = c.xy * niri_size / scale;
    vec2 grid = floor(p / CLUSTER_SPACING);
    float clock = niri_time * CIRCUIT_SPEED;
    vec3 field = vec3(0.0);
    vec3 gold_field = vec3(0.0);

    // Evaluate overlapping neighbours so branches can cross cell boundaries.
    // The lattice is only a spatial index: roots, lengths, direction, density,
    // lane count and firing phases vary per generation. Every cell has its own
    // stable clock; reseeding happens only after that colony is fully invisible.
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 id = grid + vec2(float(x), float(y));
            float period = REGENERATION_MIN + cv_hash(id + 61.0) * 3.0;
            float patch_clock = clock + cv_hash(id + 7.1) * period;
            float generation = floor(patch_clock / period);
            float age = mod(patch_clock, period);
            float lifecycle = smoothstep(0.0, 0.25, age)
                * (1.0 - smoothstep(period - 1.0, period - 0.25, age));
            if (lifecycle <= 0.0) continue;
            // Randomize the entire geometry, including its position, each cycle.
            // Bounded hash offsets avoid increasingly large spatial coordinates.
            vec2 key = id + 191.0 * vec2(cv_hash(vec2(generation, 19.0)),
                                       cv_hash(vec2(generation, 73.0)));
            float seed = cv_hash(key + 1.7);
            float density = cv_noise(key * 0.61 + 13.0);
            if (seed < 0.07 + density * 0.13) continue;
            vec2 root = (id + 0.3 + 0.4 * vec2(cv_hash(key + 31.0), cv_hash(key - 19.0))) * CLUSTER_SPACING;
            vec2 q = p - root;
            // Support < 1.3 * spacing makes the 3x3 neighbour lookup complete.
            float support = 1.0 - smoothstep(195.0, 213.0, length(q));
            if (support <= 0.0) continue;
            float angle = floor(cv_hash(key + 8.0) * 8.0) * 0.7853981634;
            q = mat2(cos(angle), -sin(angle), sin(angle), cos(angle)) * q;
            float height = 65.0 + cv_hash(key + 47.0) * 65.0;
            float excitement = smoothstep(0.07, 0.75,
                cv_light(root) * 0.5 + cv_light(root + vec2(24.0, 17.0)) * 0.25
                + cv_light(root - vec2(19.0, 23.0)) * 0.25);
            float activity = 0.75 + LIGHT_RESPONSE * excitement;
            float speed = 65.0 + seed * 58.0;
            float pulse_age = age - 0.35;
            float travel = pulse_age * speed;
            vec2 trunk = cv_segment(q, vec2(0.0), vec2(0.0, -height));
            vec3 colony = cv_copper(trunk, travel, speed, seed, activity, scale);
            colony += cv_pad(q, seed, pulse_age, activity, scale);
            vec3 gold_colony = vec3(0.0);

            for (int branch = 0; branch < 6; branch++) {
                float b = float(branch);
                float r = cv_hash(key + vec2(b * 13.7, 83.0));
                if (r < 0.12 + density * 0.17) continue;
                // Fixed branch identity: gold follows the copper, its pads and
                // secondary twigs rather than drifting across the window.
                float is_gold = step(1.0 - GOLD_FRACTION, cv_hash(key + vec2(b * 23.1, 173.4)));
                float side = mod(b, 2.0) < 1.0 ? -1.0 : 1.0;
                float takeoff = height * (0.13 + b * 0.115 + r * 0.10);
                float spread = 12.0 + cv_hash(key + vec2(91.0, b * 5.7)) * 32.0;
                float tip = 12.0 + r * 22.0;
                vec2 a = vec2(0.0, -takeoff);
                vec2 elbow = a + vec2(side * spread, -spread);
                vec2 end = elbow + vec2(0.0, -tip);
                vec2 route = cv_route(q, a, elbow, end);
                route.y += takeoff;
                vec3 branch_copper = cv_copper(route, travel, speed, r, activity, scale);
                colony = max(colony, branch_copper);
                gold_colony = max(gold_colony, branch_copper * is_gold);
                float arrival = (takeoff + spread * 1.41421356 + tip) / speed;
                vec3 pad = cv_pad(q - end, r, pulse_age - arrival, activity, scale) * 0.8;
                colony += pad;
                gold_colony += pad * is_gold;

                // Uneven secondary twigs create fine, asymmetric terminal detail.
                if (r > 0.42) {
                    vec2 fork = elbow + vec2(side * (8.0 + r * 17.0), -8.0 - r * 17.0);
                    vec2 leaf = fork + vec2(side * (9.0 + seed * 16.0), 0.0);
                    vec2 twig = cv_route(q, elbow, fork, leaf);
                    twig.y += takeoff + spread * 1.41421356;
                    vec3 twig_copper = cv_copper(twig, travel + 7.0, speed, r * 0.31, activity * 0.8, scale);
                    colony = max(colony, twig_copper);
                    gold_colony = max(gold_colony, twig_copper * is_gold);
                    float leaf_arrival = (takeoff + spread * 1.41421356
                        + length(fork - elbow) + length(leaf - fork) - 7.0) / speed;
                    vec3 leaf_pad = cv_pad(q - leaf, seed * r, pulse_age - leaf_arrival, activity, scale) * 0.45;
                    colony += leaf_pad;
                    gold_colony += leaf_pad * is_gold;
                }
            }
            // Low-frequency variation produces quiet gaps and dense glowing areas.
            float veil = mix(0.60, 1.0, cv_noise((p + root * 0.13) / 180.0));
            field += colony * veil * support * lifecycle;
            gold_field += gold_colony * veil * support * lifecycle;
        }
    }

    // Sparse, independently twinkling pollen; small enough to leave text legible.
    vec2 dust_cell = floor((p + 29.0) / 57.0);
    float dust_seed = cv_hash(dust_cell + 93.0);
    vec2 dust_p = mod(p + 29.0, 57.0) - (0.2 + 0.6 * vec2(dust_seed, cv_hash(dust_cell + 12.0))) * 57.0;
    float twinkle = pow(0.5 + 0.5 * sin(clock * (0.8 + dust_seed * 2.0) + dust_seed * 70.0), 14.0);
    float dust = exp(-length(dust_p) * (0.6 + dust_seed)) * twinkle * step(0.63, dust_seed);
    field.y += dust * 0.65;
    field.z += dust * 0.2;

    vec3 original = source.rgb / max(source.a, 0.0001);
    float lum = dot(original, vec3(0.2126, 0.7152, 0.0722));
    float protect = 1.0 - 0.7 * smoothstep(0.4, 0.95, lum);
    float hue = cv_noise(p / 210.0 + 41.0);
    vec3 green = mix(vec3(0.07, 0.91, 0.28), vec3(0.66, 1.0, 0.14), hue);
    vec3 hot = vec3(0.85, 1.0, 0.68);
    vec3 gold = vec3(1.0, 0.57, 0.08);
    vec3 gold_hot = vec3(1.0, 0.86, 0.43);
    // Separate coverage, pulse and halo weights keep gold emissions on their
    // selected branches, including where green and gold routes cross.
    vec3 gold_weight = clamp(gold_field / max(field, vec3(0.0001)), 0.0, 1.0);
    vec3 copper_color = mix(green * 0.38, gold * 0.50, gold_weight.x);
    vec3 pulse_color = mix(mix(green, hot, clamp(field.y, 0.0, 1.0)),
                          mix(gold, gold_hot, clamp(field.y, 0.0, 1.0)), gold_weight.y);
    vec3 halo_color = mix(green, gold, gold_weight.z);
    vec3 result = mix(original, copper_color, min(field.x, 1.0) * 0.26 * CIRCUIT_STRENGTH);
    result += (halo_color * field.z * 0.13 + pulse_color * field.y * 0.72)
        * protect * CIRCUIT_STRENGTH;
    return vec4(clamp(result, 0.0, 1.0) * source.a, source.a);
}
