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
| `pass` | block (repeatable) | — | One pass in a multi-pass chain. When any `pass` block is present it replaces the top-level `source`/`path`. See [Multi-pass chains](#multi-pass-chains-pass) below. |

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
| `niri_buffer` | `sampler2D` | The dedicated feedback buffer (last frame), via `tex2D_buffer(uv)`. Equals `niri_prev` unless you define `global_buffer` (see below). |
| `niri_region` | `vec4` | The region this element covers in output-normalised coords `(origin.xy, size.xy)`. `(0,0,1,1)` for a whole-output shader; a sub-box in [region mode](#cursor-radius-region-mode). |
| `niri_output_size` | `vec2` | True full-output size in physical pixels. Equals `niri_size` for a whole-output shader, but **differs in region mode** (where `niri_size` is the box). |

Convenience helpers (already defined in the prelude):

```glsl
vec4 tex2D_screen(vec2 uv);      // samples the screen below at output-normalised uv
vec4 tex2D_prev(vec2 uv);        // samples the previous frame's output at output-normalised uv
vec4 tex2D_screen_prev(vec2 uv); // samples the previous frame's screen (no effect)
vec4 tex2D_buffer(vec2 uv);      // samples the dedicated feedback buffer (last frame)
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

### Dedicated feedback buffer (`global_buffer`)

`niri_prev` is the previous *output* — your effect **and** the screen mixed together — so a trail
recovered from it picks up scrolling/video motion and smears. To avoid that, define an optional
second function that writes a **screen-independent** feedback buffer:

```glsl
vec4 global_buffer(vec3 coord);  // returns what to store in niri_buffer this frame
```

When present, biri renders `global_buffer` into a clean offscreen texture each frame (reading
last frame's buffer via `tex2D_buffer`), then your `global_color` reads **this frame's** buffer
via `tex2D_buffer` to composite it over the screen. Because the buffer never contains the screen,
trails don't smear when content scrolls underneath.

```kdl
global-shader {
    enable
    source "
    vec4 global_buffer(vec3 c){
        vec3 prev = tex2D_buffer(c.xy).rgb;            // pure trail, no screen
        float d = length(c.xy*niri_output_size - niri_cursor);
        float fresh = smoothstep(18.0, 0.0, d);
        return vec4(max(prev*0.90, vec3(0.2,0.8,1.0)*fresh), 1.0);
    }
    vec4 global_color(vec3 c){
        vec3 s = tex2D_screen(c.xy).rgb;
        vec4 b = tex2D_buffer(c.xy);                   // this frame's trail
        return vec4(mix(s, b.rgb, length(b.rgb)*0.6), 1.0);
    }"
}
```

Notes: a `global_buffer` shader is always treated as animated (redraws every frame, whole-output —
no region mode). If you omit `global_buffer`, `niri_buffer` simply aliases `niri_prev`. niri mode
only.

---

### Multi-pass chains (`pass`)

Instead of a single shader, you can run an ordered **chain** of shaders — e.g. blur → grade →
vignette — where each pass reads the previous pass's output. List the passes as repeatable
`pass {}` blocks inside `global-shader {}`. When one or more `pass` blocks are present they
*are* the chain, and the top-level `source`/`path` are ignored.

```kdl
global-shader {
    enable
    pass {
        path "/home/user/.config/niri/blur.frag"
    }
    pass {
        source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
    }
    pass {
        path "/home/user/.config/niri/vignette.frag"
        mode "niri"
    }
}
```

Each pass is an ordinary `global_color` shader (or a `hyprland`-mode raw shader). What it sees:

| Sampler / helper | Meaning for pass *N* |
|---|---|
| `niri_screen` / `tex2D_screen` | this pass's **input** = pass *N−1*'s output. Pass 0 = the real composited screen. |
| `niri_source` / `tex2D_source` | the **original** composited screen, unfiltered — the same for every pass. |
| `niri_prev` / `tex2D_prev` | this pass's **own** output from last frame (per-pass feedback). |
| `niri_screen_prev` / `tex2D_screen_prev` | the previous frame's real screen (frame-level, all passes). |
| `niri_buffer` / `tex2D_buffer` + `global_buffer` | this pass's own dedicated accumulator (see above), per pass. |

Because pass 0 reads the real screen as `niri_screen` (for pass 0, `niri_source` and `niri_screen`
both point to the real screen; later passes still see the original via `niri_source`), with
`niri_prev` being its own last output, **any existing single shader drops into a chain unchanged**,
and a chain of one pass behaves exactly like the single-shader form.

The whole-chain flags still live on the outer `global-shader {}` block: `reads-cursor` governs
whether cursor pixels appear in `niri_screen` for the entire chain, and `redraw` controls the
chain's scheduling.

Per-pass fields:

| Field | Type | Default | Description |
|---|---|---|---|
| `source` | string | — | Inline GLSL for this pass. Mutually exclusive with `path`. |
| `path` | string | — | Path to a `.frag` file for this pass. Mutually exclusive with `source`. |
| `mode` | string | the block's `mode` | API flavour for this pass: `"niri"` or `"hyprland"`. Lets you mix a hyprland pass into a niri chain. |

Notes: a multi-pass chain (two or more passes) is always whole-output — `cursor-radius` region
mode applies only to a single-pass effect. The chain is treated as animated if **any** pass uses
`niri_time`, feedback (`niri_prev`/`niri_screen_prev`), or a `global_buffer`. If any pass fails to
resolve or compile, the whole chain is disabled and the screen renders normally.

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
- **Whole-output only.** One global shader (or one multi-pass chain) applies to all outputs; per-output, per-window, or per-layer shaders are not supported. Multi-pass chains are supported (see [Multi-pass chains](#multi-pass-chains-pass)) but a chain of two or more passes is always whole-output (no `cursor-radius` region mode).
