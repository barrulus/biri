# Shaders in Capture — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single opt-in config switch (`shaders-in-capture`, default off) that renders the global, region, and window shaders into the screencast and screenshot/screencopy render paths — so Google Meet / OBS / portal screenshare, `grim`, and `wl-screenrec` show the effect — without corrupting the live output's feedback ping-pong.

**Architecture:** Add a top-level `shaders_in_capture: bool` to config. A `target_renders_shaders(target, capture_enabled)` predicate replaces the `target == Output` gate at the three shader sites (global + region in `niri.rs`, window in `tile.rs`). The window site reads the flag via `Tile.options` (carried through `Options::from_config`); the two `niri.rs` sites read `self.config` directly. For the stateful global shader, a capture render reads the live feedback (`prev`) but writes to throwaway sinks/offscreens so the live output's ping-pong is untouched. Region/window shaders are stateless (capture-and-shade) — gate-only.

**Tech Stack:** Rust, knuffel (KDL config), smithay GLES renderer, insta.

**Spec:** `docs/superpowers/specs/2026-06-23-shaders-in-capture-design.md` — read it first.

## Global Constraints

- **Default off.** Absent/false → NO shader in any capture, INCLUDING window shaders (which currently leak in unconditionally — this is the one intentional behavior change). On → global + region + window shaders render into BOTH `Screencast` and `ScreenCapture`.
- **Config node:** a bare flag `shaders-in-capture` (presence = on), parsed exactly like `prefer-no-csd` (`Flag::decode_node`). (The spec's `shaders-in-capture true` example becomes the bare-flag form — same idiom as `prefer-no-csd`/`enable`.)
- **Predicate:** `target == RenderTarget::Output || capture_enabled` (capture_enabled covers Screencast AND ScreenCapture).
- **Global feedback isolation:** when `ctx.target != Output`, the global element gets the live `prev`/`buffer_prev` (read) but FRESH throwaway `result`/`buffer_result`/`pass_offscreen`/`buffer`/`screen_result` (write) — never the shared `OutputState.global_shader_chain` sinks. The live output's ping-pong must be byte-unaffected.
- **No change** to `block-out-from`, 8-bit precision, the shader contract, or redraw scheduling.
- **Build/test crib:** dev shell `nix develop /home/barrulus/quixote#rust-compositor`; per-task `cargo check --no-default-features --features dbus,systemd` inside it with `export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib`. `niri-config` tests outside the shell: `cargo test -p niri-config`. Inline-snapshot gotcha: never `cargo insta accept` (hangs) — patch `.lib.rs.pending-snap` by hand. `cargo fmt` (plain) before each commit. Commits: NO Co-Authored-By / AI-attribution lines.

---

## File Structure

- **Modify** `niri-config/src/lib.rs` — `Config.shaders_in_capture: bool` field, `"shaders-in-capture"` dispatch, default snapshot, parse test.
- **Modify** `src/render_helpers/mod.rs` — `target_renders_shaders(target, capture_enabled) -> bool` helper near `RenderTarget`.
- **Modify** `src/layout/mod.rs` — `Options.shaders_in_capture: bool` + set it in `Options::from_config`.
- **Modify** `src/niri.rs` — global gate (`:4357`), region gate (`:4497`), global feedback isolation (`:~4444`), reading `self.config.borrow().shaders_in_capture`.
- **Modify** `src/layout/tile.rs` — window gate (`:1182`), reading `self.options.shaders_in_capture`.
- **Modify** `docs/wiki/Configuration:-Global-Shader.md` — document the switch.

---

## Task 1: Config — `shaders-in-capture`

**Files:**
- Modify: `niri-config/src/lib.rs`
- Test: `niri-config/src/lib.rs` (tests module)

**Interfaces:**
- Produces: `Config.shaders_in_capture: bool` (default `false`).

- [ ] **Step 1: Write the failing test**

Add to the tests in `niri-config/src/lib.rs`:

```rust
#[test]
fn shaders_in_capture_parses() {
    let on = Config::parse_mem("shaders-in-capture\n").unwrap();
    assert!(on.shaders_in_capture);
    let off = Config::parse_mem("").unwrap();
    assert!(!off.shaders_in_capture);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p niri-config shaders_in_capture_parses`
Expected: FAIL — `shaders_in_capture` field doesn't exist.

- [ ] **Step 3: Add the field + dispatch**

In `niri-config/src/lib.rs`:
- Add to `struct Config` (near `pub prefer_no_csd: bool,` ~line 80): `pub shaders_in_capture: bool,`.
- Add a dispatch arm next to `"prefer-no-csd"` (~line 249):

```rust
                "shaders-in-capture" => {
                    config.borrow_mut().shaders_in_capture = Flag::decode_node(node, ctx)?.0
                }
```

- [ ] **Step 4: Update the default-config inline snapshot**

`niri-config/src/lib.rs` has an inline `Debug` snapshot of the default `Config`. Adding the field changes it. Run `cargo test -p niri-config`, find the failing snapshot, and add `shaders_in_capture: false,` in the correct field-order position (mirror where `prefer_no_csd` appears in the snapshot — same relative order as the struct). Do NOT use `cargo insta accept`; patch the inline `@r#"…"#` by hand (or from `.lib.rs.pending-snap`, 8-space indent).

- [ ] **Step 5: Run the parse test + full suite**

Run: `cargo test -p niri-config`
Expected: PASS (parse test + snapshot + everything else).

- [ ] **Step 6: Commit**

```bash
git add niri-config/src/lib.rs
git commit -m "niri-config: shaders-in-capture flag"
```

---

## Task 2: Helper + Options + global/region gates + feedback isolation

**Files:**
- Modify: `src/render_helpers/mod.rs`
- Modify: `src/layout/mod.rs`
- Modify: `src/niri.rs`

**Interfaces:**
- Consumes: `Config.shaders_in_capture` (Task 1).
- Produces:
  - `pub fn target_renders_shaders(target: RenderTarget, capture_enabled: bool) -> bool` (in `render_helpers/mod.rs`).
  - `Options.shaders_in_capture: bool` (in `layout/mod.rs`) — consumed by Task 3.

- [ ] **Step 1: Add the predicate helper**

In `src/render_helpers/mod.rs`, near the `impl RenderTarget` block (~line 120), add a free function:

```rust
/// Whether shaders (global / region / window) should render for this target. Always on for the
/// real Output; on for capture targets (Screencast / ScreenCapture) only when opted in.
pub fn target_renders_shaders(target: RenderTarget, capture_enabled: bool) -> bool {
    target == RenderTarget::Output || capture_enabled
}
```

- [ ] **Step 2: Add `shaders_in_capture` to layout `Options`**

In `src/layout/mod.rs`: add `pub shaders_in_capture: bool,` to `struct Options` (~line 390), and in `Options::from_config(config: &Config)` (~line 650) add `shaders_in_capture: config.shaders_in_capture,` to the constructed `Self`. (This is how `Tile` — which holds `Rc<Options>` — learns the flag without threading `RenderCtx`.)

- [ ] **Step 3: Relax the GLOBAL gate + isolate feedback (niri.rs)**

In `src/niri.rs`, the global element block (~4352). First compute the flag once, then swap the gate. Change:

```rust
        let mut global_shader_elem: Option<GlobalShaderElement> = if ctx.target
            == RenderTarget::Output
            && Shaders::get(ctx.renderer)
                .program(ProgramType::GlobalPass(0))
                .is_some()
        {
```

to:

```rust
        let capture_enabled = self.config.borrow().shaders_in_capture;
        let capture_render = ctx.target != RenderTarget::Output;
        let mut global_shader_elem: Option<GlobalShaderElement> = if crate::render_helpers::target_renders_shaders(ctx.target, capture_enabled)
            && Shaders::get(ctx.renderer)
                .program(ProgramType::GlobalPass(0))
                .is_some()
        {
```

Then, in the `passes` construction (~4444) and the element `new(...)` call (~4459), make the WRITE handles throwaway when `capture_render`. Replace the `let passes = { … }` block with:

```rust
            let passes = {
                let mut chain = state.global_shader_chain.borrow_mut();
                chain.resize(n_passes);
                (0..n_passes)
                    .map(|i| GlobalPassState {
                        // Read the live trail either way.
                        prev: chain.prev[i].clone(),
                        buffer_prev: chain.buffer_prev[i].clone(),
                        // Capture renders write to throwaway sinks/offscreens so the live output's
                        // ping-pong is never corrupted; the Output render uses the real shared ones.
                        result: if capture_render {
                            std::rc::Rc::new(std::cell::RefCell::new(None))
                        } else {
                            chain.result[i].clone()
                        },
                        buffer_result: if capture_render {
                            std::rc::Rc::new(std::cell::RefCell::new(None))
                        } else {
                            chain.buffer_result[i].clone()
                        },
                        pass_offscreen: if capture_render {
                            std::rc::Rc::new(crate::render_helpers::offscreen::OffscreenBuffer::default())
                        } else {
                            chain.pass_offscreen[i].clone()
                        },
                        buffer: if capture_render {
                            std::rc::Rc::new(crate::render_helpers::offscreen::OffscreenBuffer::default())
                        } else {
                            chain.buffer[i].clone()
                        },
                    })
                    .collect::<Vec<_>>()
            };
            // screen_result is the niri_screen_prev ping-pong sink; throwaway for capture renders.
            let screen_result = if capture_render {
                std::rc::Rc::new(std::cell::RefCell::new(None))
            } else {
                state.global_shader_screen_result.clone()
            };
```

Then update the `GlobalShaderElement::new(...)` call's `screen_result` argument: it currently passes `state.global_shader_screen_result.clone()` — change that single argument to the new local `screen_result`. Leave the `screen_prev` argument as `state.global_shader_screen_prev.clone()` (read live). Leave `time`/`cursor`/`area`/etc. unchanged (the capture animates off the live clock — correct).

> IMPLEMENTER: confirm the exact `new(...)` argument list against `src/render_helpers/global_shader_element.rs` `pub fn new` — the order is `(id, area, scale, time, cursor, region_norm, output_size_phys, screen_prev, screen_result, passes)`. Only the `screen_result` arg changes (to the local), plus the `passes` block above.

- [ ] **Step 4: Relax the REGION gate (niri.rs)**

At `src/niri.rs:4497`, change:

```rust
        if ctx.target == RenderTarget::Output {
```

to:

```rust
        if crate::render_helpers::target_renders_shaders(ctx.target, capture_enabled) {
```

(`capture_enabled` is already in scope from Step 3 — it's computed earlier in the same `render_inner`. If the borrow-checker complains about the `self.config.borrow()` lifetime, bind `capture_enabled` to a plain `bool` at the top of `render_inner` so both gates use the copy.) Region elements are stateless `ScopedSource::Capture` — no isolation needed.

- [ ] **Step 5: Compile-check (dev shell)**

Run:
```
nix develop /home/barrulus/quixote#rust-compositor --command bash -c 'export LIBCLANG_PATH=/nix/store/wm3wq7p1a4wp5lw23b4rc8apak230f9f-clang-21.1.8-lib/lib; cd /home/barrulus/dev/biri && cargo check --no-default-features --features dbus,systemd 2>&1 | tail -30'
```
Expected: GREEN. The window gate (Task 3) is still ungated, so window shaders still render in capture until Task 3 — that's fine for this task's checkpoint. Run `cargo fmt`.

- [ ] **Step 6: Commit**

```bash
git add src/render_helpers/mod.rs src/layout/mod.rs src/niri.rs
git commit -m "shaders-in-capture: gate global + region shaders, isolate global feedback in capture renders"
```

---

## Task 3: Window-shader gate (tile.rs)

**Files:**
- Modify: `src/layout/tile.rs`

**Interfaces:**
- Consumes: `Options.shaders_in_capture` (Task 2), `target_renders_shaders` (Task 2), `ctx.target`.

- [ ] **Step 1: Gate the window-shader branch**

In `src/layout/tile.rs`, the window-shader branch (~1180) currently:

```rust
        if !pushed_resize {
            if let Some(resolved) = self.window.rules().shader.clone() {
```

Change the inner condition to also require the target/flag predicate:

```rust
        if !pushed_resize {
            if let Some(resolved) = self.window.rules().shader.clone().filter(|_| {
                crate::render_helpers::target_renders_shaders(
                    ctx.target,
                    self.options.shaders_in_capture,
                )
            }) {
```

(`self.options.shaders_in_capture` is the flag from Task 2; `ctx.target` is already available in `Tile::render`. `target_renders_shaders` is the Task 2 helper. This makes window shaders render for Output always, and for capture only when the flag is on — closing the current unconditional leak.)

- [ ] **Step 2: Compile-check (dev shell) — first full green**

Run the dev-shell `cargo check` (same command as Task 2 Step 5). Expected: GREEN. Run `cargo fmt`. Run `cargo test -p niri-config` (must stay green).

- [ ] **Step 3: Commit**

```bash
git add src/layout/tile.rs
git commit -m "shaders-in-capture: gate per-window shaders behind the flag (no unconditional capture leak)"
```

---

## Task 4: Docs + manual verification

**Files:**
- Modify: `docs/wiki/Configuration:-Global-Shader.md`

- [ ] **Step 1: Document the switch**

Add a short "Shaders in screencast / screenshots" section: the `shaders-in-capture` flag (default off); that it makes the global, region, and window shaders appear in portal screencast (Meet/OBS), `grim` screenshots, and `wl-screenrec`; that it's off by default (shaders don't leak into shares); that KMS capture (`gpu-screen-recorder -w <connector>`) always shows them regardless; and the one behavior note — with the flag OFF, window shaders are NOT in captures (they were, before this). Include a parsing example:

````markdown
```kdl
shaders-in-capture
```
````

- [ ] **Step 2: Verify it parses**

Run: `cargo test -p niri-config wiki_docs_parses`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add "docs/wiki/Configuration:-Global-Shader.md"
git commit -m "docs: shaders-in-capture"
```

- [ ] **Step 4: Manual hardware verification (owed; sixseven)**
  Per spec §5:
  1. `shaders-in-capture` + comet active → a portal capture (`gpu-screen-recorder -w portal`, or OBS) shows the comet trail; the LIVE output comet stays smooth (no stutter/corruption from the capture render). This is the key correctness check — the feedback isolation.
  2. `grim` screenshot and `wl-screenrec` (ScreenCapture) also show the shader.
  3. A region shader and a window shader (discord parchment) appear in a portal capture.
  4. Remove the flag → none appear in capture (incl. the window shader); live output unchanged throughout.

---

## Self-Review Notes (spec coverage)

- Spec §3.A config → Task 1. §3.B helper → Task 2 Step 1. §3.C three gates → Task 2 Steps 3–4 (global, region) + Task 3 (window). §3.D global feedback isolation → Task 2 Step 3. §4 example / §5 docs → Task 4.
- §5 testing → Task 1 unit test, Task 2/3 compile, Task 4 manual.
- §6 scope (one flag; reads-live-discards-writes; both capture targets; default off; window behavior change) → enforced; the window default change is called out in Global Constraints + Task 3 + docs.
- Config-node deviation from the spec's `shaders-in-capture true` to a bare `shaders-in-capture` flag (idiomatic, matches `prefer-no-csd`) — noted in Global Constraints.
- Type consistency: `target_renders_shaders(RenderTarget, bool) -> bool` defined Task 2 Step 1, used in Tasks 2–3; `Options.shaders_in_capture` defined Task 2 Step 2, used Task 3.
