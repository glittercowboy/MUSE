# DSP Primitives

All 24 built-in DSP functions available in `process` blocks. These compile to real-time-safe Rust code.

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

## Dynamics

Processors for controlling signal level and dynamics. Return type: `Processor`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `compressor(threshold, ratio)` | `Gain, Number → Processor` | Dynamics compressor with envelope follower |

- `threshold`: linear gain value (0.0–1.0), not dB
- `ratio`: compression ratio (e.g., 4.0 = 4:1)
- Fixed ~10ms attack and ~100ms release (not user-configurable in this version)

### Usage

```muse
// Simple compression
input -> compressor(param.threshold, param.ratio) -> output

// Compressor before output gain
input -> compressor(0.3, 4.0) -> gain(param.volume) -> output
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

### Usage

```muse
// Soft saturation chain
input -> gain(param.drive) -> tanh() -> output

// Wavefolder (amount controls fold intensity, 1.0 = mild, 10.0 = aggressive)
input -> fold(param.drive) -> output

// Bitcrusher (16 = transparent, 4 = crunchy, 1 = extreme)
input -> bitcrush(param.bits) -> output

// Combined distortion chain
input -> fold(param.drive) -> bitcrush(param.bits) -> gain(param.mix) -> output
```

## Utilities

General-purpose audio processors.

| Function | Signature | Return Type | Description |
|----------|-----------|-------------|-------------|
| `gain(amount)` | `Gain → Processor` | Processor | Apply gain (linear multiplier or dB with suffix) |
| `pan(position)` | `Number → Processor` | Processor | Stereo pan: -1.0 (left) to 1.0 (right) |
| `delay(time)` | `Time → Processor` | Processor | Delay line |
| `mix(dry, wet)` | `Signal, Signal → Signal` | Signal | Mix two signals (averages them) |
| `semitones_to_ratio(semitones)` | `Number → Number` | Number | Convert semitones to frequency ratio (2^(st/12)) |

### Usage

```muse
// Gain with param reference
input -> gain(param.volume) -> output

// Mixing dry/wet
let dry = input
let wet = input -> lowpass(param.cutoff)
mix(dry, wet) -> gain(param.mix) -> output

// Delay in feedback loop
input -> feedback {
  delay(100ms) -> lowpass(2000Hz) -> gain(0.7)
} -> output

// MPE pitch bend: convert semitones to frequency ratio
let bent_freq = note.pitch * semitones_to_ratio(note.bend)
let osc = saw(bent_freq)
```

## Quick Reference Table

| Category | Functions |
|----------|-----------|
| Oscillators | `sine`, `saw`, `square`, `triangle`, `noise`, `pulse`, `lfo` |
| Filters | `lowpass`, `highpass`, `bandpass`, `notch` |
| Envelopes | `adsr`, `ar` |
| Dynamics | `compressor` |
| Modulation | `chorus` |
| Waveshaping | `tanh`, `fold`, `bitcrush`, `clip` |
| Utilities | `gain`, `pan`, `delay`, `mix`, `semitones_to_ratio` |

**Total: 24 functions**
