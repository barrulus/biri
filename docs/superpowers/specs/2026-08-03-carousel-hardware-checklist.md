# Carousel Cover-Flow — Hardware Verification Checklist

**Run this on sixseven with a real second monitor connected** (deferred from the
QEMU checkpoint, 2026-08-03). Branch state: redesign complete @ `a0428f1e`,
245/245 lib tests, all reviews closed. Everything below is what only real
hardware can verify.

## Damage / perf (the redesign's core perf claims)

1. Settled assembled gallery, all content static → `nvtop`: GPU near-idle,
   VRAM flat. (Proves publish-on-damage actually engages — the spike only ever
   measured the heavier full-realloc path.)
2. Video or ticking terminal on the sibling → its panel updates live at
   content cadence; no VRAM growth over minutes.
3. mpvpaper animated wallpaper on the sibling → animates inside the panel
   (layer-shell commit nudge).
4. Real DRM/TTY session on the dGPU-wired DP-2: no latency cliff during
   rotation with damaged content (spike caveat: it was nested-EGL only).

## Mixed DPI (DP-2 @ 1.5 vs 1.0 — fix was code-verified only)

5. Panel of the scaled output: wallpaper/layers aligned with windows.
6. Lens click accuracy on the scaled output: click windows near each corner
   of the lens → correct window focused every time.
7. Flip DP-2 between scale 1 and 1.5 live with overview open → panel re-bakes
   cleanly, no stale squished texture, clicks stay accurate.

## Interaction / choreography feel

8. Pull-back from a zoomed-in overview: zoom-out → rotate → zoom-back to your
   parked zoom. Press `→` twice fast — the second press must land (fixed
   post-review; verify).
9. Ctrl+wheel mid-pull-back cancels it cleanly — your input wins, no fighting.
10. Close the overview at full reveal → judge whether the instant panel
    pop-off (before the zoom-out animation) looks acceptable.
11. **Design decision — lens size:** the settled-remote lens is a fixed 72%
    panel, NOT the full-screen takeover the spec originally promised. Judge on
    real glass: readable? If yes, the spec gets amended; if no, the center
    slot becomes reveal-responsive (small geometry change).
12. Close the remote's only window while lens'd on it → snaps home, no wedge
    (fixed post-review; verify).
13. Reveal band defaults 0.48/0.22 feel; concave taper; depth-dependent
    reveal travel (far panels travel further — eyeball it).
14. Real hotplug: unplug DP-2 with the overview open and rotated → snap home,
    no stale panel; replug rebuilds the ring.
15. Rotation start on the host: no black flash as the live strip hands off to
    the textured panel.
16. Undocked single-output sanity: carousel configured, zoom past reveal →
    nothing visible, no jank.

## Accepted-known (do not re-report)

- Panel clicks use `queue_redraw_all` (breadth matches neighboring actions).
- `Action::CloseOverview` mid-pull-back leaves the next open at assembled
  zoom once (self-heals on next toggle-close).
- Single-output configured carousel pays a hidden damage-gated self-prepass
  (perf-watch only, item 16 covers the visible side).
