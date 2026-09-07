# Multi-action binds

Design for biri issue #32 / upstream niri#914.

## Goal

Two halves, both in scope:

1. **Config.** One bind runs several actions in order:
   `Mod+G { focus-column-right; consume-or-expel-window-left; }`
2. **IPC.** Several actions sent as one batch, so they run as one atomic sequence with no
   delay between them. `niri msg action` cannot do this today; chaining two invocations
   with `&&` leaves a window in which other events run.

The fork keeps growing one-off `focus=true` flags (hidden workspaces, PR #26) to dodge
chaining. This feature removes the reason for those.

## Intent to upstream

The internal shape follows what YaLTeR and bbb651 already settled on upstream: `Bind` holds
a `Vec<Action>`. A `Action::Sequence(Vec<Action>)` variant would have been a far smaller
diff for biri, but it diverges from the agreed upstream shape and would have to be reshaped
later. The larger mechanical diff is accepted deliberately.

Decisions already made upstream and adopted here without re-litigating them:

- The hotkey overlay ignores multi-action binds.
- The CLI gets a separate `Actions` variant, because clap cannot chain subcommands.
- A failed `allow-when-locked` check skips just that action; the rest still run.
- A general `or` combinator is rejected. Existing `*-or-*` actions are left alone.

## Config shape and parsing

`niri-config/src/binds.rs`

```rust
pub struct Bind {
    pub key: Key,
    pub actions: Vec<Action>,   // was: action: Action
    pub repeat: bool,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}
```

`Bind::decode_node` drops the "only one action is allowed per keybind" error and decodes
every child node into the list, preserving source order.

- A child that fails to decode emits its error and is **skipped**; decoding continues. A
  typo in the second action still surfaces the third action's errors in the same pass.
- Zero children keeps the existing "expected an action for this keybind" error and returns
  the dummy bind with `actions: Vec::new()`. The dummy exists only so `Binds::decode_node`
  can still detect duplicate keys; an empty list is inert everywhere downstream.

Nesting is unrepresentable by construction. The list is flat and there is no `Sequence`
variant to recurse into.

### Validation folds

Two existing per-action validations become folds over the list. The choice of fold is
load-bearing in both cases.

**`allow-when-locked` requires *every* action to be `spawn`/`spawn-sh`.** It must be *all*,
not *any*. `handle_bind` passes a single `bind.allow_when_locked` flag into every
`do_action` call, so accepting a mixed bind would let

```kdl
Mod+X allow-when-locked=true { spawn "foo"; quit; }
```

run `quit` from the lock screen. That is a lock-screen bypass. *All* is the only safe fold.

**`ToggleKeyboardShortcutsInhibit` forces `allow_inhibiting = false` if *any* action is that
variant.** Otherwise the bind that un-inhibits shortcuts could itself be inhibited, leaving
no way to trigger it.

## Input dispatch

`src/input/mod.rs`

`handle_bind` loops over the list:

```rust
for action in bind.actions {
    self.do_action(action, bind.allow_when_locked);
}
```

`do_action` keeps its `Action` signature and its own lock check, which is what gives the
upstream-agreed semantics: a locked-out action is skipped, the rest still run. There is no
stop-on-failure mode, because nothing in `do_action` returns an error to stop on.

Two more folds:

- **Cooldown pre-check** (currently `input/mod.rs:692`) becomes *any*: run the bind if
  `bind.allow_when_locked || bind.actions.iter().any(allowed_when_locked)`. Otherwise skip
  the bind **without** burning its cooldown timer, matching today's behaviour.
- **`allowed_during_screenshot`** gains a `bind_allowed_during_screenshot(&bind)` helper
  folding with *all*, replacing the ~10 `allowed_during_screenshot(&bind.action)` call
  sites. The screenshot UI must never be handed a bind it can only half-run. An empty list
  is vacuously true, which is harmless because nothing runs.

Then the mechanical part: 45 `Bind { action: X, .. }` constructions — almost all synthetic
binds for the screenshot UI, overview and gestures — become `actions: vec![X]`.

## IPC

`niri-ipc/src/lib.rs`, `src/ipc/server.rs`

New request variant; the existing single-action one is untouched for compatibility.

```rust
pub enum Request {
    Action(Action),           // unchanged
    Actions(Vec<Action>),     // new
    ...
}
```

Server handling:

1. Validate **all** actions up front with the existing `validate_action`, per element.
   An error is prefixed with the 1-based index so the caller knows which action failed.
   Nothing runs if any action is invalid.
2. Queue **one** idle callback that calls `advance_animations()` once and then runs every
   action in order.
3. Reply `Response::Handled`.

Step 2 is the point of the feature. One callback means nothing can interleave between the
actions, which is the "one atomic sequence with no delay in-between" property the upstream
maintainer named as the priority. Queuing one callback per action would not provide it.

No per-action results are returned. `do_action` returns `()`, and the only fallible step is
pre-flight validation, so there is nothing per-action to report.

## CLI

`src/cli.rs`, `src/ipc/client.rs`

```rust
/// Perform several actions as one atomic sequence.
Actions {
    /// One action per argument. Reads actions from stdin, one per line, if empty.
    #[arg()]
    actions: Vec<String>,
},
```

Each argument is shell-lexed with `shlex` (already in `Cargo.lock` transitively; becomes a
direct dependency) and parsed with `Action::try_parse_from`. With no arguments, actions are
read from stdin one per line, skipping blank lines. Parse errors name the offending
argument index or stdin line number.

```sh
# one-liner, usable directly from a KDL bind
niri msg actions "toggle-workspace-visibility stash" "focus-workspace stash"

# scripted
printf 'focus-column-right\nconsume-or-expel-window-left\n' | niri msg actions
```

Argument form and stdin form must produce an identical `Vec<Action>`.

## Hotkey overlay

`src/ui/hotkey_overlay.rs`

Multi-action binds are skipped. `format_bind` and `collect_actions` match only binds whose
`actions` slice holds exactly one element, so a multi-action bind never matches an entry in
the overlay's fixed list of interesting actions and simply does not appear. No new
rendering, no new layout.

## Out of scope

- Switch binds (`SwitchAction`) and gesture binds. They do not carry an `Action`.
- Any `or` combinator or conditional action. Rejected upstream.
- Converting existing `*-or-*` actions into conditions.
- Nested sequences. Unrepresentable by construction, as above.

## Testing

`niri-config/src/binds.rs`:

- Two actions parse, in source order.
- Zero actions still errors.
- One malformed action among three reports its error and keeps the other two.
- `allow-when-locked` is rejected on a mixed bind, accepted on an all-spawn bind.
- `ToggleKeyboardShortcutsInhibit` anywhere in the list forces `allow_inhibiting = false`.

IPC and CLI:

- `Request::Actions` round-trips through serde.
- Argument lexing and stdin lexing produce the same `Vec<Action>`.
- An invalid action anywhere in the batch rejects the whole batch, and the error names its
  index.

Documentation: `docs/wiki/Configuration:-Key-Bindings.md` gains a multi-action section.

### Known risk

`Bind`'s debug output changes from `action: X` to `actions: [X]`, and 18 of those lines sit
inside the very large inline snapshots in `niri-config/src/lib.rs`. `cargo insta accept` has
hung on this repo before, so those 18 lines are hand-edited, with `.lib.rs.pending-snap`
inspection as the fallback if the diff turns out larger than expected.

## Open questions resolved

The issue listed three. All are settled above:

- **Stop-on-failure or continue** — continue, per action, matching the upstream decision.
  There is no error channel to stop on anyway.
- **Per-action results over IPC** — none. Validation is pre-flight; execution is infallible.
- **Whether `*-or-*` actions become conditions** — no, out of scope.
