
void main() {
    vec3 coord = vec3(niri_v_coords, 1.0);
    vec4 color = global_color(coord);

    color = color * niri_alpha;

#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
