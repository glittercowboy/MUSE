# DSP Primitives

All 37 built-in DSP functions available in `process` blocks. These compile to real-time-safe Rust code.

## Oscillators

Generate audio signals. Return type: `Signal`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `sine(freq)` | `Frequency → Signal` | Sine wave oscillator |
| `saw(freq)` | `Frequency → Signal` | Sawtooth wave oscillator |
| `square(freq)` | `Frequency → Signal` | Square wave oscillator |
| `triangle(freq)` | `Frequency → Signal` | Triangle wave oscillator |
| `noise()` | `() → Signal` | White noise generator (no parameters) |
| `pulse(freq, width)` | `Frequency, Number → Signal` | Pulse wave with variable duty cycle |
| `lfo(rate)` | `Rate → Signal` | Low-frequency oscillator (sine wave, for modulation) |

### Usage

```muse
// Instrument mode — frequency from MIDI
let osc = saw(note.pitch)

// With literal frequency
let tone = sine(440Hz)

// Pulse wave with variable width (0.5 = square, 0.1 = narrow)
let pw = pulse(note.pitch, param.width)

// LFO for modulation (dedicated primitive, cleaner than sine at sub-audio rates)
let mod_signal = lfo(param.rate)
let mod_gain = 1.0 - param.depth + param.depth * mod_signal
input -> gain(mod_gain) -> output
```

Oscillators work in both instrument and effect plugins. Each call site maintains its own phase state. `lfo` is a dedicated modulation primitive — use it instead of `sine` when the intent is modulation rather than audio-rate sound generation.

## Filters

Process audio through frequency-selective circuits. Return type: `Processor` (use with `->` chain).

| Function | Signature | Description |
|----------|-----------|-------------|
| `lowpass(cutoff, resonance?)` | `Frequency, Number? → Processor` | Passes frequencies below cutoff |
| `highpass(cutoff, resonance?)` | `Frequency, Number? → Processor` | Passes frequencies above cutoff |
| `bandpass(cutoff, resonance?)` | `Frequency, Number? → Processor` | Passes frequencies near cutoff |
| `notch(cutoff, resonance?)` | `Frequency, Number? → Processor` | Rejects frequencies near cutoff |

- `cutoff`: frequency in Hz (use `Hz` or `kHz` suffix, or a param reference)
- `resonance`: optional, 0.0–1.0 (default: 0.707 if omitted)

### Usage

```muse
// In a chain
input -> lowpass(param.cutoff, param.resonance) -> output

// With literal values
input -> highpass(200Hz) -> output

// Omitting resonance (uses default 0.707)
input -> bandpass(1000Hz) -> output

// Notch filter to remove a specific frequency
input -> notch(60Hz, 0.9) -> output
```

## EQ / Shelving Filters

Parametric and shelving equalizer bands. Return type: `Processor`. Each call site maintains its own biquad state.

| Function | Signature | Description |
|----------|-----------|-------------|
| `peak_eq(freq, gain_db, q?)` | `Frequency, Gain, Number? → Processor` | Parametric EQ band (bell curve) |
| `low_shelf(freq, gain_db, q?)` | `Frequency, Gain, Number? → Processor` | Low shelf filter — boost/cut below freq |
| `high_shelf(freq, gain_db, q?)` | `Frequency, Gain, Number? → Processor` | High shelf filter — boost/cut above freq |

- `freq`: center/corner frequency in Hz
- `gain_db`: boost/cut amount in dB (positive = boost, negative = cut)
- `q`: optional Q factor (default: 0.707 if omitted). Higher = narrower band for peak_eq, steeper slope for shelves.

### Usage

```muse
// 4-band parametric EQ chain
input
  -> low_shelf(param.low_freq, param.low_gain)
  -> peak_eq(param.mid1_freq, param.mid1_gain, param.mid1_q)
  -> peak_eq(param.mid2_freq, param.mid2_gain, param.mid2_q)
  -> high_shelf(param.high_freq, param.high_gain)
  -> output

// Simple high-frequency cut
input -> high_shelf(8000Hz, -3.0) -> output
```

## Envelopes

Generate time-varying control signals (0.0–1.0). Return type: `Envelope`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `adsr(attack, decay, sustain, release)` | `Time, Time, Number, Time → Envelope` | ADSR envelope generator |
| `ar(attack, release)` | `Time, Time → Envelope` | Attack-release envelope |

- `attack`, `decay`, `release`: time in ms or s
- `sustain`: level 0.0–1.0 (Number, not Time)

### Usage

```muse
// Envelope modulating gain
let env = adsr(param.attack, param.decay, param.sustain, param.release)
osc -> gain(env) -> output

// Simple AR envelope
let amp = ar(10ms, 200ms)
```

Envelopes are driven by MIDI gate state — they respond to `note.gate` automatically.

## Time-Based Effects

Delay-based processors for echo, chorus, flanging, and phasing. Return type: `Processor`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `delay(time)` | `Time → Processor` | Delay line |
| `mod_delay(time, depth, rate)` | `Time, Number, Frequency → Processor` | Modulated delay for chorus/flanger |
| `allpass(time, feedback)` | `Time, Number → Processor` | Schroeder allpass (for phasers) |
| `comb(time, feedback)` | `Time, Number → Processor` | Feedback comb filter |

- `time`: delay length in seconds or milliseconds (use `s` or `ms` suffix)
- `feedback`: feedback amount, typically 0.0–0.95 (>1.0 will blow up)
- `depth`/`rate`: modulation parameters for mod_delay

### Usage

```muse
// Simple echo: delay + mix dry/wet
let delayed = input -> delay(param.time) -> gain(param.mix_amt)
mix(input, delayed) -> output

// Phaser: chained allpass stages
input
  -> allpass(param.rate_val, param.depth)
  -> allpass(param.rate_val, param.depth)
  -> allpass(param.rate_val, param.depth)
  -> allpass(param.rate_val, param.depth)
  -> output

// Comb filter for metallic/resonant effects
input -> comb(0.005s, 0.8) -> output
```

Each call site maintains its own delay buffer. Multiple allpass stages create deeper phasing.

## Dynamics

Processors for controlling signal level and dynamics. Return type: `Processor`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `compressor(threshold, ratio)` | `Gain, Number → Processor` | Dynamics compressor with envelope follower |
| `rms(window_ms?)` | `Time? → Processor` | Sliding-window RMS level measurement |
| `peak_follow(attack_ms?, release_ms?)` | `Time?, Time? → Processor` | Envelope follower (peak detection) |
| `gate(threshold_db?, attack_ms?, release_ms?, hold_ms?)` | `Gain?, Time?, Time?, Time? → Processor` | Noise gate — silences below threshold |

- `compressor`: threshold is linear gain (0.0–1.0, NOT dB), ratio is compression ratio (e.g. 4.0 = 4:1). Fixed ~10ms attack and ~100ms release.
- `rms`: optional window size in ms (default varies). Outputs RMS level of incoming signal.
- `peak_follow`: optional attack/release times. Tracks the peak envelope of the signal.
- `gate`: all params optional. `threshold_db` is in dB (use `dB` suffix). Silences signal below threshold with configurable timing.

### Usage

```muse
// Simple compression
input -> compressor(param.threshold, param.ratio) -> output

// Noise gate with explicit threshold
input -> gate(-40dB, param.attack, param.release, 10.0) -> output

// Gate with all defaults (useful as starting point)
input -> gate() -> output
```

Each call site maintains its own envelope follower state.

## Modulation

Time-based modulation effects. Return type: `Processor`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `chorus(rate, depth)` | `Rate, Number → Processor` | Chorus effect (modulated delay line) |

- `rate`: LFO frequency in Hz
- `depth`: modulation depth 0.0–1.0

### Usage

```muse
// Simple chorus
input -> chorus(param.rate, param.depth) -> output

// Chorus with gain control
input -> chorus(1.5, 0.4) -> gain(param.mix) -> output
```

Each call site maintains its own delay buffer and LFO phase.

## Waveshaping & Distortion

Nonlinear processors for adding harmonics and character. Return type: `Processor`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `tanh()` | `() → Processor` | Soft saturation (hyperbolic tangent) |
| `fold(amount)` | `Number → Processor` | Wavefolding distortion |
| `bitcrush(bits)` | `Number → Processor` | Bit depth reduction |
| `clip(min, max)` | `Number, Number → Processor` | Hard clip signal to range |
| `soft_clip(drive)` | `Number → Processor` | Soft saturation: `x/(1+|x|)` — gentler than tanh |

### Usage

```muse
// Soft saturation chain
input -> gain(param.drive) -> tanh() -> output

// Gentler saturation without tanh harshness
input -> soft_clip(param.drive) -> output

// Wavefolder (amount controls fold intensity, 1.0 = mild, 10.0 = aggressive)
input -> fold(param.drive) -> output

// Bitcrusher (16 = transparent, 4 = crunchy, 1 = extreme)
input -> bitcrush(param.bits) -> output

// Combined distortion chain
input -> fold(param.drive) -> bitcrush(param.bits) -> gain(param.mix) -> output
```

## Utilities

General-purpose audio processors and signal functions.

| Function | Signature | Return Type | Description |
|----------|-----------|-------------|-------------|
| `gain(amount)` | `Gain → Processor` | Processor | Apply gain (linear multiplier or dB with suffix) |
| `pan(position)` | `Number → Processor` | Processor | Stereo pan: -1.0 (left) to 1.0 (right) |
| `mix(dry, wet)` | `Signal, Signal → Signal` | Signal | Mix two signals (averages them) |
| `crossfade(a, b, mix)` | `Signal, Signal, Number → Signal` | Signal | Equal-power crossfade between two signals |
| `dc_block()` | `() → Processor` | Processor | Remove DC offset (first-order highpass) |
| `sample_and_hold(trigger)` | `Number → Processor` | Processor | Capture input on rising edge of trigger |
| `semitones_to_ratio(semitones)` | `Number → Number` | Number | Convert semitones to frequency ratio (2^(st/12)) |

**Note on `crossfade`:** Unlike most processors, `crossfade` is a **standalone function** returning `Signal`, NOT a chain `Processor`. Use it like `mix()`:

```muse
let dry = input
let wet = input -> lowpass(param.cutoff)
crossfade(dry, wet, param.mix) -> output
```

It uses equal-power crossfade: `a * sqrt(1 - mix) + b * sqrt(mix)`. At mix=0 you hear only `a`, at mix=1 only `b`, at mix=0.5 both at equal power with no volume dip.

### Usage

```muse
// Gain with param reference
input -> gain(param.volume) -> output

// Mixing dry/wet
let dry = input
let wet = input -> lowpass(param.cutoff)
mix(dry, wet) -> gain(param.mix) -> output

// Equal-power crossfade (better than mix for wet/dry blending)
let dry = input
let wet = input -> lowpass(param.cutoff) -> gain(2.0)
crossfade(dry, wet, param.blend) -> output

// DC blocking after distortion (removes accumulated DC offset)
input -> fold(param.drive) -> dc_block() -> output

// Sample-and-hold for glitchy/stepped effects
input -> sample_and_hold(param.trigger) -> output

// MPE pitch bend: convert semitones to frequency ratio
let bent_freq = note.pitch * semitones_to_ratio(note.bend)
let osc = saw(bent_freq)
```

## Quick Reference Table

| Category | Functions |
|----------|-----------|
| Oscillators (7) | `sine`, `saw`, `square`, `triangle`, `noise`, `pulse`, `lfo` |
| Filters (4) | `lowpass`, `highpass`, `bandpass`, `notch` |
| EQ / Shelving (3) | `peak_eq`, `low_shelf`, `high_shelf` |
| Envelopes (2) | `adsr`, `ar` |
| Time-Based (4) | `delay`, `mod_delay`, `allpass`, `comb` |
| Dynamics (4) | `compressor`, `rms`, `peak_follow`, `gate` |
| Modulation (1) | `chorus` |
| Waveshaping (5) | `tanh`, `fold`, `bitcrush`, `clip`, `soft_clip` |
| Utilities (7) | `gain`, `pan`, `mix`, `crossfade`, `dc_block`, `sample_and_hold`, `semitones_to_ratio` |

**Total: 37 functions**

## Sample Playback

Play back declared audio samples. These are **audio primitives** — they operate on named `sample` declarations, not on the DSP registry. Requires a `sample <name> "<path>"` declaration in the plugin.

| Function | Signature | Description |
|----------|-----------|-------------|
| `play(sample)` | `SampleName → Signal` | One-shot playback. Plays the named sample once from start to end, then outputs silence. Resets on each NoteOn. |
| `loop(sample)` | `SampleName → Signal` | Looped playback. Wraps position back to the beginning when end is reached, producing continuous output while the note is held. |
| `loop(sample, start, end)` | `SampleName, Number, Number → Signal` | Looped playback with start/end range. Position wraps within [start, end) as float sample positions. |

### Usage

```muse
// One-shot drum hit — plays the sample once per note
sample kick "samples/kick.wav"
// ... in process:
play(kick) -> gain(note.velocity) -> output

// Continuous loop — wraps when end is reached
sample pad "samples/pad.wav"
// ... in process:
loop(pad) -> gain(note.velocity) -> output

// Looped with specific region
loop(pad, 1000.0, 5000.0) -> output
```

- `play()` outputs silence after the sample ends — use for drums, one-shot SFX.
- `loop()` wraps position to 0.0 at end — use for sustained pads, textures, loops.
- `loop(sample, start, end)` wraps position to `start` at `end` — use for loop regions within a sample.
- All three reset their position to 0.0 on each MIDI NoteOn event.
- The sample name must match a declared `sample` in the plugin. E003 if unknown.

## Wavetable

Pitched wavetable oscillator with position morphing. This is an **audio primitive** — it operates on a named `wavetable` declaration, not on the DSP registry. Requires a `wavetable <name> "<path>"` declaration in the plugin.

| Function | Signature | Description |
|----------|-----------|-------------|
| `wavetable_osc(table, pitch, position)` | `WavetableName, Frequency, Number → Signal` | Wavetable oscillator with dual-axis interpolation. `pitch` tracks MIDI frequency, `position` (0.0–1.0) morphs between wavetable frames. |

### Usage

```muse
// Declare a wavetable (WAV file with concatenated frames, default frame_size 2048)
wavetable wt "samples/saw_stack.wav"

// ... in process:
let snd = wavetable_osc(wt, note.pitch, param.position)
snd -> gain(note.velocity) -> output
```

- The WAV file contains concatenated single-cycle waveform frames. Default frame size: 2048 samples.
- `pitch` controls playback frequency — typically `note.pitch` for MIDI tracking.
- `position` (0.0–1.0) selects/interpolates between frames — use a param for timbral morphing.
- Dual-axis interpolation: linear between adjacent frames (position axis) and between adjacent samples within a frame (pitch axis).
- The wavetable name must match a declared `wavetable` in the plugin. E003 if unknown.

## Audio Primitives Quick Reference

| Category | Functions |
|----------|-----------|
| Sample Playback (3) | `play`, `loop` (1-arg), `loop` (3-arg) |
| Wavetable (1) | `wavetable_osc` |

**Total: 37 DSP functions + 4 audio primitive call forms (3 sample playback + 1 wavetable)**
