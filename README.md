# Muse

An AI-native language for building audio plugins. Write a `.muse` file, get a VST3/CLAP binary.

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

That's a complete audio plugin. One file. DSP, metadata, tests, everything.

Run `muse build examples/gain.muse` and get a `.clap` + `.vst3` binary you can load in Ableton, Bitwig, Reaper, or any DAW.

## What Muse Is

Muse is a domain-specific language designed from the ground up for AI agents to write audio plugins. The syntax is constrained and unambiguous — there's one obvious way to express any plugin. The compiler catches domain errors at compile time (you can't pass a frequency where a time value belongs). Test blocks let the AI verify its own work before shipping.

The pipeline: `.muse` source → parse → semantic analysis → Rust/nih-plug codegen → `cargo build` → native CLAP + VST3 binaries.

## Signal Flow

Audio processing reads left-to-right with the `->` operator:

```muse
input -> lowpass(param.cutoff, param.resonance) -> gain(param.volume) -> output
```

Parallel routing with split/merge:

```muse
input -> split {
  lowpass(400Hz) -> gain(param.drive) -> tanh()
  bandpass(2000Hz) -> gain(0.8)
  highpass(4000Hz) -> gain(0.6) -> tanh()
} -> merge -> gain(param.mix) -> output
```

MIDI instruments with oscillators and envelopes:

```muse
process {
  let env = adsr(param.attack, param.decay, param.sustain, param.release)
  let osc1 = saw(note.pitch)
  let osc2 = square(note.pitch)
  mix(osc1, osc2) -> lowpass(param.cutoff) -> gain(env) -> output
}
```

## In-Language Testing

Plugins test themselves. Feed real audio through the compiled DSP and assert on the output:

```muse
test "lowpass attenuates high frequencies" {
  input  sine 10000Hz 1024 samples
  set    param.cutoff = 200.0
  assert output.rms < -6dB
}
```

`muse test` compiles the plugin, runs `cargo test` on the generated crate, and reports structured JSON results. An AI agent can write a plugin, verify it works, and ship it — no human listening required.

## CLI

```
muse check <file>                    # Parse + semantic validation
muse test <file> [--format json]     # Compile and run test blocks
muse build <file> [--output-dir .]   # Full build → .clap + .vst3
```

## Built-in DSP

17 primitives covering the fundamentals:

| Category | Functions |
|----------|-----------|
| Oscillators | `sine` `saw` `square` `triangle` `noise` |
| Filters | `lowpass` `highpass` `bandpass` `notch` |
| Envelopes | `adsr` `ar` |
| Utilities | `gain` `pan` `delay` `mix` `clip` `tanh` |

## Domain Type System

Numbers carry meaning. The compiler prevents you from passing milliseconds where Hertz are expected.

| Type | Suffix | Example |
|------|--------|---------|
| Frequency | `Hz` `kHz` | `440Hz` `4kHz` |
| Time | `ms` `s` | `50ms` `0.5s` |
| Gain | `dB` | `-12dB` |

## Examples

Five complete plugins in [`examples/`](examples/):

- **[gain.muse](examples/gain.muse)** — Single knob gain stage
- **[filter.muse](examples/filter.muse)** — Resonant filter with conditional saturation
- **[synth.muse](examples/synth.muse)** — Subtractive MIDI synthesizer
- **[multiband.muse](examples/multiband.muse)** — Parallel multiband processor
- **[tremolo.muse](examples/tremolo.muse)** — LFO amplitude modulation

## AI Skill File

Muse ships with a [skill file](skill/SKILL.md) that teaches AI agents to write plugins autonomously. It includes a language reference, DSP primitive documentation, plugin recipes, error code guide, and step-by-step workflows for creating, debugging, and extending plugins.

## Quick Start

```bash
# Build the compiler
cargo build --release

# Check a .muse file
./target/release/muse check examples/gain.muse

# Run tests
./target/release/muse test examples/gain.muse --format json

# Build plugin binaries (macOS, requires Rust toolchain)
./target/release/muse build examples/gain.muse --output-dir ./build

# Install to DAW plugin directories
cp -R "./build/Warm Gain.clap" ~/Library/Audio/Plug-Ins/CLAP/
cp -R "./build/Warm Gain.vst3" ~/Library/Audio/Plug-Ins/VST3/
```

## Architecture

```
.muse source
  → Lexer (logos)
  → Parser (chumsky) → AST
  → Semantic resolver → type-checked AST
  → Code generator → standalone Rust/nih-plug crate
  → cargo build --release → native binary
  → Bundle assembly → .clap + .vst3
```

The generated code is allocation-free in the audio thread. No runtime interpreter, no abstraction layer — the output binary is identical to a hand-written nih-plug plugin.

## Project Status

**M001 (Language & Compiler Core)** — Complete. Full compiler pipeline from `.muse` source to loadable plugin binaries. 227 tests, zero clippy warnings.

**M002 (Build Pipeline & AI Tooling)** — Complete. In-language test harness, dual-format build (CLAP + VST3), AI skill file with workflows and references.

**Next:** Expanded DSP primitives (dynamics, delay, EQ), GUI system, polyphony, cross-platform builds.

## Requirements

- Rust toolchain (stable)
- macOS (plugin binary output is macOS-only for now)

## License

MIT
