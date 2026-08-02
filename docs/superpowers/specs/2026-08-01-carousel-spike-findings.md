# Carousel Cover-Flow Redesign — Spike Findings (Gate A)

**Date:** 2026-08-02
**Plan:** `docs/superpowers/plans/2026-08-01-carousel-redesign-spike.md`
**Spike code:** commits `7d5c8bbe..92bdc870` (behind `NIRI_PANEL_SPIKE=1`)

## Verdict: PASS on both gate criteria — Approach A (retained damage-gated offscreen + homography quad) is validated. Gate B/C planning may proceed.

### Criterion 1 — NVIDIA latency: PASS

Nested winit session on sixseven, forced onto the RTX 5060 Laptop GPU via
`__NV_PRIME_RENDER_OFFLOAD=1` +
`__EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/10_nvidia.json`
(default EGL device selection lands on the Intel iGPU — verify with nvtop
whenever re-testing).

- Static content: sweep perfectly smooth (transform-only frames; offscreen
  not re-rendered).
- Worst case (ticking-clock terminal → panel content damaged every frame):
  smooth sweep, niri at ~10% GPU / 106 MiB VRAM on the RTX, **no VRAM
  growth**, no frame hitches. The `OffscreenBuffer` texture-uniqueness
  recreation path (offscreen.rs:114, provoked deliberately by the retained
  texture clone) did NOT reproduce the historical ~1 s NVIDIA latency cliff.
- **Double-buffer ping-pong fallback: not needed.** Single retained
  `OffscreenBuffer` per output is sufficient.
- Incidental: the Intel iGPU path was also observed smooth (~67% Render/3D
  under continuous damage) before the offload vars were added.
- Caveat: this was an EGL-Wayland nested session, not the DRM/TTY backend.
  The real-session confirmation happens implicitly during Gate B hardware
  testing (sixseven's DP-2 is hard-wired to the dGPU). Given the historical
  cliff also manifested through the same driver allocation paths, confidence
  is high.

### Criterion 2 — virgl correctness: PASS

Biri VM (virtio-gpu-gl / virgl), `NIRI_PANEL_SPIKE=1` via systemd user-unit
environment in the VM flake:

- Correct trapezoid perspective; near edge visibly magnified (matches
  `panel_quad` math).
- Panel content **right-side up** — the anticipated Y-flip correction in
  `panel.frag` was NOT needed with `OffscreenBuffer`-sourced textures.
- Live content updates inside the panel (ticking clock) while the panel
  sweeps; no niri journal errors; compositor stays responsive.

## Known spike artifacts (do not carry into Gate B)

1. **Unscoped forced redraws** while the flag is on:
   `unfinished_animations_remain |= panel_spike_enabled()` runs for every
   output, including ones with no panel. Gate B must scope redraw scheduling
   to actual panel animation state.
2. **Panel is topmost over everything**, including layer-shell overlays —
   centered launchers (fuzzel) open invisibly behind it. Spike-only z-order;
   Gate B's panel stack has its own z-order design (backdrop → side panels
   back-to-front → center panel).
3. `_sync` fence from `OffscreenBuffer::render` is discarded. No tearing was
   observed on either GPU or virgl, but if artifacts ever appear, this is
   the first suspect — Gate B should thread the SyncPoint through properly.
4. Panel content zoom is hardcoded 0.5 and renders the host's own overview;
   real sizing (`fill_zoom`) and sibling outputs are Gate B work.

## Environment notes for future spike/VM runs

- The fork does not compile in the plain devshell on current nixpkgs
  (libdisplay-info 0.4.0 vs the smithay pin); run the nix-built binary from
  the VM closure instead (`nix path-info -r ~/dev/biri-vm/result | grep
  niri-`).
- `~/dev/biri-vm` silently writes `flake.lock` pinning the biri input if a
  build runs while the tree is clean; `rm -f flake.lock` after branch
  switches and verify the closure's `niri-<rev>` before testing.
