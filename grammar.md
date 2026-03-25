# Muse Language Grammar

> Formal specification of the Muse audio plugin DSL.
> Version 0.1.0 — Initial language design.

## Overview

Muse is a domain-specific language for declaring audio plugins. A `.muse` file describes a single plugin: its identity, parameters, audio I/O, format-specific metadata, and signal processing logic. The language compiles to Rust/nih-plug, producing VST3 and CLAP plugin binaries.

**Design principles:**

1. **One obvious way** — constrained syntax with minimal ambiguity. AI agents should produce valid Muse on the first attempt.
2. **Visual rhythm** — code reads like a musical score: declarations flow top-to-bottom, signal chains flow left-to-right.
3. **Domain resonance** — keywords and structures borrow from audio/music terminology, not general-purpose programming.
4. **Brace-delimited** — all blocks use `{ }`. No significant whitespace. The grammar is context-free.

## Notation

This grammar uses EBNF-like notation:

- `"literal"` — exact string match
- `rule` — non-terminal reference
- `rule?` — optional (zero or one)
- `rule*` — zero or more
- `rule+` — one or more
- `rule | rule` — alternation
- `( group )` — grouping
- `/* comment */` — grammar annotation

## Lexical Elements

### Comments

```ebnf
line_comment   = "//" , { any_char - newline } , newline ;
block_comment  = "/*" , { any_char } , "*/" ;
```

Line comments extend to end of line. Block comments nest.

### Identifiers

```ebnf
identifier     = ( letter | "_" ) , { letter | digit | "_" } ;
letter         = "a".."z" | "A".."Z" ;
digit          = "0".."9" ;
```

### Keywords

```
plugin  param   process  input   output
clap    vst3    midi     note    cc
true    false   mono     stereo  
in      if      else     let     return
split   merge   feedback
```

Reserved for future use:
```
voice   poly    sample   import  test
bus
```

### Literals

```ebnf
string_literal   = '"' , { string_char } , '"' ;
string_char      = any_char - '"' - '\' | escape_sequence ;
escape_sequence  = '\' , ( '"' | '\' | 'n' | 't' | 'r' ) ;

number_literal   = integer_part , ( "." , digit+ )? ;
integer_part     = digit+ ;

bool_literal     = "true" | "false" ;
```

Numbers are always base-10. No hex, octal, or scientific notation — audio parameters don't need them.

### Unit Suffixes

```ebnf
unit_suffix      = "Hz" | "kHz" | "ms" | "s" | "dB" | "%" | "st" ;
```

Unit suffixes attach directly to number literals with no space: `440Hz`, `50ms`, `0.5s`, `-12dB`, `50%`, `2st` (semitones). They are purely semantic annotations for the type system — the compiler uses them to infer parameter types and validate ranges.

### Operators

```ebnf
/* Arithmetic */
arith_op         = "+" | "-" | "*" | "/" | "%" ;

/* Signal flow */
chain_op         = "->" ;

/* Comparison */
compare_op       = "==" | "!=" | "<" | ">" | "<=" | ">=" ;

/* Logical */
logical_op       = "&&" | "||" ;

/* Assignment */
assign_op        = "=" ;

/* Access */
dot_op           = "." ;

/* Unary */
unary_op         = "-" | "!" ;
```

### Delimiters

```ebnf
delimiters       = "{" | "}" | "(" | ")" | "[" | "]" | "," | ":" | ".." ;
```

## Syntactic Grammar

### Top-Level Structure

A `.muse` file contains exactly one plugin definition.

```ebnf
file             = plugin_def ;

plugin_def       = "plugin" , string_literal , "{" , plugin_body , "}" ;

plugin_body      = plugin_item* ;

plugin_item      = metadata_field
                 | format_block
                 | io_decl
                 | param_decl
                 | midi_decl
                 | process_block ;
```

Items within a plugin body may appear in any order, but the conventional ordering is:

1. Metadata fields (vendor, version, url, email, category)
2. Format blocks (clap, vst3)
3. I/O declarations (input, output)
4. MIDI declaration
5. Parameter declarations
6. Process block

### Metadata Fields

```ebnf
metadata_field   = metadata_key , string_literal ;

metadata_key     = "vendor" | "version" | "url" | "email" | "category" ;
```

Category values are identifiers (not strings): `effect`, `instrument`, `analyzer`, `utility`.

```
vendor    "category" exception: category uses bare identifier
category_field = "category" , category_value ;
category_value = "effect" | "instrument" | "analyzer" | "utility" ;
```

Corrected metadata production:

```ebnf
metadata_field   = string_metadata | category_field ;
string_metadata  = ( "vendor" | "version" | "url" | "email" ) , string_literal ;
category_field   = "category" , category_value ;
category_value   = "effect" | "instrument" | "analyzer" | "utility" ;
```

### Format-Specific Blocks

```ebnf
format_block     = clap_block | vst3_block ;

clap_block       = "clap" , "{" , clap_item* , "}" ;
clap_item        = clap_id | clap_desc | clap_features ;
clap_id          = "id" , string_literal ;
clap_desc        = "description" , string_literal ;
clap_features    = "features" , "[" , feature_list , "]" ;
feature_list     = identifier , ( "," , identifier )* ;

vst3_block       = "vst3" , "{" , vst3_item* , "}" ;
vst3_item        = vst3_id | vst3_subcategories ;
vst3_id          = "id" , string_literal ;
vst3_subcategories = "subcategories" , "[" , feature_list , "]" ;
```

### I/O Declarations

```ebnf
io_decl          = ( "input" | "output" ) , channel_spec ;

channel_spec     = "mono" | "stereo" | channel_count ;
channel_count    = number_literal ;
```

`mono` is sugar for 1 channel, `stereo` for 2. Explicit channel counts support surround and ambisonics layouts.

### MIDI Declaration

```ebnf
midi_decl        = "midi" , "{" , midi_item* , "}" ;

midi_item        = note_handler | cc_handler ;

note_handler     = "note" , "{" , statement* , "}" ;
cc_handler       = "cc" , number_literal , "{" , statement* , "}" ;
```

The `midi` block declares that this plugin accepts MIDI input. `note` handles note-on/note-off events with implicit bindings (`note.pitch`, `note.velocity`, `note.gate`). `cc` handlers bind to specific CC numbers.

> **Note:** Full MIDI AST semantics are defined in S05. The parser recognizes this structural syntax; the AST nodes are populated with runtime behavior in a later slice.

### Parameter Declarations

```ebnf
param_decl       = "param" , identifier , ":" , param_type , param_default? , param_range? , param_body? ;

param_type       = "float" | "int" | "bool" | "enum" , enum_variants ;
enum_variants    = "[" , identifier , ( "," , identifier )* , "]" ;

param_default    = "=" , expression ;

param_range      = "in" , expression , ".." , expression ;

param_body       = "{" , param_option* , "}" ;

param_option     = smoothing_option | display_option | unit_option ;

smoothing_option = "smoothing" , smoothing_type , expression ;
smoothing_type   = "linear" | "logarithmic" | "exponential" ;

display_option   = "display" , string_literal ;
unit_option      = "unit" , string_literal ;
```

A parameter declaration reads naturally:

```
param gain: float = 0.0 in -30.0..30.0 {
  smoothing logarithmic 50ms
  unit "dB"
}
```

The type is explicit. Default value and range follow the name. Options are nested in an optional body block for parameters that need smoothing, display formatting, or unit labels. Simple parameters with no options omit the body:

```
param bypass: bool = false
```

### Process Block

The process block contains the audio processing logic — how input becomes output.

```ebnf
process_block    = "process" , "{" , statement* , "}" ;
```

### Statements

```ebnf
statement        = let_statement
                 | assign_statement
                 | return_statement
                 | expression_statement ;

let_statement    = "let" , identifier , "=" , expression ;
assign_statement = identifier , "=" , expression ;
return_statement = "return" , expression ;
expression_statement = expression ;
```

Note: `if` is an expression, not a statement. It appears in expression position (including as the right-hand side of `let`). See the Expressions section below.

### Expressions

Expressions use standard arithmetic with signal-flow extensions.

```ebnf
expression       = chain_expr ;

/* Signal chain: lowest precedence — left-to-right data flow */
chain_expr       = logical_expr , ( "->" , logical_expr )* ;

/* Logical */
logical_expr     = comparison_expr , ( logical_op , comparison_expr )* ;

/* Comparison */
comparison_expr  = additive_expr , ( compare_op , additive_expr )? ;

/* Additive: + - */
additive_expr    = multiplicative_expr , ( ( "+" | "-" ) , multiplicative_expr )* ;

/* Multiplicative: * / % */
multiplicative_expr = unary_expr , ( ( "*" | "/" | "%" ) , unary_expr )* ;

/* Unary: - ! */
unary_expr       = unary_op , unary_expr | postfix_expr ;

/* Postfix: function calls, field access */
postfix_expr     = primary_expr , postfix_tail* ;
postfix_tail     = "." , identifier
                 | "(" , arg_list? , ")" ;

/* Primary */
primary_expr     = number_literal , unit_suffix?
                 | string_literal
                 | bool_literal
                 | identifier
                 | if_expr
                 | split_expr
                 | merge_expr
                 | feedback_expr
                 | "(" , expression , ")" ;

if_expr          = "if" , expression , "{" , statement* , expression , "}"
                 , ( "else" , "{" , statement* , expression , "}" )? ;

arg_list         = expression , ( "," , expression )* ;
```

### Operator Precedence (highest to lowest)

| Level | Operators | Associativity | Description |
|-------|-----------|---------------|-------------|
| 7 | `.` `()` | left | Field access, function call |
| 6 | `-` `!` | right | Unary negation, logical not |
| 5 | `*` `/` `%` | left | Multiply, divide, modulo |
| 4 | `+` `-` | left | Add, subtract |
| 3 | `==` `!=` `<` `>` `<=` `>=` | none | Comparison |
| 2 | `&&` `\|\|` | left | Logical and, or |
| 1 | `->` | left | Signal chain |

### Signal Chain Semantics

The `->` operator passes the left-hand signal through the right-hand expression. It is the primary composition mechanism for audio processing.

```
input -> highpass(200Hz) -> gain(param.volume) -> output
```

This reads: "take input, pass through a 200Hz highpass filter, then apply gain controlled by the volume parameter, then send to output." Each stage receives the signal from the previous stage as an implicit first argument.

### Signal Routing

`split`, `merge`, and `feedback` extend the chain operator with parallel and recursive signal flow.

#### EBNF Productions

```ebnf
split_expr       = "split" , "{" , split_branch , ( split_branch )* , "}" ;
split_branch     = statement* ;

merge_expr       = "merge" ;

feedback_expr    = "feedback" , "{" , statement* , "}" ;
```

`split` branches are separated by newlines (each line in the block is an independent branch). `merge` is a zero-argument keyword expression. `feedback` takes a brace-delimited body of statements, same shape as a process block.

#### Type Rules

| Expression | Input Type (via `->`) | Output Type | Description |
|------------|----------------------|-------------|-------------|
| `split { ... }` | `Signal` | `Signal` | Fans input to N parallel branches; each branch receives the same input signal and must produce `Signal` |
| `merge` | `Signal` | `Signal` | Sums the parallel branches from a preceding `split` back into a single signal |
| `feedback { ... }` | `Signal` | `Signal` | Creates a feedback loop with implicit one-sample delay; body receives/produces `Signal` |

#### Composition with `->` 

`split`, `merge`, and `feedback` compose with `->` in expression position like any other primary expression:

```muse
// Parallel processing with split/merge
input -> split {
  lowpass(400Hz)
  highpass(4000Hz)
} -> merge -> gain(param.volume) -> output

// Feedback delay loop
input -> feedback {
  delay(100ms) -> lowpass(2000Hz) -> gain(0.7)
} -> output

// Nested split with chains inside branches
input -> split {
  lowpass(400Hz) -> gain(0.8)
  bandpass(1000Hz, 0.5) -> gain(1.0)
  highpass(4000Hz) -> gain(0.6)
} -> merge -> output
```

**Constraints:**
- Every `split` must be followed by a `merge` in the same chain — `split` without `merge` is an error (E007).
- `merge` must follow a `split` — `merge` without a preceding `split` is an error (E008).
- The `feedback` body must be a `Signal → Signal` chain — if the body does not produce `Signal`, that is a type error (E009).
- Branches inside `split` blocks are independent chains. Each branch implicitly receives the split input signal and must produce `Signal`.
- Nesting is allowed: a `split` branch may contain another `split`/`merge` pair.

## Implicit Bindings

Within a `process` block, these names are implicitly bound:

| Name | Type | Description |
|------|------|-------------|
| `input` | signal | Audio input (mono or stereo per I/O declaration) |
| `output` | signal | Audio output (assigned to produce sound) |
| `sample_rate` | float | Host sample rate in Hz |

Within a `midi > note` block:

| Name | Type | Description |
|------|------|-------------|
| `note.pitch` | float | MIDI note number (0–127, as float for microtuning) |
| `note.velocity` | float | Note velocity (0.0–1.0, normalized) |
| `note.gate` | bool | True while note is held |

Within a `midi > cc` block:

| Name | Type | Description |
|------|------|-------------|
| `cc.value` | float | CC value (0.0–1.0, normalized) |

## Built-in Functions

Standard library functions available in process blocks. These are DSP primitives — each compiles to real-time-safe Rust code.

### Oscillators

| Function | Signature | Description |
|----------|-----------|-------------|
| `sine` | `(freq) -> signal` | Sine wave oscillator |
| `saw` | `(freq) -> signal` | Band-limited sawtooth |
| `square` | `(freq) -> signal` | Band-limited square wave |
| `triangle` | `(freq) -> signal` | Band-limited triangle wave |
| `noise` | `() -> signal` | White noise generator |

### Filters

| Function | Signature | Description |
|----------|-----------|-------------|
| `lowpass` | `(cutoff, resonance?) -> processor` | Low-pass filter |
| `highpass` | `(cutoff, resonance?) -> processor` | High-pass filter |
| `bandpass` | `(cutoff, resonance?) -> processor` | Band-pass filter |
| `notch` | `(cutoff, resonance?) -> processor` | Notch filter |

### Envelopes

| Function | Signature | Description |
|----------|-----------|-------------|
| `adsr` | `(attack, decay, sustain, release) -> envelope` | ADSR envelope generator |
| `ar` | `(attack, release) -> envelope` | AR envelope generator |

### Utilities

| Function | Signature | Description |
|----------|-----------|-------------|
| `gain` | `(amount) -> processor` | Apply gain (linear or dB with unit suffix) |
| `pan` | `(position) -> processor` | Stereo pan (-1.0 left to 1.0 right) |
| `delay` | `(time) -> processor` | Delay line |
| `mix` | `(dry, wet) -> signal` | Mix two signals |
| `clip` | `(min, max) -> processor` | Hard clip signal to range |
| `tanh` | `() -> processor` | Soft saturation via hyperbolic tangent |

## Type System

The Muse compiler uses a domain-specific type system to validate DSP expressions at compile time. Types flow through expressions — number literals carry types inferred from unit suffixes, function calls produce typed outputs, and the chain operator enforces signal-flow rules.

### Type Variants

| Type | Description |
|------|-------------|
| `Signal` | An audio signal (mono or stereo stream of samples) |
| `Processor` | A signal processor — receives a signal via `->`, produces a signal |
| `Envelope` | A time-varying control signal (0.0–1.0), usable as a numeric modifier |
| `Frequency` | A frequency value (Hz, kHz) |
| `Gain` | A gain/amplitude value (dB, linear) |
| `Time` | A time duration (ms, s) |
| `Rate` | A rate value (%, st) |
| `Param` | A declared plugin parameter reference |
| `Bool` | A boolean value (true/false) |
| `Number` | An untyped numeric value |

### Type Compatibility

`Number` is compatible with all numeric-domain types — a bare `0.5` can be passed where `Frequency`, `Gain`, `Time`, or `Rate` is expected. However, numeric-domain types are **not** cross-compatible: you cannot pass a `Frequency` where a `Time` is expected. This prevents accidental domain mismatches (e.g., passing a cutoff frequency as a delay time).

`Envelope` is compatible with numeric-domain types — envelopes produce 0.0–1.0 control signals that are musically meaningful as gain, rate, or time modifiers. This enables patterns like `gain(env)` where `env` is an ADSR envelope.

### Unit Suffix Types

Unit suffixes on number literals carry type information into the type system:

| Suffix | Resolves To | Example |
|--------|-------------|---------|
| `Hz` | `Frequency` | `440Hz` |
| `kHz` | `Frequency` | `4kHz` |
| `dB` | `Gain` | `-12dB` |
| `ms` | `Time` | `50ms` |
| `s` | `Time` | `0.5s` |
| `%` | `Rate` | `50%` |
| `st` | `Rate` | `2st` (semitones) |

A bare number like `0.5` resolves to `Number`, which is compatible with any numeric-domain type.

### Chain Operator Types

The `->` operator enforces signal-flow semantics:

| Left Type | Right Type | Result | Description |
|-----------|------------|--------|-------------|
| `Signal` | `Processor` | `Signal` | Standard audio chain — signal flows through processor |
| `Signal` | `Signal` | `Signal` | Output destination — `... -> output` |
| `Processor` | `Processor` | `Processor` | Processor chaining |
| `Signal` | `Envelope` | `Signal` | Envelope modulation of a signal |

Invalid combinations (e.g., `Number -> Processor`, `Signal -> Gain`) produce an E006 diagnostic.

## Error Codes

The Muse compiler uses structured error codes for all diagnostics. Parse-phase errors use E001–E002; semantic resolution errors use E003–E009.

### Parse Errors

| Code | Phase | Description |
|------|-------|-------------|
| `E001` | Parse | Unexpected token — the parser encountered a token it cannot handle in the current context |
| `E002` | Parse | Unterminated or malformed construct (e.g., missing closing brace, incomplete expression) |

### Semantic Errors

| Code | Phase | Description |
|------|-------|-------------|
| `E003` | Resolve | Unknown function — the called function is not in the DSP registry. Includes "did you mean?" suggestions for close matches. |
| `E004` | Resolve | Wrong argument count — the function was called with too few or too many arguments (accounting for optional parameters). |
| `E005` | Resolve | Type mismatch — an argument's type is not compatible with the parameter's expected type. |
| `E006` | Resolve | Invalid chain operand — the `->` operator was used with incompatible types (e.g., chaining a Number into a Processor). |
| `E007` | Resolve | Split without merge — a `split` block was used in a chain without a corresponding `merge` to combine the branches. |
| `E008` | Resolve | Merge without split — `merge` appeared in a chain without a preceding `split` block to provide parallel branches. |
| `E009` | Resolve | Feedback type error — the `feedback` block body does not produce a `Signal` type (body must be Signal → Signal). |

All diagnostics are emitted as structured JSON with the following contract:

```json
{
  "code": "E003",
  "span": [42, 54],
  "severity": "error",
  "message": "Unknown function 'frobnicator'",
  "suggestion": "Did you mean 'triangle'?"
}
```

The `span` is a `[start, end]` byte-offset pair into the source text. The `suggestion` field is optional.

## Complete Example

A minimal gain plugin demonstrating the full structure:

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
}
```
