<required_reading>
- ../references/error-codes.md — All 11 error codes (E001–E011) with causes, fix patterns, and example diagnostics
- ../references/cli-commands.md — Command syntax, `--format json` flag, exit codes, and JSON output schemas
</required_reading>

<process>

## Step 1: Capture the Error Output

If the user provides error output, use it directly. If they report "it doesn't work" without details, rerun the failing command with `--format json`:

```bash
muse check plugin.muse --format json
# or
muse test plugin.muse --format json
# or
muse build plugin.muse --format json
```

Human-readable diagnostics go to stderr; JSON goes to stdout. Use the JSON output for structured diagnosis.

## Step 2: Identify the Error Code

Extract the error code from the diagnostic output:

**From JSON output:**
```json
{
  "status": "error",
  "diagnostics": [
    { "code": "E003", "message": "Unknown function 'lowpas'", "suggestion": "Did you mean 'lowpass'?" }
  ]
}
```

**From human-readable output:**
Look for the `E0XX` pattern in the error message.

**From exit code alone:**
- Exit code `1` → Parse, semantic, or test error (E001–E009 range or test failure)
- Exit code `2` → Build/I/O error (E010–E011 or system-level issue)

## Step 3: Look Up the Error Code

Find the error code in ../references/error-codes.md and read:
- **When** it occurs (parse, resolve, or codegen phase)
- **Common causes** — the most likely reason for this specific code
- **Fix pattern** — the standard remedy

Error code categories:
| Range | Phase | Typical Fix |
|---|---|---|
| E001–E002 | Parse | Fix syntax: spelling, braces, punctuation |
| E003–E009 | Resolve (semantic) | Fix types, names, argument counts, chain structure |
| E010–E011 | Codegen | Add missing metadata or simplify unsupported constructs |

## Step 4: Apply the Fix

Based on the error code, apply the corresponding fix:

**E001 (Unexpected Token):** Read the "expected" list in the message. The parser says exactly what it wanted. Fix the token at the reported position. Common: `category "effect"` should be `category effect`, `proces` should be `process`.

**E002 (Unterminated Construct):** Count braces. Every `{` needs a `}`. Every `(` needs a `)`. Every `[` needs a `]`. Check the block around the reported span.

**E003 (Unknown Function):** Check the function name against the 23 registered DSP primitives: `sine`, `saw`, `square`, `triangle`, `noise`, `pulse`, `lfo`, `lowpass`, `highpass`, `bandpass`, `notch`, `adsr`, `ar`, `gain`, `pan`, `delay`, `mix`, `clip`, `tanh`, `fold`, `bitcrush`, `chorus`, `compressor`. If the compiler gives a "Did you mean?" suggestion, use it.

**E004 (Wrong Argument Count):** Check the function signature in ../references/dsp-primitives.md. Remember: `noise()` and `tanh()` take 0 args. `adsr` takes exactly 4. Filter resonance is optional (1 or 2 args).

**E005 (Type Mismatch):** Check unit suffixes. `Hz`/`kHz` → Frequency, `ms`/`s` → Time, `dB` → Gain. Bare numbers (`0.5`) are compatible with any numeric type. Don't mix domains: `lowpass(50ms)` is wrong — use `lowpass(500Hz)`.

**E006 (Invalid Chain):** Left side of `->` must be Signal or Processor. Right side must be Processor, Signal (output), or Envelope. You can't chain raw numbers.

**E007 (Split Without Merge):** Add `-> merge` after the `split { ... }` block in the same chain.

**E008 (Merge Without Split):** Remove the orphaned `merge` or add a preceding `split { ... }` block.

**E009 (Feedback Type):** The feedback body must be a `Signal → Signal` chain. Typically: `delay(...) -> filter(...) -> gain(...)`.

**E010 (Missing Metadata):** Add the missing section. Required for codegen: `vendor`, `clap { id }`, `vst3 { id }`, `input`, `output`, `process`.

**E011 (Unsupported Codegen):** Simplify the expression. Break complex chains into separate `let` bindings.

**Test failures** (no error code, but `"result": "fail"`): Read the `assertion`, `expected`, and `actual` fields. Adjust the test assertion threshold or fix the process logic. Remember: filter-based assertions may be imprecise due to the biquad bug.

## Step 5: Re-run the Failing Command

After applying the fix, re-run the exact same command that failed:

```bash
muse check plugin.muse --format json
# or
muse test plugin.muse --format json
```

If new errors appear, repeat from Step 2. Multiple errors may cascade — fixing the first often resolves later ones.

If `"status": "ok"`, the error is resolved.

</process>

<success_criteria>
- The error code is identified and matched to a known pattern
- The fix addresses the root cause (not a workaround)
- The originally failing command now succeeds (`"status": "ok"` or exit code 0)
- No new errors are introduced by the fix
</success_criteria>
