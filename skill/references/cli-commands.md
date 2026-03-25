# CLI Commands

The `muse` CLI has 4 commands. All accept `--format json` for machine-readable output.

## Commands

### `muse check`

Parse and resolve a `.muse` file without generating code. Fast syntax/semantic validation.

```bash
muse check <file> [--format json]
```

**Use when:** Validating syntax during editing, checking for errors before a full build.

**JSON output (success):**
```json
{"status":"ok"}
```

**JSON output (error):**
```json
{
  "status": "error",
  "diagnostics": [
    {
      "code": "E003",
      "span": [42, 54],
      "severity": "error",
      "message": "Unknown function 'frobnicator'",
      "suggestion": "Did you mean 'triangle'?"
    }
  ]
}
```

---

### `muse compile`

Parse, resolve, and generate a Rust/nih-plug crate from a `.muse` file. Optionally builds it.

```bash
muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]
```

**Options:**
- `--output-dir <dir>`: Where to put the generated crate (default: current directory)
- `--no-build`: Generate Rust crate only, skip `cargo build`
- `--release`: Build in release mode (default: debug)
- `--format json`: Structured JSON output

**Use when:** You want the generated Rust crate for inspection, or want to compile without the full bundle step.

**JSON output (success):**
```json
{
  "status": "ok",
  "plugin_name": "Warm Gain",
  "package_name": "warm-gain",
  "clap_id": "dev.museaudio.warm-gain",
  "version": "0.1.0",
  "crate_dir": "/path/to/warm-gain",
  "bundle_path": "/path/to/Warm Gain.clap"
}
```

---

### `muse test`

Compile the plugin, then run the in-language test blocks. Reports pass/fail for each test.

```bash
muse test <file> [--output-dir <dir>] [--format json]
```

**Use when:** Verifying test assertions pass. This is the primary feedback loop during development.

**JSON output (all pass):**
```json
{
  "status": "ok",
  "file": "examples/gain.muse",
  "total": 2,
  "passed": 2,
  "failed": 0,
  "tests": [
    {"name": "silence in produces silence out", "result": "pass"},
    {"name": "positive gain increases level", "result": "pass"}
  ]
}
```

**JSON output (failure):**
```json
{
  "status": "error",
  "file": "examples/broken.muse",
  "total": 2,
  "passed": 1,
  "failed": 1,
  "tests": [
    {"name": "passes", "result": "pass"},
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

---

### `muse build`

Full pipeline: compile → build → bundle → codesign. Produces macOS CLAP + VST3 plugin bundles.

```bash
muse build <file> [--output-dir <dir>] [--format json]
```

**Use when:** Producing final plugin binaries for installation in a DAW.

**JSON output (success):**
```json
{
  "status": "ok",
  "plugin_name": "Warm Gain",
  "package_name": "warm-gain",
  "phases": {
    "compile": { "duration_ms": 0 },
    "cargo_build": { "duration_ms": 8200 },
    "clap_bundle": { "duration_ms": 0 },
    "vst3_bundle": { "duration_ms": 0 },
    "codesign_clap": { "duration_ms": 25 },
    "codesign_vst3": { "duration_ms": 15 }
  },
  "artifacts": {
    "clap": { "path": "Warm Gain.clap", "size_bytes": 1440912 },
    "vst3": { "path": "Warm Gain.vst3", "size_bytes": 1440896 },
    "crate_dir": "/path/to/warm-gain"
  }
}
```

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success — no errors |
| `1` | Compile, check, or test error — diagnostics emitted |
| `2` | Build or I/O error — system-level failure |

## Common Patterns

### Validate → Test → Build workflow

```bash
# 1. Quick syntax check
muse check plugin.muse --format json

# 2. Run tests
muse test plugin.muse --format json

# 3. Build binaries
muse build plugin.muse --output-dir ./build --format json
```

### Check JSON status programmatically

```bash
# Test and check status
muse test plugin.muse --format json 2>/dev/null | grep -q '"status":"ok"'
```

Note: Human-readable diagnostics go to stderr; JSON output goes to stdout. Use `2>/dev/null` to suppress stderr when parsing JSON.
