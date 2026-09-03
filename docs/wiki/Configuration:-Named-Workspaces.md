### Overview

<sup>Since: 0.1.6</sup>

You can declare named workspaces at the top level of the config:

```kdl
workspace "browser"

workspace "chat" {
    open-on-output "Some Company CoolMonitor 1234"
}
```

Contrary to normal dynamic workspaces, named workspaces always exist, even when they have no windows.
Otherwise, they behave like any other workspace: you can move them around, move to a different monitor, and so on.

Actions like `focus-workspace` or `move-column-to-workspace` can refer to workspaces by name.
Also, you can use an `open-on-workspace` window rule to make a window open on a specific named workspace:

```kdl
// Declare a workspace named "chat" that opens on the "DP-2" output.
workspace "chat" {
    open-on-output "DP-2"
}

// Open Fractal on the "chat" workspace, if it runs at niri startup.
window-rule {
    match at-startup=true app-id=r#"^org\.gnome\.Fractal$"#
    open-on-workspace "chat"
}
```

Named workspaces initially appear in the order they are declared in the config file.
When editing the config while niri is running, newly declared named workspaces will appear at the very top of a monitor.

If you delete some named workspace from the config, the workspace will become normal (unnamed), and if there are no windows on it, it will be removed (as any other normal workspace).
There's no way to give a name to an already existing workspace, but you can simply move windows that you want to a new, empty named workspace.

<sup>Since: 0.1.9</sup> `open-on-output` can now use monitor manufacturer, model, and serial.
Before, it could only use the connector name.

<sup>Since: 25.01</sup> You can use `set-workspace-name` and `unset-workspace-name` actions to change workspace names dynamically.

<sup>Since: 25.02</sup> Named workspaces no longer update/forget their original output when opening a new window on them (unnamed workspaces will keep doing that).
This means that named workspaces "stick" to their original output in more cases, reflecting their more permanent nature.
Explicitly moving a named workspace to a different monitor will still update its original output.

### Layout config overrides

<sup>Since: 25.11</sup>

You can customize layout settings for named workspaces with a `layout {}` block:

```kdl
workspace "aesthetic" {
    // Layout config overrides just for this named workspace.
    layout {
        gaps 32

        struts {
            left 64
            right 64
            bottom 64
            top 64
        }

        border {
            on
            width 4
        }

        // ...any other setting.
    }
}
```

It accepts all the same options as [the top-level `layout {}` block](./Configuration:-Layout.md), except:

- `empty-workspace-above-first`: this is an output-level setting, doesn't make sense on a workspace.
- `insert-hint`: currently we always draw these at the output level, so it's not customizable per-workspace.

In order to unset a flag, write it with `false`, e.g.:

```kdl
layout {
    // Enabled globally.
    always-center-single-column
}

workspace "uncentered" {
    layout {
        // Unset on this workspace.
        always-center-single-column false
    }
}
```

### Hidden Workspaces

<sup>biri only</sup>

A named workspace can be hidden.
It keeps its windows, but disappears from the workspace strip, the overview and workspace switching until it is shown again.

Declare a workspace hidden at startup with `hidden true`:

```kdl
workspace "scratch" {
    hidden true
}
```

Three actions change visibility at runtime.
All of them take the workspace name and are available both as binds and through `niri msg action`:

- `toggle-workspace-visibility "name"`: hide the workspace if it is visible, show it if it is hidden.
- `hide-workspace "name"`: hide the workspace. Does nothing if it is already hidden.
- `unhide-workspace "name"`: show the workspace. Does nothing if it is already visible.

`toggle-workspace-visibility` and `unhide-workspace` accept a `focus=true` property (`--focus true` on the command line) to also focus the workspace once it is visible:

```kdl
binds {
    // Show scratch and jump to it; hide it again on the next press.
    Mod+Slash { toggle-workspace-visibility "scratch" focus=true; }
    Mod+Shift+Slash { hide-workspace "scratch"; }
}
```

```sh
niri msg action unhide-workspace scratch --focus true
```

Focusing a hidden workspace by name with `focus-workspace "name"` shows it temporarily: it hides itself again as soon as you switch away.
Use `unhide-workspace` (or `toggle-workspace-visibility`) when you want it to stay visible.

`niri msg workspaces` reports the hidden state of each workspace in the `is_hidden` field, so scripts can decide between `hide-workspace` and `unhide-workspace` instead of toggling.
