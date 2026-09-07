# Virtual / headless outputs — carrying upstream niri PR #3800

Date: 2026-09-07
Issue: barrulus/biri#37
Upstream: niri-wm/niri#3800 by willybarret, branch `wip/virtual-outputs`.
Demand: upstream discussions #714 "Headless output" (55 upvotes) and #3101 "Virtual Output Use
Cases and API Design" (21 upvotes).

## Problem

Outputs that exist without a physical monitor: a headless session over SSH, a Sunshine/Moonlight
streaming target, a tablet used as a second screen, a wayvnc target, or several independent
streaming sessions at once. biri has none of this today.

## Why carry it rather than wait

The maintainer has not engaged with #3800 in five months. The fork already carries four
fast-tracked upstream PRs (#2997, #3302, #4062, #4105) under the same rationale, and the cost of
carrying this one turned out to be far lower than issue #37 estimated. If upstream merges its own
version, that replaces ours, exactly as with the other four.

## What the investigation found

Issue #37 was written from the 2026-09-03 upstream-demand survey and is out of date. A trial merge
of `willybarret/wip/virtual-outputs` into `main`, in a throwaway worktree, established:

- **One conflict, not five.** #37 predicts conflicts in `src/niri.rs`, `src/backend/tty.rs`,
  `src/layout/mod.rs`, `niri-config/src/output.rs` and `niri-ipc/src/lib.rs`. All five auto-merge.
  The sole conflict is in `src/ipc/client.rs`, where the fork's `Msg::WorkspacesWithHidden` arm and
  the PR's `Msg::CreateVirtualOutput` / `Msg::RemoveVirtualOutput` arms are inserted at the same
  point of one `match`. All three arms are additions; the resolution is to keep all three.
- **After resolving, the tree builds clean and 341 tests pass**, including the PR's own five
  `src/tests/virtual_output.rs` tests.
- **Two of the three "known rough edges" in #37 are already fixed on the branch.** Commit
  `11cb380b` ("fix crash removing off virtual outputs, expose their name to portals") closes both
  the remove-while-`off` crash and the portal-naming complaint, and ships
  `removing_off_virtual_output_does_not_panic` as a regression test. Commit `dc0505f0` fixes the
  `max_bpc` drift #37 mentions.
- **The branch is current**: zero commits behind upstream master, still `MERGEABLE` there, last
  updated 2026-09-04.

## Fork seams

#37 asks to check the shader gates on a virtual output. Reading the code resolves all three
fork-only interactions, and none needs code:

- **`isolated` works unchanged.** `Niri::is_output_isolated` (`src/niri.rs:3528`) resolves through
  `output.user_data().get::<OutputName>()` then `config.outputs.find(name)` — the same lookup the
  PR's `output "name" { create-virtual … }` config feeds. A virtual output can be isolated like any
  other.
- **The consolidated carousel needs no filter.** `Layout::carousel_outputs`
  (`src/layout/mod.rs:2158`) filters on `!is_isolated() && has_windows()`, so a virtual output with
  windows would otherwise appear as a cover-flow panel. Marking it `isolated` excludes it, and
  `isolated`'s stated purpose — "a clean capture feed" — describes the streaming case exactly. This
  is a documentation point, not a code change.
- **Shaders require the TTY backend.** `set_custom_global_passes` and `set_scoped_programs` are
  called at startup only from `src/backend/tty.rs:854`; neither `headless.rs` nor `winit.rs` calls
  them. A virtual output on the TTY backend shares that renderer and so is unaffected. On the
  headless backend shaders are inert at startup — though `headless.rs` does implement
  `with_primary_renderer`, so a later config reload (`src/niri.rs:1913`, `:1947`) compiles them.
  That inconsistency predates this PR; this PR only makes it worth caring about. Documented here,
  filed separately, deliberately not fixed in this merge.

## Scope

In scope:

- Merge `willybarret/wip/virtual-outputs` with a `Merge PR #3800: …` commit preserving authorship,
  resolving the one `src/ipc/client.rs` conflict by keeping all three match arms.
- Register the PR's `docs/wiki/Virtual-Outputs.md` in `docs/mkdocs.yaml`. The PR adds it to
  `docs/wiki/_Sidebar.md`, which is the GitHub-wiki nav — our docs site reads `mkdocs.yaml`, so
  without this the page ships but is unreachable.
- Two fork-specific caveats on that page: a streaming virtual output wants `isolated` (which also
  keeps it out of the consolidated carousel), and shader effects need the TTY backend.
- A fifth entry under README's "Fast-tracked upstream PRs".

Out of scope:

- Making the headless backend compile shaders at startup. Separate issue.
- Upstream discussion #3160 "Splitting a display". Adjacent but distinct.
- Any change to the PR's own code beyond the conflict resolution. If something is wrong with it,
  it should be reported upstream rather than diverged here, so the eventual upstream merge stays
  clean.

## Testing

The PR's five `virtual_output` tests come with the merge and pass. The full suite must stay green,
`cargo fmt --check` clean, and `cargo clippy --all-targets` must introduce no new warnings beyond
the known pre-existing upstream `needless_bool` at `niri-config/src/output.rs:245`.

Outstanding and explicitly not claimed as done: verification with Sunshine on sixseven, per #37.
The remove-while-`off` crash is covered by the branch's own regression test rather than by a
hardware check.
