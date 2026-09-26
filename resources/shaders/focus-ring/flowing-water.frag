// A liquid focus ring with circulating swells, curling crests and airborne spray.
// Umbriel decoration contract: logical pixels in, straight RGBA out.
const float WATER_SPEED = 1.0;
const float WAVE_HEIGHT = 10.0;
const float WATER_OPACITY = 0.85;

float fw_hash(float n) { return fract(sin(n * 127.1 + 311.7) * 43758.5453); }

float fw_perimeter(vec2 coords) {
    vec2 half_size = max(ring_size * 0.5, vec2(1.0));
    vec2 q = coords - half_size;
    q /= max(max(abs(q.x) / half_size.x, abs(q.y) / half_size.y), 0.0001);
    if (abs(q.y) / half_size.y >= abs(q.x) / half_size.x)
        return q.y < 0.0 ? q.x + half_size.x : ring_size.x + ring_size.y + half_size.x - q.x;
    return q.x > 0.0 ? ring_size.x + q.y + half_size.y
        : 2.0 * ring_size.x + ring_size.y + half_size.y - q.y;
}

float fw_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    return length(p - a - v * clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0));
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float d = ring_distance(coords);
    float aa = 0.65 / max(niri_scale, 0.01);
    float extent = min(ring_width + ring_padding, ring_width * 4.0 + 12.0);
    if (d <= 0.0 || d >= extent) return vec4(0.0);
    float perimeter = 2.0 * (ring_size.x + ring_size.y);
    float along = fw_perimeter(coords);
    float u = along / perimeter;
    float t = niri_time * WATER_SPEED;
    const float tau = 6.28318530718;

    // Integer spatial frequencies meet seamlessly at the perimeter's wrap.
    float fine_phase = u * max(floor(perimeter / 49.0), 1.0) * tau - t * 4.8;
    float broad_phase = u * max(floor(perimeter / 173.0), 1.0) * tau - t * 2.3;
    float base = min(ring_width * 1.20, extent * 0.28);
    float surface = base + sin(fine_phase) * 0.8 + sin(broad_phase) * 1.15;
    float swelling = 0.0;
    float foam = 0.0;
    float curls = 0.0;
    float spray = 0.0;
    float spray_glow = 0.0;

    // Several independent swells circulate at different speeds. Their crests
    // roll outward from the border and shed droplets as they pass.
    for (int wave = 0; wave < 5; wave++) {
        float index = float(wave);
        float seed = fw_hash(index + 17.0);
        float speed = 80.0 + seed * 100.0;
        float head = mod(seed * perimeter + t * speed + 11.0 * sin(t * 1.5 + seed * 20.0), perimeter);
        float delta = mod(along - head + perimeter * 0.5, perimeter) - perimeter * 0.5;
        float width = 26.0 + seed * 17.0;
        float swell = exp(-pow(delta / width, 2.0));
        float breathing = 0.78 + 0.22 * sin(t * 2.5 + seed * 18.0);
        float height = min(WAVE_HEIGHT, extent * 0.31) * breathing;
        // A broad rear slope and a steeper, breaking front.
        float profile = swell * height * (0.80 + 0.20 * sin(delta * 0.075));
        surface = max(surface, base + profile);
        swelling = max(swelling, swell);

        // A tapering curved lip curls forwards, then back towards the water.
        // A short polyline approximates the curl without derivatives/extensions.
        vec2 p = vec2(delta, d - base);
        float curl_distance = 1000.0;
        vec2 previous = vec2(-20.0, height * 0.36);
        for (int segment = 1; segment <= 7; segment++) {
            float f = float(segment) / 7.0;
            float angle = 2.75 - f * 3.85;
            float radius = mix(13.0, 4.0, f);
            vec2 next = vec2(-5.0, height * 0.60) + vec2(cos(angle) * radius, sin(angle) * radius * 0.75);
            curl_distance = min(curl_distance, fw_segment(p, previous, next));
            previous = next;
        }
        float lip = 1.0 - smoothstep(0.65, 0.65 + aa, curl_distance);
        curls = max(curls, lip * breathing);

        for (int drop = 0; drop < 4; drop++) {
            float droplet = float(drop);
            float r = fw_hash(index * 31.0 + droplet * 13.0 + 9.0);
            float flight = fract(t * (0.70 + seed * 0.28) + r);
            float visible = smoothstep(0.0, 0.12, flight) * (1.0 - smoothstep(0.72, 1.0, flight));
            float rise = sin(flight * 3.14159265359) * (9.0 + r * 10.0);
            vec2 center = vec2(-5.0 + (r - 0.65) * 44.0 * flight, base + height * 0.6 + rise);
            vec2 q = vec2(delta, d) - center;
            float radius = 0.75 + r * 0.85;
            float distance = length(q * vec2(1.0, 0.77));
            spray = max(spray, (1.0 - smoothstep(radius, radius + aa, distance)) * visible);
            spray_glow = max(spray_glow, exp(-distance * 0.65) * visible * 0.16);
        }
    }

    float inner = 0.9 + 0.30 * sin(broad_phase + 1.4);
    float body = smoothstep(inner - aa, inner + aa, d)
        * (1.0 - smoothstep(surface - aa, surface + aa, d));
    float depth = clamp((d - inner) / max(surface - inner, 1.0), 0.0, 1.0);

    // Drifting caustic ribbons shade a translucent blue body. Small, broken
    // highlights turn into white foam where the moving swells reach the edge.
    float caustic_phase = fine_phase + d * 1.05 + 1.4 * sin(broad_phase + d * 0.3);
    float caustic = pow(0.5 + 0.5 * sin(caustic_phase), 9.0);
    float flecks = pow(0.5 + 0.5 * sin(fine_phase * 2.0 + sin(broad_phase) * 2.0), 4.0);
    float edge = 1.0 - smoothstep(0.38, 0.38 + aa, abs(d - surface));
    foam = edge * (0.18 + swelling * 0.70 + flecks * 0.22);
    foam = max(foam, curls * 0.95);
    vec3 water = mix(vec3(0.015, 0.14, 0.42), vec3(0.025, 0.65, 0.82), depth);
    water += vec3(0.07, 0.31, 0.33) * caustic;
    float water_alpha = body * WATER_OPACITY * (0.64 + caustic * 0.24);
    float foam_alpha = clamp(foam + spray, 0.0, 1.0);
    float mist_alpha = spray_glow + exp(-abs(d - surface) * 0.60) * swelling * 0.045;
    vec3 whitewater = vec3(0.77, 0.97, 1.0);
    vec3 color_sum = water * water_alpha + whitewater * foam_alpha + vec3(0.10, 0.58, 0.85) * mist_alpha;
    float total = water_alpha + foam_alpha + mist_alpha;
    float envelope = smoothstep(0.0, 2.0 * aa, d)
        * (1.0 - smoothstep(max(extent - 2.0 * aa, 0.0), extent, d));
    return vec4(clamp(color_sum / max(total, 0.0001), 0.0, 1.0), clamp(total * envelope, 0.0, 1.0));
}
