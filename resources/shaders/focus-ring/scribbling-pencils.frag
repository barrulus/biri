// Four little pencils draw graphite around a rounded paper perimeter.
// This is also the source for the matching inner overlay. After edits run:
// python3 tools/generate-paper-overlay.py
const float PENCIL_SPEED = 46.0; // logical pixels per second
const float PENCIL_INSET = 22.0;
const float PENCIL_OUTSET = 32.0;
const float PENCIL_PI = 3.14159265359;

float pencil_radius() { return min(18.0, min(ring_size.x, ring_size.y) * 0.25); }
float pencil_perimeter() {
    return 2.0 * (ring_size.x + ring_size.y) - (8.0 - 2.0 * PENCIL_PI) * pencil_radius();
}

// Fixed authoring contour shared by both contracts, independent of native
// corner radii. The compositor still clips each pass to the real client hole.
vec2 pencil_coordinates(vec2 p) {
    float r = pencil_radius();
    vec2 span = ring_size - 2.0 * r;
    float arc = PENCIL_PI * r * 0.5;
    vec2 q = abs(p - ring_size * 0.5) - ring_size * 0.5 + r;
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
    float along;
    if (p.x > ring_size.x - r && p.y < r)
        along = span.x + r * atan(max(p.x - ring_size.x + r, 0.00001), max(r - p.y, 0.00001));
    else if (p.x > ring_size.x - r && p.y > ring_size.y - r)
        along = span.x + arc + span.y + r * atan(max(p.y - ring_size.y + r, 0.00001), max(p.x - ring_size.x + r, 0.00001));
    else if (p.x < r && p.y > ring_size.y - r)
        along = 2.0 * span.x + 2.0 * arc + span.y + r * atan(max(r - p.x, 0.00001), max(p.y - ring_size.y + r, 0.00001));
    else if (p.x < r && p.y < r)
        along = 2.0 * span.x + 3.0 * arc + 2.0 * span.y + r * atan(max(r - p.y, 0.00001), max(r - p.x, 0.00001));
    else {
        vec4 distances = abs(vec4(p.y, p.x - ring_size.x, p.y - ring_size.y, p.x));
        float nearest = min(min(distances.x, distances.y), min(distances.z, distances.w));
        if (nearest == distances.x) along = clamp(p.x - r, 0.0, span.x);
        else if (nearest == distances.y) along = span.x + arc + clamp(p.y - r, 0.0, span.y);
        else if (nearest == distances.z) along = span.x + arc * 2.0 + span.y + clamp(ring_size.x - r - p.x, 0.0, span.x);
        else along = span.x * 2.0 + arc * 3.0 + span.y + clamp(ring_size.y - r - p.y, 0.0, span.y);
    }
    return vec2(along, d);
}

// Position and unit tangent; rigid pencil sprites turn smoothly at corners.
vec4 pencil_frame(float along) {
    float r = pencil_radius();
    float s = mod(along, pencil_perimeter());
    for (int side = 0; side < 4; side++) {
        float line;
        vec2 start, tangent, center;
        if (side == 0) {
            line = ring_size.x - 2.0 * r;
            start = vec2(r, 0.0); tangent = vec2(1.0, 0.0); center = vec2(ring_size.x - r, r);
        } else if (side == 1) {
            line = ring_size.y - 2.0 * r;
            start = vec2(ring_size.x, r); tangent = vec2(0.0, 1.0); center = ring_size - r;
        } else if (side == 2) {
            line = ring_size.x - 2.0 * r;
            start = vec2(ring_size.x - r, ring_size.y); tangent = vec2(-1.0, 0.0); center = vec2(r, ring_size.y - r);
        } else {
            line = ring_size.y - 2.0 * r;
            start = vec2(0.0, ring_size.y - r); tangent = vec2(0.0, -1.0); center = vec2(r);
        }
        if (s <= line) return vec4(start + tangent * s, tangent);
        s -= line;
        float arc = PENCIL_PI * r * 0.5;
        if (s <= arc || side == 3) {
            float angle = (float(side) - 1.0) * PENCIL_PI * 0.5 + s / max(r, 0.001);
            return vec4(center + r * vec2(cos(angle), sin(angle)), -sin(angle), cos(angle));
        }
        s -= arc;
    }
    return vec4(r, 0.0, 1.0, 0.0);
}

float pencil_wiggle(float along) {
    float phase = along / pencil_perimeter() * 2.0 * PENCIL_PI;
    float loops = max(2.0, floor(pencil_perimeter() / 125.0));
    return 1.0 + 6.5 * sin(phase * loops) + 1.8 * sin(phase * (loops * 3.0 + 1.0));
}

vec4 pencil_over(vec4 under, vec3 color, float alpha) {
    alpha = clamp(alpha, 0.0, 1.0);
    return vec4(color * alpha + under.rgb * (1.0 - alpha), alpha + under.a * (1.0 - alpha));
}

// Continue the white stock from crumpled-paper.glsl across the client edge.
// Use identical logical coordinates, fold phases, grain and neutral lighting.
vec3 pencil_paper(vec2 p) {
    float relief = 0.0;
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
    vec2 grain = fract(floor(p * 1.6) * vec2(123.34, 345.45));
    grain += dot(grain, grain + 34.345);
    float fibre = (fract(grain.x * grain.y) - 0.5) * 0.018;
    relief *= 0.24;
    return vec3(clamp(0.985 * (1.0 + relief) + relief * 0.12 + fibre, 0.0, 1.0));
}

vec4 ring_color(vec2 coords) {
    if (min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    vec2 path = pencil_coordinates(coords);
    if (path.y < -PENCIL_INSET || path.y > PENCIL_OUTSET) return vec4(0.0);
    float aa = 0.65 / max(niri_scale, 0.01);
    float perimeter = pencil_perimeter();
    // One sheet beneath the entire pencil path, continued by the inner pass.
    // Fade the inner margin into content; leave the centre of the client alone.
    float paper_alpha = smoothstep(-16.0, -9.0, path.y)
        * (1.0 - smoothstep(29.5 - aa, 29.5 + aa, path.y));
    vec4 paint = vec4(pencil_paper(coords) * paper_alpha, paper_alpha);
    float grain = fract(sin(dot(floor(coords * 2.0), vec2(12.9898, 78.233))) * 43758.5453);

    // A lightly sketched double outline remains behind the moving strokes.
    float wobble = 0.55 * sin(path.x * 0.17) + 0.3 * sin(path.x * 0.43);
    float outline = min(abs(path.y - 2.4 - wobble), abs(path.y + 1.1 - wobble * 0.7));
    paint = pencil_over(paint, vec3(0.22, 0.20, 0.17),
        (1.0 - smoothstep(0.3, 0.3 + aa, outline)) * (0.30 + grain * 0.20));

    for (int i = 0; i < 4; i++) {
        float head = mod(niri_time * PENCIL_SPEED + (float(i) + 0.12) * perimeter * 0.25, perimeter);
        float behind = mod(head - path.x + perimeter, perimeter);
        float trail_length = min(260.0, perimeter * 0.235);
        // Fresh strokes follow each tip. A faint older mark remains on the
        // sheet after the dark trail passes, without allocating a history buffer.
        float trail = mix(0.18, 1.0, 1.0 - smoothstep(trail_length * 0.35, trail_length, behind));
        float stroke = abs(path.y - pencil_wiggle(path.x));
        paint = pencil_over(paint, vec3(0.15),
            (1.0 - smoothstep(0.68, 0.68 + aa, stroke)) * trail * (0.78 + grain * 0.20));

        vec4 frame = pencil_frame(head);
        vec2 normal = vec2(frame.w, -frame.z);
        vec2 tip = frame.xy + normal * pencil_wiggle(head);
        float tilt = 0.48 + 0.12 * sin(niri_time * 5.0 + float(i) * 1.7);
        vec2 axis = -frame.zw * cos(tilt) + normal * sin(tilt);
        vec2 relative = coords - tip;
        vec2 p = vec2(dot(relative, axis), dot(relative, vec2(-axis.y, axis.x)));
        if (p.x < -2.0 || p.x > 35.0 || abs(p.y) > 6.0) continue;
        float width = 2.7 * clamp(p.x / 7.0, 0.0, 1.0);
        float body = smoothstep(-aa, aa, p.x) * (1.0 - smoothstep(32.0 - aa, 32.0 + aa, p.x))
            * (1.0 - smoothstep(width - aa, width + aa, abs(p.y)));
        vec3 color = vec3(0.91, 0.65, 0.15); // lacquered yellow cedar pencil
        color *= 0.78 + 0.22 * smoothstep(-2.0, 1.3, p.y);
        color += vec3(0.16, 0.15, 0.08) * exp(-pow((p.y + 0.7) * 2.0, 2.0));
        if (p.x < 7.0) color = vec3(0.80, 0.61, 0.39) * (0.90 + p.y * 0.05);
        if (p.x < 2.6) color = vec3(0.16, 0.15, 0.14);
        if (p.x > 24.0) color = vec3(0.63, 0.66, 0.64) * (0.85 + 0.15 * sin(p.x * 5.0));
        if (p.x > 27.5) color = vec3(0.86, 0.43, 0.40) * (0.9 + p.y * 0.035);
        float rim = smoothstep(max(width - 0.7, 0.0), max(width, 0.001), abs(p.y));
        color *= 1.0 - 0.28 * rim;
        paint = pencil_over(paint, color, body);
    }
    // Return straight RGBA; each host applies its own premultiplication.
    return vec4(paint.rgb / max(paint.a, 0.0001), paint.a);
}
