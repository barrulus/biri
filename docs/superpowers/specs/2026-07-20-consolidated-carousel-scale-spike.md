# Consolidated Carousel Overview — Task 4 Scale-Mismatch Spike: Findings

**Spike run:** 2026-07-20 (laptop) → 2026-07-21 (sixseven, office).
**Prototype:** `spike/carousel-scale` @ `e2f825fd` (throwaway; +31 lines `src/niri.rs` —
a `CarouselCard` `OutputRenderElements` variant + a hardcoded block in `render_inner`
that composites one sibling monitor's `render_workspaces` output into the host via
`scale_relocate_crop(elem, output_scale, card_scale=0.3, card_geo=(80,300)+(640,420))`,
gated on `in_carousel_regime()`).
**Verdict:** Direct compositing (plan Approach 1) is **viable**. Proceed to Phase 2.

## Question

Can a sibling monitor's `render_workspaces` elements composite into the **host**
output's `render_inner` via `Rescale → Relocate → Crop` — with acceptable
correctness and latency — including when the host and sibling **output scales
differ**? (The alternative, per-sibling offscreen buffers, was pre-rejected for the
NVIDIA per-frame-alloc latency cliff.)

## Method

Two setups, same laptop (eDP-1):

- **Laptop / iGPU:** eDP-1 (1920×1080, scale 1) + HDMI-A-1 TV (1920×1080, scale 1).
- **sixseven / dGPU:** eDP-1 (1920×1080, scale 1) + DP-2 ROG PG348Q ultrawide
  (3440×1440, scale 1) — DP-2 is hard-wired to the NVIDIA dGPU, and 21:9 (different
  aspect ratio from eDP-1).

Passes: (1) both outputs scale 1 (match); (2) sibling forced to `scale 1.5` via live
config reload (mismatch), observed in **both directions** (eDP-1 host / DP-2 sibling,
and DP-2 host / eDP-1 sibling).

## Findings

### 1. Direct compositing works — confirmed on both iGPU and NVIDIA dGPU

The sibling's `render_workspaces` output composites into the host element list
through the existing `RescaleRenderElement` + `RelocateRenderElement` +
`CropRenderElement` primitives, exposed as one new `OutputRenderElements` variant.
No new render infrastructure, **no offscreen buffers**. The card renders on the
first build, on both GPUs.

### 2. Perf: no latency cliff, no GPU spike

On the NVIDIA dGPU path (DP-2), with the card visible: **no noticeable lag and no
nvtop spike.** As predicted, the per-frame-alloc latency cliff does not apply because
this path allocates nothing offscreen — it only adds elements to the existing pass.
This confirms Approach 1 over Approach 2 (offscreen) decisively.

### 3. Containment holds — including under scale mismatch

The sibling is cropped to its card box in every case tested (match, and mismatch in
both directions). No bleed into host territory, no off-screen jump, no crash. This
answers YaLTeR's overflow objection **structurally, on real hardware**.

### 4. Scale mismatch is NON-CATASTROPHIC but NOT auto-normalized (the core answer)

With the sibling at `scale 1.5` and the host at `scale 1.0`, the card renders and
stays boxed — but its **size tracks the sibling's scale**: the DP-2 card on the
scale-1 eDP-1 host was visibly larger than when DP-2 was scale 1.

**Cause:** `Monitor::render_workspaces` lays its elements out in *its own*
(sibling) physical pixels — `geo.loc.to_physical_precise_round(self.scale)` and
`CropRenderElement::from_element(elem, self.scale, …)` both use the **sibling**
scale. `scale_relocate_crop` then wraps and crops using the **host**
`output_scale`. The two scales are never reconciled, so a sibling whose scale
differs from the host renders proportionally larger/smaller by roughly
`sibling_scale / host_scale`.

**Phase 2 correction:** normalize the card by that ratio — either multiply the
card rescale factor by `sibling_scale / host_scale`, or render the sibling through
a scale-aware path so the composited card is scale-invariant. This is a **bounded,
known correction**, not a redesign. It is the single concrete thing the spike was
run to determine.

### 5. Siblings inherit the overview zoom → double-zoom (Phase-2 must control card zoom)

`set_monitors_overview_state` (Phase 1) calls `set_overview_progress(progress)` on
**every** monitor, not just the active one. So a sibling — even with
`overview_open == false` — inherits the host's overview progress and renders its
*own* overview zoomed, which `card_scale` then shrinks *again*. The observed card is
a tiny overview-of-an-overview.

**Phase 2 must render the card at a controlled zoom** independent of the host's
overview progress — either stop propagating `overview_progress` to non-active
outputs, or drive the sibling's card render at an explicit zoom (e.g. 1.0 for a live
tucked card, or a chosen preview zoom). This also decides what the card *shows*: the
sibling's live active workspace vs its full overview.

### 6. Card gate is global → draws on non-focused outputs too (Phase-2 must gate)

The prototype gate `if self.layout.in_carousel_regime()` is layout-global, so the
card composites in **every** output's `render_inner` pass — including outputs that
are not the focused/overview host (where `mon.overview_open == false`). That would
put a card on a physical monitor that is supposed to stay live and unchanged.

**Phase 2 must gate the card to the focused output only** — e.g. render it only when
the host `mon.overview_open` is true (or by comparing the render `output` to the
active monitor's output).

### 7. Card edge is a HARD crop — Phase 2 must add the edge fade

Observed by comparing a sibling's own screen vs its card: an output rendering its
**own** overview lets the infinite workspace run off the physical edge into black
(no crop). The **card**, by contrast, is clipped by `CropRenderElement` to its box —
a **binary** pixel cut, very visible when the sibling is a wide ultrawide whose
workspace exceeds the card box. This is correct *containment* but crude *visually*.

The design always called for **faded card edges** (the "faded horizontal edges" /
tucked-card treatment in the brainstorming mockups). The spike skipped it.
`CropRenderElement` cannot fade — it is a hard clip — so the fade is **additional**
Phase-2 work: a gradient-alpha mask (or shader) over the card margins that fades
content to transparent toward the box edge. This yields containment AND a soft
edge. (A "show the overflow but dimmed" variant is rejected: showing un-clipped
overflow reintroduces exactly the neighbor-overflow YaLTeR objected to. Fade-to-
transparent over a bounded margin is the correct middle ground.)

## Coordinate-space notes for Phase 2

- `scale_relocate_crop(elem, output_scale, zoom, ws_geo)` expects `ws_geo` (the card
  destination + crop box) in **host logical** coords; it converts to host physical via
  `output_scale`, rescales the element about `(0,0)` by `zoom`, relocates by the
  physical loc, and crops to the physical box. Reusable as the card wrapper.
- The wrapped type `CropRenderElement<RelocateRenderElement<RescaleRenderElement<
  MonitorRenderElement<R>>>>` needs its own `OutputRenderElements` variant
  (`CarouselCard` in the prototype) — the macro generates the `From` impl.
- `workspaces_with_render_geo` culls against the **sibling's own** `view_size`. For a
  card showing the sibling's live active workspace this is fine; if Phase 2 shows the
  sibling's full overview in the centered card, confirm the intended workspaces
  survive culling.
- Borrows: `self.layout.monitor_for_output(output)` (host) and
  `self.layout.monitors()` (siblings) are both shared borrows and coexist; the sibling
  render reuses `ctx.r()`. No borrow obstacles observed.

## Phase 2 render task — concrete shape (from this spike)

1. Composite each participating sibling (non-host, non-isolated, non-empty) as a card
   via `scale_relocate_crop`, **gated to the focused output** (finding 6).
2. Render each sibling card at a **controlled zoom**, not the inherited overview
   progress (finding 5).
3. **Normalize the card scale by `sibling_scale / host_scale`** so cards are
   scale-invariant across mixed-DPI outputs (finding 4).
4. **Fade the card edges** with a gradient-alpha mask over the crop margin, not a
   bare hard `CropRenderElement` clip (finding 7).
5. No offscreen buffers (findings 1–2).

## Cleanup owed after this doc lands

- Delete `spike/carousel-scale` (local + `origin`).
- Revert `quixote/flake.nix` biri input to `github:barrulus/biri/barrulus-custom`.
- Remove the temporary `consolidated-carousel` block from `~/.config/niri/config.kdl`
  (or keep it — harmless — once Phase 2 lands real behavior).
