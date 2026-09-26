# Terminal themes and Taffy

Add this at the end of `~/.config/biri/config.kdl`, after existing window rules:

```kdl
include "~/dev/biri/resources/shaders/terminals/all.kdl"
```

| Application | Content shader | Focus ring |
| --- | --- | --- |
| Foot, footclient, and `foot-*` | Sentient circuit v2 | Sentient runner |
| Ghostty | Prairie wind | Flowering vine, with inward growth and soft light |
| Kitty | Rainbow smoke | Lava portal, with inward growth |

All windows get Taffy pointer-drag physics. The themed rings appear on active
windows, following biri's focus-ring behaviour. The ordinary border is disabled
for these terminals to avoid a second frame underneath the themed ring. Content
shaders remain applied to inactive windows too.

`foot.kdl`, `ghostty.kdl`, and `kitty.kdl` can also be included separately.
`all.kdl` enables animations and includes `../drag/taffy.kdl` globally.

These personal presets use `~/dev/biri/resources/shaders/window/` for content
shader paths: scoped window shaders resolve paths from the process directory,
so relative paths would depend on how biri was launched. Decoration paths and
includes resolve relative to their KDL files. If you move the checkout, update
the three content paths and the main include.

Use the newly built biri binary: earlier builds do not recognise `draw-inside`
or `drag-physics`. Validate before reloading:

```sh
~/dev/biri/target/debug/niri validate -c ~/.config/biri/config.kdl
```

If you have selected or disabled a window shader using runtime actions, reset
that override to let the configured shader take effect, or open a new window.
