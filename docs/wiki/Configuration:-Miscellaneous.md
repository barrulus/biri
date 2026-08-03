This page documents all top-level options that don't otherwise have dedicated pages.

Here are all of these options at a glance:

```kdl
spawn-at-startup "waybar"
spawn-at-startup "alacritty"
spawn-sh-at-startup "qs -c ~/source/qs/MyAwesomeShell"

prefer-no-csd

screenshot-path "~/Pictures/Screenshots/Screenshot from %Y-%m-%d %H-%M-%S.png"

environment {
    QT_QPA_PLATFORM "wayland"
    DISPLAY null
}

cursor {
    xcursor-theme "breeze_cursors"
    xcursor-size 48

    hide-when-typing
    hide-after-inactive-ms 1000
}

overview {
    zoom 0.5
    backdrop-color "#262626"

    workspace-shadow {
        // off
        softness 40
        spread 10
        offset x=0 y=10
        color "#00000050"
    }
}

xwayland-satellite {
    // off
    path "xwayland-satellite"
}

clipboard {
    disable-primary
}

hotkey-overlay {
    skip-at-startup
    hide-not-bound
}

config-notification {
    disable-failed
}

blur {
    // off
    passes 3
    offset 3.0
    noise 0.02
    saturation 1.5
}
```

### `spawn-at-startup`

Add lines like this to spawn processes at niri startup.

`spawn-at-startup` accepts a path to the program binary as the first argument, followed by arguments to the program.

This option works the same way as the [`spawn` key binding action](./Configuration:-Key-Bindings.md#spawn), so please read about all its subtleties there.

```kdl
spawn-at-startup "waybar"
spawn-at-startup "alacritty"
```

Note that running niri as a systemd session supports xdg-desktop-autostart out of the box, which may be more convenient to use.
Thanks to this, apps that you configured to autostart in GNOME will also "just work" in niri, without any manual `spawn-at-startup` configuration.

### `spawn-sh-at-startup`

<sup>Since: 25.08</sup>

Add lines like this to run shell commands at niri startup.

The argument is a single string that is passed verbatim to `sh`.
You can use shell variables, pipelines, `~` expansion and everything else as expected.

See detailed description in the docs for the [`spawn-sh` key binding action](./Configuration:-Key-Bindings.md#spawn-sh).

```kdl
// Pass all arguments in the same string.
spawn-sh-at-startup "qs -c ~/source/qs/MyAwesomeShell"
```

### `prefer-no-csd`

This flag will make niri ask the applications to omit their client-side decorations.

If an application will specifically ask for CSD, the request will be honored.
Additionally, clients will be informed that they are tiled, removing some rounded corners.

With `prefer-no-csd` set, applications that negotiate server-side decorations through the xdg-decoration protocol will have focus ring and border drawn around them *without* a solid colored background.

> [!NOTE]
> Unlike most other options, changing `prefer-no-csd` will not entirely affect already running applications.
> It will make some windows rectangular, but won't remove the title bars.
> This mainly has to do with niri working around a [bug in SDL2](https://github.com/libsdl-org/SDL/issues/8173) that prevents SDL2 applications from starting.
>
> Restart applications after changing `prefer-no-csd` in the config to fully apply it.

```kdl
prefer-no-csd
```

### `screenshot-path`

Set the path where screenshots are saved.
A `~` at the front will be expanded to the home directory.

The path is formatted with `strftime(3)` to give you the screenshot date and time.

Niri will create the last folder of the path if it doesn't exist.

```kdl
screenshot-path "~/Pictures/Screenshots/Screenshot from %Y-%m-%d %H-%M-%S.png"
```

You can also set this option to `null` to disable saving screenshots to disk.

```kdl
screenshot-path null
```

### `environment`

Override environment variables for processes spawned by niri.

```kdl
environment {
    // Set a variable like this:
    // QT_QPA_PLATFORM "wayland"

    // Remove a variable by using null as the value:
    // DISPLAY null
}
```

Note that these variables do not propagate to the systemd global environment, so tools and applications started by systemd do not see them.
In particular, if you start a desktop shell like DankMaterialShell through systemd, then use its built-in application launcher, the apps won't see these environment variables.

If you want all processes to see the environment variables, you can set them in your login shell config instead (i.e. `~/.bash_profile`).
The `niri-session` shell script runs through the login shell and imports all environment variables to systemd before starting niri.
Keep in mind that all compositors will see variables set in the login shell, not just niri.

### `cursor`

Change the theme and size of the cursor as well as set the `XCURSOR_THEME` and `XCURSOR_SIZE` environment variables.

```kdl
cursor {
    xcursor-theme "breeze_cursors"
    xcursor-size 48
}
```

#### `hide-when-typing`

<sup>Since: 0.1.10</sup>

If set, hides the cursor when pressing a key on the keyboard.

> [!NOTE]
> This setting might interfere with games running in Wine in native Wayland mode that use mouselook, such as first-person games.
> If your character's point of view jumps down when you press a key and move the mouse simultaneously, try disabling this setting.

```kdl
cursor {
    hide-when-typing
}
```

#### `hide-after-inactive-ms`

<sup>Since: 0.1.10</sup>

If set, the cursor will automatically hide once this number of milliseconds passes since the last cursor movement.

```kdl
cursor {
    // Hide the cursor after one second of inactivity.
    hide-after-inactive-ms 1000
}
```

### `overview`

<sup>Since: 25.05</sup>

Settings for the [Overview](./Overview.md).

#### `zoom`

Control how much the workspaces zoom out in the overview.
`zoom` ranges from 0 to 0.75 where lower values make everything smaller.

```kdl
// Make workspaces four times smaller than normal in the overview.
overview {
    zoom 0.25
}
```

#### `backdrop-color`

Set the backdrop color behind workspaces in the overview.
The backdrop is also visible between workspaces when switching.

The alpha channel for this color will be ignored.

```kdl
// Make the backdrop light.
overview {
    backdrop-color "#777777"
}
```

You can also set the color per-output [in the output config](./Configuration:-Outputs.md#backdrop-color).

#### `workspace-shadow`

Control the shadow behind workspaces visible in the overview.

Settings here mirror the normal [`shadow` config in the layout section](./Configuration:-Layout.md#shadow), so check the documentation there.

Workspace shadows are configured for a workspace size normalized to 1080 pixels tall, then zoomed out together with the workspace.
Practically, this means that you'll want bigger spread, offset, and softness compared to window shadows.

```kdl
// Disable workspace shadows in the overview.
overview {
    workspace-shadow {
        off
    }
}
```

#### `zoom-presets`

Define a list of zoom levels to cycle through using the `overview-zoom-cycle` action.
If not set or empty, the cycle action does nothing.

```kdl
overview {
    zoom-presets 0.5 0.25 0.1
}
```

#### `consolidated-carousel`

On a multi-monitor setup, enables a single-screen carousel overview instead of niri's default per-output overview: zooming any one output's overview out far enough reveals the *other* outputs as a cover-flow style stack of panels receding to the sides, so you can browse and jump between every monitor's workspaces without leaving the output you're on.

`reveal-zoom` is the overview zoom level at which sibling outputs start to appear as panels (default `0.48`). `assembled-zoom` is the zoom level at which the reveal is complete and the whole ring is fully assembled (default `0.22`). Zooming out continuously between the two ramps the reveal from 0 to 1 — there's no snap or threshold, panels fade and slide in as you zoom. `assembled-zoom` must be smaller than `reveal-zoom`, and both must be strictly between 0 and 1; invalid values fall back to the defaults with a warning.

```kdl
overview {
    consolidated-carousel {
        reveal-zoom 0.48
        assembled-zoom 0.22
    }
}
```

Once revealed, rotate the ring (bring a different sibling output to the center) with:

- The normal focus-column-left/right binds (`Mod+Left`/`Mod+Right` by default)
- `Shift` + scroll wheel over the overview
- Clicking a side panel to bring it to the center

All three work at any overview zoom level — if you're zoomed in past `reveal-zoom`, triggering a rotation first pulls the zoom back out to `reveal-zoom` so the ring is visible, then rotates.

Rotating all the way onto a sibling output settles into the "lens": that output's own workspace strip takes over the center, live and interactive, while every other output (including the one you started on) recedes to the sides as ordinary panels. From the lens, clicking a window brings it to focus and closes the overview, jumping you straight to that window on its real output; pressing `Enter` does the same for whichever window is currently focused/hovered.

`activation-zoom` and `expand-zoom`, used by earlier iterations of this feature, have been removed — `reveal-zoom`/`assembled-zoom` replace them with the continuous ramp described above.

### `xwayland-satellite`

<sup>Since: 25.08</sup>

Settings for integration with [xwayland-satellite](https://github.com/Supreeeme/xwayland-satellite).

When a recent enough xwayland-satellite is detected, niri will create the X11 sockets and set `DISPLAY`, then automatically spawn `xwayland-satellite` when an X11 client tries to connect.
If Xwayland dies, niri will keep watching the X11 socket and restart `xwayland-satellite` as needed.
This is very similar to how built-in Xwayland works in other compositors.

`off` disables the integration: niri won't create an X11 socket and won't set the `DISPLAY` environment variable.

`path` sets the path to the `xwayland-satellite` binary.
By default, it's just `xwayland-satellite`, so it's looked up like any other non-absolute program name.

```kdl
// Use a custom build of xwayland-satellite.
xwayland-satellite {
    path "~/source/rs/xwayland-satellite/target/release/xwayland-satellite"
}
```

### `clipboard`

<sup>Since: 25.02</sup>

Clipboard settings.

Set the `disable-primary` flag to disable the primary clipboard (middle-click paste).
Toggling this flag will only apply to applications started afterward.

```kdl
clipboard {
    disable-primary
}
```

### `hotkey-overlay`

Settings for the "Important Hotkeys" overlay.

#### `skip-at-startup`

Set the `skip-at-startup` flag if you don't want to see the hotkey help at niri startup.

```kdl
hotkey-overlay {
    skip-at-startup
}
```

#### `hide-not-bound`

<sup>Since: 25.08</sup>

By default, niri will show the most important actions even if they aren't bound to any key, to prevent confusion.
Set the `hide-not-bound` flag if you want to hide all actions not bound to any key.

```kdl
hotkey-overlay {
    hide-not-bound
}
```

You can customize which binds the hotkey overlay shows using the [`hotkey-overlay-title` property](./Configuration:-Key-Bindings.md#custom-hotkey-overlay-titles).

### `config-notification`

<sup>Since: 25.08</sup>

Settings for the config created/failed notification.

Set the `disable-failed` flag to disable the "Failed to parse the config file" notification.
For example, if you have a custom one.

```kdl
config-notification {
    disable-failed
}
```

### `blur`

<sup>Since: 26.04</sup>

Blur configuration that affects all background blur.

See the [window effects page](./Window-Effects.md) for an overview of background effects.

```kdl
// These are the default values:
blur {
    // off
    passes 3
    offset 3
    noise 0.02
    saturation 1.5
}
```

#### `off`

By default, blur is available on request by a window or layer surface (via the `ext-background-effect` protocol).
You can also enable it manually with the `blur true` background effect [window](./Configuration:-Window-Rules.md#background-effect) or [layer](./Configuration:-Layer-Rules.md#background-effect) rule.

Setting the `off` flag will disable all blur, both requested by the window, and configured in window rules.

```kdl
blur {
    off
}
```

#### `passes` and `offset`

`passes` controls the number of downsample/upsample passes for dual kawase blur.
More passes produce a larger, smoother blur, but cost more GPU resources.

`offset` is the pixel offset multiplier for each pass.
Offset `1` is the original dual kawase blur.
Larger values produce a smoother blur, at no additional GPU cost.

However, setting `offset` too big will produce visual artifacts.
You will need to increase `passes` to be able to use a bigger `offset` without artifacts.

When configuring blur, try increasing `offset` first (since it doesn't cause any extra GPU load) until you start getting artifacts.
Then, if you still need smoother blur, increase `passes` by 1.
Keep doing this until you get the desired visuals. 

```kdl
blur {
    passes 3
    offset 3.0
}
```

#### `noise`

Amount of noise to add on top of the blur.

This is helpful to reduce color banding artifacts.

```kdl
blur {
    noise 0.02
}
```

#### `saturation`

Color saturation applied to the blurred background.

Values above `1` increase saturation; values below `1` reduce it.

```kdl
blur {
    saturation 1.5
}
```
