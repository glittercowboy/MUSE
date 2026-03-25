# Test Block Syntax

Test blocks live inside a `plugin` block alongside `process`, `param`, etc. They define input signals, inject MIDI events, set parameter values, and assert properties of the output.

## Grammar

```
test "test name" {
  note on <note> <velocity> at <timing>   // MIDI injection (instruments)
  note off <note> at <timing>
  input <signal> <count> samples
  set param.<name> = <value>
  assert <property> <op> <value>          // amplitude assertions
  assert frequency <freq>Hz <op> <value>  // spectral (FFT) assertions
  assert output.rms_in <start>..<end> <op> <value>  // temporal assertions
  assert no_nan                           // safety assertions
  assert no_denormal
  assert no_inf
}
```

Statements can appear in any order and repeat. Typical pattern: MIDI events → input → set params → assert output.

## MIDI Event Injection

Inject MIDI notes into instrument plugins. Required for testing instruments — without note events, instruments produce silence.

```muse
note on 69 0.8 at 0       // NoteOn: MIDI note 69 (A4), velocity 0.8, at sample 0
note off 69 at 4096        // NoteOff: MIDI note 69, at sample 4096
```

- `note on <note> <velocity> at <timing>` — trigger a note. Note is MIDI number (60=C4, 69=A4), velocity is 0.0–1.0, timing is sample offset.
- `note off <note> at <timing>` — release a note at the given sample offset.
- Events are queued and delivered sample-accurately during processing.
- Only available for instruments (`category instrument` with `midi` block).
- Use with `input silence N samples` — the silence provides the buffer, MIDI events trigger the oscillators.

### Common MIDI note numbers

| Note | MIDI | Note | MIDI |
|------|------|------|------|
| C4 | 60 | A4 | 69 |
| E4 | 64 | C5 | 72 |
| G4 | 67 | A5 | 81 |

## Signal Types

| Signal | Syntax | Description |
|--------|--------|-------------|
| Silence | `silence` | All zeros — useful for testing that no signal leaks through |
| Sine | `sine <freq>Hz` | Pure sine wave at the given frequency |
| Impulse | `impulse` | Single sample at 1.0, rest zeros — tests transient response |

### Examples

```muse
input silence 512 samples
input sine 440Hz 1024 samples
input impulse 256 samples
```

The sample count determines how many samples the test runner processes.

## Set Statements

Set a plugin parameter before processing:

```muse
set param.gain = 0.0
set param.cutoff = 200.0
set param.mix = 1.0
```

Values are bare numbers (no unit suffix in set statements). The parameter is set before processing begins.

## Amplitude Assertions

| Property | Description |
|----------|-------------|
| `output.rms` | RMS level of the output signal (in dB when comparing with dB values) |
| `output.peak` | Peak absolute sample value of the output |
| `input.rms` | RMS level of the input signal |
| `input.peak` | Peak absolute sample value of the input |

### Comparison Operators

| Operator | Meaning |
|----------|---------|
| `<` | Less than |
| `>` | Greater than |
| `==` | Exactly equal |
| `~=` | Approximately equal (with tolerance) |

### dB Suffix

Values can use the `dB` suffix for level comparisons:

```muse
assert output.rms < -120dB     // effectively silent
assert output.rms < -6dB       // attenuated
assert output.peak > 0.0       // some signal present
assert output.peak > 1.0       // signal exceeds unity
```

## Spectral (FFT) Assertions

Check the magnitude of a specific frequency in the output using FFT analysis:

```muse
assert frequency 440Hz > -20dB    // 440Hz component is above -20dB
assert frequency 10000Hz < -30dB  // 10kHz component is below -30dB
```

- Runs a forward FFT on the output, finds the bin closest to the target frequency, and measures its magnitude in dB.
- Requires enough samples for meaningful FFT resolution — use at least 1024 samples, preferably 4096+.
- Automatically adds `rustfft` as a dev-dependency in the generated crate.
- Great for verifying: oscillator pitch, filter attenuation at specific frequencies, distortion harmonic content.

## Temporal Assertions

Check audio properties within a specific sample range — useful for verifying envelope shapes, delay timing, and transient behavior:

```muse
assert output.rms_in 0..256 > -40dB      // sound present during attack
assert output.rms_in 768..1024 < -20dB    // sound decayed during release
assert output.peak_in 0..100 > 0.5        // transient arrives in first 100 samples
```

- `output.rms_in <start>..<end>` — RMS of output samples in the given range
- `output.peak_in <start>..<end>` — peak of output samples in the given range
- Range is in samples (0-indexed).
- Useful with MIDI injection: trigger a note, then check different time windows for attack/sustain/release behavior.

## Safety Assertions

Verify the output contains no corrupted or dangerous values:

```muse
assert no_nan          // no NaN samples (corrupted audio)
assert no_denormal     // no denormalized floats (CPU performance issue)
assert no_inf          // no infinity values (feedback explosion)
```

- These scan every sample in every channel of the output buffer.
- No operator or value — they pass or fail.
- **Recommended for every instrument test.** Instruments with feedback, filters, or complex state are prone to producing NaN/denormal under edge conditions.

## Complete Test Examples

### Effect: silence test
```muse
test "silence in produces silence out" {
  input  silence 512 samples
  set    param.gain = 0.0
  assert output.rms < -120dB
}
```

### Effect: level test
```muse
test "positive gain increases level" {
  input  sine 440Hz 1024 samples
  set    param.gain = 6.0
  assert output.peak > 1.0
}
```

### Effect: filter attenuation
```muse
test "filter attenuates high frequencies" {
  input  sine 10000Hz 1024 samples
  set    param.cutoff = 200.0
  assert output.rms < -6dB
}
```

### Instrument: pitch verification with FFT
```muse
test "A4 plays at correct frequency" {
  note on 69 0.8 at 0
  note off 69 at 4096
  input silence 8192 samples
  assert frequency 440Hz > -20dB
  assert output.rms > -20dB
  assert no_nan
  assert no_denormal
}
```

### Instrument: envelope shape
```muse
test "envelope shape" {
  note on 69 0.8 at 0
  note off 69 at 256
  input silence 1024 samples
  assert output.rms_in 0..256 > -40dB
  assert output.rms_in 768..1024 < -20dB
}
```

### Instrument: silence without notes
```muse
test "no note produces silence" {
  input  silence 512 samples
  assert output.rms < -120dB
}
```

## JSON Output Format

Run tests with: `muse test <file> --format json`

### All tests pass

```json
{
  "status": "ok",
  "file": "examples/poly_synth.muse",
  "total": 3,
  "passed": 3,
  "failed": 0,
  "tests": [
    { "name": "no note produces silence", "result": "pass" },
    { "name": "A4 plays at correct frequency", "result": "pass" },
    { "name": "envelope shape", "result": "pass" }
  ]
}
```

### Test failure

```json
{
  "status": "fail",
  "file": "examples/broken.muse",
  "total": 2,
  "passed": 1,
  "failed": 1,
  "tests": [
    { "name": "passes", "result": "pass" },
    {
      "name": "fails",
      "result": "fail",
      "assertion": "output.rms < -120dB",
      "expected": "< -120.0",
      "actual": "-45.3"
    }
  ]
}
```

## Limitations

- **No custom waveforms.** Only `silence`, `sine <freq>Hz`, and `impulse` — no noise, sweep, or file-based input.
- **FFT resolution depends on buffer size.** Use at least 4096 samples for accurate frequency assertions.
- **No CC injection.** Only `note on`/`note off` — no control change events in test blocks.
