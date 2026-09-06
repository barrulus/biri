# Per-output colour / shader rule

Date: 2026-09-06
Issue: barrulus/biri#35
Upstream demand: niri-wm/niri#4355 (saturation/vibrance), #4303 (user shaders on an output),
#4405 (grayscale/invert/night filter), all redirected to #913.

## Problem

Users want a persistent post-process pinned to one output: saturation for a washed-out laptop
panel, a night/temperature filter, or grayscale for accessibility. Grayscale in particular cannot
be expressed as a 1D gamma LUT, because it mixes channels.

biri can already do this, awkwardly: a `region-shader` with an `output "DP-1"` property and a
geometry covering the whole screen is a per-output shader. Two things are missing. The user must
know the output's pixel dimensions and restate them as geometry, and the user must write GLSL —
which the accessibility case specifically should not require.

## Goals

- An output-level shader rule that needs no geometry.
- Built-in colour filters (grayscale, invert, saturation, temperature) usable without writing GLSL.
- Named user presets and runtime toggle/cycle actions, mirroring the window-shader pair.
- No continuous-redraw cost for a static colour filter.

## Non-goals

- Any new rendering primitive. This reuses `ScopedShaderElement` unchanged.
- Extending presets to `global-shader` or window rules. The built-in table is written so that is a
  later one-line wiring change, but it is not done here.
- Replying to upstream #4405 with a pointer. Noted as a follow-up for the maintainer to decide.

## Config surface

### `output { shader { … } }`

A new `Option<OutputShaderPart>` child on `niri_config::output::Output`. The body is the source
spec region shaders already use (`source` / `path` / `mode` / `pass {}`), minus geometry, plus a
`preset` reference node.

```kdl
output "eDP-1" { shader { preset "grayscale" } }
output "DP-2"  { shader { preset "temperature" kelvin=4000 } }
output "HDMI-A-1" {
    shader {
        pass { preset "saturation" amount=1.4 }
        pass { preset "temperature" kelvin=4500 }
    }
}
```

Output matching goes through `Outputs::find(&OutputName)`, so it inherits make/model/serial
matching. This is deliberately better than `region-shader`, which compares a raw connector string.

### `output-shaders { preset "name" { … } }`

A top-level block mirroring `window-shaders`. Preset bodies are ordinary shader specs. Blocks
accumulate in config order, and that order is what `cycle-output-shader` walks.

```kdl
output-shaders {
    preset "night" { preset "temperature" kelvin=3200 }
    preset "mono"  { preset "grayscale" }
}
output "DP-2" { shader { preset "night" } }
```

## Preset resolution

One `preset` node, two scopes:

- **Top level of a `shader {}` body** — resolved against user `output-shaders` presets first, then
  the built-in table. A user preset shadows a built-in of the same name.
- **Inside a `pass {}`** — built-ins only.

The second rule is what makes recursion structurally impossible. A user preset body can only ever
reach built-ins, so resolution is one level deep by construction and needs no cycle detection. It
also keeps `resolve_scoped_pass_sources`'s signature unchanged, since built-in resolution is a pure
function of name and properties and needs no access to `Config`.

### Built-in table

Lives in a new `niri-config/src/shader_presets.rs`. Every generated source is niri-dialect,
premultiplied-alpha safe, and static.

| name | property | default | operation |
|---|---|---|---|
| `grayscale` | — | — | Rec.709 luma, `vec4(vec3(l), c.a)` |
| `invert` | — | — | `vec4(c.a - c.rgb, c.a)` |
| `saturation` | `amount` | `1.5` | `mix(vec3(luma), c.rgb, amount)`, clamped to `[0, a]` |
| `temperature` | `kelvin` | `4000` | per-channel constant scale |

Premultiplied-alpha reasoning, since it is the easy thing to get wrong:

- Luma is linear in rgb, so it applies to premultiplied values directly.
- Invert is `1 - rgb/a` unpremultiplied, which repremultiplies to exactly `a - rgb`.
- `mix` is linear, so saturation is safe; the clamp to `[0, a]` keeps the result a valid
  premultiplied colour when `amount > 1`.
- A per-channel constant scale is linear, so temperature is safe.

The Kelvin-to-RGB conversion (Tanner Helland approximation, normalised so no channel exceeds 1)
runs **in Rust at resolve time**, and the emitted GLSL is a single multiply by a `vec3` literal.
This keeps the shader trivial and makes `kelvin=6500` exactly identity.

### Error handling

- Unknown preset name → `warn!`, chain resolves empty, shader disabled. This matches the existing
  behaviour for an unreadable `path`.
- A property the named built-in does not use (`preset "grayscale" kelvin=4000`) → `warn!`, ignored.
- `mode` is ignored for presets; generated sources are always niri-dialect.

## Rendering

Reuse `ScopedShaderElement` as region shaders do, with `area` = the full output rect, `region_norm`
= `[0, 0, 1, 1]`, and `ScopedSource::Capture`. No new element type.

Push order in `render_for_output` (earlier push = topmost, as the pointer/global-shader interplay
at `src/niri.rs:4977` establishes):

```
global-shader   (topmost, pushed first)
output-shader   <- new
region-shaders
windows
```

A per-output colour filter therefore composites over any region shaders on that output, and a
global shader still sits over everything. This is what the accessibility case wants: grayscale on
`eDP-1` grays the region shaders too.

`scoped_shader_chains()` gains every `output.shader` chain plus every `output-shaders` preset chain.
Presets are compiled eagerly so `cycle-output-shader` never stalls on a recompile — the same
treatment `window_shaders` presets already get.

## Redraw

A new `output_shader_animate`, mirroring the `region_shader_animate` block at `src/niri.rs:5900`:
resolve the matched output's chain, run `GlobalShaderCaps::scan_chain(...).is_animating()`, and OR
it into that output's redraw decision.

Because every built-in is static, `RedrawMode::Auto` already declines to schedule continuous
redraws for them. This is the issue's stated open question, and it resolves by construction rather
than by new code — so it is covered by an assertion in the test plan, not by a claim.

## State and actions

`OutputShaderState { preset: Option<String>, disabled: bool }` on `OutputState` (`src/niri.rs:710`).
That map is mutated, not rebuilt, across config reload, so a toggle is not clobbered by an unrelated
config edit — the same guarantee `WindowShaderState` gives windows. State resets on unplug.

IPC:

```rust
Action::ToggleOutputShader { output: Option<String> }
Action::CycleOutputShader  { output: Option<String> }
```

Config binds follow the `FocusMonitor(String)` precedent rather than the `ById` pair used by the
window-shader actions, since the selector is a name rather than a u64:

```
ToggleOutputShader
ToggleOutputShaderOn(String)
CycleOutputShader
CycleOutputShaderOn(String)
```

`None` selects the focused output. `cycle` walks `default -> preset 1 -> … -> preset N -> default`
and clears `disabled`, reusing `WindowShaderState::cycle`'s semantics exactly; a preset name no
longer present in the config restarts at the first preset.

Also needed: `hotkey_overlay` display strings, and `niri msg action toggle-output-shader
--output DP-2`.

## Config reload

The gate at `src/niri.rs:1924` extends with the output shader specs and `output_shaders`, compared
as **just the shader fields** rather than whole `Output` structs — otherwise changing a scale or
mode would needlessly recompile every scoped shader. Stored preset names are re-resolved against
the new config there, as `update_shader_preset` does for windows.

## Testing

- Config parse: `output { shader }` and `output-shaders`, unit plus the `lib.rs` snapshot.
- Preset resolution: each built-in emits GLSL; `amount`/`kelvin` defaults and overrides;
  `temperature kelvin=6500` is identity; a user preset shadows a built-in; unknown name yields an
  empty chain; an unused property warns.
- `GlobalShaderCaps::scan_chain` reports every built-in as not-animating.
- `OutputShaderState` toggle and cycle, including a preset name that has vanished from config.
- The render path is not unit-testable, matching region shaders. Flagged for hardware verification.

## Docs

- `docs/wiki/Configuration:-Global-Shader.md` — an output-shader section and the built-in table.
- `docs/wiki/Configuration:-Outputs.md` — the `shader` child.
- `docs/wiki/Configuration:-Key-Bindings.md` — the two new actions.
- `resources/default-config.kdl` — a commented example, alongside the existing `window-shaders` one.
