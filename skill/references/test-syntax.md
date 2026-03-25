# Test Block Syntax

Test blocks live inside a `plugin` block alongside `process`, `param`, etc. They define input signals, set parameter values, and assert properties of the output.

## Grammar

```
test "test name" {
  input <signal> <count> samples
  set param.<name> = <value>
  assert <property> <op> <value>
}
```

Statements can appear in any order and repeat. Typical pattern: input → set params → assert output.

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

## Assertion Properties

| Property | Description |
|----------|-------------|
| `output.rms` | RMS level of the output signal (in dB when comparing with dB values) |
| `output.peak` | Peak absolute sample value of the output |
| `input.rms` | RMS level of the input signal |
| `input.peak` | Peak absolute sample value of the input |

## Comparison Operators

| Operator | Meaning |
|----------|---------|
| `<` | Less than |
| `>` | Greater than |
| `==` | Exactly equal |
| `~=` | Approximately equal (with tolerance) |

## dB Suffix

Values can use the `dB` suffix for level comparisons:

```muse
assert output.rms < -120dB     // effectively silent
assert output.rms < -6dB       // attenuated
assert output.peak > 0.0       // some signal present
assert output.peak > 1.0       // signal exceeds unity
```

When `dB` is used, the comparison converts the signal's linear measurement to dB scale (20 * log10(value)).

## Complete Test Examples

### Silence test (most reliable)
```muse
test "silence in produces silence out" {
  input  silence 512 samples
  set    param.gain = 0.0
  assert output.rms < -120dB
}
```

### Level test
```muse
test "positive gain increases level" {
  input  sine 440Hz 1024 samples
  set    param.gain = 6.0
  assert output.peak > 1.0
}
```

### Attenuation test
```muse
test "filter attenuates high frequencies" {
  input  sine 10000Hz 1024 samples
  set    param.cutoff = 200.0
  assert output.rms < -6dB
}
```

## JSON Output Format

Run tests with: `muse test <file> --format json`

### All tests pass

```json
{
  "status": "ok",
  "file": "examples/gain.muse",
  "total": 2,
  "passed": 2,
  "failed": 0,
  "tests": [
    { "name": "silence in produces silence out", "result": "pass" },
    { "name": "positive gain increases level", "result": "pass" }
  ]
}
```

### Test failure

```json
{
  "status": "error",
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

- **No MIDI test events.** Test blocks cannot inject `note_on`/`note_off`. Instrument plugins can only be tested for silence (no note = no output).
- **Filter assertion precision.** Due to a known biquad state initialization issue, filter-based assertions on exact dB levels may be unreliable. Prefer relative assertions (`< -6dB`) over exact ones.
- **Supported signals only.** Only `silence`, `sine <freq>Hz`, and `impulse` — no custom waveforms, noise, or multi-frequency signals.
