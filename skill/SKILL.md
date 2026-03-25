---
name: muse
description: Write, test, and build audio plugins in the Muse DSL. Use when asked to "create a plugin", "write an audio effect", "build a synth", "make a VST", "CLAP plugin", "audio processing", "DSP effect", or any task involving Muse .muse files.
---

<essential_principles>

## What Is Muse

Muse is a domain-specific language for declaring audio plugins. A `.muse` file describes one plugin: identity, parameters, audio I/O, format metadata, and signal processing logic. The compiler (`muse`) parses, resolves, generates Rust/nih-plug code, and builds native CLAP + VST3 binaries.

## Language Shape

```
plugin "Name" {
  // metadata: vendor, version, url, email, category
  // format blocks: clap { ... }, vst3 { ... }
  // I/O: input stereo, output stereo
  // midi { note { ... } cc N { ... } }    (instruments only)
  // param name: type = default in min..max { smoothing/unit/display }
  // process { signal chain }
  // test "name" { input/set/assert }
}
```

Signal chains use `->` to pipe audio left-to-right:
```
input -> lowpass(param.cutoff) -> gain(param.volume) -> output
```

23 built-in DSP functions: `sine`, `saw`, `square`, `triangle`, `noise`, `pulse`, `lfo`, `lowpass`, `highpass`, `bandpass`, `notch`, `adsr`, `ar`, `gain`, `pan`, `delay`, `mix`, `clip`, `tanh`, `fold`, `bitcrush`, `chorus`, `compressor`.

## Key Constraints

- **One plugin per file.** Every `.muse` file has exactly one `plugin "Name" { ... }` block.
- **Brace-delimited.** No significant whitespace. All blocks use `{ }`.
- **Category is a bare identifier** — `category effect`, not `category "effect"`.
- **Param types:** `float`, `int`, `bool`, `enum [variant1, variant2]`.
- **Unit suffixes on numbers:** `440Hz`, `50ms`, `0.5s`, `-12dB`, `50%`, `2st`. No space between number and suffix.
- **`->` is lowest precedence.** Arithmetic binds tighter than signal chains.
- **`split`/`merge` must pair.** Every `split { ... }` needs a `-> merge` in the same chain.
- **Instruments need a `midi` block** with `note { ... }` to receive MIDI. Implicit bindings: `note.pitch`, `note.velocity`, `note.gate`.
- **Process block implicit bindings:** `input`, `output`, `sample_rate`.

## Known Limitations

- **No MIDI test events.** Test blocks cannot inject `note_on`/`note_off` — you cannot test instrument plugins via test blocks. Use `assert output.rms < -120dB` for silence tests on instruments.
- **macOS only.** The build pipeline (`muse build`) produces macOS CLAP + VST3 bundles. No Linux or Windows support.
- **No GUI.** Plugins get the host's generic parameter UI. No custom editor/view support.
- **No polyphony.** Instrument plugins are monophonic (one voice). Polyphony is planned.
- **Avoid Rust reserved words as variable names.** Don't use `mod`, `fn`, `type`, etc. as `let` binding names in process blocks — they'll break the generated Rust code.

## CLI Quick Reference

```
muse check <file> [--format json]              # Parse + resolve only
muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]
muse test <file> [--format json]               # Run test blocks
muse build <file> [--output-dir <dir>] [--format json]  # Full build → CLAP + VST3
```

Exit codes: `0` success, `1` compile/check/test error, `2` build/I/O error.

## Plugin Template (Copy This)

```muse
plugin "My Effect" {
  vendor   "Your Name"
  version  "0.1.0"
  category effect

  clap {
    id          "com.yourname.my-effect"
    description "Short description"
    features    [audio_effect, stereo]
  }

  vst3 {
    id              "YourMyEffect1"
    subcategories   [Fx]
  }

  input  stereo
  output stereo

  param amount: float = 0.5 in 0.0..1.0 {
    smoothing linear 10ms
  }

  process {
    input -> gain(param.amount) -> output
  }

  test "passes signal" {
    input  sine 440Hz 1024 samples
    set    param.amount = 1.0
    assert output.peak > 0.0
  }

  test "silence in produces silence out" {
    input  silence 512 samples
    assert output.rms < -120dB
  }
}
```

</essential_principles>

<intake>

Before routing, determine what the user needs:

**What do you want to do?**
1. **Create a new plugin** from a description → `workflows/create-plugin.md`
2. **Debug a compiler error** from muse check/compile/test output → `workflows/debug-errors.md`
3. **Extend an existing plugin** (add params, change DSP, add tests) → `workflows/extend-plugin.md`

If the user's intent is clear from their message, skip the question and route directly.

</intake>

<routing>

## Workflow Routing

| User Intent | Workflow | Required Reading |
|---|---|---|
| Create new plugin from description | `workflows/create-plugin.md` | `references/language-reference.md`, `references/dsp-primitives.md`, `references/test-syntax.md`, `references/plugin-recipes.md` |
| Fix compiler/test errors | `workflows/debug-errors.md` | `references/error-codes.md`, `references/cli-commands.md` |
| Add features to existing plugin | `workflows/extend-plugin.md` | `references/language-reference.md`, `references/dsp-primitives.md`, `references/test-syntax.md` |

## Reference Files

| File | Contents |
|---|---|
| `references/language-reference.md` | Complete syntax guide: plugin structure, params, process blocks, signal chains, routing, MIDI, metadata, type system |
| `references/test-syntax.md` | Test block grammar, signal types, assertion properties, operators, JSON output format |
| `references/dsp-primitives.md` | All 23 DSP functions by category with signatures and descriptions |
| `references/error-codes.md` | E001–E011 with causes and fix patterns |
| `references/cli-commands.md` | All 4 CLI commands with flags, exit codes, JSON output schemas |
| `references/plugin-recipes.md` | 9 annotated example patterns: gain, filter, synth, multiband, tremolo, distortion, chorus, dynamics, pulse synth |

</routing>
