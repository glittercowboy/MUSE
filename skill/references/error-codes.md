# Error Codes

The Muse compiler emits structured diagnostics with error codes. Parse errors use E001–E002; semantic errors use E003–E009; codegen errors use E010–E011.

## Parse Errors

### E001 — Unexpected Token

**When:** The parser encounters a token it doesn't expect in the current context.

**Common causes:**
- Missing `plugin` keyword at the start of the file
- Typo in a keyword (`proces` instead of `process`)
- Wrong punctuation (`=` instead of `:` in param declarations)
- Using a string where an identifier is expected (e.g., `category "effect"` instead of `category effect`)

**Fix:** Read the "expected" list in the error message. The parser tells you what it was looking for.

**Example diagnostic:**
```json
{
  "code": "E001",
  "message": "unexpected token 'proces', expected 'process', 'param', or 'test'",
  "suggestion": null
}
```

### E002 — Unterminated Construct

**When:** A block or grouping is opened but never closed.

**Common causes:**
- Missing closing `}` on a plugin, process, clap, vst3, param, or test block
- Missing closing `)` on a function call argument list
- Missing closing `]` on a features/subcategories list

**Fix:** Count your braces. Each `{` needs a `}`. Each `(` needs a `)`.

**Example diagnostic:**
```json
{
  "code": "E002",
  "message": "unclosed block: expected '}'",
  "suggestion": "add closing brace '}'"
}
```

## Semantic Errors (Resolve Phase)

### E003 — Unknown Function

**When:** A function call references a name not in the DSP registry.

**Common causes:**
- Typo in function name (`lowpas` instead of `lowpass`)
- Using a function that doesn't exist (`reverb`, `chorus`, etc.)
- Confusing `fold` with `tanh` (only `tanh` is registered)

**Fix:** Check the function name against the [DSP primitives list](dsp-primitives.md). The compiler includes a "Did you mean?" suggestion for close matches.

**Example diagnostic:**
```json
{
  "code": "E003",
  "message": "Unknown function 'lowpas'",
  "suggestion": "Did you mean 'lowpass'?"
}
```

### E004 — Wrong Argument Count

**When:** A DSP function is called with too few or too many arguments.

**Common causes:**
- Passing resonance to `gain` (takes 1 arg, not 2)
- Forgetting all 4 ADSR params: `adsr(10ms)` instead of `adsr(10ms, 100ms, 0.7, 200ms)`
- Passing arguments to `noise()` or `tanh()` (both take 0 args)

**Fix:** Check the function's signature in the [DSP primitives reference](dsp-primitives.md). Remember that optional params (like filter resonance) don't count toward the minimum.

**Example diagnostic:**
```json
{
  "code": "E004",
  "message": "Function 'adsr' expects 4 arguments, got 1"
}
```

### E005 — Type Mismatch

**When:** An argument's type doesn't match what the function expects.

**Common causes:**
- Passing a Time value where Frequency is expected: `lowpass(50ms)` instead of `lowpass(500Hz)`
- Passing a string where a number is expected
- Cross-domain type confusion (Frequency vs Time vs Gain)

**Fix:** Check unit suffixes. `Hz`/`kHz` → Frequency, `ms`/`s` → Time, `dB` → Gain. Bare numbers (`0.5`) are compatible with any numeric type.

**Example diagnostic:**
```json
{
  "code": "E005",
  "message": "Expected Frequency for parameter 'cutoff', got Time"
}
```

### E006 — Invalid Chain Operand

**When:** The `->` operator is used with incompatible types.

**Common causes:**
- Chaining a Number into a Processor: `0.5 -> lowpass(...)` (left side must be Signal)
- Chaining a Signal into a Number: `input -> 0.5` (right side must be Processor or Signal)

**Fix:** The left side of `->` must be `Signal` or `Processor`. The right side must be `Processor`, `Signal` (output), or `Envelope`.

**Example diagnostic:**
```json
{
  "code": "E006",
  "message": "Cannot chain Number into Processor — left side of -> must be Signal or Processor"
}
```

### E007 — Split Without Merge

**When:** A `split { ... }` block appears in a chain without a corresponding `-> merge`.

**Common causes:**
- Forgetting `-> merge` after the split block
- Putting `merge` outside the chain (on a different statement line)

**Fix:** Add `-> merge` immediately after the split block in the same chain expression.

```muse
// Wrong:
input -> split { lowpass(400Hz); highpass(4000Hz) } -> output

// Right:
input -> split { lowpass(400Hz); highpass(4000Hz) } -> merge -> output
```

### E008 — Merge Without Split

**When:** `merge` appears in a chain without a preceding `split { ... }`.

**Common causes:**
- Using `merge` without a split block earlier in the chain
- The split and merge are in different statement contexts

**Fix:** Ensure `merge` follows a `split { ... }` in the same `->` chain.

### E009 — Feedback Type Constraint

**When:** A `feedback { ... }` block's body doesn't produce a Signal or Processor type.

**Common causes:**
- Feedback body ends with a Number expression instead of a signal chain
- Empty feedback body

**Fix:** The feedback body must be a chain that processes and returns audio. Typically: `delay(...) -> filter(...) -> gain(...)`.

## Codegen Errors

### E010 — Missing Required Metadata

**When:** Code generation cannot proceed because required plugin metadata is missing.

**Required for codegen:**
- `vendor` metadata field
- `clap { ... }` block with `id`
- `vst3 { ... }` block with `id`
- `input` and `output` declarations
- `process` block

**Fix:** Add the missing section. Use the plugin template in the SKILL.md essential principles.

### E011 — Unsupported Codegen Feature

**When:** The process block uses a language construct that the code generator doesn't handle yet.

**Common causes:**
- Using an expression form that's parsed but not yet implemented in codegen
- Edge cases in complex nested expressions

**Fix:** Simplify the expression. Break complex chains into separate `let` bindings. If the issue persists, it's a compiler limitation — file a bug.
