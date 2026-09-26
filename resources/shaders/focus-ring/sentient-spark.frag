// Independent emerald / white micro-discharges around the ENTIRE perimeter.
// No travelling head, lap phase or moving wake. Returns straight RGBA.
const float SPARK_DENSITY = 13.0; // logical pixels between potential sparks
const float SPARK_SPEED = 1.0;
const float SPARK_STRENGTH = 1.0;

float ss_hash(float p) { return fract(sin(p * 127.1 + 311.7) * 43758.5453); }

float ss_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    return length(p - a - v * clamp(dot(p - a, v) / max(dot(v, v), 0.001), 0.0, 1.0));
}

float ss_perimeter(vec2 p) {
    vec2 half_size = max(ring_size * 0.5, vec2(1.0));
    vec2 q = p - half_size;
    q /= max(max(abs(q.x) / half_size.x, abs(q.y) / half_size.y), 0.0001);
    if (abs(q.y) / half_size.y >= abs(q.x) / half_size.x)
        return q.y < 0.0 ? q.x + half_size.x : ring_size.x + ring_size.y + half_size.x - q.x;
    return q.x > 0.0 ? ring_size.x + q.y + half_size.y : 2.0 * ring_size.x + ring_size.y + half_size.y - q.y;
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float d = ring_distance(coords);
    float aa = 0.65 / max(niri_scale, 0.01);
    float extent = min(ring_width + ring_padding, ring_width * 2.8 + 3.0);
    if (d <= 0.0 || d >= extent) return vec4(0.0);
    float perimeter = 2.0 * (ring_size.x + ring_size.y);
    float cells = max(floor(perimeter / SPARK_DENSITY), 4.0);
    float spacing = perimeter / cells;
    float u = ss_perimeter(coords) / spacing;
    float core = 0.0;
    float halo = 0.0;
    for (int neighbour = -1; neighbour <= 1; neighbour++) {
        float id = floor(u) + float(neighbour);
        float seed = ss_hash(mod(id, cells));
        float phase = niri_time * SPARK_SPEED * (1.8 + seed * 2.6) + seed * 39.0;
        float event = floor(phase);
        float life = fract(phase);
        float burst = smoothstep(0.0, 0.045, life) * (1.0 - smoothstep(0.10, 0.58, life));
        float jitter = ss_hash(seed * 67.0 + event);
        vec2 p = vec2((u - id - 0.5) * spacing, d - ring_width * (0.5 + jitter * 0.6));
        float reach = 2.0 + jitter * 4.0;
        float tilt = (seed - 0.5) * 3.0;
        // A tiny broken arc and a branching radial fleck, each rooted in place.
        vec2 a = vec2(-reach, tilt);
        vec2 b = vec2(-reach * 0.2, -1.0 - jitter * 2.0);
        vec2 c = vec2(reach * 0.25, 1.2 + seed);
        vec2 e = vec2(reach, -tilt);
        float arc = min(ss_segment(p, a, b), min(ss_segment(p, b, c), ss_segment(p, c, e)));
        float fork = ss_segment(p, b, vec2(reach * 0.15, -reach * 0.8));
        float filament = 1.0 - smoothstep(0.23, 0.23 + aa, min(arc, fork));
        float glint = exp(-length(p) * 1.1);
        core += (filament + glint * 0.65) * burst;
        halo += exp(-length(p) * 0.30) * burst;
    }
    float rail = exp(-abs(d - ring_width * 0.55) * 1.7) * 0.12;
    float envelope = smoothstep(0.0, aa * 2.0, d) * (1.0 - smoothstep(extent - aa * 2.0, extent, d));
    float alpha = clamp((rail + core * 0.95 + halo * 0.19) * SPARK_STRENGTH * envelope, 0.0, 1.0);
    vec3 color = mix(vec3(0.035, 0.85, 0.34), vec3(0.82, 1.0, 0.91), clamp(core, 0.0, 1.0));
    return vec4(color, alpha);
}
