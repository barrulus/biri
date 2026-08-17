# Shader Audio — `niri_audio` uniforms (sound-reactive shaders)

Design spec. Written 2026-08-17. Implements the **audio/FFT** item from roadmap 3.5 "Richer
uniforms" in `docs/superpowers/global-shader-next-steps.md` — the unlock for the visualizer
class (Shadertoy music shaders, audio-reactive borders/backgrounds). Pick this up cold;
everything needed to plan and build is here.

> Status discipline: **[confirmed]** = verified by code reading this session;
> **[design]** = proposed here, not yet built.

---

## 1. Problem & goal

Shaders today can react to time, the cursor, and their own feedback — but not to sound.
The goal: every shader (global multi-pass chain, region shaders, per-window shaders) can
read the machine's audio as a Shadertoy-style spectrum/waveform texture plus a handful of
convenience scalars, with capture running **only while an audio-referencing shader is
active**, and every failure mode degrading to silence rather than a broken shader.

Inspiration: a demoed custom compositor feeds FFT (32 bands + beat) to all shaders as a
uniform. Its mechanism is fully portable to biri's shader engine; nothing about it needs
that compositor's architecture.

## 2. Decisions (settled in brainstorm)

1. **Contract shape:** Shadertoy-style audio texture (spectrum row + waveform row) **plus**
   convenience scalars. Ports of Shadertoy music shaders should be near-mechanical.
2. **Capture lifecycle:** on-demand — extend the existing compile-time source scan
   (`GlobalShaderCaps`); capture runs only while an active shader references audio.
3. **Source:** default sink monitor ("what's playing") by default, with a v1 config knob to
   pin a specific PipeWire node (an app, or a mic for voice-reactive effects).
4. **Scope:** available to global, region, and per-window shaders alike (shared plumbing).
5. **Pipeline (Approach A):** dedicated audio thread owns PipeWire capture + FFT + beat
   detection and publishes a small snapshot; the render path only reads the latest snapshot.
   No variable-cost DSP on the render hot path.

## 3. Shader contract [design]

New names, valid in niri-mode and hyprland-mode sources, all shader kinds:

**Sampler `niri_audio`** — 512×2 texture, 8-bit single-channel, values 0..1:

- Row 0 (`v = 0.25`): FFT spectrum, bin 0 = lowest frequency, 512 bins covering 0–12 kHz.
- Row 1 (`v = 0.75`): raw waveform, the most recent 512 samples remapped from [−1, 1] to 0..1.

This is exactly Shadertoy's sound-input layout (their `iChannel0` sound texture), so ports
replace `texture(iChannel0, vec2(x, 0.0)).x` mechanically. Helpers hide the row coordinates:

```glsl
float niri_fft(float x);   // spectrum at x in 0..1 (bin 0..511)
float niri_wave(float x);  // waveform at x in 0..1
```

**Scalar uniforms** (all `float`, 0..1):

| Uniform | Meaning |
|---|---|
| `niri_audio_level` | overall level (mean of spectrum) |
| `niri_audio_bass` | band energy ~20–250 Hz |
| `niri_audio_mid` | band energy 250 Hz–4 kHz |
| `niri_audio_treble` | band energy 4–12 kHz |
| `niri_audio_beat` | 1.0 at a detected beat onset, decaying linearly to 0 over ~150 ms |

`niri_audio_beat` is a ready-made envelope: simple pulse shaders need no state of their own.

**Analysis mapping** (mimic WebAudio `AnalyserNode` defaults for Shadertoy fidelity):
48 kHz mono input; FFT size 2048 with Hann window; magnitude → dB, mapped from
[−100 dB, −30 dB] to 0..1; per-bin temporal smoothing `s = 0.8·s_prev + 0.2·s_new`;
first 512 of the 1024 bins go in the texture (bin width ≈ 23.4 Hz → 0–12 kHz).

8-bit is deliberate and safe: Shadertoy's sound texture is also 8-bit, and this is fresh
input data each frame, not an accumulation buffer — the `feedback-buffer-8bit-decay` gotcha
does not apply.

## 4. Config [design]

One new optional top-level KDL block:

```kdl
shader-audio {
    node "alsa_input.usb-mic..."   // optional: pin a PipeWire node by name
}
```

- Block absent or `node` absent → capture the **default sink's monitor** and follow
  default-sink changes (delegated to the PipeWire session manager; see §7 risk R2).
- `node` present → capture that node instead (any source, including a mic).
- Parsed in `niri-config` (new `shader_audio.rs`, small struct + `Config` field + `"shader-audio"`
  dispatch), with a parse snapshot test — same pattern as `global-shader`.
- Privacy posture (documented in the wiki page): the compositor holds an open capture stream
  **only** while an audio-referencing shader is active; verifiable with `pw-top`.

No FFT/smoothing/beat tuning knobs in v1 (see non-goals, §9).

## 5. Capture + analysis subsystem [design]

New module `src/shader_audio.rs`, behind the existing `pipewire` cargo feature
(**[confirmed]** `pipewire = { version = "0.10.0", optional = true, ... }` is already in
`Cargo.toml` for screencast).

- `ShaderAudio` handle owned by `Niri`: `start(config)`, `stop()`, `snapshot()`.
- `start` spawns one worker thread running its own PipeWire `MainLoop` with a capture
  stream: F32 mono 48 kHz; `stream.capture.sink = true` for monitor-of-default-sink mode, or
  `target.object = <node>` when configured. PipeWire does mixdown/resampling.
- The thread keeps a 2048-sample ring buffer. Each process callback: append samples, run the
  real FFT (`realfft` crate — pure Rust, brings `rustfft`), update smoothed bins, band
  scalars, and the beat detector, then publish an `AudioSnapshot` behind `Arc<Mutex<…>>`
  (uncontended; writer is the audio thread, reader the render path).

```rust
struct AudioSnapshot {
    spectrum: [u8; 512],   // dB-mapped, smoothed
    waveform: [u8; 512],
    level: f32, bass: f32, mid: f32, treble: f32,
    beat_at: Option<Instant>,  // render side computes the 150 ms decay envelope
    seq: u64,                  // bumped on every publish; render skips upload if unchanged
    updated_at: Instant,       // staleness detection, §7
}
```

- **Beat detector:** spectral flux over the bass bins (sum of positive per-bin increases)
  against an adaptive threshold (mean + k·stddev over a ~1 s history), with a ~250 ms
  refractory period. Explicitly documented as tunable, not sacred — v1 ships fixed constants.
- **On-demand lifecycle:** whenever the active shader set changes (config reload, shader
  cycle, window-rule change), the main thread computes "does any active program use audio"
  (§6 scan flag) and starts/stops the worker accordingly. Stop joins the thread. The stream's
  presence/absence is observable in `pw-top`.
- **Shutdown:** compositor exit stops the worker cleanly (same path as stop).

## 6. Engine integration [design, anchors confirmed]

**Source scan.** `GlobalShaderCaps` (**[confirmed]** `niri-config/src/global_shader.rs`,
`scan` / `scan_chain` / `is_animating()`) gains `uses_audio: src.contains("niri_audio")
|| src.contains("niri_fft") || src.contains("niri_wave")` — same check in both niri and
hyprland modes (the names don't collide with hyprland aliases). `is_animating()` adds
`|| self.uses_audio`.

**Redraw scheduling — no new machinery.** The animation gate in `src/niri.rs`
(**[confirmed]** ~5862–5959) already computes `global_shader_animate` /
`region_shader_animate` / `window_shader_animate` from `is_animating()` and applies the
`shader-animation-max-fps` throttle plus visible-window culling. `uses_audio` flowing into
`is_animating()` rides all of it unchanged: audio shaders redraw continuously, capped by the
fps cap, and an audio shader on an invisible window costs nothing.

**Texture upload.** A per-renderer `ShaderAudioState` cached in EGL user data (same pattern
as `Shaders::get`) owns one persistent 512×2 GLES texture, zero-filled at creation. Once per
frame, early in `render_inner`, if audio is active: read the snapshot, and if `seq` changed,
`glTexSubImage2D` the two rows. Constant tiny cost; no per-frame allocation
(per the `per-frame-gpu-alloc-latency` rule). Multi-renderer note: each renderer that draws
shaders gets its own state/texture via its own EGL user data; no cross-context sharing.

**Uniform push.** At the existing `ShaderRenderElement` construction sites —
**[confirmed]** `src/render_helpers/global_shader_element.rs` (per pass) and
`src/render_helpers/scoped_shader_element.rs` (~line 170, where `niri_time`/`niri_cursor`
are pushed) — if and only if the program's `uses_audio` flag is set: append the audio
texture to the texture list and the five scalars (beat envelope computed from `beat_at` at
push time) to the uniform list. Declarations (sampler, five uniforms, two helper functions)
go into **both preludes** — `global_prelude.frag` and `global_hypr_prelude.frag`
(**[confirmed]** shared by global/region/scoped compilation via `compile_global_program`).
Push-only-when-used respects the `shader-uniform-must-be-declared` gotcha in reverse:
never push what the linked program may have optimized away wholesale.

## 7. Failure modes & risks

Everything degrades to **silence, never breakage**: zeros in the texture and scalars, shader
still runs.

- **F1 — built without `pipewire` feature:** `shader_audio` module compiles to a stub whose
  snapshot is permanently zero. One-time log line if a shader uses audio.
- **F2 — stream/connect failure, configured node missing:** snapshot stays zero; warn once.
  Reconnect is attempted when the shader set or `shader-audio` config changes. No background
  retry loop in v1.
- **F3 — sink suspends when playback stops:** process callbacks cease and the snapshot
  freezes at its last non-zero values. The render side treats `updated_at` older than
  ~200 ms as silence and eases pushed values toward zero over a few frames, so visuals decay
  instead of freezing mid-pulse.
- **R1 — risk: redraw churn from start/stop.** Lifecycle changes only happen on shader-set /
  config changes, which already trigger recompiles; no extra churn expected.
- **R2 — risk: default-sink follow.** Monitor streams opened with `capture.sink = true` are
  expected to be moved by WirePlumber on default-sink change; **must be verified on
  hardware** (plug/unplug headphones). If it does not follow, v1 fallback is documented
  behavior ("reconnects on config reload") and a listener is a v2 item.
- **R3 — risk: quantum size vs. FFT hop.** Callback cadence depends on the graph quantum
  (~256–1024 samples ⇒ ~5–21 ms). One FFT per callback over the sliding 2048-window is
  well within budget (a 2048-pt real FFT is microseconds), but verify CPU on hardware.

## 8. Testing

**Unit (no hardware, no PipeWire):** the analysis stage is a pure function
`fn analyze(&mut AnalysisState, samples: &[f32]) -> AudioSnapshot` — feed it synthetic input:

- 440 Hz sine → peak in the right bin; energy in `bass`≈0, `mid` high; silence → all zeros.
- Pulse train at 120 BPM → beat detector fires ~2 Hz with refractory respected.
- dB mapping endpoints: full-scale sine ≈ 1.0 in its bin; −100 dB floor → 0.
- `GlobalShaderCaps` scan: `niri_audio*`/`niri_fft`/`niri_wave` set `uses_audio` and
  `is_animating()`; unrelated sources don't.
- `niri-config`: `shader-audio` block parse snapshot (node present/absent/empty block).

**Hardware checklist (sixseven):**

1. Bar-spectrum global shader + music: bars move, correct band placement (bass left).
2. Silence, then pause → suspend: visuals ease to zero (F3), no frozen pulse.
3. `pw-top`: stream appears only while an audio shader is active; cycling to a non-audio
   shader (Mod+4) closes it; `off.kdl` closes it.
4. `node` override with a mic: voice reacts; removing the node in config → F2 silence.
5. Default-sink switch (headphone plug/unplug) → R2 verification.
6. Global + per-window audio shaders simultaneously; audio window on hidden workspace does
   not force redraws (visible-culling still applies).
7. `shader-animation-max-fps 30` caps an idle audio shader's recomposite rate.
8. Screencast (portal + KMS) behavior unchanged from current global-shader rules.
9. nvtop + intel_gpu_top during 1–7 (per `sixseven-gpu-wiring`, watch both).

**Validation shaders (acceptance):** a fresh bar-spectrum visualizer plus one ported
Shadertoy music shader added to the user's `~/.config/niri/global-shaders/` cycle set. These
prove the porting story end to end and update the `converting-global-shaders` skill with the
audio mapping table.

## 9. Non-goals (v1)

- Multiple simultaneous sources or per-shader source selection.
- Waveform history / scrolling spectrogram textures.
- Configurable FFT size, smoothing constant, dB range, or beat-detector tuning.
- Background reconnect loops; hot-following config changes beyond reload.
- Any change to screencast/screenshot shader policy (stays as-is).

## 10. Code map (anchors, all [confirmed] on `barrulus-custom`)

- Scan flags: `niri-config/src/global_shader.rs` — `GlobalShaderCaps::{scan, scan_chain, is_animating}`.
- Animation/redraw gate + fps throttle: `src/niri.rs` ~5862–5959.
- Global pass uniform push: `src/render_helpers/global_shader_element.rs`.
- Region/window (scoped) uniform push: `src/render_helpers/scoped_shader_element.rs` ~170.
- Preludes (shared by all custom shader kinds): `src/render_helpers/shaders/global_prelude.frag`,
  `global_hypr_prelude.frag`; compiled in `src/render_helpers/shaders/mod.rs`
  (`compile_global_program`, `ProgramType::{Global, GlobalPass, Scoped}`).
- Renderer-cached state pattern to copy: `Shaders::get` (EGL user data) in
  `src/render_helpers/shaders/mod.rs`.
- Existing PipeWire dep (screencast): `Cargo.toml` `pipewire 0.10.0` optional, feature
  `xdp-gnome-screencast`.
- Config parse + snapshot tests: `niri-config/src/lib.rs`.
