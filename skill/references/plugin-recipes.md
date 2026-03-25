# Plugin Recipes

Four annotated examples from the Muse codebase, showing common plugin patterns.

---

## Recipe 1: Simple Effect (Gain)

The simplest possible plugin — one parameter controlling signal level.

**Pattern:** `input -> dsp(param) -> output`

```muse
plugin "Warm Gain" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.warm-gain"
    description "A warm, musical gain stage"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseWarmGain1"
    subcategories   [Fx, Dynamics]
  }

  input  stereo
  output stereo

  param gain: float = 0.0 in -30.0..30.0 {
    smoothing logarithmic 50ms
    unit "dB"
  }

  process {
    input -> gain(param.gain) -> output
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    set    param.gain = 0.0
    assert output.rms < -120dB
  }

  test "positive gain increases level" {
    input  sine 440Hz 1024 samples
    set    param.gain = 6.0
    assert output.peak > 1.0
  }
}
```

**Key points:**
- Simplest signal chain: one DSP function between input and output
- `param.gain` is accessed via smoothed parameter (logarithmic smoothing prevents clicks)
- Tests use silence (reliable) and sine (level check) inputs
- `category effect` — this processes existing audio, not generating it

---

## Recipe 2: Filter with Conditionals

A resonant filter with conditional saturation — demonstrates `let` bindings, `if` expressions, and multi-step processing.

**Pattern:** `let intermediate = chain` → `if condition { path_a } else { path_b }` → `output`

```muse
plugin "Velvet Filter" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.velvet-filter"
    description "A smooth, resonant filter with character"
    features    [audio_effect, stereo, filter]
  }

  vst3 {
    id              "MuseVelvetFlt"
    subcategories   [Fx, Filter]
  }

  input  stereo
  output stereo

  param cutoff: float = 1000.0 in 20.0..20000.0 {
    smoothing logarithmic 20ms
    unit "Hz"
    display "frequency"
  }

  param resonance: float = 0.5 in 0.0..1.0 {
    smoothing linear 10ms
    display "percentage"
  }

  param mode: enum [lowpass, highpass, bandpass, notch] = lowpass

  param drive: float = 0.0 in 0.0..24.0 {
    smoothing logarithmic 30ms
    unit "dB"
  }

  param mix: float = 1.0 in 0.0..1.0 {
    unit "%"
    display "percentage"
  }

  process {
    let filtered = input -> lowpass(param.cutoff, param.resonance)
    let shaped = if param.drive > 0.0 {
      filtered -> gain(param.drive) -> tanh()
    } else {
      filtered
    }
    mix(input, shaped) -> gain(param.mix) -> output
  }

  test "lowpass attenuates high frequencies" {
    input  sine 10000Hz 1024 samples
    set    param.cutoff = 200.0
    set    param.resonance = 0.5
    assert output.rms < -6dB
  }
}
```

**Key points:**
- `let` bindings capture intermediate signals for reuse
- `if` is an expression — each branch returns a signal value
- `tanh()` provides soft saturation (no arguments — applied in chain)
- `mix(input, shaped)` blends dry and wet signals
- `enum` param for filter mode (though codegen currently uses lowpass regardless of mode)

---

## Recipe 3: Instrument with MIDI

A subtractive synthesizer — demonstrates MIDI handling, oscillators, envelopes, and instrument-mode processing.

**Pattern:** `midi { note { bindings } }` → oscillators → filter → envelope → output

```muse
plugin "Glass Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category instrument

  clap {
    id          "dev.museaudio.glass-synth"
    description "A crystalline subtractive synthesizer"
    features    [instrument, stereo, synthesizer]
  }

  vst3 {
    id              "MuseGlassSyn1"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo

  midi {
    note {
      let freq = note.pitch
      let vel = note.velocity
      let gate = note.gate
    }
  }

  param attack: float = 10.0 in 0.5..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param decay: float = 200.0 in 1.0..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param sustain: float = 0.7 in 0.0..1.0 {
    display "percentage"
  }

  param release: float = 300.0 in 1.0..10000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param cutoff: float = 4000.0 in 20.0..20000.0 {
    smoothing logarithmic 15ms
    unit "Hz"
  }

  param resonance: float = 0.3 in 0.0..1.0 {
    smoothing linear 10ms
  }

  param osc_mix: float = 0.5 in 0.0..1.0 {
    display "percentage"
  }

  param volume: float = -6.0 in -60.0..0.0 {
    unit "dB"
  }

  process {
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    let osc1 = saw(note.pitch)
    let osc2 = square(note.pitch)
    let tone = mix(osc1, osc2) -> gain(param.osc_mix)
    tone -> lowpass(param.cutoff, param.resonance) -> gain(env) -> gain(param.volume) -> output
  }

  test "no note produces silence" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `category instrument` + `midi { note { ... } }` makes this an instrument
- `note.pitch` provides MIDI frequency, `note.velocity` and `note.gate` for dynamics
- `adsr(...)` envelope automatically tracks MIDI gate state
- Multiple oscillators mixed together: `mix(osc1, osc2)`
- Chain through filter and envelope: `tone -> lowpass(...) -> gain(env) -> gain(volume) -> output`
- Instrument test blocks can only test silence (no MIDI events in test blocks)

---

## Recipe 4: Multiband Routing (Split/Merge)

A multiband effect processor — demonstrates parallel signal routing with split/merge.

**Pattern:** `input -> split { branch1; branch2; branch3 } -> merge -> output`

```muse
plugin "Multiband FX" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category effect

  clap {
    id          "dev.museaudio.multiband-fx"
    description "A multiband effect processor"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseMultibandFx"
    subcategories   [Fx, EQ]
  }

  input  stereo
  output stereo

  param drive: float = 0.5 in 0.0..1.0 {}
  param mix: float = 0.7 in 0.0..1.0 {}
  param delay_time: float = 100.0 in 10.0..500.0 {
    unit "ms"
  }

  process {
    input -> split {
      lowpass(400Hz) -> gain(param.drive) -> tanh()
      bandpass(2000Hz) -> gain(0.8)
      highpass(4000Hz) -> gain(0.6) -> tanh()
    } -> merge -> gain(param.mix) -> output
  }

  test "processes signal without crashing" {
    input  sine 1000Hz 1024 samples
    set    param.drive = 0.5
    set    param.mix = 1.0
    assert output.peak > 0.0
  }
}
```

**Key points:**
- `split { ... }` fans input to 3 parallel branches (one per line)
- Each branch is an independent signal chain with its own DSP
- `merge` sums all branches back to one signal
- Every `split` must pair with `merge` in the same chain (E007 if missing)
- Branches can have different processing: filter → gain → saturation
- Multiband tests may be unreliable due to biquad state bug — use simple assertions

---

## Choosing a Pattern

| I want to... | Use recipe |
|---|---|
| Process audio with a simple effect | Recipe 1 (Gain) |
| Add conditional processing paths | Recipe 2 (Filter) |
| Build an instrument that responds to MIDI | Recipe 3 (Synth) |
| Process different frequency bands independently | Recipe 4 (Multiband) |
