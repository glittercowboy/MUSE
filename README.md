# Muse

This is a complete audio plugin:

```muse
plugin "Warm Gain" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category effect

  clap { id "dev.museaudio.warm-gain" description "A warm gain stage" features [audio_effect, stereo] }
  vst3 { id "MuseWarmGain1" subcategories [Fx, Dynamics] }

  input  stereo
  output stereo

  param gain: float = 0.0 in -30.0..30.0 {
    smoothing logarithmic 50ms
    unit "dB"
  }

  process {
    input -> gain(param.gain) -> output
  }

  test "silence in silence out" {
    input  silence 512 samples
    set    param.gain = 0.0
    assert output.rms < -120dB
  }
}
```

Run `muse build gain.muse` and get a native VST3 + CLAP binary that loads in Ableton, Bitwig, Reaper — any DAW. One file in, real plugin out.

## Why

Building audio plugins is brutally hard. You need systems programming (Rust or C++), real-time audio constraints, platform-specific bundle formats, and hundreds of lines of framework boilerplate. A simple gain knob is 200+ lines of Rust. In Muse, it's 30 lines that read like a description of what the plugin does.

But Muse wasn't built to save humans from typing. It was built so that **AI agents can write audio plugins.**

Tell Claude "build me a tremolo effect with rate and depth knobs." It writes a `.muse` file, runs `muse test` to verify the audio processing actually works, runs `muse build` to produce binaries, and hands you a plugin. No human wrote code. No human debugged anything.

The language makes this possible: constrained syntax so there's one obvious way to write any plugin, a type system that catches domain errors at compile time, and in-language tests that let the AI verify its own work before shipping.

## How It Sounds

Audio processing reads left-to-right with the `->` operator:

```muse
input -> lowpass(param.cutoff, param.resonance) -> gain(param.volume) -> output
```

Split into parallel frequency bands:

```muse
input -> split {
  lowpass(400Hz) -> gain(param.drive) -> tanh()
  bandpass(2000Hz) -> gain(0.8)
  highpass(4000Hz) -> gain(0.6) -> tanh()
} -> merge -> gain(param.mix) -> output
```

Build a synthesizer from oscillators and envelopes:

```muse
process {
  let env = adsr(param.attack, param.decay, param.sustain, param.release)
  let osc1 = saw(note.pitch)
  let osc2 = square(note.pitch)
  mix(osc1, osc2) -> lowpass(param.cutoff) -> gain(env) -> output
}
```

The code *is* the signal flow diagram.

## Plugins That Test Themselves

Every Muse plugin can prove it works. Feed real audio through the compiled DSP and assert on the output:

```muse
test "lowpass attenuates high frequencies" {
  input  sine 10000Hz 1024 samples
  set    param.cutoff = 200.0
  assert output.rms < -6dB
}
```

This generates a 10kHz sine wave, runs it through a lowpass filter set to 200Hz, and verifies the output is attenuated. Not a mock. Not a simulation. Real audio through the real compiled plugin.

An AI writes the plugin and the tests. If the tests pass, the plugin works. No human listening required.

## Quick Start

```bash
cargo build --release

# Check syntax and types
./target/release/muse check examples/gain.muse

# Compile, run audio tests
./target/release/muse test examples/gain.muse --format json

# Build VST3 + CLAP binaries
./target/release/muse build examples/gain.muse --output-dir ./build

# Load in your DAW
cp -R "./build/Warm Gain.vst3" ~/Library/Audio/Plug-Ins/VST3/
cp -R "./build/Warm Gain.clap" ~/Library/Audio/Plug-Ins/CLAP/
```

## Built-in DSP

23 primitives — oscillators, filters, envelopes, dynamics, modulation, and utilities:

```
sine  saw  square  triangle  noise  pulse  lfo
lowpass  highpass  bandpass  notch
adsr  ar
gain  pan  delay  mix  clip  tanh  fold  bitcrush
chorus  compressor
```

Numbers carry domain types. The compiler won't let you pass milliseconds where Hertz belong:

```muse
lowpass(50ms)   // E005: Expected Frequency, got Time
lowpass(500Hz)  // correct
```

## Examples

Nine working plugins in [`examples/`](examples/):

| Plugin | What it does |
|--------|-------------|
| [gain.muse](examples/gain.muse) | Single knob gain stage |
| [filter.muse](examples/filter.muse) | Resonant filter with conditional saturation |
| [synth.muse](examples/synth.muse) | Subtractive MIDI synthesizer |
| [multiband.muse](examples/multiband.muse) | Three-band parallel processor |
| [tremolo.muse](examples/tremolo.muse) | LFO amplitude modulation |
| [distortion.muse](examples/distortion.muse) | Wavefolder + bitcrusher |
| [chorus_effect.muse](examples/chorus_effect.muse) | Modulated delay chorus |
| [dynamics.muse](examples/dynamics.muse) | Compressor with threshold and ratio |
| [pulse_synth.muse](examples/pulse_synth.muse) | Pulse wave MIDI synthesizer |

Every example compiles to a real plugin binary. Every example has test blocks that pass.

## Under the Hood

```
.muse source
  → Lexer (logos) → Parser (chumsky) → Typed AST
  → Semantic resolver (type checking, function validation)
  → Code generator → standalone Rust/nih-plug crate
  → cargo build --release → native binary
  → Bundle assembly → .clap + .vst3
```

The generated audio code is allocation-free and lock-free. No interpreter, no runtime overhead. The output binary is indistinguishable from a hand-written nih-plug plugin.

243 tests. Zero clippy warnings.

## AI Skill File

Muse ships with a [skill file](skill/SKILL.md) that teaches AI agents to write plugins autonomously — language reference, DSP primitive docs, plugin recipes, error recovery patterns, and step-by-step workflows. Give it to Claude and ask for a plugin.

## What's Next

Polyphony (voice allocation, voice stealing), declarative GUI system, expanded test assertions (FFT, MIDI injection), presets, cross-platform builds.

## Requirements

- Rust toolchain (stable)
- macOS (binary output is macOS-only for now)

## License

MIT
