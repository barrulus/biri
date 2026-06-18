### Overview

Biri supports a **global post-process shader** that runs over the entire composited output before it is displayed.
Use it for effects such as colour grading, CRT scanlines, night-light tints, motion-blur trails, or any full-screen visual transformation.

> [!NOTE]
> The global shader runs on the **TTY/DRM backend only**.
> It has no effect on the winit (nested/X11) backend.
> Screenshots and screen recordings (screencopy / screencast) render **without** the shader; they always capture the unprocessed frame.

While a global shader is active:

- The entire output is redrawn every frame (no partial-damage power savings).
- Direct scanout and overlay-plane offload are disabled for that output.

When no `global-shader` block is present (or `enable` is not set), there is **zero overhead**.

---

### Configuration Block

```kdl
global-shader {
    // Activate the shader.
    enable

    // Inline GLSL source (mutually exclusive with path).
    source "vec4 global_color(vec3 coord) { return tex2D_screen(coord.xy); }"

    // Path to a .frag file on disk (mutually exclusive with source).
    // path "/home/user/.config/niri/my-shader.frag"

    // Shader API flavour: "niri" (default) or "hyprland".
    mode "niri"

    // Composite the cursor into niri_screen (forces a software cursor).
    // reads-cursor
}
```

Fields:

| Field | Type | Default | Description |
|---|---|---|---|
| `enable` | flag | off | Activate the shader. Without this, the block is parsed but has no effect. |
| `source` | string | — | Inline GLSL fragment source. Mutually exclusive with `path`. |
| `path` | string | — | Path to a `.frag` file, re-read whenever the config is reloaded. Editing the `.frag` file alone does **not** trigger a reload — you must also reload the config (e.g. re-save `config.kdl`). Mutually exclusive with `source`. |
| `mode` | string | `"niri"` | API flavour: `"niri"` or `"hyprland"`. |
| `reads-cursor` | flag | off | Include cursor pixels in `niri_screen`. See [reads-cursor](#reads-cursor) below. |

If both `source` and `path` are set, or if neither is set while `enable` is present, biri logs a warning and disables the effect — the compositor never crashes.

The shader is hot-reloaded whenever the config is reloaded (e.g. after saving the file while niri watches it). A shader that fails to compile also logs a warning and leaves the screen rendering normally.

---

### niri Mode (default)

In niri mode you supply a single GLSL function:

```glsl
vec4 global_color(vec3 coord);
```

Biri wraps it with a `main()` that passes `coord.xy` (normalised 0..1 UV coordinates across the output) and writes your return value to `gl_FragColor`.

#### Available uniforms and helpers

| Name | Type | Description |
|---|---|---|
| `niri_time` | `float` | Seconds elapsed since the shader was activated. |
| `niri_size` | `vec2` | Output dimensions in physical pixels. |
| `niri_scale` | `float` | Output scale factor. |
| `niri_cursor` | `vec2` | Cursor position in output-local physical pixels. |
| `niri_screen` | `sampler2D` | The composited frame below the effect. |
| `niri_prev` | `sampler2D` | The previous frame's shader output (for feedback/trails). |

Convenience helpers (already defined in the prelude):

```glsl
vec4 tex2D_screen(vec2 uv);  // texture2D(niri_screen, uv)
vec4 tex2D_prev(vec2 uv);    // texture2D(niri_prev, uv)
```

The GLSL dialect is **GLES2 (`#version 100`)**.
Use `texture2D()`, not `texture()`.
Declare varyings with `varying`, not `in`/`out`.

#### Example: warm tint

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 coord) {
        vec4 c = tex2D_screen(coord.xy);
        return vec4(c.r * 1.0, c.g * 0.85, c.b * 0.7, c.a);
    }"
}
```

#### Example: feedback trail

Each frame mixes 5 % of the current screen with 95 % of the previous frame's output.

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 coord) {
        vec4 current = tex2D_screen(coord.xy);
        vec4 prev    = tex2D_prev(coord.xy);
        return mix(prev, current, 0.05);
    }"
}
```

#### Example: animated vignette

Uses `niri_time` to pulse the vignette strength.

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 coord) {
        vec4 c = tex2D_screen(coord.xy);
        vec2 uv = coord.xy - 0.5;
        float d = dot(uv, uv);
        float pulse = 0.6 + 0.4 * sin(niri_time * 2.0);
        return c * (1.0 - d * pulse * 3.0);
    }"
}
```

---

### hyprland Mode

In hyprland mode you supply a complete raw fragment shader with its own `main()` that writes `gl_FragColor`.
This is intended to make Hyprland community `screen_shader`s work with minimal changes.

The following aliases are pre-defined so that typical Hyprland shaders compile without modification:

| Alias | Resolves to |
|---|---|
| `tex` | `niri_screen` (screen sampler) |
| `v_texcoord` | normalised UV varying |
| `time` | `niri_time` |
| `wl_output` | `0` (integer constant) |

> [!NOTE]
> `niri_cursor` is **not** available in hyprland mode.

#### Compatibility caveat

Only the **GLES2 (`#version 100`)** dialect is supported.
Hyprland shaders written as `#version 300 es` need the following manual edits before use:

- `texture(sampler, uv)` → `texture2D(sampler, uv)`
- `out vec4 fragColor;` … `fragColor = …` → `gl_FragColor = …`
- `in vec2 v_texcoord;` → `varying vec2 v_texcoord;`

#### Example: load a Hyprland shader from a file

```kdl
global-shader {
    enable
    path "/home/user/.config/niri/crt.frag"
    mode "hyprland"
}
```

---

### reads-cursor

By default, the hardware cursor is composited on a separate plane after the shader runs.
This means:

- The cursor is **not** included in `niri_screen`, so shaders cannot transform it.
- `niri_cursor` (position) is still available for cursor-following effects (glow, ripple, etc.).
- The hardware cursor plane is used — minimal GPU cost, no cursor lag.

When `reads-cursor` is set:

- Biri composites the cursor into the screen texture **before** running the shader, so `niri_screen` contains the actual cursor pixels.
- This **forces a software cursor** for that output, which carries a measurable GPU and battery cost (especially on Nvidia).

Use `reads-cursor` only when your shader needs to transform the cursor pixels themselves (e.g. colour-inverting the cursor region).

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 coord) {
        return tex2D_screen(coord.xy);
    }"
    reads-cursor
}
```

---

### Hot-Reload

Any change to the `global-shader` block takes effect the next time niri reloads its config. When using `path`, the file is re-read on config reload only — editing the `.frag` file by itself does not trigger a reload, so re-save your `config.kdl` (or otherwise reload the config) to pick up shader-file edits.
If the new shader fails to compile, the old shader (if any) continues running and a warning is logged — the compositor never crashes.

---

### Performance Summary

| Condition | Overhead |
|---|---|
| No `global-shader` block | Zero |
| `global-shader` block present, `enable` absent | Zero |
| `enable` set, valid shader | Full-frame redraw every frame; no direct scanout; overlay planes disabled |
| `reads-cursor` additionally set | As above, plus software cursor (extra GPU cost) |
