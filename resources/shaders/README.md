# Shader bundle

A ready-to-use collection of shaders for biri's [post-process shader
system](../../docs/wiki/Configuration:-Global-Shader.md), collected from a live setup.
Everything here is self-contained — the `.kdl` files inline their GLSL, and the `.frag`
files are plain fragment shader sources referenced by `path` from window rules.

## Layout

| Directory | Kind | Wired via |
|---|---|---|
| `cursor/` | Full `global-shader {}` blocks: effects that follow the mouse (glow, comet, trail, ripple, spotlight, …). See the note on `cursor-radius` below. | `include` + symlink cycle (below) |
| `screen/` | Full `global-shader {}` blocks: whole-output colour grades (CRT, grayscale, vignette, warm tint). | same cycle, `screen` group |
| `close/` | `animations { window-open {} window-close {} }` blocks: a matched open/close pair per effect (whirlpool, melt, ripple, lightning). Each file **owns both nodes** — including it overrides any `window-open`/`window-close` set earlier in `config.kdl`. Nothing else in `animations` is touched. | `include "shaders/current.kdl"` + `scripts/shader-cycle` |
| `window/` | Per-window `.frag` sources for `shader {}` in window rules (CRT, parchment, pixel mosaic, fisheye + RGB split, text-legibility for transparent terminals, shimmer, ripple drops, Rorschach ink, mercury sheen). | `window-rule { shader { path "…" } }` |
| `off.kdl` | No-op — linking `current.kdl` here disables the global shader. | |
| `scripts/` | The cycle scripts the includes/binds below rely on. They expect the bundle copied to `~/.config/biri/` (override with `$BIRI_CONFIG_DIR`). | |

### `cursor-radius` and full-output cost

Only `cursor/adaptive.kdl` and `cursor/blueglow.kdl` declare `cursor-radius`, so only
those two shade a box around the cursor and leave the rest of the output
scanout-eligible. **Every other shader here reshades the whole output every frame**, with
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
