<required_reading>
- ../references/language-reference.md — Full syntax: plugin structure, params, process blocks, signal chains, routing, MIDI, GUI blocks, type system
- ../references/dsp-primitives.md — All 24 DSP functions with signatures, types, and usage examples
- ../references/test-syntax.md — Test block grammar, signal types, assertions, JSON output format
- ../references/cli-commands.md — `muse check`, `muse test`, `muse build`, `muse preview` with flags and exit codes
- ../references/error-codes.md — E001–E014 with causes and fix patterns (E012–E014 are GUI-specific)
- ../references/plugin-recipes.md — Recipe 13 (GUI Tier 1) and Recipe 14 (GUI Tier 2) as starting points
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
- **DSP behavior** — what the plugin does to audio. Identify which DSP primitives are needed.
- **GUI complexity** — determine the GUI tier:
  - User wants a quick themed editor with auto-generated knobs → **Tier 1**
  - User wants specific widget placement, panels, or visualizations → **Tier 2**
  - User wants custom CSS styling, gradients, or branded look → **Tier 2 + CSS**

## Step 2: Choose GUI Tier and Base Pattern

| User wants... | Start from | GUI Tier |
|---|---|---|
| Themed knobs, no layout control needed | Recipe 13 (GUI Gain) | Tier 1 |
| Specific widget arrangement, titled panels | Recipe 14 (GUI Spectrum) | Tier 2 |
| Visualization widgets (spectrum, waveform, EQ curve) | Recipe 14 (GUI Spectrum) | Tier 2 |
| XY pad or multi-param widgets | Recipe 14 (GUI Spectrum) | Tier 2 |
| Custom CSS gradients, shadows, fonts | `examples/gui_styled.muse` | Tier 2 + CSS |
| Simple effect + GUI | Recipe 13 modified | Tier 1 |
| Instrument + GUI | Recipe 3 (Synth) + GUI block | Tier 1 or 2 |

Copy the selected recipe's structure as your starting skeleton.

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

  // 6. GUI block (see Step 4 below)
  gui { ... }

  // 7. Process block
  process {
    input -> dsp_chain -> output
  }

  // 8. Test blocks (Step 5)
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

## Step 4: Add GUI Block

Place the `gui { }` block after param declarations, before the process block.

### Tier 1: Auto-Layout (theme + accent only)

Use when the user just wants a themed editor. The compiler generates knobs for all params automatically.

```muse
gui {
  theme dark
  accent "#E8A87C"
}
```

That's it. No layout, panels, or widgets needed. Every declared param gets a knob.

### Tier 2: Explicit Layout

Use when the user wants control over widget placement, visualization widgets, or grouped panels.

```muse
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
        knob freq
        knob resonance
        knob gain
      }
    }
  }
}
```

**Layout containers:**
- `layout vertical { ... }` — children stack top-to-bottom
- `layout horizontal { ... }` — children arrange left-to-right
- `layout grid { ... }` — CSS grid layout
- `panel "Title" { ... }` — titled grouping section with border/background
- Layouts and panels nest arbitrarily

**Widget types:**

| Widget | Syntax | Binds To |
|--------|--------|----------|
| `knob` | `knob <param>` | float/int param |
| `slider` | `slider <param>` | float/int param |
| `meter` | `meter <param>` | float/int param |
| `switch` | `switch <param>` | bool param |
| `label` | `label "Text"` | — (static text) |
| `value` | `value <param>` | float/int param |
| `xy_pad` | `xy_pad <paramX> <paramY>` | 2 float params |
| `spectrum` | `spectrum` | — (visualization) |
| `waveform` | `waveform` | — (visualization) |
| `envelope` | `envelope` | — (visualization) |
| `eq_curve` | `eq_curve` | — (visualization) |
| `reduction` | `reduction` | — (visualization) |

**Widget rules:**
- Param-bound widgets (`knob`, `slider`, `meter`, `switch`, `value`) take a param name matching a declared `param`. E014 if unknown.
- `xy_pad` takes two param names (X and Y axes). Both must be declared params.
- Visualization widgets (`spectrum`, `waveform`, `envelope`, `eq_curve`, `reduction`) take no parameters. E014 if a param is given.
- `label` takes a string literal, not a param name.

**Widget properties (optional):**

```muse
knob gain {
  style "vintage"
  class "hero-knob"
  label "Output Gain"
}
```

### Tier 2 + CSS: Custom Styling

Add a `css` string inside the gui block for full visual control:

```muse
gui {
  theme dark
  accent "#E8A87C"
  size 800 500

  layout vertical {
    knob gain { style "vintage" class "hero-knob" }
    slider mix
  }

  css ".hero-knob canvas { filter: drop-shadow(0 0 8px var(--accent)); } body { background: linear-gradient(135deg, #1a1a2e, #16213e); }"
}
```

- CSS must be a non-empty string (E014 if empty)
- Injected into a `<style>` tag in the editor HTML
- `var(--accent)` references the declared accent color
- Target widget elements via their CSS class names

### GUI Validation Rules

| Error | Cause | Fix |
|-------|-------|-----|
| E012 | WebView init or IPC failure at runtime | Check macOS WebKit availability; verify web assets exist in bundle |
| E013 | Invalid GUI declaration syntax | Check theme/accent/size/layout/widget syntax against the grammar above |
| E014 | Semantic GUI error: invalid theme, bad hex color, unknown param on widget, param on visualization, empty CSS | Fix the specific field flagged in the diagnostic message |

## Step 5: Add Test Blocks

Every plugin needs at least 2 tests. Test blocks verify DSP behavior — they do not test the GUI.

### Effect plugins

```muse
test "silence in produces silence out" {
  input  silence 512 samples
  assert output.rms < -120dB
}

test "passes signal" {
  input  sine 440Hz 1024 samples
  set    param.amount = 1.0
  assert output.peak > 0.0
}
```

### Instrument plugins

```muse
test "no note produces silence" {
  input  silence 512 samples
  assert output.rms < -120dB
}

test "note produces sound" {
  note on 69 0.8 at 0
  note off 69 at 4096
  input silence 8192 samples
  assert output.rms > -20dB
  assert no_nan
  assert no_denormal
}
```

### Test constraints
- `silence`, `sine <freq>Hz`, or `impulse` as input signals (the only three types)
- `note on`/`note off` inject MIDI for instrument testing. Use `input silence` as buffer.
- `set` values are bare numbers, no unit suffixes: `set param.cutoff = 200.0`
- Always include safety checks (`no_nan`, `no_denormal`) for instrument tests

## Step 6: Validate with `muse check`, `muse preview`, and `muse test`

Run in this order — fix errors at each stage before proceeding.

```bash
# 1. Syntax/semantic check
muse check plugin.muse --format json
```

If errors appear, look up the code in `../references/error-codes.md`. GUI-specific errors:
- E013: Invalid GUI declaration syntax — check brace matching, keyword spelling, layout nesting
- E014: Semantic GUI error — check theme value (`dark`/`light`), accent hex format, widget param names match declared params, no params on visualization widgets, CSS string is non-empty

```bash
# 2. Preview the GUI (macOS only — opens a native window)
muse preview plugin.muse --format json
```

Preview opens a native window showing the editor. Visually verify:
- Theme applied correctly (dark/light background)
- Accent color visible on active knobs/sliders
- Panels labeled and grouped correctly (Tier 2)
- Visualization widgets rendering (Tier 2)
- CSS customizations applied (Tier 2 + CSS)
- Editor size matches declared `size` dimensions

```bash
# 3. Run DSP tests
muse test plugin.muse --format json
```

Check JSON output for `"status": "ok"` and all tests passing.

## Step 7: Build Plugin Binaries

```bash
muse build plugin.muse --output-dir ./build --format json
```

On success, the JSON output includes `clap_bundle` and `vst3_bundle` paths. The bundles contain the embedded web-view editor — no separate GUI assets needed.

If build fails with exit code 2, check stderr for system-level errors.

</process>

<success_criteria>
- A `.muse` file exists with valid syntax (passes `muse check`)
- The file includes a `gui { }` block with at least `theme` and `accent`
- `muse preview` opens a window showing the custom editor (visual verification)
- The file has at least 2 test blocks
- All tests pass (`muse test` returns `"status": "ok"`)
- `muse build` produces both `.clap` and `.vst3` bundles (exit code 0)
- The plugin matches the user's original description in behavior, parameters, and GUI layout
</success_criteria>
