# Shader bundle

A ready-to-use collection of shaders for biri's [post-process shader
system](../../docs/wiki/Configuration:-Global-Shader.md), collected from a live setup.
KDL presets either inline their GLSL or reference adjacent `.frag` files. The
window preset index uses installed paths, described below.

## Layout

| Directory | Kind | Wired via |
|---|---|---|
| `cursor/` | Full `global-shader {}` blocks: effects that follow the mouse (glow, comet, trail, ripple, spotlight, …). See the note on `cursor-radius` below. | `include` + symlink cycle (below) |
| `screen/` | Full `global-shader {}` blocks: whole-output colour grades (CRT, grayscale, vignette, warm tint). | same cycle, `screen` group |
| `close/` | `animations { window-open {} window-close {} }` blocks: a matched open/close pair per effect (whirlpool, melt, ripple, lightning). Each file **owns both nodes** — including it overrides any `window-open`/`window-close` set earlier in `config.kdl`. Nothing else in `animations` is touched. | `include "shaders/current.kdl"` + `scripts/shader-cycle` |
| `drag/` | Jelly, Taffy, and lateral wobble physics presets. | `include` |
| `focus-ring/` | Editable focus-ring GLSL, including inward-growing vines, faerie magic, liquid effects, and travelling sparks, plus optional presets. | `focus-ring { shader { path "…"; } }` or `include` |
| `window/` | Per-window `.frag` sources for `shader {}` in window rules (CRT, parchment, pixel mosaic, fisheye + RGB split, text-legibility for transparent terminals, shimmer, ripple drops, Rorschach ink, mercury sheen). | `window-rule { shader { path "…" } }` |
| `off.kdl` | No-op — linking `current.kdl` here disables the global shader. | |
| `scripts/` | The cycle scripts the includes/binds below rely on. They expect the bundle copied to `~/.config/biri/` (override with `$BIRI_CONFIG_DIR`). | |

### Focus rings

Copy the entire `focus-ring/` directory to your config directory. Include `focus-ring/rainbow-ripple.kdl` for a global ring, or assign its `.frag` files to individual applications through `window-rule { focus-ring { on; shader { path "focus-ring/rainbow-ripple.frag"; padding 14; }; }; }`. Paths resolve relative to the containing config/include. Saving a shader reloads it automatically; no rebuild is needed. `pulse.frag` is a simpler second effect requiring no extra padding.

See [the decoration shader contract](../../docs/wiki/Configuration:-Layout.md#custom-focus-ring-and-border-shaders). These shaders do not have the global shaders' full-screen or capture restrictions. Add `light spread=80 intensity=1.0 threshold=0.5` inside a decoration's `shader` block to spill its bright highlights onto nearby window content. `lightning.frag` uses a travelling blue-white pulse; at width 6, use `padding 24`. The same opt-in lighting works with your existing shader files and reloads from the config without rebuilding.

`fuse.frag` draws a stationary, irregular braided cord with a travelling orange-white ember, a charred trail and flying sparks. At width 6 use `padding 48` and `light spread=90 intensity=1.4 threshold=0.5`: the cord stays dim while the ember casts warm light on nearby windows. The cord stays within the nominal ring width, so width 6 fits 6-pixel layout gaps; `padding` reserves extra space for sparks and does not push the cord outward. The optional `focus-ring/fuse.kdl` preset enables this globally; for one application, use its focus-ring block in a window rule. Edit `FUSE_SECONDS` (seconds per lap), `FUSE_WANDER` (0–1) and `FUSE_BRIGHTNESS` (ember and sparks only) in the file to tune it. Brightness does not increase the unburnt cord's contribution, keeping it below the suggested light threshold. No compositor rebuild is needed to add this shader; the `light` setting requires the light-spill-capable binary.

Both travelling effects support **1–4 simultaneous heads**, evenly spaced around the same ring. Edit the constant near the top of the relevant `.frag` file and save; it reloads automatically:

```glsl
// lightning.frag
const int LIGHTNING_COUNT = 3;

// fuse.frag — burning tips, each with its own sparks
const int EMBER_COUNT = 3;
```

Both default to `1`; values below 1 or above 4 are clamped. Each head retains the same lap time and size, and the optional `light` pass illuminates every head. More fuse tips also generate more sparks. The fuse's cord remains below the recommended light threshold regardless of `FUSE_BRIGHTNESS`.

### Effects returned from Umbriel

The collection now includes `flowering-vine`, `faerie-magic`, `flowing-water`,
`neon-bleed`, `portal-lava`, `sentient-runner`, `sentient-spark`, and
`scribbling-pencils`. Each has an adjacent KDL preset. `rainbow-bleed` is the
inward-growing version of the rainbow ring; `rainbow-ripple` retains the original
appearance. Existing local lightning and fuse head counts are preserved.

The paired inner/outer effects use one ring shader in biri: `draw-inside true`
lets the shader paint over client content as well as outside it. This keeps the
same time, corner radius, and colour on both sides of the edge, independently of
the window's content shader. It follows focus and hides when maximized or
fullscreen. Other ring shaders remain hollow by default.

```kdl
include "shaders/focus-ring/flowering-vine.kdl"
include "shaders/drag/jelly.kdl"
```

`drag/jelly.kdl`, `drag/taffy.kdl`, and `drag/lateral-wobble.kdl` enable the ported
elastic sheet simulation during pointer moves. Include one, or configure it
inline as described in [drag physics](../../docs/wiki/Configuration:-Animations.md#drag-physics).
The whole live window and its decorations deform, then settle after release.
This is opt-in; it does not change window geometry or input regions.

`close/close-paper.kdl` adds the matching paper fall/unfurl and scrunch/fall
opening and closing animations.

New content shaders include weather, fire, rainbow waves and smoke, liquid glass,
flap boards, sentient circuits, colour-vision variants, and crumpled paper.
`window/presets.kdl` registers the content collection for biri's existing
window-shader actions. It assumes the bundle is installed at
`~/.config/niri/shaders`; adjust those paths if using the separate biri directory.
The imported versions also fix undefined descending `smoothstep` edges in the
older window shaders.

The source collection is Barrulus's `bumbriel` fork, from
`docs/examples/shaders` and `examples/shaders/barrulus` (including its working
paper shaders), adapted back to biri's `niri_*` uniforms and `global_color` entry
point. The solver is a Rust port of `umbrielfx/render/drag_physics.c`.

### `cursor-radius` and full-output cost

Only `cursor/adaptive.kdl` and `cursor/blueglow.kdl` declare `cursor-radius`, so only
those two shade a box around the cursor and leave the rest of the output
scanout-eligible. **The other cursor/global shaders here reshade the whole output every frame**, with
direct scanout disabled — the battery/GPU cost the
[wiki](../../docs/wiki/Configuration:-Global-Shader.md) warns about.

That is not an oversight you can fix by adding a radius: region mode only engages when the
shader uses the cursor, is not animating, and is a single pass (`src/niri.rs`, the
`cursor_radius` match). `comet*` are multi-pass; `ripple`, `shockwave`, `rainbow-tunnel*`
and `trail` animate via `niri_time` or the feedback buffer; and `spotlight` deliberately
dims the *whole* screen, so a box would leave everything outside it bright.

For the same reason, cursor shaders here do absolute-pixel math against
`niri_output_size`, never `niri_size` — `niri_size` is the *element* size, which shrinks to
the box in region mode, while `niri_cursor` is always in absolute output px.

## Wiring (symlink cycle system)

`~/.config/biri/` is a biri-specific config dir, kept separate so a vanilla niri on the
same machine keeps its own `~/.config/niri/`. biri does **not** find it on its own — it
resolves `$XDG_CONFIG_HOME/niri/config.kdl` by default — so launch it with
`NIRI_CONFIG=~/.config/biri/config.kdl` (or `niri -c ~/.config/biri/config.kdl`), and
export the same `NIRI_CONFIG` in your session. The cycle scripts read `$BIRI_CONFIG_DIR`
if you use a different path; they cannot read `NIRI_CONFIG` themselves, because niri unsets
it before spawning children (`src/main.rs`).

1. Copy (or symlink) this directory's contents into `~/.config/biri/global-shaders/` and
`~/.config/biri/shaders/`, such as by running the following commands: 

```bash
CONFIG_DIR="$HOME/.config/biri"
mkdir -p "$CONFIG_DIR"/{global-shaders,shaders,scripts}
cp -r resources/shaders/* "$CONFIG_DIR/global-shaders/"

find "$CONFIG_DIR/global-shaders" -type f -not -path "*/scripts/*" -print0 | while IFS= read -r -d '' f; do
    rel="$(realpath --relative-to="$CONFIG_DIR/global-shaders" "$f")"
    mkdir -p "$CONFIG_DIR/shaders/$(dirname "$rel")"
    ln -s "$f" "$CONFIG_DIR/shaders/$rel"
done

for f in "resources/shaders/scripts"/*; do
    cp "$f" "$CONFIG_DIR/scripts/"
done

ln -s "$CONFIG_DIR/global-shaders/off.kdl" "$CONFIG_DIR/global-shaders/current.kdl"
ln -s "$CONFIG_DIR/shaders/off.kdl" "$CONFIG_DIR/shaders/current.kdl"
```


2. After copying the files to their respective locations, give `config.kdl` two stable includes:

```kdl
include "global-shaders/current.kdl"   // -> cursor/*.kdl | screen/*.kdl | off.kdl
include "shaders/current.kdl"          // -> close-*.kdl

binds {
    Mod+3        { spawn "sh" "-c" r#"ln -sfn off.kdl "$HOME/.config/biri/global-shaders/current.kdl" && niri msg action load-config-file"#; }
    Mod+4        { spawn "~/.config/biri/scripts/global-shader-cycle" "cursor"; }
    Mod+Shift+4  { spawn "~/.config/biri/scripts/global-shader-cycle" "screen"; }
    Mod+Shift+S  { spawn "~/.config/biri/scripts/shader-cycle"; }
}
```

`current.kdl` is a symlink in each case; the scripts relink it and reload the config over
IPC, so switching shaders never edits `config.kdl`. Both run `niri msg action
load-config-file` (override the binary with `$NIRI`) and say so in the desktop
notification if that reload fails, rather than leaving the keybind looking like a no-op.

Includes are positional overrides: put them *after* the config they are meant to override.

Window shaders are wired directly, for example:

```kdl
window-rule {
    match app-id="foot"
    shader {
        path "~/.config/biri/global-shaders/window/crt.frag"
    }
}
```

See the [global shader docs](../../docs/wiki/Configuration:-Global-Shader.md) for the
`global_color` contract, available uniforms, multi-pass chains, and the performance notes.
