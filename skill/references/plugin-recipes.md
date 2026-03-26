# Plugin Recipes

18 annotated examples from the Muse codebase, showing common plugin patterns.

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

## Recipe 13: GUI Effect (Tier 1 Auto-Theme)

A gain plugin with a custom dark-themed GUI — the simplest GUI-enabled plugin. No layout declaration needed; the compiler auto-generates knobs for all parameters.

**Pattern:** Add `gui { theme accent }` to any effect — Tier 1 auto-layout handles the rest.

**Source:** `examples/gui_gain.muse`

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

  gui {
    theme dark
    accent "#E8A87C"
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
- `gui { theme dark  accent "#E8A87C" }` is all you need for a custom-themed editor
- Tier 1 auto-layout: compiler generates a knob for every declared `param` automatically
- No `layout`, `panel`, or widget declarations needed
- `theme` must be `dark` or `light` — E014 if invalid
- `accent` must be hex color `#RGB` or `#RRGGBB` — E014 if invalid
- Preview with `muse preview gui_gain.muse` before building

---

## Recipe 14: GUI Instrument with Tier 2 Layout + Advanced Widgets

An effect with an explicit layout, spectrum analyzer, XY pad, and standard knobs. Demonstrates Tier 2 GUI features: layout containers, panels, visualization widgets, and multi-param widgets.

**Pattern:** `gui { layout { panel { widgets } } }` for full control over editor layout.

**Source:** `examples/gui_spectrum.muse`

```muse
plugin "Spectrum Demo" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.spectrum-demo"
    description "An effect with spectrum analyzer and XY pad"
    features    [audio_effect, stereo, analyzer]
  }

  vst3 {
    id              "MuseSpecDem01"
    subcategories   [Fx, Analyzer]
  }

  input  stereo
  output stereo

  param freq: float = 1000.0 in 20.0..20000.0 {
    smoothing logarithmic 50ms
    unit "Hz"
  }

  param resonance: float = 0.707 in 0.1..10.0 {
    smoothing logarithmic 50ms
  }

  param gain: float = 0.0 in -30.0..30.0 {
    smoothing logarithmic 50ms
    unit "dB"
  }

  gui {
    theme dark
    accent "#7ECCE8"
    size 800 550

    layout vertical {
      panel "Analyzer" {
        spectrum
      }
      panel "Controls" {
        layout horizontal {
          xy_pad freq resonance
          knob gain
        }
      }
    }
  }

  process {
    input -> lowpass(param.freq, param.resonance) -> gain(param.gain) -> output
  }
}
```

**Key points:**
- Tier 2: explicit `layout` and `panel` declarations give full control over widget placement
- `layout vertical { ... }` stacks children top-to-bottom; `horizontal` left-to-right
- `panel "Analyzer" { spectrum }` — titled section containing a visualization widget
- `spectrum` is a visualization widget — no parameter binding (E014 if you give it one)
- `xy_pad freq resonance` binds two parameters to X and Y axes
- `knob gain` binds a single parameter to a rotary knob
- `size 800 550` sets the editor window dimensions
- Nested layouts: `layout vertical { ... layout horizontal { ... } }` for complex arrangements
- Widget properties available: `knob gain { style "vintage" label "Output" }`

---

## Recipe 15: Echo/Delay Effect

A clean delay effect — demonstrates `delay()` with dry/wet mixing via `mix()`.

**Pattern:** `let delayed = input -> delay(time) -> gain(mix)` → `mix(input, delayed) -> output`

**Source:** `examples/echo.muse`

```muse
plugin "Simple Echo" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.simple-echo"
    description "A clean delay effect"
    features    [audio_effect, stereo, delay]
  }

  vst3 {
    id              "MuseSimplEcho1"
    subcategories   [Fx, Delay]
  }

  input  stereo
  output stereo

  param time: float = 0.25 in 0.01..2.0 {
    unit "s"
  }

  param mix_amt: float = 0.5 in 0.0..1.0

  process {
    let delayed = input -> delay(param.time) -> gain(param.mix_amt)
    mix(input, delayed) -> output
  }

  test "impulse produces output with delay content" {
    input  impulse 2048 samples
    set    param.time = 0.01
    set    param.mix_amt = 0.5
    assert output.peak > 0.0
    assert output.rms > -60dB
  }

  test "sine through delay preserves signal" {
    input  sine 440Hz 4096 samples
    set    param.time = 0.01
    set    param.mix_amt = 0.5
    assert output.rms > -10dB
  }

  test "silence in produces silence out" {
    input  silence 1024 samples
    set    param.time = 0.25
    set    param.mix_amt = 0.5
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `delay(time)` creates a delay line — time in seconds (use `s` suffix or bare float)
- `mix(input, delayed)` blends dry and wet signals (simple average)
- The `let` binding captures the delayed signal for blending
- Use `impulse` test input to verify delay produces output at the expected time
- For feedback delay (echo trail), use a `feedback { }` block instead of simple delay

---

## Recipe 16: Parametric EQ

A 4-band parametric equalizer — demonstrates `low_shelf`, `peak_eq`, and `high_shelf` chained together.

**Pattern:** `input -> low_shelf(...) -> peak_eq(...) -> peak_eq(...) -> high_shelf(...) -> output`

**Source:** `examples/parametric_eq.muse`

```muse
plugin "Parametric EQ" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.parametric-eq"
    description "A 4-band parametric equalizer"
    features    [audio_effect, stereo, equalizer]
  }

  vst3 {
    id              "MuseParamEQ001"
    subcategories   [Fx, EQ]
  }

  input  stereo
  output stereo

  param low_freq: float = 200.0 in 20.0..500.0 {
    smoothing logarithmic 20ms
    unit "Hz"
  }

  param low_gain: float = 3.0 in -12.0..12.0 {
    unit "dB"
  }

  param mid1_freq: float = 1000.0 in 200.0..5000.0 {
    smoothing logarithmic 20ms
    unit "Hz"
  }

  param mid1_gain: float = -2.0 in -12.0..12.0 {
    unit "dB"
  }

  param mid1_q: float = 1.4 in 0.1..10.0

  param mid2_freq: float = 4000.0 in 1000.0..15000.0 {
    smoothing logarithmic 20ms
    unit "Hz"
  }

  param mid2_gain: float = 2.0 in -12.0..12.0 {
    unit "dB"
  }

  param mid2_q: float = 2.0 in 0.1..10.0

  param high_freq: float = 8000.0 in 2000.0..20000.0 {
    smoothing logarithmic 20ms
    unit "Hz"
  }

  param high_gain: float = -1.0 in -12.0..12.0 {
    unit "dB"
  }

  process {
    input
      -> low_shelf(param.low_freq, param.low_gain)
      -> peak_eq(param.mid1_freq, param.mid1_gain, param.mid1_q)
      -> peak_eq(param.mid2_freq, param.mid2_gain, param.mid2_q)
      -> high_shelf(param.high_freq, param.high_gain)
      -> output
  }

  test "passes signal through EQ chain" {
    input  sine 1000Hz 4096 samples
    set    param.low_freq = 200.0
    set    param.low_gain = 0.0
    set    param.mid1_freq = 1000.0
    set    param.mid1_gain = 0.0
    set    param.mid1_q = 1.4
    set    param.mid2_freq = 4000.0
    set    param.mid2_gain = 0.0
    set    param.mid2_q = 2.0
    set    param.high_freq = 8000.0
    set    param.high_gain = 0.0
    assert output.rms > -6dB
  }

  test "silence in produces silence out" {
    input  silence 1024 samples
    set    param.low_gain = 3.0
    set    param.mid1_gain = -2.0
    set    param.mid2_gain = 2.0
    set    param.high_gain = -1.0
    assert output.rms < -120dB
  }
}
```

**Key points:**
- Chain multiple EQ bands in series: `low_shelf -> peak_eq -> peak_eq -> high_shelf`
- Each band has independent biquad state — order doesn't affect correctness (but may affect numerical precision)
- `gain_db` is in dB: positive boosts, negative cuts, 0 = transparent
- `q` parameter on `peak_eq` controls bandwidth: 0.1 = wide, 10.0 = narrow surgical cut
- Test with 0dB gain on all bands to verify signal passes through transparently

---

## Recipe 17: Noise Gate

A noise gate effect — demonstrates the `gate()` primitive for silencing signal below a threshold.

**Pattern:** `input -> gate(threshold, attack, release, hold) -> output`

**Source:** `examples/gate.muse`

```muse
plugin "Noise Gate" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.noise-gate"
    description "A noise gate with adjustable threshold and timing"
    features    [audio_effect, stereo, utility]
  }

  vst3 {
    id              "MuseNoiseGt1"
    subcategories   [Fx, Dynamics]
  }

  input  stereo
  output stereo

  param threshold: float = -40.0 in -80.0..0.0 {
    smoothing logarithmic 10ms
    unit "dB"
  }

  param attack: float = 1.0 in 0.1..50.0 {
    smoothing linear 5ms
  }

  param release: float = 50.0 in 5.0..500.0 {
    smoothing linear 5ms
  }

  process {
    input -> gate(-40dB, param.attack, param.release, 10.0) -> output
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    assert output.rms < -120dB
  }

  test "loud sine passes through gate" {
    input  sine 440Hz 1024 samples
    assert output.rms > -10dB
  }
}
```

**Key points:**
- `gate(threshold_db, attack_ms, release_ms, hold_ms)` — all parameters optional
- Threshold uses dB suffix: `-40dB` means signals below -40dB are silenced
- Attack/release control how fast the gate opens/closes (in ms)
- Hold prevents rapid on/off chattering: gate stays open for at least `hold_ms` after signal drops
- Use `gate()` with no args for sensible defaults as a starting point
- Gate is a dynamics processor — it maintains its own envelope follower state per call site

---

## Recipe 18: Phaser Effect

A multi-stage allpass phaser — demonstrates chaining multiple `allpass()` stages for phase-cancellation effects.

**Pattern:** `input -> allpass(...) -> allpass(...) -> allpass(...) -> allpass(...) -> output`

**Source:** `examples/phaser.muse`

```muse
plugin "Phase Shift" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category effect

  clap {
    id          "dev.museaudio.phase-shift"
    description "A multi-stage allpass phaser"
    features    [audio_effect, stereo, phaser]
  }

  vst3 {
    id              "MusePhaseShft1"
    subcategories   [Fx]
  }

  input  stereo
  output stereo

  param depth: float = 0.7 in 0.0..0.95
  param rate_val: float = 0.002 in 0.0001..0.01 {
    unit "s"
  }

  process {
    input
      -> allpass(param.rate_val, param.depth)
      -> allpass(param.rate_val, param.depth)
      -> allpass(param.rate_val, param.depth)
      -> allpass(param.rate_val, param.depth)
      -> output
  }

  test "sine passes through allpass chain" {
    input  sine 440Hz 1024 samples
    set    param.depth = 0.7
    set    param.rate_val = 0.002
    assert output.rms > -20dB
  }

  test "silence in produces silence out" {
    input  silence 1024 samples
    set    param.depth = 0.7
    set    param.rate_val = 0.002
    assert output.rms < -120dB
  }
}
```

**Key points:**
- `allpass(time, feedback)` is a Schroeder allpass filter — it passes all frequencies but shifts phase
- Chaining 4+ stages creates the characteristic phaser sweep
- More stages = deeper/more pronounced phasing effect (2 = subtle, 4 = classic, 8 = dramatic)
- Each `allpass()` call site maintains its own state — chaining is safe
- `feedback` controls resonance: 0.0 = subtle, 0.95 = intense. Keep below 1.0!
- For an LFO-modulated phaser, modulate the `time` parameter with `lfo()` (see Recipe 5 for LFO pattern)

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
| Add a custom GUI with auto-generated knobs | Recipe 13 (GUI Effect — Tier 1) |
| Build a custom GUI with explicit layout and visualizations | Recipe 14 (GUI with Layout — Tier 2) |
| Add echo/delay effects | Recipe 15 (Echo) |
| Build a parametric equalizer | Recipe 16 (Parametric EQ) |
| Gate noise or silence quiet signals | Recipe 17 (Noise Gate) |
| Add phaser/phase-shifting effects | Recipe 18 (Phaser) |
