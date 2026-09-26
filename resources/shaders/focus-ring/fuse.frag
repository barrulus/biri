// An irregular braided fuse with one to four travelling embers, ash and sparks.
// File-based focus-ring shader: return straight RGBA; no main() or #version.
// Suggested settings: width 6; padding 48;
// shader { path "~/.config/niri/focus-ring/fuse.frag"; padding 48;
//          light spread=90 intensity=1.4 threshold=0.5; }
// Padding should be at least 8 * width for sparks, not window spacing.
// The cord itself stays within the nominal ring width, including its bends.
// This fits a width-6 ring into ordinary 6-pixel gaps, even at output edges.
// The charred trail gradually recovers to make the animation repeat seamlessly.
// Number of burning tips (1-4; out-of-range values are clamped).
const int EMBER_COUNT = 1;
const float FUSE_SECONDS = 10.0;
// Fire brightness only: the unburnt cord stays below the 0.5 light threshold.
const float FUSE_BRIGHTNESS = 1.0;
const float FUSE_WANDER = 1.0;
const float FUSE_TAU = 6.28318530718;

float fuse_hash(float x) {
    return fract(sin(x * 127.1 + 37.7) * 43758.5453);
}

float fuse_wrap(float x) {
    return fract(x + 0.5) - 0.5;
}

// Clockwise, continuous across the four sides, including the closing seam.
float fuse_perimeter(vec2 p) {
    vec2 half_size = max(ring_size * 0.5, vec2(1.0));
    p -= half_size;
    vec2 q = p / max(max(abs(p.x) / half_size.x, abs(p.y) / half_size.y), 0.0001);
    float s;
    if (abs(q.y) / half_size.y >= abs(q.x) / half_size.x) {
        s = q.y < 0.0 ? q.x + half_size.x : ring_size.x + ring_size.y + half_size.x - q.x;
    } else {
        s = q.x > 0.0 ? ring_size.x + q.y + half_size.y : 2.0 * ring_size.x + ring_size.y + half_size.y - q.y;
    }
    return s / (2.0 * (ring_size.x + ring_size.y));
}

// Integer harmonics keep the handmade bends continuous at the perimeter seam.
float fuse_path(float u, float turns) {
    float bend = 0.64 * sin(FUSE_TAU * u * turns + 0.8)
        + 0.30 * sin(FUSE_TAU * u * (turns * 2.0 + 1.0) + 2.1)
        + 0.12 * sin(FUSE_TAU * u * (turns * 5.0 + 3.0) - 0.7);
    return ring_width * (0.5 + 0.20 * clamp(FUSE_WANDER, 0.0, 1.0) * bend);
}

float fuse_segment(vec2 p, vec2 a, vec2 b) {
    vec2 v = b - a;
    return length(p - a - v * clamp(dot(p - a, v) / max(dot(v, v), 0.0001), 0.0, 1.0));
}

vec4 ring_color(vec2 coords) {
    if (ring_width <= 0.0 || min(ring_size.x, ring_size.y) <= 0.0) return vec4(0.0);
    float w = ring_width;
    float aa = 0.5 / max(niri_scale, 0.01);
    float d = ring_distance(coords);
    float limit = min(w * 9.0, w + ring_padding);
    if (d <= 0.0 || d >= limit + aa) return vec4(0.0);

    float perimeter = 2.0 * (ring_size.x + ring_size.y);
    float u = fuse_perimeter(coords);
    float turns = max(3.0, floor(perimeter / 125.0 + 0.5));
    float phase = fract(niri_time / max(FUSE_SECONDS, 0.1));
    float centre = fuse_path(u, turns);
    float across = d - centre;
    // Use the nearest tip for the burn and the most recent tip for the ash trail.
    // Dividing by count retains each ember's size and speed around the whole window.
    float count = clamp(float(EMBER_COUNT), 1.0, 4.0);
    float spacing = perimeter / count;
    float ahead = fuse_wrap((u - phase) * count) * spacing;
    float behind = fract((phase - u) * count) * spacing;

    // Uneven thickness, twisted strands and small loose fibres. These never animate.
    float strands = max(1.0, floor(perimeter / 5.0));
    float twist = FUSE_TAU * u * strands + across / w * 5.5;
    float rib = 0.5 + 0.5 * sin(twist);
    float radius = w * (0.15 + 0.025 * sin(FUSE_TAU * u * (turns * 7.0 + 1.0)));
    float rope = 1.0 - smoothstep(radius - aa, radius + aa, abs(across));
    float fibres = 1.0 - smoothstep(aa * 0.3, aa * 1.2,
        abs(abs(across) - radius - w * (0.08 + 0.06 * sin(twist * 2.0))));
    fibres *= 0.24 * pow(0.5 + 0.5 * sin(FUSE_TAU * u * (strands + 7.0)), 8.0);
    float ash = (1.0 - smoothstep(spacing * 0.10, spacing * 0.32, behind))
        * smoothstep(w, w * 3.0, behind);
    vec3 hemp = mix(vec3(0.22, 0.13, 0.055), vec3(0.46, 0.34, 0.17), rib);
    hemp *= 0.70 + 0.30 * sqrt(max(0.0, 1.0 - pow(across / (radius + aa), 2.0)));
    vec3 colour = mix(hemp, vec3(0.085, 0.065, 0.045) * (0.6 + rib * 0.4), ash);
    float alpha = max(rope, fibres);
    vec3 premul = colour * alpha;

    // A ragged incandescent tip, with a shorter orange afterglow on the burnt cord.
    float flicker = 0.86 + 0.09 * sin(FUSE_TAU * phase * 73.0)
        + 0.05 * sin(FUSE_TAU * phase * 119.0 + 1.0);
    float tip = exp(-pow(ahead / (w * 1.20), 2.0));
    float hot = tip * exp(-pow(across / (w * 0.52 * flicker), 2.0));
    float wake = exp(-behind / (w * 4.5)) * exp(-abs(across) / (w * 0.32));
    float flare = tip * exp(-abs(across) / (w * 1.3)) * 0.32 * flicker;
    float ember = clamp(hot + wake * 0.60, 0.0, 1.0);
    vec3 fire = mix(vec3(1.0, 0.12, 0.008), vec3(1.0, 0.87, 0.36), pow(ember, 2.0));
    float fire_alpha = clamp((ember + flare) * FUSE_BRIGHTNESS, 0.0, 1.0);
    premul = mix(premul, fire, fire_alpha);
    alpha += fire_alpha * (1.0 - alpha);

    // Small ballistic streaks are born on the rope at the tip's earlier position.
    // Their IDs repeat every lap, so neither the rope nor the particles jump at wrap.
    for (int emitter = 0; emitter < 4; emitter++) {
        if (float(emitter) >= count) break;
        float emitter_phase = fract(phase + float(emitter) / count);
        if (abs(fuse_wrap(u - emitter_phase)) * perimeter < perimeter * 0.14 + w * 12.0) {
            float clock = phase * 96.0;
            for (int i = 0; i < 12; i++) {
                float birth = floor(clock) - float(i);
                float seed = mod(birth, 96.0) + float(emitter) * 137.0;
                float age = fract(clock) + float(i);
                float life = 5.0 + 6.0 * fuse_hash(seed + 3.0);
                float t = age / life;
                if (t < 1.0) {
                    float origin = fract(birth / 96.0 + float(emitter) / count);
                    float tangent = (fuse_hash(seed + 17.0) - 0.5) * w * 8.0;
                    float outward = w * (2.0 + 3.0 * fuse_hash(seed + 31.0));
                    vec2 start = vec2(0.0, fuse_path(origin, turns));
                    vec2 velocity = vec2(tangent, outward);
                    vec2 pos = start + velocity * t + vec2(0.0, w * 0.6 * t * t);
                    vec2 tail = pos - velocity * (0.045 + 0.04 * t);
                    vec2 local = vec2(fuse_wrap(u - origin) * perimeter, d);
                    float distance = fuse_segment(local, tail, pos);
                    float spark = (1.0 - smoothstep(w * 0.065, w * 0.065 + aa, distance))
                        * (1.0 - smoothstep(0.25, 1.0, t));
                    spark = clamp(spark * FUSE_BRIGHTNESS, 0.0, 1.0);
                    vec3 spark_colour = mix(vec3(1.0, 0.90, 0.48), vec3(1.0, 0.19, 0.012), t);
                    premul = mix(premul, spark_colour, spark);
                    alpha += spark * (1.0 - alpha);
                }
            }
        }
    }
    float envelope = smoothstep(0.0, 2.0 * aa, d)
        * (1.0 - smoothstep(limit - 2.0 * aa, limit, d));
    return vec4(clamp(premul / max(alpha, 0.0001), 0.0, 1.0), alpha * envelope);
}
