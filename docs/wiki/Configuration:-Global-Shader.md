### Overview

Biri supports a **global post-process shader** that runs over the entire composited output before it is displayed.
Use it for effects such as colour grading, CRT scanlines, night-light tints, motion-blur trails, or any full-screen visual transformation.

> [!NOTE]
> The global shader runs on the **TTY/DRM backend only**.
> It has no effect on the winit (nested/X11) backend.
> Screenshots and screen recordings (screencopy / screencast) render **without** the shader; they always capture the unprocessed frame.

> [!WARNING]
> **Performance cost — read this before enabling.** While a global shader is active, the compositor cannot take any of its normal power-saving shortcuts for that output:
> - **The entire output is redrawn every frame**, continuously, even when nothing on screen changed. There are no partial-damage savings — expect constant GPU load and **significantly higher battery drain** on a laptop.
> - **Direct scanout and overlay-plane offload are disabled**, so full-screen video / games lose their zero-copy fast path while the shader is on.
> - With `reads-cursor` set, the **hardware cursor is forced to software**, adding extra per-frame GPU cost (and it's worse on NVIDIA).
>
> This is inherent to a whole-screen post-process pass, not a tuning problem. Treat a global shader as an always-on GPU effect, and turn it off when you don't need it.

When no `global-shader` block is present (or `enable` is not set), there is **zero overhead** — the compositor renders exactly as it does without the feature.

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

    // Redraw scheduling: "auto" (default), "on-damage", or "continuous".
    redraw "auto"

    // For cursor-local effects: reshade/damage only a box of this radius (logical px)
    // around the cursor, leaving the rest of the output scanout-eligible.
    // cursor-radius 200
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
| `redraw` | string | `"auto"` | Redraw scheduling. `"auto"`: animate every frame only if the shader uses `niri_time` or the feedback buffer (`niri_prev`/`tex2D_prev`); otherwise redraw only on real damage. `"on-damage"`: never force idle redraws. `"continuous"`: always redraw every frame. See [Redraw scheduling](#redraw-scheduling) below. |
| `cursor-radius` | integer (px) | — | For cursor-local shaders: reshade and damage only a box of this radius (logical px) around the cursor, preserving direct scanout elsewhere. Omit for a whole-output effect. See [cursor-radius](#cursor-radius-region-mode) below. |

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
| `niri_size` | `vec2` | Element dimensions in physical pixels. **In [region mode](#cursor-radius-region-mode) this is the box size, not the full output** — use `niri_output_size` for absolute-pixel math. |
| `niri_scale` | `float` | Output scale factor. |
| `niri_cursor` | `vec2` | Cursor position in output-local physical pixels. |
| `niri_screen` | `sampler2D` | The composited frame below the effect. |
| `niri_prev` | `sampler2D` | The previous frame's shader output (effect + screen), via `tex2D_prev(uv)`. |
| `niri_screen_prev` | `sampler2D` | The **previous** frame's screen (no effect), via `tex2D_screen_prev(uv)`. Use `prev − tex2D_screen_prev` to recover a feedback trail without scroll-smear. |
| `niri_region` | `vec4` | The region this element covers in output-normalised coords `(origin.xy, size.xy)`. `(0,0,1,1)` for a whole-output shader; a sub-box in [region mode](#cursor-radius-region-mode). |
| `niri_output_size` | `vec2` | True full-output size in physical pixels. Equals `niri_size` for a whole-output shader, but **differs in region mode** (where `niri_size` is the box). |

Convenience helpers (already defined in the prelude):

```glsl
vec4 tex2D_screen(vec2 uv);      // samples the screen below at output-normalised uv
vec4 tex2D_prev(vec2 uv);        // samples the previous frame's output at output-normalised uv
vec4 tex2D_screen_prev(vec2 uv); // samples the previous frame's screen (no effect)
```

`coord.xy` (passed to `global_color`) and the `uv` arguments to `tex2D_screen`/`tex2D_prev` are always **output-normalised** (0..1 across the whole output), regardless of region mode.

**Region-mode contract (`cursor-radius` set):**
- Use `niri_output_size` — not `niri_size` — for absolute-pixel math (e.g. `coord.xy * niri_output_size` to get physical px). `niri_cursor` is already in absolute output px.
- You may only sample `tex2D_screen`/`tex2D_prev` **within** the region (near the cursor); samples outside the box clamp to a transparent border.
- These uniforms exist in `niri` mode only.

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

### Redraw scheduling

By default (`redraw "auto"`) biri scans your shader source to decide how often to redraw:

- If it references `niri_time` or the feedback buffer (`niri_prev` / `tex2D_prev`), the effect is **animated**, so biri redraws every frame — it keeps animating even when the desktop is completely idle.
- Otherwise the effect is a pure function of the screen below, so biri redraws **only when something actually changes** (no wasted GPU when idle).

Override this with `redraw`:

| Value | Behaviour |
|---|---|
| `"auto"` (default) | Animate every frame iff the shader uses `niri_time`/`niri_prev`/`tex2D_prev`. |
| `"on-damage"` | Never force idle redraws — animate only when other on-screen activity triggers a frame. |
| `"continuous"` | Always redraw every frame, even for a static shader. |

The scan is a literal text match, so it errs on the side of *more* redraws (e.g. an identifier like `lifetime` in a `hyprland`-mode shader counts as using `time`). Use `redraw "on-damage"` to opt out if that happens.

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 c) { return tex2D_screen(c.xy); }"
    cursor-radius 200
    redraw "auto"
}
```

---

### cursor-radius (region mode)

For effects that only touch the area around the cursor (a ring, spotlight, magnifier), set `cursor-radius` to the effect's radius in logical pixels. Biri then reshades and damages only a box of that size around the cursor instead of the whole output, so the rest of the screen keeps direct scanout / overlay-plane offload and the GPU cost drops sharply.

Region mode applies only when the shader uses `niri_cursor` and is **not** animated (no `niri_time` / feedback) — an animated whole-screen effect still reshades the whole output. Because it keys off `niri_cursor`, region mode is **`niri` mode only**; `hyprland`-mode shaders (which have no cursor uniform) always render whole-output and `cursor-radius` is ignored for them. The coordinate contract for region-mode shaders is documented under [niri Mode](#niri-mode-default).

---

### Performance Summary

| Condition | Overhead |
|---|---|
| No `global-shader` block | Zero |
| `global-shader` block present, `enable` absent | Zero |
| `enable` set, valid shader | Full-frame redraw every frame; no direct scanout; overlay planes disabled |
| `reads-cursor` additionally set | As above, plus software cursor (extra GPU cost) |

---

### Scope and Limitations

- **TTY/DRM only.** The effect applies on the real (DRM/KMS) output. It is intentionally **not** applied on the nested winit backend (running niri in a window), nor to screenshots or screen recordings (screencast/screencopy) — those render without the effect.
- **Output transform.** The effect is verified on outputs with the default (`normal`) transform. On outputs configured with a non-default `transform` (e.g. `90`, `270`, `flipped`), the shader's view of `niri_screen`/`niri_prev` may be mis-oriented. If you use a rotated or flipped output, verify your shader there before relying on it.
- **Single shader.** One global shader applies to all outputs; per-output or per-window shaders are not supported.
