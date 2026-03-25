# Muse Language Reference

## File Structure

A `.muse` file contains exactly one plugin definition:

```
plugin "Plugin Name" {
  <plugin_items>
}
```

Plugin items may appear in any order. Conventional ordering:

1. Metadata fields (`vendor`, `version`, `url`, `email`, `category`)
2. Format blocks (`clap { ... }`, `vst3 { ... }`)
3. I/O declarations (`input`, `output`)
4. MIDI declaration (`midi { ... }`)
5. Parameter declarations (`param ...`)
6. Process block (`process { ... }`)
7. Test blocks (`test "name" { ... }`)

## Metadata Fields

```muse
vendor   "Muse Audio"
version  "0.1.0"
url      "https://museaudio.dev"
email    "hello@museaudio.dev"
category effect
```

- `vendor`, `version`, `url`, `email` take string literals.
- `category` takes a bare identifier: `effect`, `instrument`, `analyzer`, or `utility`.
- `vendor` is required for code generation. The rest are optional but recommended.

## Format Blocks

### CLAP

```muse
clap {
  id          "com.example.my-plugin"
  description "What this plugin does"
  features    [audio_effect, stereo, utility]
}
```

- `id`: reverse-domain string (required for build)
- `description`: short description string
- `features`: bracket-delimited list of identifiers

### VST3

```muse
vst3 {
  id              "MyPluginVST3"
  subcategories   [Fx, Dynamics]
}
```

- `id`: string identifier (max 16 chars recommended)
- `subcategories`: bracket-delimited list of identifiers

Both `clap` and `vst3` blocks are required for code generation.

## I/O Declarations

```muse
input  stereo    // 2 channels
output stereo    // 2 channels
input  mono      // 1 channel
output 4         // explicit channel count
```

Both `input` and `output` are required. `mono` = 1 channel, `stereo` = 2 channels.

## Parameter Declarations

### Basic syntax

```muse
param name: type = default in min..max { options }
```

### Types

| Type | Syntax | Example |
|------|--------|---------|
| Float | `float` | `param gain: float = 0.0 in -30.0..30.0` |
| Integer | `int` | `param steps: int = 4 in 1..16` |
| Boolean | `bool` | `param bypass: bool = false` |
| Enum | `enum [v1, v2, ...]` | `param mode: enum [lowpass, highpass, bandpass] = lowpass` |

### Options (inside `{ }`)

```muse
param cutoff: float = 1000.0 in 20.0..20000.0 {
  smoothing logarithmic 20ms    // linear | logarithmic | exponential
  unit "Hz"                     // display unit label
  display "frequency"           // display format hint
}
```

- **smoothing**: `linear`, `logarithmic`, or `exponential` followed by a time value
- **unit**: string label shown in the host UI
- **display**: format hint string

Simple parameters omit the body: `param bypass: bool = false`

## Process Block

The process block contains DSP logic — how input becomes output.

```muse
process {
  input -> gain(param.volume) -> output
}
```

### Signal Chain Operator (`->`)

The `->` operator pipes audio left-to-right. It has the **lowest precedence** of all operators.

```muse
input -> highpass(200Hz) -> gain(param.volume) -> output
```

Each stage receives the signal from the previous stage as an implicit first argument.

### Let Bindings

```muse
process {
  let filtered = input -> lowpass(param.cutoff, param.resonance)
  let shaped = filtered -> gain(param.drive) -> tanh()
  shaped -> output
}
```

### If Expressions

`if` is an expression that produces a value:

```muse
let result = if param.drive > 0.0 {
  filtered -> gain(param.drive) -> tanh()
} else {
  filtered
}
result -> output
```

### Signal Routing

#### Split/Merge (Parallel Processing)

```muse
input -> split {
  lowpass(400Hz) -> gain(0.8)
  bandpass(2000Hz) -> gain(1.0)
  highpass(4000Hz) -> gain(0.6)
} -> merge -> output
```

- `split` fans input to N parallel branches (each line is one branch)
- `merge` sums branches back to a single signal
- Every `split` must pair with `merge` — E007 if missing, E008 if merge without split

#### Feedback

```muse
input -> feedback {
  delay(100ms) -> lowpass(2000Hz) -> gain(0.7)
} -> output
```

Feedback creates a loop with implicit one-sample delay. Body must be a `Signal → Signal` chain (E009 if not).

### Implicit Bindings

| Name | Type | Available In |
|------|------|-------------|
| `input` | Signal | `process` block |
| `output` | Signal | `process` block (assignment target) |
| `sample_rate` | Number | `process` block |
| `note.pitch` | Frequency | `midi > note` block |
| `note.velocity` | Number | `midi > note` block |
| `note.gate` | Bool | `midi > note` block |
| `cc.value` | Number | `midi > cc N` block |

## MIDI Block (Instruments)

```muse
midi {
  note {
    let freq = note.pitch
    let vel = note.velocity
    let gate = note.gate
  }
  cc 1 {
    // cc.value is 0.0–1.0
  }
}
```

Plugins with `midi` blocks are instruments. They receive MIDI events and synthesize audio. Requires `category instrument`.

## Type System

| Type | Description |
|------|-------------|
| `Signal` | Audio stream (mono or stereo) |
| `Processor` | Signal processor (receives signal via `->`, produces signal) |
| `Envelope` | Time-varying 0.0–1.0 control signal |
| `Frequency` | Hz or kHz value |
| `Gain` | dB or linear value |
| `Time` | ms or s value |
| `Rate` | % or st value |
| `Param` | Declared plugin parameter reference |
| `Bool` | true/false |
| `Number` | Untyped numeric — compatible with all numeric-domain types |

### Unit Suffixes

| Suffix | Type | Example |
|--------|------|---------|
| `Hz` | Frequency | `440Hz` |
| `kHz` | Frequency | `4kHz` |
| `dB` | Gain | `-12dB` |
| `ms` | Time | `50ms` |
| `s` | Time | `0.5s` |
| `%` | Rate | `50%` |
| `st` | Rate | `2st` |

Bare numbers (e.g. `0.5`) are `Number`, compatible with any numeric-domain type.

### Chain Type Rules

| Left | Right | Result |
|------|-------|--------|
| Signal | Processor | Signal |
| Signal | Signal | Signal (output destination) |
| Processor | Processor | Processor |
| Signal | Envelope | Signal (envelope modulation) |

## Operator Precedence (highest to lowest)

| Level | Operators | Description |
|-------|-----------|-------------|
| 7 | `.` `()` | Field access, function call |
| 6 | `-` `!` | Unary negation, logical not |
| 5 | `*` `/` `%` | Multiply, divide, modulo |
| 4 | `+` `-` | Add, subtract |
| 3 | `==` `!=` `<` `>` `<=` `>=` | Comparison |
| 2 | `&&` `\|\|` | Logical |
| 1 | `->` | Signal chain |

## Comments

```muse
// Line comment
/* Block comment (can nest) */
```
