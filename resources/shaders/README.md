# Shader bundle

A ready-to-use collection of shaders for biri's [post-process shader
system](../../docs/wiki/Configuration:-Global-Shader.md), collected from a live setup.
Everything here is self-contained — the `.kdl` files inline their GLSL, and the `.frag`
files are plain fragment shader sources referenced by `path` from window rules.

## Layout

| Directory | Kind | Wired via |
|---|---|---|
| `cursor/` | Full `global-shader {}` blocks: effects that follow the mouse (glow, comet, trail, ripple, spotlight, …). Most declare `cursor-radius`, so only a box around the cursor reshades and the rest of the output stays scanout-eligible. | `include` + symlink cycle (below) |
| `screen/` | Full `global-shader {}` blocks: whole-output colour grades (CRT, grayscale, vignette, warm tint). | same cycle, `screen` group |
| `close/` | `animations { window-close {} }` blocks with custom close shaders (whirlpool, melt, ripple, lightning). | `include "shaders/current.kdl"` + `scripts/shader-cycle` |
| `window/` | Per-window `.frag` sources for `shader {}` in window rules (CRT, parchment, pixel mosaic, fisheye + RGB split, text-legibility for transparent terminals, shimmer, ripple drops, Rorschach ink, mercury sheen). | `window-rule { shader { path "…" } }` |
| `off.kdl` | No-op — linking `current.kdl` here disables the global shader. | |
| `scripts/` | The cycle scripts the includes/binds below rely on. They expect the bundle copied to `~/.config/biri/`. | |

## Wiring (symlink cycle system)

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

`current.kdl` is a symlink in each case; the scripts relink it atomically and reload the
config over IPC, so switching shaders never edits `config.kdl`.

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
