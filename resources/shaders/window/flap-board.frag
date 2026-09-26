// Rectangular flaps turn in travelling waves, showing inverted live content.
// Resting tiles return the input exactly: no persistent grid, tint or history.
const vec2 FLAP_SIZE = vec2(38.0, 26.0); // logical pixels
const float FLAP_DURATION = 1.15;       // seconds for one complete turn
const float FLAP_PERIOD = 5.5;          // seconds between wave fronts
const float FLAP_GAP = 1.0;             // seam visible only during a turn
const float FLAP_SPEED = 1.0;

vec4 global_color(vec3 coords) {
    vec4 source = tex2D_screen(coords.xy);
    float scale = max(niri_scale, 0.01);
    vec2 size = niri_size / scale;
    vec2 p = coords.xy * size;
    vec2 cell = floor(p / FLAP_SIZE);
    vec2 origin = cell * FLAP_SIZE;
    // Partial tiles at the right/bottom edges have their own complete hinge.
    vec2 extent = min(FLAP_SIZE, size - origin);
    vec2 centre = origin + extent * 0.5;
    vec2 q = p - centre;

    // A bent diagonal front: neighbours turn in sequence instead of flickering.
    float delay = cell.x * 0.10 + cell.y * 0.16
        + 0.24 * sin(cell.y * 0.48 + cell.x * 0.16);
    float age = mod(niri_time * FLAP_SPEED - delay, FLAP_PERIOD);
    if (age >= FLAP_DURATION || source.a <= 0.0) return source;
    float phase = age / FLAP_DURATION;
    float activity = smoothstep(0.0, 0.08, phase)
        * (1.0 - smoothstep(0.92, 1.0, phase));
    if (activity <= 0.0) return source;

    // A full turn returns the original face upright, with an inverted reverse.
    float angle = 6.28318530718 * smoothstep(0.0, 1.0, phase);
    float tilt = cos(angle);
    float depth = sin(angle);
    float camera = FLAP_SIZE.y * 3.0;
    // Inverse perspective projection around each rectangle's horizontal hinge.
    float denominator = tilt - q.y * depth / camera;
    if (abs(tilt) < 0.015 || abs(denominator) < 0.015) return source;
    float localY = q.y / denominator;
    float perspective = 1.0 + localY * depth / camera;
    vec2 local = vec2(q.x * perspective, localY);
    vec2 halfFace = max(extent * 0.5 - FLAP_GAP * 0.5, vec2(0.0));
    vec2 aa = vec2(0.65 / scale, 0.65 / (scale * max(abs(tilt), 0.015)));
    vec2 edge = vec2(1.0) - smoothstep(halfFace - aa, halfFace + aa, abs(local));
    float face = edge.x * edge.y * smoothstep(0.015, 0.06, abs(tilt));
    if (face <= 0.0 || perspective <= 0.0) return source;

    // Clamp within this tile, including partial edge tiles, to avoid neighbour bleed.
    vec2 inset = min(vec2(0.5 / scale), extent * 0.5);
    vec2 samplePoint = clamp(centre + local, origin + inset, origin + extent - inset);
    vec4 turned = tex2D_screen(samplePoint / size);
    vec3 inverse = vec3(1.0) - clamp(turned.rgb / max(turned.a, 0.0001), 0.0, 1.0);
    // Keep the destination alpha intact, including translucent client content.
    float coverage = activity * face * step(0.0001, turned.a);
    return vec4(mix(source.rgb, inverse * source.a, coverage), source.a);
}
