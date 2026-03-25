<required_reading>
- ../references/language-reference.md — Full syntax guide for all plugin constructs
- ../references/dsp-primitives.md — All 16 DSP functions with signatures and types
- ../references/test-syntax.md — Test block grammar, signal types, assertion properties and operators
</required_reading>

<process>

## Step 1: Read the Existing Plugin

Read the full `.muse` file. Identify its current structure:

- **Category:** `effect` or `instrument`
- **Parameters:** list all `param` declarations with types and ranges
- **Process block:** understand the current signal chain
- **Test blocks:** list existing tests and what they cover
- **MIDI block:** present or absent (instruments only)

## Step 2: Identify What to Change

Map the user's request to one or more concrete modifications:

| User wants to... | Modification type |
|---|---|
| Add a new knob/control | Add a `param` declaration + use it in `process` |
| Change the sound/DSP | Modify the `process` block's signal chain |
| Add a filter/effect stage | Insert DSP function into the `->` chain |
| Add parallel processing | Wrap section in `split { ... } -> merge` |
| Add saturation/distortion | Add `-> gain(param.drive) -> tanh()` to chain |
| Add a delay/echo | Add `-> delay(param.time)` or use `feedback { ... }` |
| Add a test | Add a `test "name" { ... }` block |
| Change parameter range | Modify the `in min..max` on an existing param |
| Add smoothing | Add `{ smoothing linear 10ms }` body to a param |
| Convert effect to instrument | Add `category instrument`, `midi { note { ... } }`, oscillators |

## Step 3: Apply the Change

### Adding a Parameter

Insert the new `param` declaration after existing params, before the `process` block:

```muse
param new_param: float = 0.5 in 0.0..1.0 {
  smoothing linear 10ms
  unit "%"
}
```

Then reference it in the process block as `param.new_param`.

**Naming rules:** Parameter names are identifiers — lowercase letters, digits, underscores. No spaces, no hyphens.

### Modifying the Process Block

When adding a new DSP stage, insert it into the existing `->` chain at the appropriate point:

```muse
// Before: simple gain
input -> gain(param.volume) -> output

// After: add filter before gain
input -> lowpass(param.cutoff) -> gain(param.volume) -> output
```

For complex modifications, use `let` bindings to keep the chain readable:

```muse
process {
  let filtered = input -> lowpass(param.cutoff, param.resonance)
  let shaped = filtered -> gain(param.drive) -> tanh()
  shaped -> gain(param.volume) -> output
}
```

### Adding Split/Merge

Wrap a section of the chain in parallel branches:

```muse
// Before: single chain
input -> lowpass(param.cutoff) -> gain(param.volume) -> output

// After: multiband
input -> split {
  lowpass(400Hz) -> gain(param.low_gain)
  highpass(4000Hz) -> gain(param.high_gain)
} -> merge -> gain(param.volume) -> output
```

Every `split` must pair with `-> merge` in the same chain.

### Adding Test Blocks

Add new test blocks after the existing ones, inside the plugin block:

```muse
test "new behavior works" {
  input  sine 440Hz 1024 samples
  set    param.new_param = 1.0
  assert output.peak > 0.0
}
```

**Test guidelines:**
- Always test the new parameter or behavior specifically
- Use `set` to configure the parameter being tested
- Avoid exact dB assertions on filter output (biquad precision bug)
- `set` values are bare numbers: `set param.cutoff = 200.0`, not `200Hz`
- Instrument tests can only assert silence (no MIDI events in test blocks)

## Step 4: Validate and Test

Run `muse check` first for fast syntax/semantic validation:

```bash
muse check plugin.muse --format json
```

If errors appear, use the debug-errors workflow to fix them.

Then run all tests to ensure existing behavior isn't broken and new tests pass:

```bash
muse test plugin.muse --format json
```

Check that:
- `"status": "ok"` — all tests pass
- `"passed"` count equals `"total"` — no regressions
- New tests appear in the results

If a previously passing test now fails, the modification broke existing behavior. Revert or adjust the change to preserve backward compatibility.

</process>

<success_criteria>
- The requested modification is applied to the .muse file
- `muse check` passes with no errors
- All existing tests still pass (no regressions)
- New tests cover the added behavior
- `muse test` returns `"status": "ok"` with all tests passing
</success_criteria>
