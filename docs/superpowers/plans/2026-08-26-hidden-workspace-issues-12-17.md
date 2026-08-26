# Hidden-Workspace Issues #12–#17 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the six hidden-workspace bugs filed as barrulus/biri issues #12–#17, in dependency order, with a regression test per fix and proptest coverage at the end.

**Architecture:** All fixes live in `src/layout/monitor.rs` and `src/layout/mod.rs`. Task 1 makes the test invariants hidden-aware (unblocking all later tests), Tasks 2–6 fix the individual bugs bottom-up (shared helpers first), Task 7 adds hide/unhide to the randomized-op harness so the whole family stays covered.

**Tech Stack:** Rust (niri fork "biri"), proptest for randomized ops, plain `#[test]` unit tests in `src/layout/tests.rs`.

**Spec:** GitHub issues barrulus/biri#12 through #17 (each contains repro + suggested fix). Key invariants (from PR #11 review): hidden workspaces are contiguous at the END of `Monitor::workspaces`; an empty unnamed workspace sits directly before the hidden block (it doubles as the usual trailing empty workspace); hidden workspaces are always named (unhide paths look up by name).

## Global Constraints

- Build/test ONLY via `direnv exec . bash -c 'cargo <cmd>'` from `/home/barrulus/dev/biri`. Never `cargo +nightly`, never `nix develop`.
- Run tests with `cargo test --lib <filter>`; full suite `cargo test --lib`.
- `cargo fmt --all` must be run TWICE before checking (known rustfmt non-idempotency in this repo); verify with `cargo fmt --all -- --check`.
- NO Co-Authored-By lines, NO AI attribution anywhere in commits.
- TDD: every fix gets a failing test first; watch it fail before implementing.
- One commit per task, message style: `<area>: <what changed>` (see git log for examples).
- Work on branch `hidden-ws-issues` off `barrulus-custom`.

---

### Task 1: Hidden-aware `Monitor::verify_invariants` (issue #17)

`verify_invariants` (monitor.rs, `#[cfg(test)]`, currently ~line 3024) asserts the LAST workspace is empty/unnamed and that non-active workspaces are never empty+unnamed. Both are false once a hidden block exists (last = hidden named workspace; the empty guard before the block is empty+unnamed mid-vec). This blocks every other test in this plan.

**Files:**
- Modify: `src/layout/monitor.rs` (`verify_invariants`)
- Test: `src/layout/tests.rs`

**Interfaces:**
- Produces: `verify_invariants` accepts legal hidden states and asserts: hidden-contiguous-at-end, hidden⇒named, last VISIBLE workspace empty+unnamed. Later tasks' tests call `layout.verify_invariants()` after hide/unhide sequences.

- [ ] **Step 1: Write the failing test** (in `src/layout/tests.rs`, next to `unhide_next_to_remaining_hidden_block_keeps_empty_workspace_before_it` which has the same setup):

```rust
#[test]
fn verify_invariants_accepts_hidden_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SetWorkspaceName {
            new_ws_name: 2,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SetWorkspaceName {
            new_ws_name: 3,
            ws_name: None,
        },
    ];
    let mut layout = check_ops(ops);

    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();
}
```

- [ ] **Step 2: Run it, verify it fails** with "monitor must have an empty workspace in the end" (or the unnamed variant):
`direnv exec . bash -c 'cargo test --lib verify_invariants_accepts_hidden_workspaces 2>&1' | tail -15`

- [ ] **Step 3: Rewrite the hidden-affected section of `verify_invariants`.** Replace the block from `assert!(!self.workspaces.last().unwrap().has_windows(), ...)` through the end of the non-active-empty `for` loop (currently lines ~3046–3100) with:

```rust
        // Hidden workspaces are contiguous at the end of the vec and always named
        // (unhide and toggle-visibility look workspaces up by name).
        let visible_count = self
            .workspaces
            .iter()
            .position(|ws| ws.hidden)
            .unwrap_or(self.workspaces.len());
        for ws in &self.workspaces[visible_count..] {
            assert!(
                ws.hidden,
                "hidden workspaces must be contiguous at the end of the workspace vec"
            );
            assert!(ws.name.is_some(), "hidden workspaces must be named");
        }
        assert!(
            visible_count > 0,
            "monitor must have at least one visible workspace"
        );

        // The visible region ends with an empty unnamed workspace. When a hidden
        // block exists, that same workspace is the guard directly before the block.
        let last_visible = &self.workspaces[visible_count - 1];
        assert!(
            !last_visible.has_windows(),
            "monitor must have an empty workspace at the end of the visible region"
        );
        if self.options.layout.empty_workspace_above_first {
            assert!(
                !self.workspaces.first().unwrap().has_windows(),
                "first workspace must be empty when empty_workspace_above_first is set"
            )
        }

        assert!(
            last_visible.name.is_none(),
            "monitor must have an unnamed workspace at the end of the visible region"
        );
        if self.options.layout.empty_workspace_above_first {
            assert!(
                self.workspaces.first().unwrap().name.is_none(),
                "first workspace must be unnamed when empty_workspace_above_first is set"
            )
        }

        if self.options.layout.empty_workspace_above_first && visible_count == self.workspaces.len()
        {
            assert!(
                self.workspaces.len() != 2,
                "if empty_workspace_above_first is set there must be just 1 or 3+ workspaces"
            )
        }

        // If there's no workspace switch in progress, there can't be any non-last
        // non-active empty workspaces in the visible region. If
        // empty_workspace_above_first is set then the first workspace will be empty too.
        let pre_skip = if self.options.layout.empty_workspace_above_first {
            1
        } else {
            0
        };
        if self.workspace_switch.is_none() {
            for (idx, ws) in self
                .workspaces
                .iter()
                .enumerate()
                .take(visible_count)
                .skip(pre_skip)
                .rev()
                // skip the last visible workspace
                .skip(self.workspaces.len() - visible_count + 1)
            {
                if idx != self.active_workspace_idx {
                    assert!(
                        ws.has_windows_or_name(),
                        "non-active workspace can't be empty and unnamed except the last visible one"
                    );
                }
            }
        }
```

NOTE on the iterator: `.take(visible_count)` limits to the visible region but `.rev().skip(...)` operates on what `enumerate().take()` yields — simpler and less error-prone is:

```rust
        if self.workspace_switch.is_none() {
            for (idx, ws) in self.workspaces[..visible_count].iter().enumerate() {
                if idx >= pre_skip && idx != visible_count - 1 && idx != self.active_workspace_idx {
                    assert!(
                        ws.has_windows_or_name(),
                        "non-active workspace can't be empty and unnamed except the last visible one"
                    );
                }
            }
        }
```

Use the second form. The `visible_count == self.workspaces.len()` gate on the len!=2 assert is deliberate: with a hidden block present, `clean_up_workspaces`' 2-workspace collapse doesn't run (Task 2), so `[emptyTop, empty, H...]` is legal.

- [ ] **Step 4: Run the new test — must pass. Run the full suite** (`cargo test --lib`) — all 253+ must pass (the pre-existing hidden-free states must still satisfy the reformulated asserts; `visible_count == len` reduces every changed assert to its original form).

- [ ] **Step 5: Commit** `git add src/layout/monitor.rs src/layout/tests.rs && git commit -m "layout: make Monitor::verify_invariants hidden-workspace-aware"`

---

### Task 2: `clean_up_workspaces` 2-workspace collapse vs hidden workspaces (issue #13)

The `empty_workspace_above_first && len == 2` special case asserts both workspaces are empty+unnamed; `[emptyTop, hidden-named]` is legal and panics.

**Files:**
- Modify: `src/layout/monitor.rs` (`clean_up_workspaces`, currently ~line 886)
- Test: `src/layout/tests.rs`

- [ ] **Step 1: Write the failing test.** `check_ops_with_options` exists for options; `Options` has `layout.empty_workspace_above_first` (find the exact field construction pattern by grepping tests.rs for `empty_workspace_above_first` — there are existing tests using it; copy their Options construction verbatim).

```rust
#[test]
fn clean_up_workspaces_skips_two_workspace_collapse_with_hidden() {
    let options = Options {
        layout: niri_config::LayoutPart {
            empty_workspace_above_first: true,
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    // ^ If this doesn't compile, copy the Options construction from an existing
    // empty_workspace_above_first test in this file instead.

    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ];
    // [emptyTop, "ws1"(win1), empty]
    let mut layout = check_ops_with_options(options, ops);

    // Hiding "ws1" collapses the two remaining empties and re-adds the hidden
    // one at the end: [emptyTop, "ws1"(hidden)].
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    // A later cleanup (e.g. after a workspace-switch gesture ends) must not
    // assert on the legal [empty, hidden-named] state.
    let monitor = match &mut layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &mut monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    monitor.clean_up_workspaces();
    layout.verify_invariants();
}
```

- [ ] **Step 2: Run it, verify it fails** with the `!self.workspaces[1].has_windows_or_name()` assert (from `clean_up_workspaces`), NOT a compile error. If the state ends up different from `[emptyTop, H]`, print `monitor.workspaces` names/hidden flags and adjust ops until the len==2 state is reached.

- [ ] **Step 3: Fix.** In `clean_up_workspaces`, change the special case to:

```rust
        // Special case handling when empty_workspace_above_first is set and all workspaces
        // are empty. With a hidden workspace present ([empty, hidden-named]) there is
        // nothing to collapse.
        if self.options.layout.empty_workspace_above_first
            && self.workspaces.len() == 2
            && !self.workspaces.iter().any(|ws| ws.hidden)
        {
```

- [ ] **Step 4: Run the test — pass. Full suite — pass.**

- [ ] **Step 5: Commit** `git commit -m "layout: don't collapse [empty, hidden-named] two-workspace state"`

---

### Task 3: Maintain `original_idx` and make `add_workspace_bottom` hidden-aware (issue #15)

Nothing adjusts hidden workspaces' stored `original_idx` when the visible region shrinks/grows, so unhide placement silently degrades (the Task-in-PR-#11 clamp only prevents panics). Also `add_workspace_bottom` inserts at `len` — PAST the hidden block; it must insert at the end of the visible region.

**Files:**
- Modify: `src/layout/monitor.rs` (`add_workspace_bottom`, `clean_up_workspaces`, `remove_workspace_by_idx`, `insert_workspace`, new helpers)
- Test: `src/layout/tests.rs`

**Interfaces:**
- Produces: `fn shift_hidden_original_indices_for_removal(&mut self, removed_idx: usize)` and `fn shift_hidden_original_indices_for_insertion(&mut self, inserted_idx: usize)` on `Monitor` (private). `add_workspace_bottom` now inserts at the visible end — Task 6 relies on this.

- [ ] **Step 1: Write the failing placement test:**

```rust
#[test]
fn unhide_placement_survives_workspace_cleanup() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SetWorkspaceName {
            new_ws_name: 3,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
    ];
    // [win1, win2, "ws3"(win3), win4, empty]
    let mut layout = check_ops(ops);

    // Hide "ws3" (records original_idx 2): [win1, win2, win4, empty, H].
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();

    // Close win1; its workspace empties out and gets cleaned up:
    // [win2, win4, empty, H]. The stored original_idx must follow (2 -> 1).
    Op::CloseWindow(1).apply(&mut layout);
    layout.verify_invariants();

    // Unhide: "ws3" must reappear between win2 and win4, its original relative
    // position — not dumped at the end of the visible region.
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();

    let monitor = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    let pos_ws3 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&3))
        .unwrap();
    let pos_win2 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&2))
        .unwrap();
    let pos_win4 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&4))
        .unwrap();
    assert!(
        pos_win2 < pos_ws3 && pos_ws3 < pos_win4,
        "unhidden workspace must return to its original relative position \
         (got win2={pos_win2}, ws3={pos_ws3}, win4={pos_win4})"
    );
}
```

Caveats: `Op::CloseWindow` closes by id; check `Op::CloseWindow(1)` semantics in `apply` (it's `Op::CloseWindow(id)`). Closing the window of a non-active workspace triggers cleanup only when no switch is in flight — `check_ops`/`apply` uses `Op` machinery that already completes animations; if the empty workspace survives, add `Op::AdvanceAnimations { msec_delta: 1000 }` (grep the exact variant name in tests.rs) after the close.

- [ ] **Step 2: Run, verify it fails** with the position assert (ws3 landing after win4, before the trailing empty).

- [ ] **Step 3: Implement.** In `monitor.rs`:

(a) Helpers (place right after `clean_up_workspaces`):

```rust
    // Stored original_idx values of hidden workspaces refer to positions in the
    // visible region; keep them in sync when that region shrinks or grows.
    fn shift_hidden_original_indices_for_removal(&mut self, removed_idx: usize) {
        for ws in &mut self.workspaces {
            if ws.hidden {
                if let Some(original_idx) = &mut ws.original_idx {
                    if *original_idx > removed_idx {
                        *original_idx -= 1;
                    }
                }
            }
        }
    }

    fn shift_hidden_original_indices_for_insertion(&mut self, inserted_idx: usize) {
        for ws in &mut self.workspaces {
            if ws.hidden {
                if let Some(original_idx) = &mut ws.original_idx {
                    if *original_idx >= inserted_idx {
                        *original_idx += 1;
                    }
                }
            }
        }
    }
```

(b) Call `self.shift_hidden_original_indices_for_removal(idx)` immediately after each visible-workspace removal:
- in `clean_up_workspaces`, after `self.workspaces.remove(idx);` in the loop;
- in `remove_workspace_by_idx`, after `let mut ws = self.workspaces.remove(idx);` but ONLY when `!ws.hidden` (removing a hidden workspace doesn't shift the visible region).

(c) Call `self.shift_hidden_original_indices_for_insertion(idx)` after each visible insertion:
- in `add_workspace_at`, after `self.workspaces.insert(idx, ws);`
- in `insert_workspace`, after `self.workspaces.insert(idx, ws);`

(d) Make `add_workspace_bottom` insert at the visible end:

```rust
    pub fn add_workspace_bottom(&mut self) {
        // The bottom of the visible region — hidden workspaces stay past it.
        let visible_end = self
            .workspaces
            .iter()
            .position(|ws| ws.hidden)
            .unwrap_or(self.workspaces.len());
        self.add_workspace_at(visible_end);
    }
```

- [ ] **Step 4: Run the test — pass. Full suite — pass** (the PR #11 regression test `unhide_next_to_remaining_hidden_block_keeps_empty_workspace_before_it` must still pass — the clamp remains as a safety net).

- [ ] **Step 5: Commit** `git commit -m "layout: maintain hidden original_idx across workspace churn"`

---

### Task 4: Route hidden workspaces through `insert_hidden_workspace` (issue #12)

`Layout::move_workspace_to_output_by_id` can move a HIDDEN workspace (lookups don't filter hidden; `remove_workspace_by_idx` preserves `hidden = true`) and then `target.insert_workspace(...)` puts `hidden = true` into the visible region — panics on a single-workspace target (`last_hidden_idx(0) - 1` with overflow-checks on), silently breaks contiguity otherwise.

**Files:**
- Modify: `src/layout/monitor.rs` (`insert_workspace`)
- Test: `src/layout/tests.rs`

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn move_hidden_workspace_to_other_output_keeps_it_hidden() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ];
    let mut layout = check_ops(ops);

    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    // Move the hidden workspace to output 2. Find its index on monitor 0 first.
    let (old_idx, old_output, new_output) = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => {
            let old_idx = monitors[0]
                .workspaces
                .iter()
                .position(|ws| ws.name.as_deref() == Some("ws1"))
                .unwrap();
            (
                old_idx,
                monitors[0].output.clone(),
                monitors[1].output.clone(),
            )
        }
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    layout.move_workspace_to_output_by_id(old_idx, Some(old_output), &new_output);
    layout.verify_invariants();

    // The workspace lives on monitor 1 now, still hidden, still reachable by name.
    let monitor = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &monitors[1],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    let ws = monitor
        .workspaces
        .iter()
        .find(|ws| ws.name.as_deref() == Some("ws1"))
        .expect("moved workspace must be on the target monitor");
    assert!(ws.hidden, "moved workspace must stay hidden");

    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();
}
```

NOTE: the panic repro (single-workspace target) fires under `overflow-checks`; in a debug test build the subtraction overflow panics inside `clean_up_workspaces` — the test fails by panic, which is the expected RED. If output naming differs (`Op::AddOutput(2)` creating "output-2"), grep tests.rs for how existing tests get `Output` handles.

- [ ] **Step 2: Run, verify it fails** (panic in `clean_up_workspaces` or `verify_invariants` hidden-contiguity assert).

- [ ] **Step 3: Fix.** At the top of `insert_workspace` in `monitor.rs`:

```rust
    pub fn insert_workspace(&mut self, ws: Workspace<W>, idx: usize, activate: bool) -> usize {
        // A hidden workspace never belongs in the visible region (this happens when
        // moving a hidden workspace to another output). Route it to the hidden
        // block; it stays named, so it remains reachable, and can't be activated.
        if ws.hidden {
            let id = ws.id();
            self.insert_hidden_workspace(ws, idx);
            return self
                .workspaces
                .iter()
                .position(|w| w.id() == id)
                .unwrap_or(self.workspaces.len() - 1);
        }
        ...
```

(Adjust the `mut ws`/`mut idx` bindings: the early return consumes `ws` before the `mut` uses; keep `mut ws: Workspace<W>, mut idx: usize` as-is — the early return is fine with that.)

Also add `ensure_empty_before_hidden()` at the end of `insert_hidden_workspace` (after its `clean_up_workspaces()`): a hidden workspace arriving on a monitor that never had one has no guard empty yet. Check `insert_hidden_workspace`'s current tail; if `clean_up_workspaces` would remove a just-added guard, add the guard after cleanup.

- [ ] **Step 4: Run the test — pass. Full suite — pass.**

- [ ] **Step 5: Commit** `git commit -m "layout: keep hidden workspaces out of the visible region on insert"`

---

### Task 5: Hidden-aware `append_workspaces` (issue #14)

On output disconnect, `append_workspaces` pops the LAST workspace assuming it's the trailing empty — with a hidden block it pops a hidden workspace and splices incoming workspaces mid-block.

**Files:**
- Modify: `src/layout/monitor.rs` (`append_workspaces`, currently ~line 1102)
- Test: `src/layout/tests.rs`

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn output_disconnect_preserves_hidden_blocks() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        // Window + hidden named workspace on output 1 (primary).
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SetWorkspaceName {
            new_ws_name: 2,
            ws_name: None,
        },
    ];
    let mut layout = check_ops(ops);
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();

    // Disconnect output 2; its workspaces (incl. the hidden one) land on output 1.
    Op::RemoveOutput(2).apply(&mut layout);
    layout.verify_invariants();

    // Both hidden workspaces must be reachable again.
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();
}
```

Grep tests.rs for the exact `Op::FocusOutput`/`Op::RemoveOutput` variant names and window-to-output targeting (there is `Op::AddWindow` + a focused-output rule, or `TestWindowParams` has an output field; copy whatever `operations_from_starting_state_dont_panic` uses to place windows on specific outputs).

- [ ] **Step 2: Run, verify it fails** (hidden-contiguity assert in `verify_invariants`, or a wrongly-popped hidden workspace).

- [ ] **Step 3: Rewrite `append_workspaces`:**

```rust
    pub fn append_workspaces(&mut self, mut workspaces: Vec<Workspace<W>>) {
        if workspaces.is_empty() {
            return;
        }

        for ws in &mut workspaces {
            ws.set_output(Some(self.output.clone()));
            ws.update_config(self.options.clone());
        }

        // Incoming hidden workspaces go to the hidden block; their recorded
        // original_idx refers to the dead monitor's layout and is meaningless here.
        let (incoming_visible, mut incoming_hidden): (Vec<_>, Vec<_>) =
            workspaces.into_iter().partition(|ws| !ws.hidden);
        for ws in &mut incoming_hidden {
            ws.original_idx = None;
        }

        // The visible region ends with the empty trailing workspace (which also
        // guards the hidden block when one exists). Splice the incoming visible
        // workspaces right before it, and append incoming hidden ones at the end.
        let visible_end = self
            .workspaces
            .iter()
            .position(|ws| ws.hidden)
            .unwrap_or(self.workspaces.len());
        let empty_idx = visible_end - 1;
        let empty_id = self.workspaces[empty_idx].id();
        let empty_was_focused = self.active_workspace_idx == empty_idx;

        for _ in 0..incoming_visible.len() {
            self.shift_hidden_original_indices_for_insertion(empty_idx);
        }
        self.workspaces.splice(empty_idx..empty_idx, incoming_visible);
        self.workspaces.extend(incoming_hidden);

        // If empty_workspace_above_first is set and the first workspace is now no
        // longer empty, add a new empty workspace on top.
        if self.options.layout.empty_workspace_above_first
            && self.workspaces[0].has_windows_or_name()
        {
            self.add_workspace_top();
        }

        // If the empty workspace was focused on the primary monitor, keep it focused.
        if empty_was_focused {
            self.active_workspace_idx = self
                .workspaces
                .iter()
                .position(|ws| ws.id() == empty_id)
                .unwrap();
        }

        // FIXME: if we're adding workspaces to currently invisible positions
        // (outside the workspace switch), we don't need to cancel it.
        self.workspace_switch = None;
        self.clean_up_workspaces();
    }
```

- [ ] **Step 4: Run the test — pass. Full suite — pass.**

- [ ] **Step 5: Commit** `git commit -m "layout: splice appended workspaces before the hidden block on output removal"`

---

### Task 6: `set_workspace_name` compensation on the owning monitor; `hide_workspace_by_idx(0)` stale idx (issue #16)

Two defects: (a) `Layout::set_workspace_name` (mod.rs ~6111) resolves the workspace on ANY monitor but applies the empty-top/empty-bottom compensation to `monitors[active_monitor_idx]`; (b) `hide_workspace_by_idx` with `empty_workspace_above_first && idx == 0` calls `add_workspace_top()` and keeps using stale idx 0, hiding the fresh empty instead of the target.

**Files:**
- Modify: `src/layout/mod.rs` (`set_workspace_name`), `src/layout/monitor.rs` (`hide_workspace_by_idx`)
- Test: `src/layout/tests.rs`

- [ ] **Step 1: Failing test for (a):**

```rust
#[test]
fn naming_workspace_on_non_active_monitor_compensates_there() {
    let options = /* empty_workspace_above_first: true — same construction as Task 2 */;
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        // Focus stays on output 1 (active monitor). Output 2's monitor has
        // [emptyTop, empty] or [empty] depending on setup.
    ];
    let mut layout = check_ops_with_options(options, ops);

    // Name the FIRST workspace of the non-active monitor by id.
    let wsid = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => monitors[1].workspaces[0].id(),
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    layout.set_workspace_name(
        "ws1".to_string(),
        Some(WorkspaceReference::Id(wsid.get() /* check the Id repr */)),
    );
    layout.verify_invariants();
}
```

Check `WorkspaceReference::Id`'s payload type (grep `WorkspaceReference` in mod.rs / niri-ipc) and how tests construct it; `find_workspace_by_ref` is the resolver. RED: `verify_invariants` fails with "first workspace must be unnamed when empty_workspace_above_first is set" — on monitor 1, because the compensating `add_workspace_top` went to monitor 0.

- [ ] **Step 2: Run, verify RED for (a).**

- [ ] **Step 3: Fix (a).** In `set_workspace_name`, replace the compensation block:

```rust
        if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            // Apply the compensation on the monitor that actually owns the
            // workspace — it is not necessarily the active one.
            let Some(monitor) = monitors
                .iter_mut()
                .find(|mon| mon.workspaces.iter().any(|ws| ws.id() == wsid))
            else {
                return;
            };
            if monitor.options.layout.empty_workspace_above_first
                && monitor
                    .workspaces
                    .first()
                    .is_some_and(|first| first.id() == wsid)
            {
                monitor.add_workspace_top();
            }
            // The named workspace may have been the last visible (trailing empty /
            // hidden-block guard) — restore an empty one after it.
            let visible_end = monitor
                .workspaces
                .iter()
                .position(|ws| ws.hidden)
                .unwrap_or(monitor.workspaces.len());
            if visible_end > 0 && monitor.workspaces[visible_end - 1].id() == wsid {
                monitor.add_workspace_bottom();
            }
        }
```

(`add_workspace_bottom` inserts at the visible end after Task 3, so this also repairs the hidden-block guard case that the old `.last()` check missed.)

- [ ] **Step 4: Failing test for (b)** — manufacture the illegal-but-reachable-in-release state directly:

```rust
#[test]
fn hide_workspace_at_index_zero_hides_the_right_workspace() {
    let options = /* empty_workspace_above_first: true — as above */;
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];
    let mut layout = check_ops_with_options(options, ops);

    let monitor = match &mut layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &mut monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    // Simulate the pre-fix set_workspace_name defect: a named workspace at idx 0.
    monitor.workspaces[0].name = Some("ws1".to_string());

    monitor.hide_workspace_by_idx(0);

    // The named workspace must be the hidden one — not the freshly added empty.
    let hidden: Vec<_> = monitor.workspaces.iter().filter(|ws| ws.hidden).collect();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].name.as_deref(), Some("ws1"));
}
```

(`ws.name` is a pub(crate)-visible field — already accessed as `ws.name.clone()` throughout monitor.rs; tests are in the same crate.)

- [ ] **Step 5: Run, verify RED for (b)** (the hidden workspace is unnamed).

- [ ] **Step 6: Fix (b).** In `hide_workspace_by_idx`:

```rust
    pub fn hide_workspace_by_idx(&mut self, mut idx: usize) {
        if idx == self.workspaces.len() - 1 {
            return;
        }
        if self.options.layout.empty_workspace_above_first && idx == 0 {
            self.add_workspace_top();
            idx += 1;
        }
        ...
```

- [ ] **Step 7: Run both tests — pass. Full suite — pass.**

- [ ] **Step 8: Commit** `git commit -m "layout: fix workspace-name compensation monitor and stale hide idx"`

---

### Task 7: Randomized-op coverage for hide/unhide (issue #17, second half)

**Files:**
- Modify: `src/layout/tests.rs` (`enum Op`, `Op::apply`, the static op lists in `operations_dont_panic` and `operations_from_starting_state_dont_panic`)

- [ ] **Step 1: Add the op variant** to `enum Op` (next to `UnsetWorkspaceName`):

```rust
    ToggleWorkspaceVisibility(#[proptest(strategy = "1..=5usize")] usize),
```

- [ ] **Step 2: Handle it in `Op::apply`** (next to the `UnsetWorkspaceName` arm):

```rust
            Op::ToggleWorkspaceVisibility(ws_name) => {
                layout.toggle_workspace_visibility(format!("ws{ws_name}"));
            }
```

- [ ] **Step 3: Add instances to the static op lists** in `operations_dont_panic` and `operations_from_starting_state_dont_panic` (alongside the other workspace ops):

```rust
        Op::ToggleWorkspaceVisibility(1),
        Op::ToggleWorkspaceVisibility(2),
```

- [ ] **Step 4: Run the fast suite** (`cargo test --lib`) — pass. **Then run the slow randomized tests time-boxed:**
`direnv exec . bash -c 'RUN_SLOW_TESTS=1 cargo test --lib operations 2>&1' | tail -20` (10-minute timeout).
The proptest suite (`every_op` / `random_operations_dont_panic` — whatever `#[proptest]`/`proptest!` blocks exist) picks the new variant up automatically via the derive.

- [ ] **Step 5: If proptest finds failures:** minimize (proptest prints the failing op sequence), write the minimized sequence as a named regression test, fix the underlying bug, and re-run. Budget: this is the step where unknown-unknowns surface; treat each as its own mini red-green cycle. Do NOT weaken `verify_invariants` to make failures go away — every failure here is a real state-machine bug.

- [ ] **Step 6: Commit** `git commit -m "layout: exercise workspace hide/unhide in randomized op tests"`

---

### Task 8: Finalize

- [ ] **Step 1:** `direnv exec . bash -c 'cargo fmt --all && cargo fmt --all && cargo fmt --all -- --check'` — must exit 0.
- [ ] **Step 2:** `direnv exec . bash -c 'cargo clippy -p niri -p niri-config -p niri-ipc --all-targets 2>&1' | grep -E "^error"` — no errors, and no NEW warnings in touched code.
- [ ] **Step 3:** Full suite one last time: `cargo test --lib` — all pass.
- [ ] **Step 4:** Merge `hidden-ws-issues` into `barrulus-custom` (`--no-ff`, message "Merge: fix hidden-workspace issues #12-#17"), push origin.
- [ ] **Step 5:** Close issues #12–#17 with `gh issue close <n> --repo barrulus/biri --comment "Fixed in <commit sha>: <one-line what/how>"` — reference the specific commit for each.
