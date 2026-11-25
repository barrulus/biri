# Contributing to biri

biri is a fork of [niri](https://github.com/niri-wm/niri). This guide covers contributions to biri, with development guidance adapted from upstream below. The LLM policy here applies to this repository; other projects set their own contribution policies.

You can also support biri development by [supporting barrulus on Ko-fi](https://ko-fi.com/barrulus).

## Where things go

- **Bugs** → [Issues](https://github.com/barrulus/biri/issues/new/choose). If the problem also happens on upstream niri, report it there instead.
- **Ideas and feature requests** → [Discussions → Ideas](https://github.com/barrulus/biri/discussions/new?category=ideas). Search first and upvote an existing idea rather than opening a duplicate: upvotes are how ideas get prioritised.
- **Questions** → [Discussions → Q&A](https://github.com/barrulus/biri/discussions/new?category=q-a).

## Themes

Every issue and discussion carries exactly one `theme:` label. The bug report and idea forms ask for it, and a workflow applies the label (falling back to a keyword guess, or `needs-theme` when it cannot tell). The themes are:

| Label | Scope |
|---|---|
| `theme: shaders` | global / region / window shaders, presets, feedback buffers, shaders in capture |
| `theme: overview` | carousel overview, zoom presets, lens, grid and rounded-corner asks |
| `theme: workspaces` | named, hidden and scratch workspaces, placement rules, per-output fallbacks |
| `theme: floating` | floating and sticky windows, PiP, float-above-fullscreen, snapping |
| `theme: layout` | tiling, columns, tabs, vertical layout, fullscreen and maximize behaviour |
| `theme: window-rules` | matchers, focus and activation rules, per-app animation or effect overrides |
| `theme: input-binds` | binds, chords and modes, multi-action binds, gestures, runtime device toggles |
| `theme: outputs` | isolated outputs, per-output colour, HDR/VRR, multi-GPU, resolutions |
| `theme: ipc` | `niri msg`, IPC events and properties, scripting surface |
| `theme: capture` | screencast, screenshots, block-out, portals |
| `theme: upstream-sync` | merges from upstream niri, drift fixes, fast-tracked upstream PRs |
| `theme: project` | build, Nix and PKGBUILD packaging, CI, docs, wiki |

`bug` marks defects and is orthogonal to the theme. There are no status labels: status lives on the [Biri project board](https://github.com/users/barrulus/projects/5), which tracks committed work only. `.github/scripts/theme_field_sync.py` mirrors theme labels onto the board's Theme field.

## From idea to issue

Promoting an idea is a maintainer decision: promote, merge into an existing idea, close, or keep collecting votes. A promoted idea becomes an issue with the discussion body copied and a footer `Promoted from discussion <url>`, and the issue link is posted back on the discussion. The discussion stays open until the work ships, then is closed with a note on what shipped.

## Pull requests

- Target `main`. The `upstream` branch is a pristine mirror of niri and never receives fork commits.
- Reference the issue the PR closes; the PR inherits that issue's theme.
- Run `cargo fmt`, `cargo clippy`, and the tests. For wiki changes, `cd docs && uv run --locked mkdocs build` must pass in strict mode.

## LLM policy

LLM-assisted and LLM-generated contributions are welcome in biri, including code, tests, documentation, issues, and reviews. If you use an LLM, you are responsible for reviewing its output, checking that the work does what it claims, and following through on feedback. The same quality standards apply to every contribution, regardless of the tools used.

- **Understand and own the submission.** Read the complete change before submitting it. Be able to explain the problem, how the solution works, and any significant tradeoffs. You are welcome to learn with an LLM; ask for help when something remains unclear.
- **Verify that it works.** Run the relevant tests and checks, and exercise changed behaviour where practical. For bug fixes, reproduce the failure and check that the fix resolves it. Add focused regression coverage when useful, testing observable behaviour rather than simply repeating the implementation.
- **Report what you actually checked.** Include the commands or manual steps you ran and their results. State anything you could not test, including hardware or environment limitations. An LLM's claim that tests pass is not a test result.
- **Check factual claims.** Verify referenced code, APIs, links, logs, and reproduction steps. Distinguish observed behaviour from a suspected cause or a proposed solution. Do not invent evidence or present generated examples as observations from your system.
- **Keep submissions focused and readable.** Follow the project's existing patterns, remove unrelated changes, and edit generated explanations down to what a reviewer needs. For issues and discussions, describe the problem and relevant context in your own reviewed submission.
- **Follow through on review.** Answer questions, investigate problems, make corrections, and re-test changes. Do not leave maintainers to validate an unreviewed generated patch on your behalf.

No LLM-use declaration or attribution is required. Reviews focus on correctness, clarity, maintainability, and evidence that the work has been checked.

---

# Development guidance adapted from niri

Thanks for your interest in niri!
The project has grown quite a bit, and we could use all help that we can.

Make sure to join our Matrix chat if you have any questions or want to discuss anything: https://matrix.to/#/#niri:matrix.org

## Issues and discussions

This is a good way to help many new and existing users without programming knowledge.

- Answer and help people in GitHub issues and discussions.
- Check and point out duplicate issues.
- Check for issues that are likely application bugs (and not niri bugs).
    - Ask or try to reproduce on another non-Smithay-based compositor (sway, KDE/KWin, GNOME/Mutter). If the issue reproduces, it's likely an application bug.
    - Ask or try to reproduce on another *Smithay-based* compositor ([cosmic-comp], [anvil]). If the issue reproduces only on Smithay compositors, it may be a Smithay bug.
    - Make sure you're testing the Wayland version of the app on all compositors. Apps may silently use X11 when an X11 `$DISPLAY` is available.
    - Problems with X11 apps should be reported to [xwayland-satellite]. When testing xwayland-satellite on different compositors, make sure you use xwayland-satellite's `$DISPLAY` (rather than another compositor's built-in Xwayland `$DISPLAY`).
    - After testing, mention where you could and couldn't reproduce, as well as the exact steps to reproduce if the issue is missing them.
- Try to reproduce the issue on your own system and write if you could or couldn't reproduce it.
- Upvote issues with a thumbs up reaction as you like.
- Ideas and feature requests from new users should go to Discussions.

If your issue is a duplicate, or not a niri issue (application bug, hardware problem, configuration problem), then please close it.

## Reviewing and testing pull requests

With the growing popularity, the volume of pull requests is honestly more than I can manage myself in my free time.
I would really appreciate help with testing and reviewing them.

### Testing

Pick a pull request you like, then build it and give it a go.
The [Developing niri wiki page](https://niri-wm.github.io/niri/Development:-Developing-niri) has guidance on running niri test builds.

Be really thorough with your testing.
We're striving for polished features in niri, so point out any issues and bugs, even small ones like animation jank.

- Think of weird edge cases or unexpected interactions and try them to see that they work reasonably.
- Try to break the feature and check that it behaves well.
- Where applicable, try different input devices: keyboard, mouse, trackpad, tablet, touchscreen.
- Watch out for any new performance drops.

For bug fixes, first make sure you can reproduce the bug, then do the same steps in the PR test build, and verify that the bug is fixed.
Be similarly thorough: test any similar or related edge cases to verify that the fix doesn't introduce any new problems.

Write your findings in the pull request: any issues you found, or if everything worked well.
Re-test after the author updates the code to see that your issues were fixed.

Don't hesitate to test even if someone else already did; very frequently different people will stumble upon different problems.

### Reviewing

Reviewing pull requests is something I need the most help with since there are a lot of them, and it's quite time-consuming.
Anyone with code accepted into niri is welcome, but this is not a requirement; even if you aren't familiar with Rust you may find some logic problems.

Pick a pull request, then review its code.

- Check that everything looks good, check various conditions for edge cases.
- See if there are any scenarios the author forgot to handle.
- Check that the code fits well into the rest of niri, follows its design and code style.
    - I understand this is vague. The idea is: look at the surrounding code and at similar modules (e.g. when implementing a new protocol, check other protocol implementations), and try to follow the style and structure.
- Check for unrelated changes that may be better split into their own pull request.
- Check that the wiki had been updated if necessary (for example, new config options were documented with examples, and have a correct Since annotation).

Point out everything you find as review comments (don't forget to submit the review).
Be constructive and respectful; some people may be new to programming and Rust.
As the author addresses the comments and issues, check the code again to see that the problems were fixed.
If everything looks good, say that, so I know someone has reviewed the PR.

As with testing, don't hesitate to look through and comment even if someone else already had.
Extra pairs of eyes catch more problems.

## Writing pull requests

When creating pull requests, please keep the following in mind.

- Make sure new features align with niri's design directions. Ideally, there should be an existing issue or discussion where we settled on that solution.
- Keep pull requests focused on a single feature or bug fix with no unrelated changes.
- Document unobvious design and architecture decisions. Why you chose one way over the other, which alternatives you tried. This helps to evaluate the code.
- Try to split your changes into small, self-contained commits. Every commit should build and pass tests. This makes it much easier to review your PR, and bisect for regressions in the future.
    - When addressing PR comments, try to squash the changes straight into the relevant commits.
    - In some cases when the requested changes are big/unclear, you can leave them as separate commits on top, but please squash and otherwise clean up the history when the changes are finalized.
    - To update the main branch, please rebase instead of merging. Try to force-push the main update rebase separately from other changes, this way it's easy to skip during review since it's usually not interesting.
    - When working on bigger features, I usually start with a big messy commit, then gradually split out smaller self-contained changes from it as the code gets into shape.
    - [git-rebase.io](https://git-rebase.io/) is a helpful guide for splitting commits and cleaning up history in git.
- When you address a review comment, mark it as resolved.
- Remember to [run tests](https://niri-wm.github.io/niri/Development:-Developing-niri#tests) and format the code with `cargo +nightly fmt --all`.
- For new layout actions, remember to add them to the randomized tests. For weird Wayland handling, adding client-server tests in `src/tests/` could be very useful.
- Test your changes by hand thoroughly, including for edge cases and weird interactions. See the Testing section above for some tips.
- Remember to document new config options on the wiki.
- When opening a pull request, ensure "Allow edits from maintainers" is enabled, so I can make final tweaks before merging.

### How to get your pull request reviewed more quickly

- Make it small and self-contained. Avoid mixing several unrelated changes in one PR.
- Split the PR into small and self-contained commits. This makes it much easier to review.
- Discuss new features, options, or behavior changes beforehand; make sure there's consensus about the design.
- When creating the pull request, clearly write what it does, what problem it solves, how to test it.
- Follow the rest of the advice from this document.


[cosmic-comp]: https://github.com/pop-os/cosmic-comp
[anvil]: https://github.com/Smithay/smithay/tree/master/anvil
[xwayland-satellite]: https://github.com/Supreeeme/xwayland-satellite
