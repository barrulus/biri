<h1 align="center"><img alt="niri" src="https://github.com/user-attachments/assets/07d05cd0-d5dc-4a28-9a35-51bae8f119a0"></h1>
<p align="center">A scrollable-tiling Wayland compositor.</p>
<p align="center">
    <a href="https://matrix.to/#/#niri:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/matrix-%23niri-blue?logo=matrix"></a>
    <a href="https://github.com/niri-wm/niri/blob/main/LICENSE"><img alt="GitHub License" src="https://img.shields.io/github/license/niri-wm/niri"></a>
    <a href="https://github.com/niri-wm/niri/releases"><img alt="GitHub Release" src="https://img.shields.io/github/v/release/niri-wm/niri?logo=github"></a>
</p>

<p align="center">
    <a href="https://niri-wm.github.io/niri/Getting-Started.html">Getting Started</a> | <a href="https://niri-wm.github.io/niri/Configuration%3A-Introduction.html">Configuration</a> | <a href="https://github.com/niri-wm/niri/discussions/325">Setup&nbsp;Showcase</a>
</p>

<img width="1280" height="720" alt="niri with a few windows open" src="https://github.com/user-attachments/assets/dea5909e-1859-4aaa-9d88-d37f9663e00b" />

> [!IMPORTANT]
> **This is biri, a custom fork of [niri](https://github.com/niri-wm/niri).**
>
> It tracks upstream niri and adds a set of extra features on top: vertical scrolling for portrait outputs, GPU post-process shaders (global, per-region, and per-window), file-based animated focus-ring shaders, a consolidated multi-monitor carousel overview, dynamic overview zoom presets, isolated "signage" outputs, and runtime touchpad/DWT toggles.
> See [Fork Features](#fork-features) below for the full list.
>
> Everything documented for upstream niri still applies. Bugs you hit here should be reported to this fork, not to upstream niri.

> [!WARNING]
> **Branch rename (2026-09-03).** The fork's working branch is now `main` (formerly `barrulus-custom`), and the pristine upstream mirror is now `upstream` (formerly `main`).
> Open pull requests against `main`. If you track this repo, update flake inputs, package scripts, and clones from `barrulus/biri/barrulus-custom` to `barrulus/biri/main`. The old `barrulus-custom` branch has been removed.

https://github.com/user-attachments/assets/a5e2b72b-a5a3-4aab-a83d-51973a75f6cc

## Fork Features

These exist only in biri, not in upstream niri. Unless noted, each is off by default and inert when unconfigured.

A standard install keeps the upstream command names, including `niri` and `niri-session`, and reads `~/.config/niri/config.kdl` (`$XDG_CONFIG_HOME/niri/config.kdl` if set). The examples below use that default location. A different config path must be selected explicitly with `niri -c /path/to/config.kdl` or `NIRI_CONFIG`.

### Vertical scrolling for portrait outputs

Use `main-axis "vertical"` to arrange windows in rows that scroll top-to-bottom. Configure it per output to keep a portrait monitor scrolling vertically alongside a landscape monitor scrolling horizontally:

```kdl
output "DP-2" {
    layout {
        main-axis "vertical"
        default-column-width { proportion 0.5; }
    }
}
```

Replace `DP-2` with your output name. In vertical mode, `default-column-width` sets row height; `proportion 0.5` fits two rows in the visible area. Window content stays upright, and directional focus and move shortcuts keep their screen directions.

You can also set the axis globally or per named workspace, and changes apply on config reload. See [Layout: main-axis](./docs/wiki/Configuration:-Layout.md#main-axis) for sizing, gestures and workspace behavior.

### Post-process shaders

A GLSL fragment shader pipeline layered on top of niri's rendering, in three scopes that can all be active at once:

- **[`global-shader`](./docs/wiki/Configuration:-Global-Shader.md)** — a full-screen post-process pass over the whole composited output: colour grading, CRT scanlines, night-light tints, motion-blur trails, and so on. TTY/DRM backend only.
- **`region-shader`** — the same shader contract scoped to a fixed screen rectangle, optionally pinned to one output. Repeatable.
- **`shader {}` in a `window-rule`** — a shader applied to a single window's content, with borders and shadows rendered outside it. Animated, and only redrawn while the window is actually visible.

Supporting machinery:

- Named **`window-shaders` presets** driven by the `toggle-window-shader` and `cycle-window-shader` binds: flip the focused window's shader off/on, or rotate it through your presets at runtime — no config editing or reload needed.
- **Per-output colour filters**: `output "eDP-1" { shader { preset "grayscale"; }; }` — built-in grayscale, invert, saturation and temperature filters with no GLSL to write, plus `toggle-output-shader` and `cycle-output-shader` binds. Answers upstream niri #4355, #4303 and #4405.

- Two API flavours: a native `niri` mode and a `hyprland` mode that accepts most Hyprland `screen_shader` files with light edits.
- Multi-pass chains via repeatable `pass {}` blocks, where each pass reads the previous pass's output.
- A previous-frame feedback buffer (`niri_prev` / `tex2D_prev`) plus a dedicated `global_buffer` pass for trails and accumulation effects.
- Redraw scheduling (`redraw "auto" | "on-damage" | "continuous"`) so a static shader doesn't force a continuous redraw loop, and `cursor-radius` to reshade only a box around the cursor and keep the rest of the output scanout-eligible.
- `shader-animation-max-fps` to cap shader-driven redraws independently of the output refresh rate.
- `shaders-in-capture` (top-level flag) to opt shader output into portal screencasts and screencopy; by default shaders stay a local display effect and never leak into shared or recorded content.
- Hot-reload on config reload; a shader that fails to compile logs a warning and leaves the screen rendering normally.

Note the cost: an active global shader disables direct scanout and redraws the whole output every frame. The [shader documentation](./docs/wiki/Configuration:-Global-Shader.md) covers this in detail.

### Custom focus-ring shaders

Load a GLSL file for a focus ring or border, with a different effect for each application. Shader files reload automatically when saved; adding or changing an effect needs no compositor rebuild.

Copy [`resources/shaders/focus-ring/`](./resources/shaders/focus-ring) to `~/.config/niri/focus-ring/`, then add a window rule:

```kdl
window-rule {
    match app-id=r#"^com\.mitchellh\.ghostty$"#
    focus-ring {
        on
        width 6
        shader {
            path "~/.config/niri/focus-ring/rainbow-ripple.frag"
            padding 14
        }
    }
}
```

The supplied rainbow shader has flowing pastel colours, uneven wax-like edges, and drifting highlights. Edit its `.frag` file to change the material, or point another window rule at `pulse.frag` or your own shader. Rings stay hollow behind transparent windows; extra drawing space from `padding` does not change window sizes. The same `shader` block works in `layout { focus-ring { ... } }` and in borders.

For light spilling onto the focused window and nearby windows, add `light` inside the existing shader block. It follows the shader's actual bright spots, including the travelling pulse in `lightning.frag`:

```kdl
shader {
    path "~/.config/niri/focus-ring/lightning.frag"
    padding 24
    light spread=80 intensity=1.0 threshold=0.5
}
```

The bundled [`fuse.frag`](./resources/shaders/focus-ring/fuse.frag) follows an irregular braided cord with a burning ember, ash and sparks. At width 6, use `padding 48` and `light spread=90 intensity=1.4 threshold=0.5` for warm illumination. Set `EMBER_COUNT` in `fuse.frag` or `LIGHTNING_COUNT` in `lightning.frag` to 1–4 for multiple travelling tips or pulses; saving the file reloads the effect.

`spread` controls the glow's reach, `intensity` its brightness, and `threshold` excludes dim parts of the ring. This is a soft screen-space lighting effect; shader files and lighting settings remain editable without rebuilding.

Animation respects `shader-animation-max-fps`, and static shaders can use `animated false`. Invalid shaders log an error and fall back to configured colours. See [Custom focus-ring and border shaders](./docs/wiki/Configuration:-Layout.md#custom-focus-ring-and-border-shaders) for the shader contract, reload behaviour, and examples. The original `rainbow-ripple` configuration remains supported.

### Consolidated carousel overview

`overview { consolidated-carousel { ... } }` replaces the per-monitor overview on multi-monitor setups with a single-screen, cover-flow style browser. Zooming one output's overview out past `reveal-zoom` continuously reveals the *other* outputs as perspective panels receding to the sides, fully assembled by `assembled-zoom` — no snapping or thresholds.

Rotate the ring with the normal `focus-column-left`/`right` binds, `Shift`+scroll, or by clicking a side panel; any of these work at any zoom level, pulling out to show the ring and then returning to where you were. Rotating a sibling output into the centre gives you its live, interactive workspace strip ("the lens"), and clicking a window there — or pressing `Enter` — jumps focus straight to that window on its real output.

See [Configuration: Miscellaneous](./docs/wiki/Configuration:-Miscellaneous.md#consolidated-carousel).

### Dynamic overview zoom

`overview { zoom-presets 0.5 0.25 0.1 }` plus three new actions — `overview-zoom-in`, `overview-zoom-out`, and `overview-zoom-cycle` — let you move between zoom levels while in the [Overview](./docs/wiki/Overview.md#dynamic-zoom). The zoom actions also open and close the overview at the ends of the range, so two binds (typically `Mod`+scroll) cover the whole interaction. Transitions are animated via the new `overview-zoom` [animation](./docs/wiki/Configuration:-Animations.md#overview-zoom).

### Isolated outputs

An `isolated` flag on an [output](./docs/wiki/Configuration:-Outputs.md#isolated) keeps compositor UI — the overview, the Alt-Tab switcher, the hotkey overlay, and the config error notification — off that screen, for projection, digital signage, shop-front visuals, or a clean capture feed. Windows and layer-shell surfaces still render normally. `power-off-monitors skip-isolated=true` leaves isolated outputs lit while the rest sleep on idle.

### Runtime input toggles

- [`toggle-touchpad`](./docs/wiki/Configuration:-Key-Bindings.md#toggle-touchpad) — enable/disable the touchpad without editing the config.
- [`toggle-dwt`](./docs/wiki/Configuration:-Key-Bindings.md#toggle-dwt) — same for disable-while-typing.

Both compose with the config settings (config and toggle must agree), reset on restart, and apply to newly hot-plugged touchpads.

### Fast-tracked upstream PRs

Open niri pull requests merged here ahead of upstream, several of them originally combined in [niri-qol](https://github.com/AmmoniumX/niri-qol) (now absorbed into this fork):

- **Hidden workspaces** ([niri#2997](https://github.com/niri-wm/niri/pull/2997)) — a named workspace can be hidden: it keeps its windows but disappears from the workspace strip, the overview, and workspace switching until toggled back. Declare it hidden at startup with `workspace "name" { hidden true }`, or change it at runtime with the [`toggle-workspace-visibility`, `hide-workspace` and `unhide-workspace`](./docs/wiki/Configuration:-Named-Workspaces.md#hidden-workspaces) actions (binds or `niri msg action`; add `focus=true` to jump to the workspace as it appears). Hidden workspaces also stay out of the consolidated carousel's panels.
- **Sticky floating windows** ([niri#3302](https://github.com/niri-wm/niri/pull/3302)) — floating windows that follow you across all workspaces of their output. Set [`open-sticky true`](./docs/wiki/Configuration:-Window-Rules.md#open-sticky) in a window rule (implies `open-floating`), or toggle any floating window with the [`toggle-window-sticky`](./docs/wiki/Configuration:-Key-Bindings.md#toggle-window-sticky) bind. In the consolidated carousel, sticky windows show on every workspace panel, and clicking one in the lens focuses it.
- **Virtual outputs** ([niri#3800](https://github.com/niri-wm/niri/pull/3800)) — outputs that exist without a physical monitor, for a headless session over SSH, a Sunshine/Moonlight streaming target, a tablet used as a second screen, or a wayvnc target. Create and remove them at runtime with `niri msg output`, or declare them in config with `output "name" { create-virtual ... }`. Works on both the TTY and headless backends. See [Virtual Outputs](./docs/wiki/Virtual-Outputs.md) — one used for streaming usually wants the `isolated` flag too.
- **`float-above-fullscreen`** ([niri#4062](https://github.com/niri-wm/niri/pull/4062)) — a [window rule](./docs/wiki/Configuration:-Window-Rules.md#float-above-fullscreen) that keeps a floating window visible on top when a fullscreen window occupies the workspace. Off by default.
- **Per-keyboard configuration** ([niri#4459](https://github.com/niri-wm/niri/pull/4459)) — give a `keyboard` block a device name to configure one physical keyboard: `keyboard "Logitech USB Receiver" { xkb { layout "gb" } }` next to the unnamed block. A named block inherits anything it leaves unset from the unnamed one, and the keymap follows whichever keyboard you last typed on, so an external board can run a different layout or options than the built-in one without a switching script. Num lock, locale1 settings and the IPC layout event survive the switch (fixed on top of the upstream PR). See [Per-Keyboard Configuration](./docs/wiki/Configuration:-Input.md#per-keyboard-configuration).

```kdl
workspace "scratch" {
    hidden true
}

window-rule {
    match app-id="firefox$" title="^Picture-in-Picture$"
    open-sticky true
    float-above-fullscreen true
}

binds {
    Mod+H { toggle-workspace-visibility "scratch"; }
    Mod+S { toggle-window-sticky; }
}
```

If these land upstream, the upstream versions replace them here.

## About

By default, windows are arranged in columns on an infinite strip going to the right.
The fork's [vertical layout](#vertical-scrolling-for-portrait-outputs) arranges them in rows on a strip going down instead.
Opening a new window never causes existing windows to resize.

Every monitor has its own separate window strip.
Windows can never "overflow" onto an adjacent monitor.

Workspaces are dynamic and arranged vertically by default, or horizontally when viewing a vertical layout.
Every monitor has an independent set of workspaces, and there's always one empty workspace at the end.

The workspace arrangement is preserved across disconnecting and connecting monitors where it makes sense.
When a monitor disconnects, its workspaces will move to another monitor, but upon reconnection they will move back to the original monitor.

## Features

- Built from the ground up for scrollable tiling
- [Dynamic workspaces](https://niri-wm.github.io/niri/Workspaces.html) like in GNOME
- An [Overview](https://github.com/user-attachments/assets/379a5d1f-acdb-4c11-b36c-e85fd91f0995) that zooms out workspaces and windows
- Built-in screenshot UI
- Monitor and window screencasting through xdg-desktop-portal-gnome
    - You can [block out](https://niri-wm.github.io/niri/Configuration%3A-Window-Rules.html#block-out-from) sensitive windows from screencasts
    - [Dynamic cast target](https://niri-wm.github.io/niri/Screencasting.html#dynamic-screencast-target) that can change what it shows on the go
- [Touchpad](https://github.com/niri-wm/niri/assets/1794388/946a910e-9bec-4cd1-a923-4a9421707515) and [mouse](https://github.com/niri-wm/niri/assets/1794388/8464e65d-4bf2-44fa-8c8e-5883355bd000) gestures
- Group windows into [tabs](https://niri-wm.github.io/niri/Tabs.html)
- Configurable layout: gaps, borders, struts, window sizes
- [Gradient borders](https://niri-wm.github.io/niri/Configuration%3A-Layout.html#gradients) with Oklab and Oklch support
- [Background blur](https://niri-wm.github.io/niri/Window-Effects.html) for windows and layer-shell surfaces
- [Animations](https://github.com/niri-wm/niri/assets/1794388/ce178da2-af9e-4c51-876f-8709c241d95e) with support for [custom shaders](https://github.com/niri-wm/niri/assets/1794388/27a238d6-0a22-4692-b794-30dc7a626fad)
- Live-reloading config
- Works with [screen readers](https://niri-wm.github.io/niri/Accessibility.html)

## Video Demo

https://github.com/niri-wm/niri/assets/1794388/bce834b0-f205-434e-a027-b373495f9729

Also check out these videos that showcase a lot of the niri functionality:

- [Niri Is My New Favorite Wayland Compositor](https://www.youtube.com/watch?v=DeYx2exm04M) by Brodie Robertson
- [How Is niri This Good? Live Demo + Config](https://www.youtube.com/watch?v=7XmD5UyyhZQ) by Nick Janetakis

## Status

Niri is stable for day-to-day use and does most things expected of a Wayland compositor.
Many people are daily-driving niri, and are happy to help in our [Matrix channel].

Give it a try!
Follow the instructions on the [Getting Started](https://niri-wm.github.io/niri/Getting-Started.html) page.
Grab a desktop shell like [DankMaterialShell] or [Noctalia] (or build a more traditional setup): niri by itself is not a complete desktop environment.
Also check out [awesome-niri], a list of niri-related links and projects.

Here are some points you may have questions about:

- **Multi-monitor**: yes, a core part of the design from the very start. Mixed DPI works.
- **Fractional scaling**: yes, plus all niri UI stays pixel-perfect.
- **NVIDIA**: seems to work fine.
- **Floating windows**: yes, starting from niri 25.01.
- **Input devices**: niri supports tablets, touchpads, and touchscreens.
You can map the tablet to a specific monitor, or use [OpenTabletDriver].
We have touchpad gestures, but no touchscreen gestures yet.
- **Wlr protocols**: yes, we have most of the important ones like layer-shell, gamma-control, screencopy.
You can check on [wayland.app](https://wayland.app) at the bottom of each protocol's page.
- **Performance**: while I run niri on beefy machines, I try to stay conscious of performance.
I've seen someone use it fine on an Eee PC 900 from 2008, of all things.
- **Xwayland**: [integrated](https://niri-wm.github.io/niri/Xwayland.html#using-xwayland-satellite) via xwayland-satellite starting from niri 25.08.

## Media

[niri: Making a Wayland compositor in Rust](https://youtu.be/Kmz8ODolnDg?list=PLRdS-n5seLRqrmWDQY4KDqtRMfIwU0U3T) · *December 2024*

My talk from the 2024 Moscow RustCon about niri, and how I do randomized property testing and profiling, and measure input latency.
The talk is in Russian, but I prepared full English subtitles that you can find in YouTube's subtitle language selector.

[An interview with Ivan, the developer behind Niri](https://www.trommelspeicher.de/podcast/special_the_developer_behind_niri) · *June 2025*

An interview by a German tech podcast Das Triumvirat (in English).
We talk about niri development and history, and my experience building and maintaining niri.

[A tour of the niri scrolling-tiling Wayland compositor](https://lwn.net/Articles/1025866/) · *July 2025*

An LWN article with a nice overview and introduction to niri.

## Contributing

If you'd like to help with niri, there are plenty of both coding- and non-coding-related ways to do so.
See [CONTRIBUTING.md](https://github.com/niri-wm/niri/blob/main/CONTRIBUTING.md) for an overview.

For the fork-specific features listed above, open issues and pull requests against this repository rather than upstream niri.

## Inspiration

Niri is heavily inspired by [PaperWM] which implements scrollable tiling on top of GNOME Shell.

One of the reasons that prompted me to try writing my own compositor is being able to properly separate the monitors.
Being a GNOME Shell extension, PaperWM has to work against Shell's global window coordinate space to prevent windows from overflowing.

## Tile Scrollably Elsewhere

Here are some other projects which implement a similar workflow:

- [PaperWM]: scrollable tiling on top of GNOME Shell.
- [karousel]: scrollable tiling on top of KDE.
- [scroll](https://github.com/dawsers/scroll) and [papersway]: scrollable tiling on top of sway/i3.
- Hyprland has a built-in [scrolling layout](https://wiki.hypr.land/Configuring/Layouts/Scrolling-Layout/).
- [Paneru] and [PaperWM.spoon]: scrollable tiling on top of macOS.

## Contact

Our main communication channel is a Matrix chat, feel free to join and ask a question: https://matrix.to/#/#niri:matrix.org

We also have a community Discord server: https://discord.gg/vT8Sfjy7sx

[PaperWM]: https://github.com/paperwm/PaperWM
[waybar]: https://github.com/Alexays/Waybar
[fuzzel]: https://codeberg.org/dnkl/fuzzel
[awesome-niri]: https://github.com/niri-wm/awesome-niri
[karousel]: https://github.com/peterfajdiga/karousel
[papersway]: https://spwhitton.name/tech/code/papersway/
[Paneru]: https://github.com/karinushka/paneru
[PaperWM.spoon]: https://github.com/mogenson/PaperWM.spoon
[Matrix channel]: https://matrix.to/#/#niri:matrix.org
[OpenTabletDriver]: https://opentabletdriver.net/
[DankMaterialShell]: https://danklinux.com/
[Noctalia]: https://noctalia.dev/
