<required_reading>
- ../references/language-reference.md — Full syntax: plugin structure, params, process blocks, signal chains, routing, MIDI, GUI blocks, type system
- ../references/dsp-primitives.md — All 24 DSP functions with signatures, types, and usage examples
- ../references/test-syntax.md — Test block grammar, signal types, assertions, JSON output format
- ../references/cli-commands.md — `muse check`, `muse test`, `muse build`, `muse preview` with flags and exit codes
- ../references/plugin-recipes.md — 14 annotated patterns (gain, filter, synth, multiband, tremolo, distortion, chorus, dynamics, pulse synth, poly, MPE, unison, GUI Tier 1, GUI Tier 2) to use as starting points
</required_reading>

<process>

## Step 1: Gather Requirements

Extract from the user's description:

- **Plugin name** — a short, descriptive name (e.g., "Warm Gain", "Velvet Filter")
- **Category** — `effect` (processes audio) or `instrument` (generates audio from MIDI). If unclear, ask.
- **Parameters** — what the user wants to control. Map natural language to param types:
  - Continuous values → `float` with appropriate range and unit (`Hz`, `dB`, `ms`, `%`)
  - On/off toggles → `bool`
  - Mode selectors → `enum [option1, option2, ...]`
  - Stepped values → `int` with min..max range
- **DSP behavior** — what the plugin does to audio. Identify which DSP primitives are needed from the 23 available functions.

If the user says "tremolo with rate and depth", that maps to:
- Category: `effect`
- Params: `rate: float` (in Hz range), `depth: float` (0.0–1.0)
- DSP: `sine` oscillator as LFO modulating `gain`

## Step 2: Choose Base Pattern

Select the closest recipe from plugin-recipes.md:

| User wants... | Start from |
|---|---|
| Simple one-knob effect | Recipe 1 (Gain) — `input -> dsp(param) -> output` |
| Multi-stage effect with conditionals | Recipe 2 (Filter) — `let` bindings + `if` expressions |
| MIDI instrument / synthesizer | Recipe 3 (Synth) or Recipe 9 (Pulse Synth) — `midi { note { ... } }` + oscillators + envelopes |
| Multiband or parallel processing | Recipe 4 (Multiband) — `split { ... } -> merge` |
| Modulation effects (tremolo, vibrato) | Recipe 5 (Tremolo) — LFO modulating gain |
| Distortion / lo-fi effects | Recipe 6 (Distortion) — `fold` + `bitcrush` chain |
| Chorus / detuning effects | Recipe 7 (Chorus) — `chorus(rate, depth)` |
| Dynamics / compression | Recipe 8 (Dynamics) — `compressor(threshold, ratio)` |
| Polyphonic instrument (chords) | Recipe 10 (Poly Synth) — add `voices 8` |
| MPE-enabled instrument | Recipe 11 (MPE Synth) — `note.pressure`/`bend`/`slide` |
| Thick unison sound | Recipe 12 (Unison Synth) — `unison { count 3 detune 15 }` |
| Plugin with custom themed GUI (auto-layout) | Recipe 13 (GUI Effect — Tier 1) — add `gui { theme accent }` |
| Plugin with explicit layout and visualizations | Recipe 14 (GUI with Layout — Tier 2) — `gui { layout { panel { widgets } } }` |

Copy the recipe's structure as your starting skeleton. Modify the metadata, params, process block, and tests.

## Step 3: Write the .muse File

Build the file section by section. Follow this exact order:

```muse
plugin "Plugin Name" {
  // 1. Metadata
  vendor   "Author Name"
  version  "0.1.0"
  category effect  // or instrument — bare identifier, no quotes

  // 2. Format blocks (both required for build)
  clap {
    id          "com.author.plugin-name"
    description "What it does"
    features    [audio_effect, stereo]  // or [instrument, stereo, synthesizer]
  }
  vst3 {
    id              "AuthorPlugin1"  // max 16 chars recommended
    subcategories   [Fx]             // or [Instrument, Synth]
  }

  // 3. I/O
  input  stereo
  output stereo

  // 4. MIDI block (instruments only)
  // midi { note { let freq = note.pitch ... } }

  // 5. Parameters
  param name: float = default in min..max {
    smoothing linear 10ms
    unit "Hz"
  }

  // 6. Process block
  process {
    input -> dsp_chain -> output
  }

  // 7. Test blocks (added in Step 4)
}
```

**Critical rules:**
- One plugin per file.
- `category` is a bare identifier — write `category effect`, NOT `category "effect"`.
- Every `split` must pair with `-> merge` in the same chain.
- `->` has the lowest precedence — arithmetic binds tighter.
- Instruments need a `midi { note { ... } }` block.
- All param references in process/test use `param.name` syntax.
- Unit suffixes attach directly to numbers: `440Hz`, `50ms`, `-12dB`. No space.

## Step 3.5: Add GUI Block (Optional)

If the user wants a custom editor UI, add a `gui { }` block after the param declarations.

**Tier 1 (auto-layout):** Just set theme and accent — the compiler auto-generates knobs for all params:

```muse
gui {
  theme dark
  accent "#E8A87C"
}
```

**Tier 2 (explicit layout):** Declare layout containers, panels, and individual widgets:

```muse
gui {
  theme dark
  accent "#7ECCE8"
  size 800 550

  layout vertical {
    panel "Controls" {
      layout horizontal {
        knob cutoff
        knob resonance
        knob gain
      }
    }
  }
}
```

For more details on GUI blocks, see `references/language-reference.md` (GUI Block section) and `workflows/create-gui-plugin.md` for the full GUI-focused workflow.

**GUI rules:**
- `theme` must be `dark` or `light`
- `accent` must be hex: `#RGB` or `#RRGGBB`
- Widget param names must match declared `param` names
- Visualization widgets (`spectrum`, `waveform`, `envelope`, `eq_curve`, `reduction`) take no params
- Use `muse preview` to visually verify the GUI before building

## Step 4: Add Test Blocks

Every plugin needs at least 2 tests. Write them inside the plugin block after the process block.

### Effect plugins

**Required test: Silence passthrough**
```muse
test "silence in produces silence out" {
  input  silence 512 samples
  assert output.rms < -120dB
}
```

**Required test: Signal presence**
```muse
test "passes signal" {
  input  sine 440Hz 1024 samples
  set    param.amount = 1.0
  assert output.peak > 0.0
}
```

**Additional effect tests:**
- Gain increase: `set param.gain = 6.0` → `assert output.peak > 1.0`
- Attenuation: `set param.gain = -30.0` → `assert output.rms < -6dB`
- Bypass/zero: `set param.mix = 0.0` → `assert output.rms < -120dB`

### Instrument plugins

**Required test: Silence without notes**
```muse
test "no note produces silence" {
  input  silence 512 samples
  assert output.rms < -120dB
}
```

**Required test: Note produces sound with safety checks**
```muse
test "note produces sound" {
  note on 69 0.8 at 0
  note off 69 at 4096
  input silence 8192 samples
  assert output.rms > -20dB
  assert no_nan
  assert no_denormal
}
```

**Optional: Pitch verification (FFT)**
```muse
test "A4 plays at correct frequency" {
  note on 69 0.8 at 0
  note off 69 at 4096
  input silence 8192 samples
  assert frequency 440Hz > -20dB
}
```

**Optional: Envelope shape (temporal)**
```muse
test "envelope shape" {
  note on 69 0.8 at 0
  note off 69 at 256
  input silence 1024 samples
  assert output.rms_in 0..256 > -40dB
  assert output.rms_in 768..1024 < -20dB
}
```

### Test constraints
- Use `silence`, `sine <freq>Hz`, or `impulse` as input signals (the only three types).
- `note on`/`note off` inject MIDI events for instrument testing. Use `input silence` as the buffer — MIDI triggers the oscillators.
- `assert no_nan`, `assert no_denormal`, `assert no_inf` — always include safety checks on instrument tests.
- `assert frequency <freq>Hz` uses FFT — use at least 4096 samples for reliable results.
- `assert output.rms_in <start>..<end>` checks a specific sample range — useful for envelope verification.
- `set` values are bare numbers, no unit suffixes: `set param.cutoff = 200.0`, not `set param.cutoff = 200Hz`.

## Step 5: Validate with `muse check` and `muse test`

Run these commands in sequence. Fix any errors before proceeding to build.

```bash
# Quick syntax/semantic check
muse check plugin.muse --format json
```

If errors appear, read the error code and look it up in ../references/error-codes.md. Common first-attempt errors:
- E001: Typo or wrong syntax (check keyword spelling, brace matching)
- E003: Unknown function name (check ../references/dsp-primitives.md for exact names)
- E004: Wrong argument count (check function signatures)
- E005: Type mismatch (check unit suffixes — `Hz` for frequency, `ms` for time, `dB` for gain)
- E010: Missing metadata (need `vendor`, `clap { id }`, `vst3 { id }`, `input`, `output`, `process`)

Fix all errors, then run tests:

```bash
# Compile and run test blocks
muse test plugin.muse --format json
```

Check the JSON output. If `"status": "ok"` and all tests pass, proceed to build. If a test fails, read the `assertion`, `expected`, and `actual` fields to diagnose.

## Step 6: Build Plugin Binaries

```bash
muse build plugin.muse --output-dir ./build --format json
```

On success, the JSON output includes `clap_bundle` and `vst3_bundle` paths. These are macOS plugin bundles ready for installation in a DAW.

If build fails with exit code 2, it's a system-level error (missing toolchain, disk issue). Check stderr for details.

</process>

<success_criteria>
- A `.muse` file exists with valid syntax (passes `muse check`)
- The file has at least 2 test blocks
- All tests pass (`muse test` returns `"status": "ok"`)
- `muse build` produces both `.clap` and `.vst3` bundles (exit code 0)
- The plugin matches the user's original description in behavior and parameters
</success_criteria>
