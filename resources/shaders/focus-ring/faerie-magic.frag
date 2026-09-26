// Enchanted focus ring: iridescent threads, wand-light and popping fairy dust.
// Shared with the matching inner-content overlay; straight RGBA output.
const float MAGIC_SPEED = 1.0;
const float MAGIC_OUTSET = 12.0;
const float MAGIC_INSET = 26.0;
const float SPARK_SPACING = 53.0;

float fae_hash(float p) { return fract(sin(p * 127.1 + 311.7) * 43758.5453); }

float fae_offset() {
    return min(ring_width * 0.55, min(ring_width + ring_padding, MAGIC_OUTSET) * 0.40);
}

vec4 fae_radii() {
    return clamp(ring_radius, 0.0, min(ring_size.x, ring_size.y) * 0.5) + fae_offset();
}

float fae_length() {
    vec2 size = ring_size + 2.0 * fae_offset();
    return 2.0 * (size.x + size.y) - (2.0 - 1.57079632679) * dot(fae_radii(), vec4(1.0));
}

// Rounded arclength keeps the moving sparkles evenly spaced through corners.
float fae_perimeter(vec2 coords) {
    vec2 p = coords + fae_offset();
    vec2 size = ring_size + 2.0 * fae_offset();
    vec4 r = fae_radii(); // TL, TR, BR, BL
    const float quarter = 1.57079632679;
    float top = size.x - r.x - r.y;
    float right_start = top + quarter * r.y;
    float br_start = right_start + size.y - r.y - r.z;
    float bottom_start = br_start + quarter * r.z;
    float bl_start = bottom_start + size.x - r.z - r.w;
    float left_start = bl_start + quarter * r.w;
    float tl_start = left_start + size.y - r.w - r.x;
    if (p.x >= size.x - r.y && p.y <= r.y)
        return top + r.y * atan(max(p.x - size.x + r.y, 0.00001), max(r.y - p.y, 0.00001));
    if (p.x >= size.x - r.z && p.y >= size.y - r.z)
        return br_start + r.z * atan(max(p.y - size.y + r.z, 0.00001), max(p.x - size.x + r.z, 0.00001));
    if (p.x <= r.w && p.y >= size.y - r.w)
        return bl_start + r.w * atan(max(r.w - p.x, 0.00001), max(p.y - size.y + r.w, 0.00001));
    if (p.x <= r.x && p.y <= r.x)
        return tl_start + r.x * atan(max(r.x - p.y, 0.00001), max(r.x - p.x, 0.00001));
    vec4 distances = abs(vec4(p.y, p.x - size.x, p.y - size.y, p.x));
    float nearest = min(min(distances.x, distances.y), min(distances.z, distances.w));
    if (nearest == distances.x) return clamp(p.x - r.x, 0.0, top);
    if (nearest == distances.y) return right_start + clamp(p.y - r.y, 0.0, size.y - r.y - r.z);
    if (nearest == distances.z) return bottom_start + clamp(size.x - r.z - p.x, 0.0, size.x - r.z - r.w);
    return left_start + clamp(size.y - r.w - p.y, 0.0, size.y - r.w - r.x);
}

// A rigid pixel frame keeps starbursts and circular pops in proportion.
vec4 fae_frame(float along) {
    float s = mod(along, fae_length());
    float offset = fae_offset();
    vec2 size = ring_size + 2.0 * offset;
    vec4 r = fae_radii();
    const float quarter = 1.57079632679;
    for (int side = 0; side < 4; side++) {
        float line;
        float radius;
        vec2 start;
        vec2 tangent;
        vec2 center;
        if (side == 0) {
            line = size.x - r.x - r.y; radius = r.y;
            start = vec2(r.x, 0.0); tangent = vec2(1.0, 0.0); center = vec2(size.x - r.y, r.y);
        } else if (side == 1) {
            line = size.y - r.y - r.z; radius = r.z;
            start = vec2(size.x, r.y); tangent = vec2(0.0, 1.0); center = vec2(size.x - r.z, size.y - r.z);
        } else if (side == 2) {
            line = size.x - r.z - r.w; radius = r.w;
            start = vec2(size.x - r.z, size.y); tangent = vec2(-1.0, 0.0); center = vec2(r.w, size.y - r.w);
        } else {
            line = size.y - r.w - r.x; radius = r.x;
            start = vec2(0.0, size.y - r.w); tangent = vec2(0.0, -1.0); center = vec2(r.x, r.x);
        }
        if (s <= line) return vec4(start + tangent * s - offset, tangent);
        s -= line;
        if (s <= quarter * radius || side == 3) {
            float angle = (float(side) - 1.0) * quarter + clamp(s / max(radius, 0.0001), 0.0, quarter);
            return vec4(center + radius * vec2(cos(angle), sin(angle)) - offset, -sin(angle), cos(angle));
        }
        s -= quarter * radius;
    }
    return vec4(r.x - offset, -offset, 1.0, 0.0);
}

vec4 fae_over(vec4 under, vec3 color, float alpha) {
    alpha = clamp(alpha, 0.0, 1.0);
    return vec4(color * alpha + under.rgb * (1.0 - alpha), alpha + under.a * (1.0 - alpha));
}

vec3 fae_palette(float seed) {
    vec3 tint = mix(vec3(0.59, 0.32, 1.0), vec3(0.22, 0.97, 0.79), step(0.34, seed));
    return mix(tint, vec3(1.0, 0.73, 0.25), step(0.72, seed));
}

float fae_charge(float along, float perimeter, float t) {
    float charge = 0.0;
    for (int wand = 0; wand < 4; wand++) {
        float seed = fae_hash(float(wand) + 47.0);
        float direction = wand == 3 ? -1.0 : 1.0;
        float head = mod(seed * perimeter + direction * t * (95.0 + seed * 83.0)
            + 18.0 * sin(t * 1.3 + seed * 21.0), perimeter);
        float delta = mod(along - head + perimeter * 0.5, perimeter) - perimeter * 0.5;
        charge += exp(-pow(delta / 27.0, 2.0)) + 0.35 * exp(-abs(delta) / 95.0);
    }
    return clamp(charge, 0.0, 1.5);
}

float fae_star(vec2 p, float size, float aa) {
    // Four long points, with a crisp diamond at the heart of each sparkle.
    float diamond = (abs(p.x) + abs(p.y)) / max(size * 0.38, 0.01);
    float core = 1.0 - smoothstep(0.6, 1.0 + aa / max(size, 0.1), diamond);
    float rays = exp(-abs(p.x) * 4.5 - abs(p.y) / max(size * 0.48, 0.1))
        + exp(-abs(p.y) * 4.5 - abs(p.x) / max(size * 0.48, 0.1));
    return clamp(core + rays, 0.0, 1.0);
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float d = ring_distance(coords);
    float aa = 0.60 / max(niri_scale, 0.01);
    float extent = min(ring_width + ring_padding, MAGIC_OUTSET);
    if (d <= -MAGIC_INSET || d >= extent) return vec4(0.0);
    float t = niri_time * MAGIC_SPEED;
    float perimeter = fae_length();
    float along = fae_perimeter(coords);
    float u = along / perimeter;
    const float tau = 6.28318530718;
    float turns = max(floor(perimeter / 118.0), 1.0);
    float charge = fae_charge(along, perimeter, t);
    vec4 paint = vec4(0.0);

    // Fine elven threads braid together and brighten under passing wand-light.
    for (int thread = 0; thread < 2; thread++) {
        float n = float(thread);
        float phase = u * turns * tau - t * 1.8 + n * 3.14159265;
        float center = fae_offset() + 1.0 * sin(phase)
            + 0.35 * sin(u * (turns + 7.0) * tau + t * 0.8);
        float distance = abs(d - center);
        float core = 1.0 - smoothstep(0.32 + charge * 0.22, 0.32 + charge * 0.22 + aa, distance);
        vec3 color = mix(vec3(0.55, 0.30, 0.95), vec3(0.18, 0.86, 0.76), n);
        color = mix(color, vec3(1.0, 0.91, 0.66), clamp(charge * 0.7, 0.0, 1.0));
        paint = fae_over(paint, color, exp(-distance * 0.5) * (0.045 + charge * 0.13));
        paint = fae_over(paint, color, core * (0.48 + charge * 0.5));
    }

    float cells = max(floor(perimeter / SPARK_SPACING), 4.0);
    float spacing = perimeter / cells;
    float drift = mod(t * 32.0 + 6.0 * sin(t * 0.6), perimeter);
    float cell = floor((along - drift) / spacing);
    for (int neighbour = -1; neighbour <= 1; neighbour++) {
        float index = cell + float(neighbour);
        float id = mod(index, cells);
        float seed = fae_hash(id + 17.0);
        float period = 1.25 + seed * 2.0;
        float clock = t + seed * period;
        float generation = floor(clock / period);
        float life = mod(clock, period) / period;
        float burst = smoothstep(0.0, 0.055, life) * (1.0 - smoothstep(0.16, 0.68, life));
        if (burst <= 0.0) continue;
        float root_along = (index + 0.23 + seed * 0.54) * spacing + drift;
        vec4 frame = fae_frame(root_along);
        vec2 relative = coords - frame.xy;
        vec2 p = vec2(dot(relative, frame.zw), dot(relative, vec2(frame.w, -frame.z)) + fae_offset());
        if (abs(p.x) > 34.0) continue;
        float event_seed = fae_hash(id * 29.0 + mod(generation, 4096.0) * 7.0);
        vec3 color = fae_palette(event_seed);
        float energy = 1.30 + fae_charge(root_along, perimeter, t) * 1.10;

        // A quick bubble-pop opens into a four-point star, then scatters motes.
        // Most of the burst leans into the window, leaving the outer edge snug.
        vec2 origin = vec2(0.0, fae_offset() - 1.0 - event_seed * 5.0);
        vec2 q = p - origin;
        float angle = seed * tau + 0.16 * sin(t + seed * 12.0);
        vec2 star_p = mat2(cos(angle), -sin(angle), sin(angle), cos(angle)) * q;
        float size = (5.0 + seed * 4.0) * (0.7 + 0.3 * sin(life * 3.14159265));
        float star = fae_star(star_p, size, aa) * burst;
        float halo = exp(-length(q) * 0.35) * burst;
        paint = fae_over(paint, color, halo * 0.40 * energy);
        paint = fae_over(paint, mix(color, vec3(1.0, 0.99, 0.95), 0.90), star * energy);
        float pop_radius = 0.5 + life * 17.0;
        float pop = (1.0 - smoothstep(0.25, 0.25 + aa, abs(length(q) - pop_radius)))
            * (1.0 - smoothstep(0.12, 0.32, life)) * smoothstep(0.0, 0.04, life);
        paint = fae_over(paint, mix(color, vec3(1.0), 0.65), pop * 0.95 * energy);

        for (int mote = 0; mote < 5; mote++) {
            float n = float(mote);
            float r = fae_hash(id * 13.0 + n * 19.0 + event_seed * 43.0);
            float flight = clamp(life / 0.70, 0.0, 1.0);
            vec2 velocity = vec2((r - 0.5) * 35.0, -(5.0 + fae_hash(n + seed * 39.0) * 15.0));
            if (r > 0.80) velocity.y = 3.0;
            vec2 center = origin + velocity * flight
                + vec2(sin(flight * 5.0 + r * tau) - sin(r * tau), 0.0) * 1.4;
            vec2 local = p - center;
            float radius = 0.42 + r * 0.62;
            float dot = 1.0 - smoothstep(radius, radius + aa, length(local));
            float fizz = 0.5 + 0.5 * pow(0.5 + 0.5 * sin(t * (14.0 + r * 15.0) + n * 7.0), 3.0);
            float fade = smoothstep(0.015, 0.09, life) * (1.0 - smoothstep(0.28, 0.68, life));
            vec3 tint = mix(color, fae_palette(fract(r + 0.39)), 0.4);
            paint = fae_over(paint, tint, exp(-length(local) * 0.9) * fade * 0.28 * energy);
            paint = fae_over(paint, mix(tint, vec3(1.0, 0.97, 0.85), 0.72), dot * fade * fizz * energy);
        }
    }

    float envelope = smoothstep(-MAGIC_INSET, -MAGIC_INSET + 2.0 * aa, d)
        * (1.0 - smoothstep(max(extent - 2.0 * aa, 0.0), extent, d));
    return vec4(clamp(paint.rgb / max(paint.a, 0.0001), 0.0, 1.0), paint.a * envelope);
}
