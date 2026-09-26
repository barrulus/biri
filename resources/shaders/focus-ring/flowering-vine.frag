// Living focus ring: intertwined vines, unfurling leaves and opening flowers.
// Umbriel decoration shader; logical coordinates, straight RGBA output.
const float VINE_SPEED = 0.75;
const float GROWTH_SECONDS = 13.0;
const float SPROUT_SPACING = 88.0;
const float BLOOM_DRIFT = 18.0; // logical pixels per second along the vine
const float VINE_OUTSET = 12.0; // fit inside the usual screen-edge margin
const float VINE_INSET = 24.0; // matching window pass paints inward growth

float vine_hash(float p) { return fract(sin(p * 127.1 + 311.7) * 43758.5453); }

float vine_offset() {
    return min(ring_width * 0.55, min(ring_width + ring_padding, VINE_OUTSET) * 0.40);
}

vec4 vine_radii() {
    return clamp(ring_radius, 0.0, min(ring_size.x, ring_size.y) * 0.5) + vine_offset();
}

float vine_length() {
    vec2 size = ring_size + 2.0 * vine_offset();
    return 2.0 * (size.x + size.y) - (2.0 - 1.57079632679) * dot(vine_radii(), vec4(1.0));
}

// True arclength along a rounded reference contour through the middle of the
// braid. Unlike radial projection, straight-edge coordinates do not shear.
float vine_perimeter(vec2 coords) {
    vec2 p = coords + vine_offset();
    vec2 size = ring_size + 2.0 * vine_offset();
    vec4 r = vine_radii(); // TL, TR, BR, BL
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

// Inverse arclength: pixel position and UNIT tangent. A flower uses this rigid
// local frame, preserving round petals through the corners and perimeter seam.
vec4 vine_frame(float along) {
    float s = mod(along, vine_length());
    float offset = vine_offset();
    vec2 size = ring_size + 2.0 * offset;
    vec4 r = vine_radii();
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

float vine_height(float along, float perimeter, float t, float strand) {
    float u = along / perimeter;
    float turns = max(floor(perimeter / 145.0), 1.0);
    float phase = 6.28318530718 * turns * u - t * 0.95;
    // Hug the client; inward petals are continued by the matching window pass.
    float center = vine_offset();
    return center + ring_width * 0.31 * sin(phase + strand * 3.14159265359)
        + 0.55 * sin(6.28318530718 * u * (turns * 2.0 + 1.0) + t * 0.53);
}

float vine_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    return length(p - a - v * clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0));
}

vec2 vine_branch(float f, float height, float lean, float reach) {
    // Quadratic curve; a sprout grows along it rather than stretching the stem.
    vec2 a = vec2(0.0, height);
    vec2 b = vec2(lean * 0.12, height + reach * 0.65);
    vec2 c = vec2(lean, height + reach);
    return mix(mix(a, b, f), mix(b, c, f), f);
}

vec4 vine_over(vec4 under, vec3 color, float alpha) {
    alpha = clamp(alpha, 0.0, 1.0);
    return vec4(color * alpha + under.rgb * (1.0 - alpha), alpha + under.a * (1.0 - alpha));
}

vec4 vine_leaf(vec2 p, vec2 origin, vec2 direction, float size, float seed, float aa) {
    if (size < 0.1) return vec4(0.0);
    vec2 axis = normalize(direction);
    vec2 q = vec2(dot(p - origin, axis), dot(p - origin, vec2(-axis.y, axis.x)));
    float f = q.x / size;
    float width = size * 0.32 * pow(max(sin(clamp(f, 0.0, 1.0) * 3.14159265359), 0.0), 0.85);
    float cover = smoothstep(0.0, 0.08, f) * (1.0 - smoothstep(0.92, 1.0, f))
        * (1.0 - smoothstep(max(width - aa, 0.0), width + aa, abs(q.y)));
    float midrib = exp(-abs(q.y) * 2.7);
    float side_veins = pow(0.5 + 0.5 * sin(q.x * 2.2 - abs(q.y) * 2.6), 8.0);
    vec3 color = mix(vec3(0.08, 0.36, 0.09), vec3(0.33, 0.72, 0.15), seed);
    color *= 0.80 + 0.20 * smoothstep(-1.5, 1.5, q.y);
    color += vec3(0.24, 0.24, 0.06) * midrib + vec3(0.05, 0.09, 0.01) * side_veins;
    return vec4(color, cover);
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float d = ring_distance(coords);
    float aa = 0.6 / max(niri_scale, 0.01);
    float extent = min(ring_width + ring_padding, VINE_OUTSET);
    // Border host clips the client; the window pass draws that same inner half.
    if (d <= -VINE_INSET || d >= extent) return vec4(0.0);
    float perimeter = vine_length();
    float along = vine_perimeter(coords);
    float t = niri_time * VINE_SPEED;
    vec4 paint = vec4(0.0);

    // Two gently moving stems weave over and under one another around the client.
    for (int strand = 0; strand < 2; strand++) {
        float center = vine_height(along, perimeter, t, float(strand));
        float distance = abs(d - center);
        float stem = 1.0 - smoothstep(0.85, 0.85 + aa, distance);
        float sheen = exp(-abs(d - center + 0.34) * 3.0);
        vec3 green = mix(vec3(0.08, 0.31, 0.085), vec3(0.22, 0.58, 0.13), float(strand));
        green += vec3(0.22, 0.25, 0.055) * sheen;
        paint = vine_over(paint, green, stem * 0.96);
    }

    float cells = max(floor(perimeter / SPROUT_SPACING), 4.0);
    float spacing = perimeter / cells;
    // Move the lookup with the flowers so attachments remain continuous across
    // corners and the closing seam. Each sprout keeps its own growth clock.
    float drift = mod(niri_time * BLOOM_DRIFT + 5.0 * sin(t * 0.38), perimeter);
    float cell = floor((along - drift) / spacing);
    for (int neighbour = -1; neighbour <= 1; neighbour++) {
        float index = cell + float(neighbour);
        float id = mod(index, cells);
        float seed = vine_hash(id + 11.0);
        float wander = spacing * 0.13 * sin(t * 0.43 + seed * 19.0);
        float root_along = (index + 0.25 + seed * 0.5) * spacing + drift + wander;
        vec4 frame = vine_frame(root_along);
        vec2 relative = coords - frame.xy;
        vec2 normal = vec2(frame.w, -frame.z);
        vec2 p = vec2(dot(relative, frame.zw), dot(relative, normal) + vine_offset());
        if (abs(p.x) > 31.0) continue;
        float period = GROWTH_SECONDS + vine_hash(id + 51.0) * 7.0;
        float age = mod(niri_time + seed * period, period) / period;
        float visibility = smoothstep(0.0, 0.05, age) * (1.0 - smoothstep(0.86, 1.0, age));
        float growth = smoothstep(0.03, 0.32, age);
        float opening = smoothstep(0.28, 0.57, age);
        float root = vine_height(root_along, perimeter, t, step(0.5, seed));
        float facing = vine_hash(id + 103.0) < 0.5 ? -1.0 : 1.0;
        // Inward shoots extend over content; outward ones stay compact so they
        // remain visible when the window is against a screen edge.
        float room = facing < 0.0 ? VINE_INSET + root - 7.0 : extent - root - 7.0;
        float reach = facing * min(13.0 + vine_hash(id + 27.0) * 6.0, max(room, 0.0));
        float lean = (seed - 0.5) * 15.0 + sin(t * 0.9 + seed * 30.0) * 2.0;
        vec2 previous = vec2(0.0, root);
        float stem_distance = 1000.0;
        for (int segment = 1; segment <= 5; segment++) {
            vec2 next = vine_branch(float(segment) * growth / 5.0, root, lean, reach);
            stem_distance = min(stem_distance, vine_segment(p, previous, next));
            previous = next;
        }
        float sprout = (1.0 - smoothstep(0.48, 0.48 + aa, stem_distance)) * visibility * growth;
        paint = vine_over(paint, vec3(0.28, 0.60, 0.14), sprout);

        for (int leaf = 0; leaf < 2; leaf++) {
            float n = float(leaf);
            float location = 0.30 + n * 0.33;
            float unfolding = smoothstep(location, location + 0.24, growth);
            vec2 origin = vine_branch(location, root, lean, reach);
            float side = leaf == 0 ? -1.0 : 1.0;
            vec2 direction = vec2(side * (0.9 + seed * 0.2), facing * (0.48 + 0.18 * sin(t + seed * 12.0 + n)));
            float size = (6.0 + vine_hash(id + n * 23.0) * 2.0) * unfolding;
            vec4 foliage = vine_leaf(p, origin, direction, size, fract(seed + n * 0.4), aa);
            paint = vine_over(paint, foliage.rgb, foliage.a * visibility);
        }

        // Some shoots remain leafy, giving open flowers room between them.
        if (seed > 0.24) {
            vec2 tip = vine_branch(growth, root, lean, reach);
            vec2 q = p - tip;
            float angle = atan(q.y, q.x + 0.00001) + seed * 12.0 + 0.12 * sin(t + seed * 9.0);
            float petals = seed > 0.65 ? 6.0 : 5.0;
            float lobe = pow(0.5 + 0.5 * cos(angle * petals), 0.65);
            float size = (4.0 + seed * 1.5) * opening + 0.5 * growth;
            float edge = size * mix(0.68, 0.55 + lobe * 0.45, opening);
            float flower = (1.0 - smoothstep(edge - aa, edge + aa, length(q))) * visibility * growth;
            vec3 petal = mix(vec3(0.94, 0.25, 0.45), vec3(0.72, 0.43, 0.97), step(0.56, seed));
            petal = mix(petal, vec3(1.0, 0.83, 0.59), step(0.84, seed));
            float rim = smoothstep(size * 0.12, max(size * 0.94, 0.001), length(q));
            petal = mix(petal * 0.66, mix(petal, vec3(1.0, 0.91, 0.90), 0.32), rim);
            paint = vine_over(paint, petal, flower);
            float center = (1.0 - smoothstep(1.0, 1.0 + aa, length(q))) * opening * visibility;
            paint = vine_over(paint, vec3(1.0, 0.76, 0.14), center);
        }
    }

    float envelope = smoothstep(-VINE_INSET, -VINE_INSET + 2.0 * aa, d)
        * (1.0 - smoothstep(max(extent - 2.0 * aa, 0.0), extent, d));
    return vec4(clamp(paint.rgb / max(paint.a, 0.0001), 0.0, 1.0), paint.a * envelope);
}
