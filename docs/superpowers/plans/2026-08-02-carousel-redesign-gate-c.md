# Carousel Cover-Flow Redesign — Gate C Plan (interaction + choreography + cleanup)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **Prerequisite:** Gate B complete and its Task 7 VM checkpoint recorded PASS in `.superpowers/sdd/progress.md`. Line anchors below describe the POST-Gate-B tree; re-locate with `rg` where drift is likely.

**Goal:** Make the cover-flow carousel drivable: pull-back rotation from any overview zoom, click-to-rotate on side panels, window-level focus-jump on the settled-remote lens, clean exit/hotplug semantics, and removal of all superseded code.

**Architecture:** A three-leg pull-back sequencer on `Layout` (zoom-out → rotate → zoom-in) reusing the existing zoom and rotation animations; per-frame panel hit-geometry cached on the host `OutputState` for input; focus-jump maps lens clicks through the inverse homography into the target output's overview geometry.

**Spec:** `docs/superpowers/specs/2026-08-01-carousel-redesign-design.md` (Behaviour → "Rotation and the pull-back choreography", "Selection and exit"). **Gate B interfaces:** `carousel_rotation/_target`, `rotate_carousel`, `carousel_reveal`, `carousel_ring`, `in_carousel_lens`, `PanelPlacement`, `panel_placement`, `PanelSource`, `panel_elems`.

## Global Constraints

- Same build rules as Gate B (devshell, no `+nightly`, no `cargo insta`, no `--release`).
- Pull-back runs the SAME choreography for every rotation trigger (keys, scroll, click); triggers differ only in the rotation target.
- Window-level interaction ONLY on the centered, settled panel; side panels are panel-level click targets.
- Closing the overview always returns rotation home (instant) and the host to its own desktop.
- Commit prefixes per file area; no AI attribution, no Co-Authored-By.

---

### Task 1: Pull-back sequencer

**Files:**
- Modify: `src/layout/mod.rs` — new state + driver beside the rotation animation from Gate B; `toggle_overview`/`close_overview` interaction

**Interfaces:**
- Produces:
  - `pub fn carousel_request_rotate(&mut self, delta: isize)` and `pub fn carousel_request_rotate_to(&mut self, ring_target: f64)` — THE entry points every input path uses. If `carousel_reveal() >= 1.` → plain `rotate_carousel` retarget. Else start a pull-back.
  - `enum PullbackPhase { ZoomOut, Rotate, ZoomIn }` + `struct Pullback { phase: PullbackPhase, target: f64, restore_zoom: f64 }`, field `carousel_pullback: Option<Pullback>`.
- Mechanics: `ZoomOut` retargets the active monitor's zoom animation to `assembled_zoom` (reuse `Monitor::animate_zoom_to` via a `Layout` helper — same call `overview_zoom_in/out` use, monitor.rs:1521); when the zoom animation completes (observe in the same advance-animations pass that drives rotation), transition to `Rotate` → `rotate_carousel` retarget to `target`; on rotation settle → `ZoomIn` retargets zoom back to `restore_zoom`; on completion clear `carousel_pullback`. A new rotate request mid-pullback just replaces `target` (stays in current phase). Closing the overview cancels the pullback outright.
- `restore_zoom` = the zoom target at request time (NOT current animated value), so repeated rotations return to where the user parked their zoom.

- [ ] **Step 1: Tests first** (in `src/layout/tests.rs`, using `set_overview_zoom_for_test` + advancing the clock the way existing animation tests do — find the clock-advance helper used by the overview gesture tests near tests.rs:3900 and mirror it):
  - `pullback_runs_three_phases`: two outputs, overview open at zoom 0.5, `carousel_request_rotate(1)`; assert phase transitions land rotation at 1.0 and zoom target back at 0.5.
  - `pullback_skipped_when_assembled`: zoom at 0.15, request → no pullback state, rotation animates directly.
  - `pullback_cancelled_by_overview_close`: mid-`Rotate`, `toggle_overview()` → pullback None, rotation snapped to 0.
- [ ] **Step 2:** RED run: `cargo test -p niri --lib pullback 2>&1 | tail -6`.
- [ ] **Step 3:** Implement; the phase driver lives where rotation/zoom animations already advance (Gate B Task 4's advance site) so there is exactly one place that observes animation completion.
- [ ] **Step 4:** GREEN run; also `cargo test -p niri --lib carousel 2>&1 | tail -5`.
- [ ] **Step 5: Commit** — `layout: pull-back choreography for carousel rotation`

---

### Task 2: Input — rotation triggers at any overview zoom

**Files:**
- Modify: `src/input/mod.rs` — the three intercepts: `FocusColumnLeft` arm (~1086-1097), `FocusColumnRight` arm (~1111-1122), Shift+scroll in overview (~3387-3400)

**Interfaces:** consumes `carousel_request_rotate` only.

- [ ] **Step 1:** Replace the `in_carousel_regime()` gates (that symbol is gone after Gate B) with: overview open && `consolidated_carousel` configured && `carousel_ring().len() > 1`. On match call `self.niri.layout.carousel_request_rotate(∓1)` + `queue_redraw_all` + early return — preserving each site's existing structure. Scroll keeps its per-tick accumulation exactly as today.
- [ ] **Step 2:** `cargo check` clean. Manual semantics note for the checkpoint: `←`/`→` now rotate from ANY overview zoom (previously only in-band).
- [ ] **Step 3: Commit** — `input: carousel rotation from any overview zoom`

---

### Task 3: Panel hit-testing — click side panels to rotate

**Files:**
- Modify: `src/niri.rs` — the Gate B panel-stack block in `render_inner` (cache hit geometry), `OutputState` (one field)
- Modify: `src/render_helpers/panel_quad.rs` — one new pure fn
- Modify: `src/input/mod.rs` — pointer button handling in the overview branch (the block containing the "click to activate window and close the overview" comment, ~3015)

**Interfaces:**
- `panel_quad`: `pub fn point_in_quad_uv(corners: &[[f64; 2]; 4], p: (f64, f64)) -> Option<(f64, f64)>` — bbox-normalize `p`, apply `sampling_matrix`, return uv if inside [0,1]²; `None` outside or degenerate.
- `OutputState.panel_hits: RefCell<Vec<(f64, [[f64; 2]; 4])>>` — (ring_pos, corners) per drawn panel, refreshed by the panel-stack block each frame, nearest (highest z) FIRST; cleared when panels aren't drawn.
- Input: on left-button press in overview with panels drawn, walk `panel_hits` in order; first quad containing the pointer position (output-local logical coords): if its ring_pos is the settled center → fall through to Task 4's lens handling; else `carousel_request_rotate_to(ring_pos.round())`, consume the event.

- [ ] **Step 1:** TDD the pure fn (unit tests: inside-center returns ~(0.5, 0.5) for a flat quad; outside returns None; tilted quad round-trips a known corner).
- [ ] **Step 2:** Cache the hit list in the render block (corners are already computed there — push a copy).
- [ ] **Step 3:** Wire the button handler; `cargo check` + panel_quad tests green.
- [ ] **Step 4: Commit** — `niri: click side carousel panels to rotate`

---

### Task 4: Focus-jump on the settled-remote lens

**Files:**
- Modify: `src/input/mod.rs` — same button-handling block (settled-center case from Task 3); Enter handling in the overview key path (locate with `rg "toggle_overview_to_workspace" src/input/` — the click-activate path — and the keyboard confirm path near it)
- Modify: `src/layout/mod.rs` — one helper

**Interfaces:**
- `Layout::carousel_focus_jump(&mut self, uv: (f64, f64)) -> bool`: only when `in_carousel_lens()`. Map uv → the centered output's content logical coords: the panel content was rendered at `fill_zoom` over the target's own view (Gate B Task 5's recipe), so content point = `(uv.x * target_view.w, uv.y * target_view.h)` un-scaled by the same letterbox the prepass applied — reuse the prepass's `fill_zoom` computation, do not re-derive. Find the window under that point via the target monitor's `workspaces_with_render_geo_at_zoom(fill_zoom)` (monitor.rs:1719) + each workspace's window geometry at that zoom (mirror how the existing click-to-activate resolves a window in overview — locate the resolution used by `toggle_overview_to_workspace`'s callers in `src/input/move_grab.rs:96` and follow the same window-lookup, adapted to the target monitor). On hit: activate that window (`ws.activate_window`), focus the target output (`focus_output`), close the overview (`close_overview`), rotation snaps home (Task 5's exit rule), return true. No hit (backdrop/gaps): return false (click does nothing).
- Enter on settled-remote lens: jump to the target's currently-focused window (same path, skipping the uv lookup).

- [ ] **Step 1:** Layout-level test: two outputs, rotate to 1, settled; `carousel_focus_jump((0.5, 0.5))` activates a window on output 2, overview closed, active output = output 2.
- [ ] **Step 2:** RED → implement → GREEN (`cargo test -p niri --lib focus_jump` + carousel filter).
- [ ] **Step 3:** Wire input (click via Task 3's fall-through; Enter via the keyboard confirm path).
- [ ] **Step 4: Commit** — `layout: focus-jump from carousel lens to real output`

---

### Task 5: Exit, hotplug, and state refresh

**Files:**
- Modify: `src/layout/mod.rs` — `toggle_overview`/`close_overview`/`close_overview_preserving_zoom` (~4754-4830), `add_output`/`remove_output` (locate: `rg "fn remove_output" src/layout/mod.rs`), `set_monitors_overview_state` callers, `focus_output` / active-output change path

**Interfaces:** none new — semantics:

- Any overview close: `carousel_pullback = None; carousel_rotation = 0.; carousel_rotation_anim = None;` (instant home — the close animation is the overview's own).
- `remove_output`: if the ring loses the output the rotation currently points at, snap rotation home (instant) and drop its `panel_elems` entry (host state) — the `PanelSource` dies with its `OutputState`.
- Active-output change while overview open (focus moves to another monitor): re-invoke `set_monitors_overview_state` (the Phase-2c leftover; call site = wherever `focus_output` commits the change) and reset rotation home — the ring is host-relative and must rebase.
- Single output: `carousel_ring().len() <= 1` already makes every entry point a no-op — add one regression test asserting `carousel_request_rotate` with one output changes nothing.

- [ ] **Step 1:** Tests: `overview_close_snaps_rotation_home`, `output_removal_recenters_ring`, `rotate_noop_single_output`.
- [ ] **Step 2:** RED → implement → GREEN (carousel + overview filters).
- [ ] **Step 3: Commit** — `layout: carousel exit, hotplug, and refocus semantics`

---

### Task 6: Cleanup + docs

**Files:**
- Modify: `src/layout/mod.rs` — delete `carousel_participants` (1804-1813; verify zero callers: `rg carousel_participants`)
- Modify: `src/niri.rs` — delete `CarouselFade` variant (~7548) and its `BorderRenderElement` fade-strip remnants IF Gate B's render task left any (verify: `rg CarouselFade`)
- Modify: `docs/wiki/Configuration:-Overview.md` (or the fork's overview config doc — locate with `rg "consolidated-carousel" docs/`) — document `reveal-zoom`/`assembled-zoom`, the behavior, and that `activation-zoom`/`expand-zoom` are gone
- Modify: `resources/default-config.kdl` — commented example reflects the new nodes
- Modify: `~/.config/niri-biri/config.kdl` line ~50 (`// BIRI: consolidated-carousel { activation-zoom 0.25; }`) — CONTROLLER does this one (outside repo), not the implementer

**Steps:**
- [ ] **Step 1:** Deletions with `rg` verification each; `cargo check` clean; full lib test run for the layout: `cargo test -p niri --lib 2>&1 | tail -6` (known-flaky snapshot tests excluded — if unrelated failures appear, report them, don't fix them).
- [ ] **Step 2:** Docs updated.
- [ ] **Step 3: Commit** — `chore: remove superseded carousel code and update docs`

---

### Task 7: Final checkpoint (STOP — needs the human) + whole-branch review

**Controller:** rebuild VM (two-output config from Gate B Task 7; `rm -f flake.lock` first); after user verification, run the final whole-branch review over Gates B+C combined (`review-package <gate-B-base> HEAD`) on the most capable model, folding in both gates' recorded Minor findings.

**Human checks:**
1. `Mod+O` → `→` at normal zoom: full pull-back story (zoom out, gallery assembles, swing, zoom back onto the sibling's overview as the lens).
2. Click a side panel: same choreography to that output.
3. On the lens: click a window → focus lands on that window on the physical sibling monitor, overview closes. Enter → same for the focused window.
4. Ctrl+wheel zoom through the band: reveal is continuous, no pop at either threshold.
5. Exit via every path (Mod+O, Esc, click-through, focus-jump) → host desktop restored, next overview opens centered on host.
6. Hotplug: `niri msg output ... off/on` (or QEMU display toggle) mid-carousel → no crash, rotation home.
7. Hardware (sixseven, real session when convenient): the full choreography on the ultrawide + laptop pair; watch nvtop for the settled-carousel idle state (damage-gated ⇒ near-zero GPU when nothing moves).
