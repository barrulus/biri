---
name: converting-global-shaders
description: Use when porting a Shadertoy or Hyprland screen_shader (or any GLSL fragment shader) into a biri/niri global-shader block, or writing a new cursor/post-process shader for the global-shader config. Covers the global_color contract, uniform mapping, the Y-flip, and the GLES2 dialect.
---

# Converting Shaders to the biri global-shader Format

## Overview

biri's `global-shader` runs one GLSL fragment shader over the whole composited output every frame. It has **two authoring modes**: `niri` (a guard-railed `global_color()` contract — preferred for new shaders and Shadertoy ports) and `hyprland` (a raw shader with its own `main()` — for dropping in Hyprland `screen_shader`s). All shaders compile as **GLES2 `#version 100`**: use `varying`, `gl_FragColor`, `texture2D` — never `in`/`out`/`texture()`/`fragColor`, and never write a `#version` line (the compiler prepends it).

## The niri-mode contract

You write exactly one function:

```glsl
vec4 global_color(vec3 c) { /* return the final pixel */ }
```

| Symbol | Type | Meaning |
|---|---|---|
| `c.xy` | vec2 | 0..1 across the output. **`c.y = 0` is the TOP.** |
| `niri_time` | float | seconds since the shader activated |
| `niri_size` | vec2 | output size in **physical pixels** |
| `niri_scale` | float | fractional output scale |
| `niri_cursor` | vec2 | cursor position in **output-local physical pixels** (`c.y=0` top, matches `c`) |
| `tex2D_screen(uv)` | vec4 | sample the composited frame below (uv 0..1, y-top) |
| `tex2D_prev(uv)` | vec4 | sample the **previous** output frame — for feedback/trails |

Pixel position of a fragment is `c.xy * niri_size`; distance to cursor is `length(c.xy * niri_size - niri_cursor)`. Work in pixel space for circular effects (it's already aspect-correct). `niri_alpha` is applied for you — just return the color.

## Porting a Shadertoy shader → niri mode

Shadertoy's coordinate origin is **bottom-left** (y up); ours is **top-left**. Wrap the body in this shim so the math stays internally consistent, then paste:

```glsl
vec4 global_color(vec3 c) {
    // --- Shadertoy compatibility shims (y-up) ---
    vec2  iResolution = niri_size;
    float iTime       = niri_time;
    vec2  fragCoord   = vec2(c.x, 1.0 - c.y) * niri_size;          // y-up pixel coord
    vec4  iMouse      = vec4(niri_cursor.x, niri_size.y - niri_cursor.y, 0.0, 0.0);
    vec2  uv          = fragCoord / iResolution;                   // y-up 0..1

    // --- paste the body of `mainImage(out vec4 fragColor, in vec2 fragCoord)` here ---
    // 1. rename `fragColor` -> `color`
    // 2. replace `texture(iChannel0, X)` with `tex2D_screen(vec2(X.x, 1.0 - X.y))`
    //    (or `tex2D_prev(...)` if iChannel0 was a feedback buffer)
    // 3. `iResolution.xy` already provided; `iTime`, `iMouse`, `uv`, `fragCoord` provided
    vec4 color = vec4(0.0);
    /* ...shader body... */
    return color;
}
```

Mapping summary: `iTime`→`niri_time`, `iResolution.xy`→`niri_size`, `iMouse.xy`→`niri_cursor` (y-flipped), `texture(iChannel0,X)`→`tex2D_screen(vec2(X.x,1.0-X.y))`. Drop unsupported channels (audio, video, cubemaps).

## Porting a Hyprland screen_shader → hyprland mode

Hyprland shaders bring their own `main()` writing `gl_FragColor`. The hyprland-mode prelude provides aliases: `tex` (= screen sampler), `v_texcoord`, `time`, `wl_output` (= 0). There is **no cursor** in hyprland mode.

- **GLES2-dialect shader** (`varying`, `gl_FragColor`, `texture2D`): paste verbatim into `source`, set `mode "hyprland"`.
- **`#version 300 es` shader**: edit before pasting — delete the `#version 300 es` line; `in vec2 v_texcoord` → `varying vec2 v_texcoord`; delete the `out vec4 fragColor` declaration and replace `fragColor` with `gl_FragColor`; `texture(` → `texture2D(`.

## Config block

```kdl
global-shader {
    enable
    source "vec4 global_color(vec3 c){ return tex2D_screen(c.xy); }"
    // path "~/.config/niri/shaders/foo.frag"   // alternative to source (mutually exclusive)
    // mode "niri"        // or "hyprland"
    // reads-cursor       // ONLY if the shader must transform the cursor sprite itself
}
```

The `source` string may span **multiple lines** (KDL allows literal newlines in quoted strings) — keep ported shaders readable rather than collapsing to one line. `source` and `path` are mutually exclusive. niri hot-reloads on **config** reload (editing a `path` file alone does not reload — re-save the config). A shader that fails to compile logs a warning and is disabled; it never crashes the compositor.

## Common mistakes

| Mistake | Fix |
|---|---|
| Used `texture()`, `in`/`out`, or `fragColor` | GLES2 only: `texture2D()`, `varying`, `gl_FragColor` |
| Added a `#version` line | Remove it — the compiler prepends `#version 100` |
| Effect is upside-down | Shadertoy/Hyprland y-up vs our y-top — apply the Y-flip on coords AND on screen-sampling uv |
| Cursor effect tracks inverted vertically | use `vec2(niri_cursor.x, niri_size.y - niri_cursor.y)` |
| Feedback shader blows out to white | decay prev and subtract the screen: `vec3 tp = max(tex2D_prev(c.xy).rgb - tex2D_screen(c.xy).rgb, 0.0) * 0.9;` then add fresh contribution |
| Expected cursor in hyprland mode | not available — use niri mode for cursor effects |
| Ovals instead of circles | work in pixel space (`c.xy * niri_size`), not in 0..1 `c.xy` |

## Limitations to state when sharing

TTY/DRM output only (no winit/nested). Excluded from screenshots and screen recordings by design — to capture the effect, use **KMS/scanout capture** (e.g. `gpu-screen-recorder -w <monitor>`), not portal/screencopy. Verified on `normal`-transform outputs; rotated/flipped outputs may mis-orient. While active: full-frame redraw every frame, no direct scanout — real GPU/battery cost.

## Worked example: Shadertoy radial warp → niri mode

Shadertoy source:
```glsl
void mainImage(out vec4 fragColor, in vec2 fragCoord){
    vec2 uv = fragCoord/iResolution.xy;
    vec2 d = uv - 0.5;
    uv += d * 0.1 * sin(iTime + length(d)*20.0);
    fragColor = texture(iChannel0, uv);
}
```
Converted:
```glsl
vec4 global_color(vec3 c){
    float iTime = niri_time;
    vec2 uv = vec2(c.x, 1.0 - c.y);             // y-up 0..1
    vec2 d = uv - 0.5;
    uv += d * 0.1 * sin(iTime + length(d)*20.0);
    return tex2D_screen(vec2(uv.x, 1.0 - uv.y)); // flip back to y-top for our sampler
}
```

See `docs/wiki/Configuration:-Global-Shader.md` for the full reference and more ready-made examples.
