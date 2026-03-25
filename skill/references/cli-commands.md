# CLI Commands

The `muse` CLI has 5 commands. All accept `--format json` for machine-readable output.

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

---

### `muse preview`

Live audio preview with hot-reload. Compiles the plugin, loads it as a dynamic library, and plays audio through the system output device. Watches the source file for changes and hot-reloads the plugin without restarting audio.

```bash
muse preview <file> [--format json] [--midi-port <name|list>] [--input <source>]
```

**Options:**
- `--format json`: Emit structured JSON events to stdout (one JSON object per line)
- `--midi-port <name>`: Connect to a specific MIDI input port for instrument plugins
- `--midi-port list`: List available MIDI input ports and exit
- `--input <source>`: Audio input source for effect plugins:
  - `silence` (default) — effect processes silence
  - `mic` — capture from system microphone (requires macOS microphone permission)
  - `file:<path>` — play a WAV file in a loop through the plugin

**Use when:** Iterating on a plugin's sound — hear changes immediately after saving the `.muse` file. Effect plugins can process live mic input or a WAV file. Instrument plugins receive MIDI from a connected controller.

**Behavior:**
- Initial compile happens at 44100 Hz; if the system audio device runs at a different rate (e.g. 48000 Hz), the plugin is automatically rebuilt at the device rate.
- File changes trigger a full recompile → rebuild → reload cycle. The old plugin keeps playing until the new one is ready.
- Ctrl+C stops preview gracefully.
- For instrument plugins, `--input` is ignored (instruments generate audio from MIDI).
- For effect plugins without `--input`, the plugin processes silence.

**Constraints:**
- macOS only (uses CoreAudio via CPAL for audio I/O)
- `--input mic` requires macOS microphone permission (system prompt on first use)
- `--input file:<path>` accepts WAV files only (mono/stereo, i16/i24/i32/f32). A warning is printed if the file sample rate differs from the device rate (no resampling).
- Blocks the terminal while running (Ctrl+C to stop)

**JSON events** (`--format json`):

Each event is a single JSON object on one line to stdout. Human-readable messages go to stderr.

| Event | Fields | When |
|-------|--------|------|
| `started` | `plugin_name`, `sample_rate`, `channels`, `is_instrument` | After initial compile and audio start |
| `file_changed` | — | Source file modification detected |
| `reloaded` | — | Hot-reload succeeded |
| `error` | `phase`, `diagnostics` (compile) or `message` (other) | Compile or reload error |
| `stopped` | — | Ctrl+C or watcher disconnect |

**JSON event examples:**

```json
{"event":"started","plugin_name":"Warm Gain","sample_rate":48000.0,"channels":2,"is_instrument":false}
{"event":"file_changed"}
{"event":"reloaded"}
{"event":"error","phase":"compile","diagnostics":[{"code":"E003","span":[42,54],"severity":"error","message":"Unknown function 'frobnicator'"}]}
{"event":"stopped"}
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

### Effect preview with microphone

```bash
# Process live mic input through an effect plugin
muse preview filter.muse --input mic
# Speak into microphone → hear filtered audio through speakers
# Edit filter.muse → plugin hot-reloads without restarting audio
```

### Effect preview with WAV file

```bash
# Loop a WAV file through an effect plugin
muse preview delay.muse --input file:drums.wav
```

### Instrument preview with MIDI controller

```bash
# List available MIDI ports
muse preview synth.muse --midi-port list

# Connect to a specific MIDI port
muse preview synth.muse --midi-port "USB MIDI Controller"
```

### Agent-driven preview with JSON events

```bash
# Machine-readable event stream for automation
muse preview plugin.muse --format json --input mic 2>/dev/null
# stdout receives: {"event":"started",...}  {"event":"file_changed"}  {"event":"reloaded"}  etc.
```
