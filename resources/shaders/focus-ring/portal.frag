// Violet/cyan portal: braided plasma, circulating swells and curling energy spray.
// Umbriel decoration contract: logical pixels in, straight RGBA out.
const float PORTAL_SPEED = 0.85;
const float WAVE_HEIGHT = 12.0;
const float PORTAL_BRIGHTNESS = 1.10;

float pw_hash(float n) { return fract(sin(n * 127.1 + 311.7) * 43758.5453); }

float pw_perimeter(vec2 coords) {
    vec2 half_size = max(ring_size * 0.5, vec2(1.0));
    vec2 q = coords - half_size;
    q /= max(max(abs(q.x) / half_size.x, abs(q.y) / half_size.y), 0.0001);
    if (abs(q.y) / half_size.y >= abs(q.x) / half_size.x)
        return q.y < 0.0 ? q.x + half_size.x : ring_size.x + ring_size.y + half_size.x - q.x;
    return q.x > 0.0 ? ring_size.x + q.y + half_size.y
        : 2.0 * ring_size.x + ring_size.y + half_size.y - q.y;
}

float pw_segment(vec2 p, vec2 a, vec2 b) {
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
    float along = pw_perimeter(coords);
    float u = along / perimeter;
    float t = niri_time * PORTAL_SPEED;
    const float tau = 6.28318530718;

    // Integer spatial frequencies meet seamlessly at the perimeter's wrap.
    float fine_phase = u * max(floor(perimeter / 49.0), 1.0) * tau - t * 4.8;
    float broad_phase = u * max(floor(perimeter / 173.0), 1.0) * tau - t * 2.3;
    float base = min(ring_width * 1.20, extent * 0.28);
    float surface = base + sin(fine_phase) * 0.8 + sin(broad_phase) * 1.15;
    float swelling = 0.0;
    float curl_glow = 0.0;
    float curls = 0.0;
    float spray = 0.0;
    float spray_glow = 0.0;

    // Independent surges circulate around the portal. Their crests deform the
    // braided core, roll outward and release bright motes into the violet aura.
    for (int wave = 0; wave < 5; wave++) {
        float index = float(wave);
        float seed = pw_hash(index + 17.0);
        float speed = 95.0 + seed * 110.0;
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
            curl_distance = min(curl_distance, pw_segment(p, previous, next));
            previous = next;
        }
        float lip = 1.0 - smoothstep(0.65, 0.65 + aa, curl_distance);
        curls = max(curls, lip * breathing);
        curl_glow = max(curl_glow, exp(-curl_distance * 0.50) * breathing * 0.38);

        for (int drop = 0; drop < 4; drop++) {
            float droplet = float(drop);
            float r = pw_hash(index * 31.0 + droplet * 13.0 + 9.0);
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

    // Retain the original portal's two braided plasma strands and violet aura.
    // Passing swells expand the braid as well as brightening it, giving the
    // energy a rolling contour rather than a spot on an otherwise static ring.
    float center = surface * 0.60 + 0.45 * sin(broad_phase + 1.4);
    float braid_width = 1.0 + swelling * 1.65;
    float helix = sin(u * max(floor(perimeter / 97.0), 1.0) * tau - t * 4.2 + d * 0.22);
    float helix2 = sin(u * max(floor(perimeter / 71.0), 1.0) * tau + t * 3.0 - d * 0.27);
    float distance1 = abs(d - center - braid_width * helix);
    float distance2 = abs(d - center - braid_width * helix2);
    float strand1 = 1.0 - smoothstep(0.42, 0.42 + aa, distance1);
    float strand2 = 1.0 - smoothstep(0.38, 0.38 + aa, distance2);
    float halo1 = exp(-distance1 * 0.8);
    float halo2 = exp(-distance2 * 0.8);
    float aura = exp(-abs(d - center) * 0.32);
    float wave_edge = 1.0 - smoothstep(0.44, 0.44 + aa, abs(d - surface));
    float crest = wave_edge * swelling;
    float pulse = 0.72 + swelling * 0.65;

    // Cyan and violet remain distinct through the braid; only the brightest
    // crest tips and airborne particles approach an icy, lavender white.
    vec3 cyan = vec3(0.12, 0.85, 1.0);
    vec3 violet = vec3(0.78, 0.40, 1.0);
    vec3 deep_violet = vec3(0.32, 0.055, 0.72);
    vec3 white = vec3(0.82, 0.93, 1.0);
    float cyan_alpha = strand1 * pulse + halo1 * 0.10;
    float violet_alpha = strand2 * pulse + halo2 * 0.12;
    float crest_alpha = crest * 0.70 + curls * 0.90;
    float spray_alpha = spray * 0.95;
    float aura_alpha = aura * (0.26 + swelling * 0.22) + curl_glow + spray_glow;
    vec3 color_sum = cyan * cyan_alpha + violet * violet_alpha
        + mix(cyan, white, swelling * 0.7) * crest_alpha
        + white * spray_alpha + deep_violet * aura_alpha;
    float total = cyan_alpha + violet_alpha + crest_alpha + spray_alpha + aura_alpha;
    float envelope = smoothstep(0.0, 2.0 * aa, d)
        * (1.0 - smoothstep(max(extent - 2.0 * aa, 0.0), extent, d));
    vec3 rgb = clamp(color_sum / max(total, 0.0001) * PORTAL_BRIGHTNESS, 0.0, 1.0);
    return vec4(rgb, clamp(total * envelope, 0.0, 1.0));
}
