//! Tests for the semantic resolution/validation pass.

use muse_lang::ast::*;
use muse_lang::diagnostic::Diagnostic;
use muse_lang::dsp::builtin_registry;
use muse_lang::parser::parse_to_diagnostics;
use muse_lang::resolve::{resolve_plugin, ResolvedPlugin};
use muse_lang::types::DspType;

// ── Helpers ──────────────────────────────────────────────────

fn parse_and_resolve(source: &str) -> Result<ResolvedPlugin<'static>, Vec<Diagnostic>> {
    // We need the AST to live long enough. Use Box::leak for test convenience.
    let (ast, parse_diags) = parse_to_diagnostics(source);
    assert!(
        parse_diags.is_empty(),
        "Parse errors (expected none): {:?}",
        parse_diags
    );
    let plugin = ast.expect("Expected successful parse");
    let plugin: &'static PluginDef = Box::leak(Box::new(plugin));
    let registry = builtin_registry();
    resolve_plugin(plugin, &registry)
}

fn resolve_expect_ok(source: &str) -> ResolvedPlugin<'static> {
    match parse_and_resolve(source) {
        Ok(resolved) => resolved,
        Err(diags) => panic!(
            "Expected resolution to succeed, got {} errors:\n{}",
            diags.len(),
            diags
                .iter()
                .map(|d| format!("  {}: {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

fn resolve_expect_errors(source: &str) -> Vec<Diagnostic> {
    match parse_and_resolve(source) {
        Err(diags) => diags,
        Ok(_) => panic!("Expected resolution errors, got success"),
    }
}

fn find_error<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| {
            panic!(
                "Expected error code {code}, found: {:?}",
                diags.iter().map(|d| &d.code).collect::<Vec<_>>()
            )
        })
}

// ── Example file tests ───────────────────────────────────────

#[test]
fn gain_muse_resolves_without_errors() {
    let source = include_str!("../examples/gain.muse");
    let resolved = resolve_expect_ok(source);
    // The type map should have entries for expressions in the process block
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}

#[test]
fn filter_muse_resolves_without_errors() {
    let source = include_str!("../examples/filter.muse");
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}

#[test]
fn synth_muse_resolves_without_errors() {
    let source = include_str!("../examples/synth.muse");
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}

#[test]
fn voices_without_midi_is_error() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  voices 8
  process {
    input -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e010 = find_error(&diags, "E010");
    assert!(
        e010.message.contains("requires a midi block"),
        "Expected midi requirement error, got: {}",
        e010.message
    );
}

#[test]
fn voices_with_midi_is_ok() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let f = note.pitch
    }
  }
  voices 8
  process {
    sine(note.pitch) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn voices_out_of_range_is_error() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let f = note.pitch
    }
  }
  voices 129
  process {
    sine(note.pitch) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e010 = find_error(&diags, "E010");
    assert!(
        e010.message.contains("between 1 and 128"),
        "Expected range error, got: {}",
        e010.message
    );
}

#[test]
fn duplicate_voices_is_error() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let f = note.pitch
    }
  }
  voices 8
  voices 4
  process {
    sine(note.pitch) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e010 = find_error(&diags, "E010");
    assert!(
        e010.message.contains("duplicate voices declaration"),
        "Expected duplicate voice declaration error, got: {}",
        e010.message
    );
}

// ── E003: Unknown function ───────────────────────────────────

#[test]
fn unknown_function_produces_e003() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lolpass(440Hz) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e003 = find_error(&diags, "E003");
    assert!(
        e003.message.contains("lolpass"),
        "E003 message should mention the unknown function name"
    );
}

#[test]
fn unknown_function_suggests_similar_name() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lowpas(440Hz) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e003 = find_error(&diags, "E003");
    assert!(
        e003.suggestion
            .as_ref()
            .map_or(false, |s: &String| s.contains("lowpass")),
        "E003 should suggest 'lowpass', got suggestion: {:?}",
        e003.suggestion
    );
}

// ── E004: Wrong argument count ───────────────────────────────

#[test]
fn too_few_args_produces_e004() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lowpass() -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e004 = find_error(&diags, "E004");
    assert!(
        e004.message.contains("lowpass"),
        "E004 message should mention the function name"
    );
}

#[test]
fn too_many_args_produces_e004() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> gain(1.0, 2.0, 3.0) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e004 = find_error(&diags, "E004");
    assert!(
        e004.message.contains("gain"),
        "E004 message should mention the function name"
    );
}

#[test]
fn optional_param_allows_fewer_args() {
    // lowpass has cutoff (required) + resonance (optional)
    // calling with 1 arg should be fine
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lowpass(1000Hz) -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty());
}

#[test]
fn optional_param_allows_max_args() {
    // lowpass with both cutoff + resonance should also work
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lowpass(1000Hz, 0.5) -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty());
}

// ── E005: Type mismatch ──────────────────────────────────────

#[test]
fn type_mismatch_produces_e005() {
    // sine expects Frequency, pass a Bool
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    sine(true) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e005 = find_error(&diags, "E005");
    assert!(
        e005.message.contains("Frequency") && e005.message.contains("Bool"),
        "E005 should mention expected and actual types, got: {}",
        e005.message
    );
}

#[test]
fn number_is_compatible_with_frequency() {
    // sine(440.0) — plain number where Frequency expected → should work
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    sine(440.0) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn unit_suffix_carries_type_info() {
    // sine(440Hz) — Hz suffix → Frequency, compatible with Frequency param
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    sine(440Hz) -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    // Find the 440Hz literal in the type map — it should be Frequency
    let freq_entries: Vec<_> = resolved
        .type_map
        .iter()
        .filter(|(_, ty)| **ty == DspType::Frequency)
        .collect();
    assert!(
        !freq_entries.is_empty(),
        "Type map should contain at least one Frequency entry for 440Hz"
    );
}

// ── E006: Invalid chain operand ──────────────────────────────

#[test]
fn chain_number_into_number_produces_e006() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    42.0 -> 43.0
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e006 = find_error(&diags, "E006");
    assert!(
        e006.message.contains("->"),
        "E006 should mention the chain operator"
    );
}

#[test]
fn chain_signal_into_number_produces_e006() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> 42.0
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e006 = find_error(&diags, "E006");
    assert!(
        e006.message.contains("Processor"),
        "E006 should mention that right side must be a Processor"
    );
}

// ── Let bindings propagate types ─────────────────────────────

#[test]
fn let_binding_propagates_type() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    let sig = sine(440Hz)
    sig -> gain(0.5) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn let_binding_type_used_in_chain() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    let filtered = input -> lowpass(1000Hz)
    filtered -> output
  }
}
"#;
    resolve_expect_ok(source);
}

// ── param.X field access ─────────────────────────────────────

#[test]
fn param_float_resolves_to_number() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  param gain: float = 0.0 in -30.0..30.0 {}
  process {
    input -> gain(param.gain) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn param_bool_resolves_to_bool() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  param bypass: bool = false
  process {
    if param.bypass {
      input
    } else {
      input -> gain(0.5)
    }
  }
}
"#;
    // This should resolve — param.bypass is Bool, used as condition
    resolve_expect_ok(source);
}

// ── note.X field access ──────────────────────────────────────

#[test]
fn note_pitch_resolves_to_frequency() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  midi {
    note {
      let f = note.pitch
    }
  }
  process {
    sine(note.pitch) -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    // note.pitch should resolve to Frequency
    let freq_entries: Vec<_> = resolved
        .type_map
        .iter()
        .filter(|(_, ty)| **ty == DspType::Frequency)
        .collect();
    assert!(
        !freq_entries.is_empty(),
        "note.pitch should resolve to Frequency"
    );
}

#[test]
fn note_velocity_resolves_to_number() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  midi {
    note {
      let v = note.velocity
    }
  }
  process {
    sine(440Hz) -> gain(note.velocity) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn note_gate_resolves_to_bool() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  midi {
    note {
      let g = note.gate
    }
  }
  process {
    if note.gate {
      sine(440Hz) -> output
    } else {
      input
    }
  }
}
"#;
    resolve_expect_ok(source);
}

// ── Envelope compatibility ───────────────────────────────────

#[test]
fn envelope_is_compatible_with_gain_param() {
    // adsr returns Envelope, gain expects Gain — Envelope should be compatible
    let source = r#"
plugin "Test" {
  input mono
  output mono
  param attack: float = 10.0 in 0.5..5000.0 { unit "ms" }
  param decay: float = 200.0 in 1.0..5000.0 { unit "ms" }
  param sustain: float = 0.7 in 0.0..1.0 {}
  param release: float = 300.0 in 1.0..10000.0 { unit "ms" }
  process {
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    sine(440Hz) -> gain(env) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

// ── Diagnostic JSON format ───────────────────────────────────

#[test]
fn diagnostics_serialize_to_json() {
    let source = r#"
plugin "Test" {
  input mono
  output mono
  process {
    input -> lolpass(440Hz) -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    // Verify JSON serialization works
    let json = muse_lang::diagnostics_to_json(&diags);
    assert!(json.contains("E003"), "JSON should contain error code E003");
    assert!(
        json.contains("lolpass"),
        "JSON should contain the function name"
    );
}

// ── Split/Merge/Feedback routing ─────────────────────────────

#[test]
fn split_branches_resolve() {
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> split {
      lowpass(400Hz) -> gain(0.5)
      highpass(4000Hz) -> gain(0.8)
    } -> merge -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}

#[test]
fn merge_after_split() {
    // A valid split→merge chain should resolve without errors
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> split {
      lowpass(1000Hz)
      highpass(2000Hz)
    } -> merge -> gain(0.5) -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty());
}

#[test]
fn feedback_body_resolves() {
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> feedback {
      delay(100ms) -> gain(0.5)
    } -> output
  }
}
"#;
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}

#[test]
fn error_e007_split_without_merge() {
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> split {
      lowpass(400Hz)
      highpass(4000Hz)
    } -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e007 = find_error(&diags, "E007");
    assert!(
        e007.message.contains("split without merge"),
        "E007 message should mention split without merge, got: {}",
        e007.message
    );
}

#[test]
fn error_e008_merge_without_split() {
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> merge -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e008 = find_error(&diags, "E008");
    assert!(
        e008.message.contains("merge without preceding split"),
        "E008 message should mention merge without split, got: {}",
        e008.message
    );
}

#[test]
fn error_e009_feedback_type_error() {
    let source = r#"
plugin "Test" {
  input stereo
  output stereo
  process {
    input -> feedback {
      42.0
    } -> output
  }
}
"#;
    let diags = resolve_expect_errors(source);
    let e009 = find_error(&diags, "E009");
    assert!(
        e009.message.contains("feedback body must be a signal processing chain"),
        "E009 message should describe feedback body error, got: {}",
        e009.message
    );
}

// ── MPE expression fields ────────────────────────────────────

#[test]
fn note_pressure_resolves_to_number() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let p = note.pressure
    }
  }
  voices 8
  process {
    sine(note.pitch) -> gain(note.pressure) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn note_bend_resolves_to_number() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let b = note.bend
    }
  }
  voices 8
  process {
    sine(note.pitch) -> gain(note.bend) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn note_slide_resolves_to_number() {
    let source = r#"
plugin "Test" {
  input mono
  output stereo
  midi {
    note {
      let sl = note.slide
    }
  }
  voices 8
  process {
    sine(note.pitch) -> gain(note.slide) -> output
  }
}
"#;
    resolve_expect_ok(source);
}

#[test]
fn mpe_synth_example_resolves_without_errors() {
    let source = include_str!("../examples/mpe_synth.muse");
    let resolved = resolve_expect_ok(source);
    assert!(!resolved.type_map.is_empty(), "Type map should not be empty");
}
