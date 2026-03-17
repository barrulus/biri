# toggle-dwt: Implementation Checklist

Companion PR to [#3316 (toggle-touchpad)](https://github.com/niri-wm/niri/pull/3316).
Adds a `toggle-dwt` action to toggle disable-while-typing on/off at runtime via keybind or IPC.

---

## Pattern Reference (from toggle-touchpad PR)

The toggle-touchpad PR touches **6 files** with **+57 −4** lines. The DWT toggle follows the same pattern exactly, with one key difference: instead of toggling the `SendEventsMode` (on/off), you're toggling the libinput DWT enabled state.

---

## Files to Change

### 1. `niri-ipc/src/lib.rs` — IPC Action enum

**What toggle-touchpad did:** Added `ToggleTouchpad {}` variant with doc comment to the `Action` enum (~line 807).

**What to do:**
- [ ] Add `ToggleDwt {}` variant to `pub enum Action`
- [ ] Doc comment: `/// Toggle disable-while-typing on/off at runtime.`
- [ ] Place it adjacent to `ToggleTouchpad {}` for grouping

### 2. `niri-config/src/binds.rs` — Config Action enum + IPC mapping

**What toggle-touchpad did:**
- Added `ToggleTouchpad` to `pub enum Action` (~line 108)
- Added `niri_ipc::Action::ToggleTouchpad {} => Self::ToggleTouchpad` in the `From<niri_ipc::Action>` impl (~line 661)

**What to do:**
- [ ] Add `ToggleDwt` variant to `pub enum Action`
- [ ] Add mapping in `From<niri_ipc::Action>`: `niri_ipc::Action::ToggleDwt {} => Self::ToggleDwt`

### 3. `src/niri.rs` — Runtime state field

**What toggle-touchpad did:**
- Added `pub touchpad_disabled_by_toggle: bool` field to `pub struct Niri` (~line 263)
- Initialised it to `false` in `Niri::new()` (~line 2459)
- Passed it through to `apply_libinput_settings()` in the config-reload path (~line 1636)

**What to do:**
- [ ] Add `pub dwt_disabled_by_toggle: bool` field to `pub struct Niri`
- [ ] Initialise to `false` in `Niri::new()`
- [ ] Pass it through to `apply_libinput_settings()` in the config-reload path

### 4. `src/input/mod.rs` — Action handler + libinput application

This is the meatiest file. Three touch points:

#### 4a. `apply_libinput_settings()` signature & DWT logic

**What toggle-touchpad did:** Added `touchpad_disabled_by_toggle: bool` param, then combined it with `c.off`:
```rust
let is_off = c.off || touchpad_disabled_by_toggle;
```

**What to do:**
- [ ] Add `dwt_disabled_by_toggle: bool` param to `apply_libinput_settings()`
- [ ] In the touchpad branch, where DWT is applied (currently something like `device.config_dwt_set_enabled(c.dwt)`), combine with the toggle:
```rust
let dwt_enabled = c.dwt && !dwt_disabled_by_toggle;
let _ = device.config_dwt_set_enabled(dwt_enabled);
```

> **Note on semantics:** The toggle-touchpad PR uses OR logic (`off || toggled`) because both conditions *disable*. For DWT, the config `dwt` flag *enables* DWT, so the toggle should override it off. The logic is: DWT is enabled only if config says `dwt` AND the toggle hasn't disabled it. This matches Fireye04's use case — they want DWT on normally but toggled off for gameplay.

#### 4b. Device added handler (~line 229)

**What toggle-touchpad did:** Passed `self.niri.touchpad_disabled_by_toggle` to `apply_libinput_settings()` on device add.

**What to do:**
- [ ] Also pass `self.niri.dwt_disabled_by_toggle` in the same call

#### 4c. Action handler (~line 722)

**What toggle-touchpad did:** Added `Action::ToggleTouchpad` arm that flips the bool and re-applies settings to all touchpads.

**What to do:**
- [ ] Add `Action::ToggleDwt` arm:
```rust
Action::ToggleDwt => {
    self.niri.dwt_disabled_by_toggle = !self.niri.dwt_disabled_by_toggle;
    let config = self.niri.config.borrow();
    for mut device in self.niri.devices.iter().cloned() {
        if device.config_tap_finger_count() > 0 {
            apply_libinput_settings(
                &config.input,
                &mut device,
                self.niri.touchpad_disabled_by_toggle,
                self.niri.dwt_disabled_by_toggle,
            );
        }
    }
}
```

> **Note:** If the toggle-touchpad PR hasn't landed yet, your branch will need to add the `touchpad_disabled_by_toggle` param too (or just default it to `false`). If it has landed, you just extend the existing signature.

### 5. `docs/wiki/Configuration:-Input.md` — Input docs

**What toggle-touchpad did:** Added a tip box under the `off` setting pointing to the toggle-touchpad action.

**What to do:**
- [ ] Add a similar tip under the `dwt` setting:
```markdown
> [!TIP]
> <sup>Since: next release</sup> You can also toggle DWT on/off at runtime
> using the [`toggle-dwt` action](./Configuration:-Key-Bindings.md#toggle-dwt),
> without editing the config.
```

### 6. `docs/wiki/Configuration:-Key-Bindings.md` — Keybinding docs

**What toggle-touchpad did:** Added a `#### toggle-touchpad` section at the end with description, example bind, and behaviour notes.

**What to do:**
- [ ] Add `#### toggle-dwt` section:
```markdown
#### `toggle-dwt`

<sup>Since: next release</sup>

Toggle disable-while-typing on or off at runtime.
This provides a quick way to disable DWT without editing the config file.

    ```kdl
    binds {
        Mod+F10 { toggle-dwt; }
    }
    ```

The toggle state is combined with the [`dwt` setting](./Configuration:-Input.md#pointing-devices)
in the config: both the config and the toggle must enable DWT for it to be active.
The toggle state resets when niri restarts, and applies to newly hot-plugged touchpads.
```

---

## Design Decision: Shared Signature vs Separate Params

If both PRs land, `apply_libinput_settings` will have two toggle bools:

```rust
pub fn apply_libinput_settings(
    config: &niri_config::Input,
    device: &mut input::Device,
    touchpad_disabled_by_toggle: bool,
    dwt_disabled_by_toggle: bool,
)
```

This is fine for two toggles. If a third arrives (e.g. `toggle-dwtp`, `toggle-natural-scroll`), it would be worth refactoring to a struct:

```rust
pub struct InputToggleOverrides {
    pub touchpad_off: bool,
    pub dwt_off: bool,
    // future: dwtp_off, natural_scroll_off, etc.
}
```

But that's a future concern — keep it simple for now.

---

## Test Plan

- [ ] Bind `toggle-dwt` to a key, verify DWT toggles off (touchpad works while typing) and back on
- [ ] Test via IPC: `niri msg action toggle-dwt`
- [ ] Verify interaction with config `dwt` setting (toggle has no effect if dwt is not set in config)
- [ ] Hot-plug a touchpad while DWT is toggled off, verify DWT stays off
- [ ] Verify `toggle-touchpad` and `toggle-dwt` work independently

---

## PR Description Template

```markdown
## Summary

* Adds a new `toggle-dwt` action to toggle disable-while-typing on/off at runtime
* Can be bound to a key or invoked via IPC with `niri msg action toggle-dwt`
* The toggle state is combined with the config's `dwt` setting — both must
  enable DWT for it to be active

Companion to #3316 — extends the same runtime toggle pattern to DWT.

Requested by @Fireye04 in #3316 (comment).

## Notes

* State resets on restart
* Applies to newly hot-plugged touchpads

## Test plan

* Bind `toggle-dwt` to a key and verify it toggles DWT
* Test via IPC: `niri msg action toggle-dwt`
* Verify interaction with config `dwt` setting
* Hot-plug a touchpad while toggled off and verify it stays off
```
