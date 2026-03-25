# DSP Primitives

All 16 built-in DSP functions available in `process` blocks. These compile to real-time-safe Rust code.

## Oscillators

Generate audio signals. Return type: `Signal`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `sine(freq)` | `Frequency → Signal` | Sine wave oscillator |
| `saw(freq)` | `Frequency → Signal` | Band-limited sawtooth wave |
| `square(freq)` | `Frequency → Signal` | Band-limited square wave |
| `triangle(freq)` | `Frequency → Signal` | Band-limited triangle wave |
| `noise()` | `() → Signal` | White noise generator (no parameters) |

### Usage

```muse
// Standalone (instrument mode — frequency from MIDI)
let osc = saw(note.pitch)

// With literal frequency
let tone = sine(440Hz)
```

Oscillators are typically used in instrument plugins with MIDI. Each call site maintains its own phase state.

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
```

**Note:** Filter tests may exhibit imprecision due to a known biquad state initialization issue. Use relative assertions (`< -6dB`) rather than exact dB values.

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

## Utilities

General-purpose audio processors.

| Function | Signature | Return Type | Description |
|----------|-----------|-------------|-------------|
| `gain(amount)` | `Gain → Processor` | Processor | Apply gain (linear multiplier or dB with suffix) |
| `pan(position)` | `Number → Processor` | Processor | Stereo pan: -1.0 (left) to 1.0 (right) |
| `delay(time)` | `Time → Processor` | Processor | Delay line |
| `mix(dry, wet)` | `Signal, Signal → Signal` | Signal | Mix two signals (averages them) |
| `clip(min, max)` | `Number, Number → Processor` | Processor | Hard clip signal to range |
| `tanh()` | `() → Processor` | Processor | Soft saturation (hyperbolic tangent waveshaper) |

### Usage

```muse
// Gain with param reference
input -> gain(param.volume) -> output

// Gain with dB literal
input -> gain(-6dB) -> output

// Soft saturation chain
input -> gain(param.drive) -> tanh() -> output

// Mixing dry/wet
let dry = input
let wet = input -> lowpass(param.cutoff)
mix(dry, wet) -> gain(param.mix) -> output

// Delay in feedback loop
input -> feedback {
  delay(100ms) -> lowpass(2000Hz) -> gain(0.7)
} -> output
```

## Quick Reference Table

| Category | Functions |
|----------|-----------|
| Oscillators | `sine`, `saw`, `square`, `triangle`, `noise` |
| Filters | `lowpass`, `highpass`, `bandpass`, `notch` |
| Envelopes | `adsr`, `ar` |
| Utilities | `gain`, `pan`, `delay`, `mix`, `clip`, `tanh` |

**Total: 16 functions** (5 oscillators + 4 filters + 2 envelopes + 6 utilities – note: `noise` is categorized as an oscillator but takes no arguments)
