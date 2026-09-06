# Multi-Action Binds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let one keybind run several actions in order, and let one IPC request run several actions as a single uninterruptible batch.

**Architecture:** `Bind.action: Action` becomes `Bind.actions: Vec<Action>`, matching the shape upstream (YaLTeR, bbb651) already agreed on so this can be offered to niri later. Task 1 is a pure type refactor with no behaviour change; Task 2 turns on multi-action parsing; Tasks 3 and 4 add the IPC batch and its CLI front end. Every fold over the new list (`all` vs `any`) is specified explicitly, because two of them are security-relevant.

**Tech Stack:** Rust (nightly via devshell), `knuffel` for KDL config decoding, `clap` for the CLI, `serde` for the IPC protocol, `insta` for config snapshot tests.

**Spec:** `docs/superpowers/specs/2026-09-06-multi-action-binds-design.md`

## Global Constraints

- **Build and test only through the devshell.** From `/home/barrulus/dev/biri`, run every cargo command as `direnv exec . bash -c '<cmd>'`. Do **not** use `nix develop ...--command`: it changes `PKG_CONFIG_*`/`BINDGEN_*` env vars that feed cargo build-script fingerprints and forces a fresh `libspa-sys`/`pipewire-sys` bindgen.
- The devshell toolchain is nightly by default. **Never** prefix `+nightly` — rustup is not the driver and `cargo +nightly` errors.
- Branch: `multi-action-binds`, already created off `origin/main` at `2e076c89`.
- **No AI attribution** in commit messages. No `Co-Authored-By` lines, no "Generated with" trailers.
- `cargo insta accept` has hung on this repo. Hand-edit inline snapshots instead; if a snapshot diff is unexpectedly large, inspect `niri-config/src/.lib.rs.pending-snap` rather than reaching for `insta accept`.
- Upstream decisions adopted verbatim, do not re-litigate: the hotkey overlay ignores multi-action binds; the CLI gets a separate `Actions` variant; a failed `allow-when-locked` check skips just that action; no `or` combinator.

---

### Task 1: Refactor `Bind.action` to `Bind.actions: Vec<Action>`

Pure type change. The parser still rejects two or more actions per bind, so **no observable behaviour changes** and every existing test must still pass unmodified in intent. The folds introduced here are all no-ops for a one-element list, which is what makes them safe to land before the feature.

**Files:**
- Modify: `niri-config/src/binds.rs:23-31` (struct), `:925-980` (dummy bind + `Ok(Self { .. })` construction), `:1101-1116` (existing test)
- Modify: `niri-config/src/lib.rs:714` (test), plus 18 inline-snapshot blocks at lines 2178, 2198, 2214, 2234, 2252, 2268, 2286, 2302, 2320, 2338, 2354, 2374, 2394, 2412, 2428, 2551, 2573, 2595
- Modify: `src/input/mod.rs:685-716` (`handle_bind`), 11 `allowed_during_screenshot(&bind.action)` sites at lines 3159, 3574, 3584, 3686, 3692, 3839, 3845, 3877, 3883, 4299, 4928, and 19 `Bind { .. }` literals
- Modify: `src/ui/mru.rs:1848-1865` (`make_preset_opened_binds`), `:1902-1934` (`make_dynamic_opened_binds`)
- Modify: `src/ui/hotkey_overlay.rs:155-194` (`format_bind`), `:197-260` (`collect_actions`)

**Interfaces:**
- Produces: `niri_config::Bind { key: Key, actions: Vec<Action>, repeat: bool, cooldown: Option<Duration>, allow_when_locked: bool, allow_inhibiting: bool, hotkey_overlay_title: Option<Option<String>> }`
- Produces: `State::do_actions(&mut self, actions: Vec<Action>, allow_when_locked: bool)` in `src/input/mod.rs`
- Produces: `fn bind_allowed_during_screenshot(bind: &Bind) -> bool` in `src/input/mod.rs`

- [ ] **Step 1: Record the baseline**

Run the suite before touching anything, so a later failure is unambiguous.

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test 2>&1 | tail -30'
```

Expected: all tests pass. Save the summary line (e.g. `test result: ok. N passed`) — Step 12 must match it.

- [ ] **Step 2: Change the struct field**

`niri-config/src/binds.rs`, replace lines 23-31:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Bind {
    pub key: Key,
    pub actions: Vec<Action>,
    pub repeat: bool,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}
```

- [ ] **Step 3: Update the decoder's two construction sites**

Still one action only — this step does not add the feature. In `Bind::decode_node`, the dummy (around line 933) becomes:

```rust
        let dummy = Self {
            key,
            actions: Vec::new(),
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };
```

and the success construction (around line 962) becomes:

```rust
                    Ok(Self {
                        key,
                        actions: vec![action],
                        repeat,
                        cooldown,
                        allow_when_locked,
                        allow_inhibiting,
                        hotkey_overlay_title,
                    })
```

Leave the `"only one action is allowed per keybind"` error at line 940 in place. Task 2 removes it.

- [ ] **Step 4: Rewrite `handle_bind` to loop, and add `do_actions`**

`src/input/mod.rs`, replace `handle_bind` (lines 685-716) with:

```rust
    pub fn handle_bind(&mut self, bind: Bind) {
        let Some(cooldown) = bind.cooldown else {
            self.do_actions(bind.actions, bind.allow_when_locked);
            return;
        };

        // Check this first so that it doesn't trigger the cooldown.
        //
        // Run the bind if *any* of its actions would be allowed; do_action skips the rest
        // individually.
        if self.niri.is_locked()
            && !(bind.allow_when_locked || bind.actions.iter().any(allowed_when_locked))
        {
            return;
        }

        match self.niri.bind_cooldown_timers.entry(bind.key) {
            // The bind is on cooldown.
            Entry::Occupied(_) => (),
            Entry::Vacant(entry) => {
                let timer = Timer::from_duration(cooldown);
                let token = self
                    .niri
                    .event_loop
                    .insert_source(timer, move |_, _, state| {
                        if state.niri.bind_cooldown_timers.remove(&bind.key).is_none() {
                            error!("bind cooldown timer entry disappeared");
                        }
                        TimeoutAction::Drop
                    })
                    .unwrap();
                entry.insert(token);

                self.do_actions(bind.actions, bind.allow_when_locked);
            }
        }
    }

    pub fn do_actions(&mut self, actions: Vec<Action>, allow_when_locked: bool) {
        for action in actions {
            self.do_action(action, allow_when_locked);
        }
    }
```

The `move` closure captures only `bind.key` (Rust 2021 disjoint capture, and `Key` is `Copy`), so `bind.actions` is still owned and usable afterwards. `do_action` itself is unchanged: it keeps its `Action` signature and its own per-action lock check.

- [ ] **Step 5: Add the screenshot fold helper**

`src/input/mod.rs`, immediately after the existing `allowed_during_screenshot` function (which ends around line 5215), add:

```rust
/// A bind is usable while the screenshot UI is open only if *every* one of its actions is.
/// The screenshot UI must never be handed a bind it can only half-run.
fn bind_allowed_during_screenshot(bind: &Bind) -> bool {
    bind.actions.iter().all(allowed_during_screenshot)
}
```

- [ ] **Step 6: Point the 11 screenshot call sites at the helper**

```bash
cd /home/barrulus/dev/biri
sed -i 's/allowed_during_screenshot(&bind\.action)/bind_allowed_during_screenshot(\&bind)/g' src/input/mod.rs
grep -c 'bind_allowed_during_screenshot(&bind)' src/input/mod.rs
```

Expected: `11`. If the count differs, stop and inspect — do not proceed.

Some sites hold `bind` by reference already; if the compiler complains about `&&bind` at any site in Step 10, drop the extra `&` there.

- [ ] **Step 7: Update the 19 synthetic `Bind` literals in `src/input/mod.rs`**

There are **19** `Bind { .. }` literals in this file, each with an `action: X,` field that becomes `actions: vec![X],`. List them and their fields with:

```bash
cd /home/barrulus/dev/biri
grep -c 'Bind {' src/input/mod.rs   # expect 19
grep -n 'action:' src/input/mod.rs
```

Work through every `action:` hit that sits inside a `Bind { .. }` literal (skip `OutputAction`/`SwitchAction` hits). Step 10's build is the backstop: any literal missed here becomes a compile error naming its line. For example, the screenshot-UI bind around line 4935:

```rust
                final_bind = screenshot_ui.action(raw, mods).map(|action| Bind {
                    key: Key {
                        trigger: Trigger::Keysym(raw),
                        modifiers: mods,
                    },
                    actions: vec![action],
                    repeat: true,
                    cooldown: None,
                    allow_when_locked: false,
                    allow_inhibiting: false,
                    hotkey_overlay_title: None,
                });
```

- [ ] **Step 8: Update `src/ui/mru.rs`**

In `make_preset_opened_binds` (line ~1852), the `push` closure builds a `Bind`:

```rust
    let mut push = |trigger, action| {
        rv.push(Bind {
            key: Key {
                trigger: Trigger::Keysym(trigger),
                // The modifier is filled dynamically.
                modifiers: Modifiers::empty(),
            },
            actions: vec![action],
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: false,
            hotkey_overlay_title: None,
        })
    };
```

In `make_dynamic_opened_binds` (line ~1906), only single-action binds map to an MRU action — a multi-action bind must not silently become an MRU navigation bind:

```rust
    for bind in &config.binds.0 {
        // Multi-action binds don't map onto a single MRU action.
        let [bind_action] = bind.actions.as_slice() else {
            continue;
        };

        let action = match bind_action {
            Action::FocusColumnRight
            | Action::FocusColumnRightOrFirst
            | Action::FocusColumnOrMonitorRight
            | Action::FocusWindowDownOrColumnRight => Action::MruAdvance {
                direction: MruDirection::Forward,
                scope: None,
                filter: None,
            },
            Action::FocusColumnLeft
            | Action::FocusColumnLeftOrLast
            | Action::FocusColumnOrMonitorLeft
            | Action::FocusWindowUpOrColumnLeft => Action::MruAdvance {
                direction: MruDirection::Backward,
                scope: None,
                filter: None,
            },
            Action::FocusColumnFirst => Action::MruFirst,
            Action::FocusColumnLast => Action::MruLast,
            Action::CloseWindow => Action::MruCloseCurrentWindow,
            x @ Action::Screenshot(_, _) => x.clone(),
            _ => continue,
        };

        binds.entry(bind.key.trigger).or_default().push(Bind {
            actions: vec![action],
            ..bind.clone()
        });
    }
```

- [ ] **Step 9: Update `src/ui/hotkey_overlay.rs`**

In `format_bind` (line 161), a bind matches the wanted action only if it holds exactly that one action:

```rust
    for bind in binds {
        // Multi-action binds are not shown in the hotkey overlay.
        if bind.actions.as_slice() != std::slice::from_ref(action) {
            continue;
        }
```

In `collect_actions` (lines 205-250), the five `bind.action` reads become slice patterns:

```rust
    if binds
        .iter()
        .any(|bind| bind.actions.as_slice() == [Action::Quit(false)])
    {
        actions.push(&Action::Quit(false));
    } else if binds
        .iter()
        .any(|bind| bind.actions.as_slice() == [Action::Quit(true)])
    {
        actions.push(&Action::Quit(true));
    } else {
        actions.push(&Action::Quit(false));
    }
```

and, for the two workspace-down/-up blocks:

```rust
    // Prefer move-column-to-workspace-down, but fall back to move-window-to-workspace-down.
    if let Some(bind) = binds
        .iter()
        .find(|bind| matches!(bind.actions.as_slice(), [Action::MoveColumnToWorkspaceDown(_)]))
    {
        actions.push(&bind.actions[0]);
    } else if binds
        .iter()
        .any(|bind| matches!(bind.actions.as_slice(), [Action::MoveWindowToWorkspaceDown(_)]))
    {
        actions.push(&Action::MoveWindowToWorkspaceDown(true));
    } else {
        actions.push(&Action::MoveColumnToWorkspaceDown(true));
    }

    // Same for -up.
    if let Some(bind) = binds
        .iter()
        .find(|bind| matches!(bind.actions.as_slice(), [Action::MoveColumnToWorkspaceUp(_)]))
    {
        actions.push(&bind.actions[0]);
    } else if binds
        .iter()
        .any(|bind| matches!(bind.actions.as_slice(), [Action::MoveWindowToWorkspaceUp(_)]))
    {
        actions.push(&Action::MoveWindowToWorkspaceUp(true));
    } else {
        actions.push(&Action::MoveColumnToWorkspaceUp(true));
    }
```

- [ ] **Step 10: Build and fix any remaining compile errors**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo build 2>&1 | tail -40'
```

Expected on the first run: errors naming any `action:` field or `bind.action` read still left over. Fix each by the same rules — a construction becomes `actions: vec![X]`, a single read becomes a slice pattern or `bind.actions[0]`. Repeat until the build is clean.

- [ ] **Step 11: Update the two existing tests that read `bind.action`**

`niri-config/src/binds.rs:1111` inside `parse_window_shader_actions`:

```rust
        let actions: Vec<_> = config
            .binds
            .0
            .iter()
            .map(|b| b.actions[0].clone())
            .collect();
```

`niri-config/src/lib.rs:714`:

```rust
            .map(|bind| bind.actions[0].clone())
```

- [ ] **Step 12: Update the 18 inline snapshot blocks and run the tests**

`Bind`'s debug output changes from `action: X,` to `actions: [\n    X,\n],` with the surrounding indentation. Hand-edit the 18 blocks in `niri-config/src/lib.rs`. Do **not** run `cargo insta accept` — it has hung on this repo.

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test 2>&1 | tail -30'
```

Expected: the same pass count recorded in Step 1. If a snapshot test fails, read the diff it prints (or `niri-config/src/.lib.rs.pending-snap`) and hand-apply it.

- [ ] **Step 13: Format, lint, commit**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo fmt --all && cargo clippy --all-targets 2>&1 | tail -20'
git add -A niri-config/src src/input/mod.rs src/ui/mru.rs src/ui/hotkey_overlay.rs
git commit -m "config: hold a list of actions per bind

Pure refactor: Bind.action becomes Bind.actions: Vec<Action>. The parser
still rejects more than one action per bind, so behaviour is unchanged.
Folds over the new list are chosen so they are no-ops for a one-element
list: 'any' for the cooldown lock pre-check, 'all' for screenshot-UI
eligibility, exactly-one for the hotkey overlay and MRU binds."
```

---

### Task 2: Parse and validate multiple actions per bind

Turns the feature on for the config half.

**Files:**
- Modify: `niri-config/src/binds.rs:925-980` (`Bind::decode_node`)
- Test: `niri-config/src/binds.rs` (`mod tests` at line ~1096)
- Modify: `docs/wiki/Configuration:-Key-Bindings.md`

**Interfaces:**
- Consumes: `Bind { actions: Vec<Action>, .. }` from Task 1
- Produces: a bind whose `actions` holds every child node in source order

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `niri-config/src/binds.rs`:

```rust
    #[test]
    fn parse_multiple_actions_in_order() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+G { focus-column-right; consume-or-expel-window-left; }
            }
            "#,
        )
        .unwrap();

        assert_eq!(config.binds.0.len(), 1);
        assert_eq!(
            config.binds.0[0].actions,
            [Action::FocusColumnRight, Action::ConsumeOrExpelWindowLeft],
        );
    }

    #[test]
    fn parse_bind_with_no_actions_still_errors() {
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+G { }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn parse_bad_action_among_several_keeps_the_others() {
        // The bad action in the middle must not swallow the third one's decoding.
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+G { focus-column-right; not-a-real-action; close-window; }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn allow_when_locked_rejected_on_mixed_bind() {
        // Every action must be spawn/spawn-sh, otherwise allow-when-locked would let a
        // non-spawn action run from the lock screen.
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+X allow-when-locked=true { spawn "foo"; quit; }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn allow_when_locked_accepted_on_all_spawn_bind() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+X allow-when-locked=true { spawn "foo"; spawn-sh "bar"; }
            }
            "#,
        )
        .unwrap();

        assert!(config.binds.0[0].allow_when_locked);
        assert_eq!(config.binds.0[0].actions.len(), 2);
    }

    #[test]
    fn toggle_inhibit_anywhere_in_list_forces_allow_inhibiting_false() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+Escape { close-window; toggle-keyboard-shortcuts-inhibit; }
            }
            "#,
        )
        .unwrap();

        assert!(!config.binds.0[0].allow_inhibiting);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test -p niri-config binds 2>&1 | tail -30'
```

Expected: `parse_multiple_actions_in_order`, `allow_when_locked_accepted_on_all_spawn_bind` and `toggle_inhibit_anywhere_in_list_forces_allow_inhibiting_false` FAIL (the parser still emits "only one action is allowed per keybind"). `parse_bind_with_no_actions_still_errors` and `parse_bad_action_among_several_keeps_the_others` should already PASS.

- [ ] **Step 3: Rewrite the decoder's child loop**

In `niri-config/src/binds.rs`, replace everything from `let mut children = node.children();` (line ~925) through the end of `decode_node` with:

```rust
        // If the actions are invalid but the key is fine, we still want to return something.
        // That way, the parent can handle the existence of duplicate keybinds,
        // even if their contents are not valid.
        let dummy = Self {
            key,
            actions: Vec::new(),
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };

        let mut actions = Vec::new();
        let mut had_error = false;
        for child in node.children() {
            match Action::decode_node(child, ctx) {
                // Emit the error and keep going, so that a typo in one action still surfaces
                // the errors in the actions after it.
                Err(e) => {
                    ctx.emit_error(e);
                    had_error = true;
                }
                Ok(action) => actions.push(action),
            }
        }

        if actions.is_empty() {
            if !had_error {
                ctx.emit_error(DecodeError::missing(
                    node,
                    "expected an action for this keybind",
                ));
            }
            return Ok(dummy);
        }

        // allow-when-locked must hold for *every* action: handle_bind passes one flag into
        // every do_action call, so a mixed bind would let a non-spawn action run from the
        // lock screen.
        if !actions
            .iter()
            .all(|a| matches!(a, Action::Spawn(_) | Action::SpawnSh(_)))
        {
            if let Some(node) = allow_when_locked_node {
                ctx.emit_error(DecodeError::unexpected(
                    node,
                    "property",
                    "allow-when-locked can only be set on binds where every action is spawn",
                ));
            }
        }

        // The toggle-inhibit action must always be uninhibitable. Otherwise, it would be
        // impossible to trigger it.
        if actions
            .iter()
            .any(|a| matches!(a, Action::ToggleKeyboardShortcutsInhibit))
        {
            allow_inhibiting = false;
        }

        Ok(Self {
            key,
            actions,
            repeat,
            cooldown,
            allow_when_locked,
            allow_inhibiting,
            hotkey_overlay_title,
        })
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test -p niri-config 2>&1 | tail -30'
```

Expected: all six new tests PASS and the rest of `niri-config` is still green.

- [ ] **Step 5: Run the whole suite**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test 2>&1 | tail -30'
```

Expected: PASS. The old `"only one action is allowed per keybind"` string is gone; if a test asserted on it, update that assertion.

- [ ] **Step 6: Document the config half**

Add to `docs/wiki/Configuration:-Key-Bindings.md`, as a new `### Multiple Actions` section placed after `### Custom Hotkey Overlay Titles` (line 133) and before `### Actions` (line 172):

````markdown
### Multiple Actions

A bind can list several actions. They run in order, with nothing else running in
between them.

```kdl
binds {
    Mod+G { focus-column-right; consume-or-expel-window-left; }
}
```

Two limitations apply to multi-action binds:

- They never appear in the hotkey overlay, and `hotkey-overlay-title` has no effect on
  them.
- `allow-when-locked=true` is accepted only when *every* action in the bind is `spawn`
  or `spawn-sh`. Otherwise the flag would let a non-spawn action run from the lock
  screen.

If an action cannot run — for example a non-spawn action on a locked screen — it is
skipped and the remaining actions still run.
````

- [ ] **Step 7: Format, lint, commit**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo fmt --all && cargo clippy --all-targets 2>&1 | tail -20'
git add niri-config/src/binds.rs docs/wiki/Configuration:-Key-Bindings.md
git commit -m "config: allow several actions in one bind

Mod+G { focus-column-right; consume-or-expel-window-left; } now parses,
running both actions in source order. A malformed action is reported and
skipped so later actions still get decoded. allow-when-locked now requires
every action to be spawn/spawn-sh, and toggle-keyboard-shortcuts-inhibit
anywhere in the list forces allow-inhibiting off."
```

---

### Task 3: `Request::Actions` IPC batch

**Files:**
- Modify: `niri-ipc/src/lib.rs:91` (`Request` enum)
- Modify: `src/ipc/server.rs:393-411` (add an `Actions` arm next to the existing `Action` arm)
- Test: `niri-ipc/src/lib.rs` (the existing `mod tests` at line 2188)

**Interfaces:**
- Consumes: `State::do_actions(Vec<niri_config::Action>, bool)` from Task 1
- Produces: `niri_ipc::Request::Actions(Vec<Action>)`, answered with `Response::Handled`

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` in `niri-ipc/src/lib.rs` (line 2188). `serde_json` is already
a normal dependency of the crate, and `Request` already derives `Debug`.

```rust
    #[test]
    fn actions_request_round_trips() {
        let request = Request::Actions(vec![
            Action::FocusColumnRight {},
            Action::CloseWindow { id: None },
        ]);

        let json = serde_json::to_string(&request).unwrap();
        let back: Request = serde_json::from_str(&json).unwrap();

        let Request::Actions(actions) = back else {
            panic!("expected Actions, got {back:?}");
        };
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::FocusColumnRight {}));
        assert!(matches!(actions[1], Action::CloseWindow { id: None }));
    }
```

- [ ] **Step 2: Run it to verify it fails**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test -p niri-ipc actions_request_round_trips 2>&1 | tail -20'
```

Expected: FAIL — `no variant named 'Actions' found for enum 'Request'`.

- [ ] **Step 3: Add the request variant**

`niri-ipc/src/lib.rs`, directly after the existing `Action(Action),` at line 91:

```rust
    /// Perform several actions as one atomic sequence.
    ///
    /// All actions are validated before any of them runs; if any is invalid, none run. The
    /// actions then run in order with nothing else running in between, which is what
    /// distinguishes this from several [`Request::Action`] calls.
    Actions(Vec<Action>),
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test -p niri-ipc actions_request_round_trips 2>&1 | tail -20'
```

Expected: PASS.

- [ ] **Step 5: Handle the request in the server**

`src/ipc/server.rs`, add this arm immediately after the existing `Request::Action(action) => { .. }` arm (which ends at line 411):

```rust
        Request::Actions(actions) => {
            // Validate everything up front: an invalid action anywhere rejects the whole
            // batch, so a partial sequence never runs.
            for (idx, action) in actions.iter().enumerate() {
                validate_action(action).map_err(|err| format!("action {}: {err}", idx + 1))?;
            }

            let (tx, rx) = async_channel::bounded(1);

            let actions: Vec<niri_config::Action> =
                actions.into_iter().map(niri_config::Action::from).collect();
            // One idle callback for the whole batch. This is the point of the request:
            // nothing can interleave between the actions. Queuing one callback per action
            // would not give that guarantee.
            ctx.event_loop.insert_idle(move |state| {
                // Make sure some logic like workspace clean-up has a chance to run before
                // doing actions.
                state.niri.advance_animations();
                state.do_actions(actions, false);
                let _ = tx.send_blocking(());
            });

            // Wait until the actions have been processed before returning, for the same
            // reason as the single-action request.
            let _ = rx.recv().await;
            Response::Handled
        }
```

- [ ] **Step 6: Build and run the suite**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo build 2>&1 | tail -20 && cargo test 2>&1 | tail -20'
```

Expected: build clean, all tests PASS. If the compiler reports a non-exhaustive match anywhere else over `Request`, add an `Actions` arm there too.

- [ ] **Step 7: Format, lint, commit**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo fmt --all && cargo clippy --all-targets 2>&1 | tail -20'
git add niri-ipc/src/lib.rs src/ipc/server.rs
git commit -m "ipc: add Request::Actions for atomic action batches

Every action is validated before any runs, and the whole batch runs inside a
single idle callback so nothing interleaves between the actions. That is the
property chaining 'niri msg action' calls cannot provide."
```

---

### Task 4: `niri msg actions` CLI

**Files:**
- Modify: `Cargo.toml` (add the `shlex` dependency)
- Modify: `src/cli.rs:85-88` (add the `Actions` variant, and an `ActionParser` helper)
- Modify: `src/ipc/client.rs:19-30` (path fixup), `:39` (request mapping), `:321` (response check)
- Test: `src/cli.rs` (`mod tests`)
- Modify: `docs/wiki/Configuration:-Key-Bindings.md`

**Interfaces:**
- Consumes: `niri_ipc::Request::Actions(Vec<Action>)` from Task 3
- Produces: `Msg::Actions { actions: Vec<String> }` in `src/cli.rs`
- Produces: `fn parse_action_words(words: Vec<String>) -> anyhow::Result<Action>` in `src/cli.rs`
- Produces: `fn parse_actions(args: &[String], stdin: &str) -> anyhow::Result<Vec<Action>>` in `src/cli.rs`

- [ ] **Step 1: Add the dependency**

In the root `Cargo.toml`, under `[dependencies]` (alongside `clap` at line 64):

```toml
shlex = "1.3"
```

`shlex 1.3.0` is already in `Cargo.lock` as a transitive dependency, so this pulls nothing new.

- [ ] **Step 2: Write the failing tests**

`src/cli.rs` has no test module yet (the file is 139 lines). Create one at the end:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_and_stdin_produce_the_same_actions() {
        let args = vec![
            String::from("toggle-workspace-visibility stash"),
            String::from("focus-workspace stash"),
        ];
        let from_args = parse_actions(&args, "").unwrap();

        let stdin = "toggle-workspace-visibility stash\nfocus-workspace stash\n";
        let from_stdin = parse_actions(&[], stdin).unwrap();

        assert_eq!(from_args.len(), 2);
        assert_eq!(format!("{from_args:?}"), format!("{from_stdin:?}"));
    }

    #[test]
    fn blank_stdin_lines_are_skipped() {
        let stdin = "focus-column-right\n\n   \nclose-window\n";
        let actions = parse_actions(&[], stdin).unwrap();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn quoted_arguments_survive_lexing() {
        let args = vec![String::from(r#"spawn sh -c "echo hello world""#)];
        let actions = parse_actions(&args, "").unwrap();
        let Action::Spawn { command } = &actions[0] else {
            panic!("expected Spawn, got {:?}", actions[0]);
        };
        assert_eq!(command, &["sh", "-c", "echo hello world"]);
    }

    #[test]
    fn a_bad_action_names_its_position() {
        let args = vec![
            String::from("focus-column-right"),
            String::from("not-a-real-action"),
        ];
        let err = parse_actions(&args, "").unwrap_err().to_string();
        assert!(err.contains('2'), "error should name argument 2: {err}");
    }

    #[test]
    fn no_args_and_no_stdin_is_an_error() {
        assert!(parse_actions(&[], "").is_err());
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test --bin niri parse_actions 2>&1 | tail -20'
```

Expected: FAIL to compile — `cannot find function 'parse_actions'`.

- [ ] **Step 4: Add the CLI variant and the parsing functions**

In `src/cli.rs`, add the subcommand directly after the existing `Action { .. }` variant at line 85-88:

```rust
    /// Perform several actions as one atomic sequence.
    ///
    /// Each argument is one action, for example:
    ///
    ///   niri msg actions "toggle-workspace-visibility stash" "focus-workspace stash"
    ///
    /// With no arguments, actions are read from stdin, one per line.
    Actions {
        /// One action per argument. Reads actions from stdin, one per line, if empty.
        #[arg()]
        actions: Vec<String>,
    },
```

Then, at the end of the file (before any `mod tests`):

```rust
/// Wrapper that lets a single `Action` be parsed from a bare word list.
///
/// `Action` is a clap `Subcommand`, so it needs a `Parser` around it, and
/// `no_binary_name` so that the first word is read as the subcommand rather than as
/// argv[0].
#[derive(clap::Parser)]
#[command(name = "", no_binary_name = true)]
struct ActionParser {
    #[command(subcommand)]
    action: Action,
}

/// Parses one action from an already-lexed word list.
pub fn parse_action_words(words: Vec<String>) -> anyhow::Result<Action> {
    use clap::Parser as _;

    Ok(ActionParser::try_parse_from(words)?.action)
}

/// Parses a batch of actions from CLI arguments, falling back to stdin when there are none.
///
/// Argument form and stdin form must produce identical output.
pub fn parse_actions(args: &[String], stdin: &str) -> anyhow::Result<Vec<Action>> {
    use anyhow::{anyhow, Context as _};

    let sources: Vec<(String, String)> = if args.is_empty() {
        stdin
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(idx, line)| (format!("stdin line {}", idx + 1), line.to_owned()))
            .collect()
    } else {
        args.iter()
            .enumerate()
            .map(|(idx, arg)| (format!("argument {}", idx + 1), arg.clone()))
            .collect()
    };

    if sources.is_empty() {
        return Err(anyhow!(
            "no actions given; pass one action per argument, or one per line on stdin"
        ));
    }

    let mut actions = Vec::with_capacity(sources.len());
    for (label, text) in sources {
        let words = shlex::split(&text)
            .ok_or_else(|| anyhow!("{label}: unbalanced quotes in {text:?}"))?;
        if words.is_empty() {
            return Err(anyhow!("{label}: empty action"));
        }
        actions.push(parse_action_words(words).with_context(|| format!("{label}"))?);
    }

    Ok(actions)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo test --bin niri 2>&1 | tail -20'
```

Expected: all five new tests PASS.

- [ ] **Step 6: Wire the variant through the client**

`src/ipc/client.rs`. First, in `handle_msg`, read stdin and build the request. Add this immediately after the existing path-fixup block that ends at line 30:

```rust
    // Resolve `niri msg actions` before the request match, since it may read stdin.
    let batched_actions = if let Msg::Actions { actions } = &msg {
        use std::io::Read as _;

        let mut stdin = String::new();
        if actions.is_empty() {
            std::io::stdin()
                .read_to_string(&mut stdin)
                .context("error reading actions from stdin")?;
        }

        let mut parsed = crate::cli::parse_actions(actions, &stdin)?;
        // For actions taking paths, prepend the niri CLI's working directory.
        for action in &mut parsed {
            if let Action::Screenshot { path, .. }
            | Action::ScreenshotScreen { path, .. }
            | Action::ScreenshotWindow { path, .. } = action
            {
                if let Some(path) = path {
                    ensure_absolute_path(path).context("error making the path absolute")?;
                }
            }
        }
        Some(parsed)
    } else {
        None
    };
```

Then add the request mapping next to `Msg::Action` at line 39:

```rust
        Msg::Actions { .. } => Request::Actions(batched_actions.clone().unwrap()),
```

And the response check next to `Msg::Action { .. }` at line 321:

```rust
        Msg::Actions { .. } => {
            let Response::Handled = response else {
                bail!("unexpected response: expected Handled, got {response:?}");
            };
        }
```

- [ ] **Step 7: Build and run the whole suite**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo build 2>&1 | tail -20 && cargo test 2>&1 | tail -20'
```

Expected: build clean, all tests PASS. If the compiler reports a non-exhaustive match over `Msg`, add an `Actions` arm there.

- [ ] **Step 8: Check the help text renders**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c './target/debug/niri msg actions --help'
```

Expected: the help shows the `actions` subcommand with the multi-line example from Step 4.

- [ ] **Step 9: Document the IPC half**

Append to the `### Multiple Actions` section added in Task 2, in `docs/wiki/Configuration:-Key-Bindings.md`:

````markdown
The same sequencing is available over IPC. `niri msg action` runs one action per
invocation, so chaining two with `&&` leaves a window in which other events run;
`niri msg actions` sends the whole list as one batch that runs with nothing in between.

```sh
# one action per argument
niri msg actions "toggle-workspace-visibility stash" "focus-workspace stash"

# or one per line on stdin
printf 'focus-column-right\nconsume-or-expel-window-left\n' | niri msg actions
```

If any action in the batch is invalid, none of them run.
````

- [ ] **Step 10: Format, lint, commit**

```bash
cd /home/barrulus/dev/biri
direnv exec . bash -c 'cargo fmt --all && cargo clippy --all-targets 2>&1 | tail -20'
git add Cargo.toml Cargo.lock src/cli.rs src/ipc/client.rs docs/wiki/Configuration:-Key-Bindings.md
git commit -m "cli: add 'niri msg actions' for atomic action batches

One action per argument, shell-lexed and parsed as a subcommand; with no
arguments, one action per stdin line. Both forms produce the same batch. clap
cannot chain subcommands, which is why this is a separate command rather than
an extension of 'niri msg action'."
```

---

## Manual verification

Automated tests cannot cover ordering inside a live compositor. After Task 4, run the compositor and confirm:

1. Add to your config and reload:
   ```kdl
   binds {
       Mod+G { focus-column-right; consume-or-expel-window-left; }
   }
   ```
   Press `Mod+G` with at least two columns open. Both actions must take effect, in that order, in one frame.
2. `niri msg actions "toggle-workspace-visibility stash" "focus-workspace stash"` must switch to the now-visible workspace with no intermediate flash — the chain from biri #24 that previously needed `spawn-sh` with `&&`.
3. `niri msg actions "focus-column-right" "not-a-real-action"` must print an error naming argument 2 and must **not** move focus.
4. Open the hotkey overlay (`Mod+Shift+Escape` by default). The multi-action `Mod+G` bind must not appear.
5. Lock the screen. A bind such as `Mod+X allow-when-locked=true { spawn "notify-send hi"; spawn-sh "true"; }` still works; the config must refuse to load if `quit` is added to that bind.
