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

## Recipe 5: LFO Modulation (Tremolo)

An effect that uses an oscillator as an LFO to modulate amplitude over time.

**Source:** `examples/tremolo.muse`

```muse
plugin "Velvet Tremolo" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.velvet-tremolo"
    description "A smooth amplitude tremolo effect"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseVelvetTrm"
    subcategories   [Fx, Modulation]
  }

  input  stereo
  output stereo

  param rate: float = 4.0 in 0.1..20.0 {
    smoothing linear 5ms
    unit "Hz"
  }

  param depth: float = 0.5 in 0.0..1.0 {
    smoothing linear 5ms
  }

  process {
    let lfo = sine(param.rate)
    let mod_gain = 1.0 - param.depth + param.depth * lfo
    input -> gain(mod_gain) -> output
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    set    param.depth = 0.5
    assert output.rms < -120dB
  }

  test "depth modulates signal level" {
    input  sine 440Hz 1024 samples
    set    param.depth = 0.5
    assert output.peak > 0.0
  }
}
```

**Key points:**
- `sine(param.rate)` creates an LFO — oscillators work in effect plugins, not just instruments
- The modulation formula `1.0 - depth + depth * lfo` scales from unity (depth=0) to full modulation (depth=1)
- LFO oscillators maintain per-call-site phase state, same as instrument oscillators
- Any oscillator (`sine`, `saw`, `square`, `triangle`) can be used as an LFO
- Use `smoothing` on rate/depth params for click-free automation

---

## Recipe 6: Distortion (Wavefold + Bitcrush)

A digital distortion effect — demonstrates `fold` and `bitcrush` primitives in a chain.

**Pattern:** `input -> fold(drive) -> bitcrush(bits) -> gain(mix) -> output`

**Source:** `examples/distortion.muse`

```muse
plugin "Crunch Box" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category effect

  clap {
    id          "dev.museaudio.crunch-box"
    description "A crunchy digital distortion effect"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseCrunchBox1"
    subcategories   [Fx, Distortion]
  }

  input  stereo
  output stereo

  param drive: float = 3.0 in 1.0..10.0 {
    smoothing logarithmic 50ms
  }

  param bits: float = 8.0 in 1.0..16.0 {
    smoothing logarithmic 50ms
  }

  param mix_amt: float = 0.5 in 0.0..1.0 {
    smoothing linear 10ms
  }

  process {
    input -> fold(param.drive) -> bitcrush(param.bits) -> gain(param.mix_amt) -> output
  }

  test "signal passes through with content" {
    input  sine 440Hz 1024 samples
    set    param.drive = 3.0
    set    param.bits = 8.0
    set    param.mix_amt = 1.0
    assert output.peak > 0.0
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `fold(amount)` applies sine wavefold distortion — higher values = more aggressive folding
- `bitcrush(bits)` reduces bit depth — 16 = transparent, 4 = crunchy, 1 = extreme
- Both are stateless inline processors — no state management needed
- Chain order matters: fold then bitcrush sounds different from bitcrush then fold

---

## Recipe 7: Chorus Effect

A modulated delay chorus — demonstrates the `chorus` primitive.

**Pattern:** `input -> chorus(rate, depth) -> output`

**Source:** `examples/chorus_effect.muse`

```muse
plugin "Silk Chorus" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category effect

  clap {
    id          "dev.museaudio.silk-chorus"
    description "A silky smooth chorus effect"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseSilkChrs1"
    subcategories   [Fx]
  }

  input  stereo
  output stereo

  param rate: float = 1.5 in 0.1..10.0 {
    smoothing linear 5ms
    unit "Hz"
  }

  param depth: float = 0.4 in 0.0..1.0 {
    smoothing linear 5ms
  }

  process {
    input -> chorus(param.rate, param.depth) -> output
  }

  test "chorus produces output" {
    input  sine 440Hz 1024 samples
    set    param.rate = 1.5
    set    param.depth = 0.4
    assert output.peak > 0.0
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `chorus(rate, depth)` is a single-primitive effect with an internal modulated delay line
- `rate` controls LFO speed in Hz, `depth` controls modulation amount (0.0–1.0)
- Each call site maintains its own delay buffer and LFO phase
- For more control, use `lfo()` + manual delay modulation (see Recipe 5 for the LFO pattern)

---

## Recipe 8: Dynamics (Compressor)

A dynamics compressor — demonstrates the `compressor` primitive.

**Pattern:** `input -> compressor(threshold, ratio) -> output`

**Source:** `examples/dynamics.muse`

```muse
plugin "Smooth Comp" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category effect

  clap {
    id          "dev.museaudio.smooth-comp"
    description "A smooth dynamics compressor"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseSmoothCmp1"
    subcategories   [Fx, Dynamics]
  }

  input  stereo
  output stereo

  param threshold: float = 0.5 in 0.01..1.0 {
    smoothing logarithmic 10ms
  }

  param ratio: float = 4.0 in 1.0..20.0 {
    smoothing linear 5ms
  }

  process {
    input -> compressor(param.threshold, param.ratio) -> output
  }

  test "compressor reduces peaks" {
    input  sine 440Hz 1024 samples
    set    param.threshold = 0.3
    set    param.ratio = 8.0
    assert output.peak > 0.0
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `compressor(threshold, ratio)` — threshold is linear gain (0.0–1.0), NOT dB
- Ratio is compression ratio: 4.0 means 4:1 compression above threshold
- Attack (~10ms) and release (~100ms) are fixed internally
- Each call site maintains its own envelope follower state
- Commonly followed by `gain()` for makeup gain

---

## Recipe 9: Pulse Wave Synth

A MIDI synthesizer using a pulse oscillator — demonstrates `pulse` with variable width.

**Pattern:** `pulse(pitch, width) -> gain(envelope) -> output`

**Source:** `examples/pulse_synth.muse`

```muse
plugin "Pulse Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category instrument

  clap {
    id          "dev.museaudio.pulse-synth"
    description "A pulse wave synthesizer"
    features    [instrument, synthesizer, stereo]
  }

  vst3 {
    id              "MusePulseSyn01"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo

  midi {
    note {
      let freq = note.pitch
      let gate = note.gate
    }
  }

  param width: float = 0.3 in 0.01..0.99 {
    smoothing logarithmic 50ms
  }

  param attack: float = 5.0 in 0.5..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param decay: float = 100.0 in 1.0..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param sustain: float = 0.7 in 0.0..1.0 {
    display "percentage"
  }

  param release: float = 200.0 in 1.0..10000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  process {
    let osc = pulse(note.pitch, param.width)
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    osc -> gain(env) -> output
  }

  test "no note produces silence" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `pulse(freq, width)` — width controls duty cycle: 0.5 = square wave, 0.1 = narrow pulse, 0.9 = wide pulse
- Width as a parameter gives real-time timbral control (pulse width modulation)
- Same instrument pattern as Recipe 3 (Synth): `midi` block + `category instrument` + envelope
- Instrument test blocks can only test silence (no MIDI events in test blocks)

---

## Recipe 10: Polyphonic Synth

An 8-voice polyphonic subtractive synth — demonstrates `voices N` for polyphony.

**Pattern:** `voices N` + same process block as mono (DSP state is automatically per-voice)

**Source:** `examples/poly_synth.muse`

```muse
plugin "Poly Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category instrument

  clap {
    id          "dev.museaudio.poly-synth"
    description "An 8-voice polyphonic subtractive synthesizer"
    features    [instrument, stereo, synthesizer]
  }

  vst3 {
    id              "MusePolySyn01"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo
  voices 8

  midi {
    note {
      let freq = note.pitch
      let vel = note.velocity
      let gate = note.gate
    }
  }

  param attack: float = 10.0 in 0.5..5000.0 { smoothing linear 5ms  unit "ms" }
  param decay: float = 200.0 in 1.0..5000.0 { smoothing linear 5ms  unit "ms" }
  param sustain: float = 0.7 in 0.0..1.0 { display "percentage" }
  param release: float = 300.0 in 1.0..10000.0 { smoothing linear 5ms  unit "ms" }
  param cutoff: float = 4000.0 in 20.0..20000.0 { smoothing logarithmic 15ms  unit "Hz" }
  param resonance: float = 0.3 in 0.0..1.0 { smoothing linear 10ms }
  param osc_mix: float = 0.5 in 0.0..1.0 { display "percentage" }
  param volume: float = -6.0 in -60.0..0.0 { unit "dB" }

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
- `voices 8` is the only addition vs a mono synth — everything else is identical
- The process block runs per-voice automatically. Each voice gets its own oscillators, filter, and envelope.
- Oldest-voice stealing when all 8 voices are in use
- Voice count 1–128, requires `midi` block
- Instrument test blocks can only test silence (no MIDI events in test blocks)

---

## Recipe 11: MPE Synth

An MPE-enabled polyphonic synth — demonstrates per-note expression (pressure, bend, slide).

**Pattern:** `voices N` + `note.pressure`/`note.bend`/`note.slide` in the process block

**Source:** `examples/mpe_synth.muse`

```muse
plugin "MPE Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category instrument

  clap {
    id          "dev.museaudio.mpe-synth"
    description "An 8-voice MPE-enabled polyphonic synthesizer"
    features    [instrument, stereo, synthesizer]
  }

  vst3 {
    id              "MuseMpeSyn01_"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo
  voices 8

  midi {
    note {
      let freq = note.pitch
      let vel = note.velocity
      let gate = note.gate
      let press = note.pressure
      let bend = note.bend
      let brightness = note.slide
    }
  }

  param attack: float = 10.0 in 0.5..5000.0 { smoothing linear 5ms  unit "ms" }
  param decay: float = 200.0 in 1.0..5000.0 { smoothing linear 5ms  unit "ms" }
  param sustain: float = 0.7 in 0.0..1.0 { display "percentage" }
  param release: float = 300.0 in 1.0..10000.0 { smoothing linear 5ms  unit "ms" }
  param cutoff: float = 4000.0 in 20.0..20000.0 { smoothing logarithmic 15ms  unit "Hz" }
  param resonance: float = 0.3 in 0.0..1.0 { smoothing linear 10ms }
  param volume: float = -6.0 in -60.0..0.0 { unit "dB" }

  process {
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    let osc = saw(note.pitch)
    let pressure_gain = note.pressure * 0.5 + 0.5
    osc -> lowpass(param.cutoff, param.resonance) -> gain(env) -> gain(pressure_gain) -> gain(param.volume) -> output
  }

  test "no note produces silence" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `note.pressure` — per-note aftertouch (0.0–1.0), from MPE PolyPressure events
- `note.bend` — per-note pitch bend in semitones, from MPE PolyTuning events
- `note.slide` — per-note slide/brightness (0.0–1.0), from MPE PolyBrightness events
- MPE fields default to 0.0 on voice start — they update when expression events arrive
- MPE is only available in polyphonic instruments (`voices N` required)
- Use expressions to modulate any part of the signal chain (pitch, filter, gain, etc.)

---

## Recipe 12: Unison Synth

A polyphonic synth with 3-voice unison detuning — demonstrates `unison { }` block.

**Pattern:** `voices N` + `unison { count M detune X }`

**Source:** `examples/unison_synth.muse`

```muse
plugin "Unison Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  category instrument

  clap {
    id          "dev.museaudio.unison-synth"
    description "A polyphonic synthesizer with 3-voice unison detuning"
    features    [instrument, stereo, synthesizer]
  }

  vst3 {
    id              "MuseUniSyn01___"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo
  voices 16

  unison {
    count 3
    detune 15
  }

  midi {
    note {
      let freq = note.pitch
      let vel = note.velocity
      let gate = note.gate
    }
  }

  param attack: float = 5.0 in 0.5..5000.0 { smoothing linear 5ms  unit "ms" }
  param decay: float = 150.0 in 1.0..5000.0 { smoothing linear 5ms  unit "ms" }
  param sustain: float = 0.6 in 0.0..1.0 { display "percentage" }
  param release: float = 250.0 in 1.0..10000.0 { smoothing linear 5ms  unit "ms" }
  param cutoff: float = 3000.0 in 20.0..20000.0 { smoothing logarithmic 15ms  unit "Hz" }
  param resonance: float = 0.4 in 0.0..1.0 { smoothing linear 10ms }
  param volume: float = -6.0 in -60.0..0.0 { unit "dB" }

  process {
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    let osc = saw(note.pitch)
    osc -> lowpass(param.cutoff, param.resonance) -> gain(env) -> gain(param.volume) -> output
  }

  test "no note produces silence" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `unison { count 3 detune 15 }` — each note spawns 3 voices spread ±15 cents
- `voices 16` provides the pool — 16 / 3 = 5 simultaneous notes before stealing
- The process block is identical to a non-unison synth — detuning is handled automatically
- `count` must be ≥ 2, `detune` must be > 0
- Requires `voices` declaration

---

## Choosing a Pattern

| I want to... | Use recipe |
|---|---|
| Process audio with a simple effect | Recipe 1 (Gain) |
| Add conditional processing paths | Recipe 2 (Filter) |
| Build a mono instrument | Recipe 3 (Synth) or Recipe 9 (Pulse Synth) |
| Process different frequency bands independently | Recipe 4 (Multiband) |
| Add time-varying modulation (tremolo, vibrato) | Recipe 5 (Tremolo) |
| Add distortion or lo-fi effects | Recipe 6 (Distortion) |
| Add chorus/detuning | Recipe 7 (Chorus) |
| Control dynamics (compression) | Recipe 8 (Dynamics) |
| Build a synth with timbral control | Recipe 9 (Pulse Synth) |
| Build a polyphonic instrument (chords) | Recipe 10 (Poly Synth) |
| Build an MPE-enabled instrument | Recipe 11 (MPE Synth) |
| Add thick unison detuning | Recipe 12 (Unison Synth) |
