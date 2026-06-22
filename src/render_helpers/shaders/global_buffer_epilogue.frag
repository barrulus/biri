
void main() {
    vec3 coord = vec3(niri_region.xy + niri_v_coords * niri_region.zw, 1.0);
    gl_FragColor = global_buffer(coord);
}
