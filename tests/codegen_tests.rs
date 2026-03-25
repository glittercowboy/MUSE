//! Integration tests for code generation.
//!
//! These tests parse .muse files, resolve them, generate Rust/nih-plug crates,
//! and run `cargo check` to prove the generated code compiles.
//! Also includes unit tests for generated code structure and diagnostic tests.

use std::path::PathBuf;
use std::process::Command;

use muse_lang::{builtin_registry, diagnostics_to_json, generate_plugin, parse, resolve_plugin};

/// Helper: parse + resolve + codegen from source, return the generated crate path.
fn generate_from_source(source: &str, output_dir: &std::path::Path) -> PathBuf {
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");
    generate_plugin(&resolved, &registry, output_dir).expect("codegen failed")
}

/// Helper: parse + resolve + codegen from source, return generated Cargo.toml and lib.rs as strings.
fn generate_code_strings(source: &str) -> (String, String) {
    let tmp = std::env::temp_dir().join(format!(
        "muse-codegen-test-strings-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    let cargo_toml = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    (cargo_toml, lib_rs)
}

/// Helper: run `cargo check` on a generated crate and assert it succeeds.
fn assert_cargo_check(crate_dir: &std::path::Path) {
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(crate_dir)
        .output()
        .expect("failed to run cargo check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("=== cargo check FAILED ===");
        eprintln!("stdout:\n{}", stdout);
        eprintln!("stderr:\n{}", stderr);
        panic!(
            "cargo check failed with exit code {:?}",
            output.status.code()
        );
    }

    eprintln!("cargo check passed for {}", crate_dir.display());
}

// ══════════════════════════════════════════════════════════════
// Unit tests: verify generated code structure (string assertions)
// ══════════════════════════════════════════════════════════════

#[test]
fn cargo_toml_contains_cdylib() {
    let source = include_str!("../examples/gain.muse");
    let (cargo_toml, _) = generate_code_strings(source);
    assert!(
        cargo_toml.contains(r#"crate-type = ["cdylib"]"#),
        "Cargo.toml should contain cdylib crate-type, got:\n{}",
        cargo_toml
    );
}

#[test]
fn cargo_toml_contains_nih_plug_dep() {
    let source = include_str!("../examples/gain.muse");
    let (cargo_toml, _) = generate_code_strings(source);
    assert!(
        cargo_toml.contains("nih_plug"),
        "Cargo.toml should depend on nih_plug"
    );
    assert!(
        cargo_toml.contains("github.com/robbert-vdh/nih-plug.git"),
        "Cargo.toml should reference nih-plug git repo"
    );
}

#[test]
fn params_struct_has_derive_params() {
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(
        lib_rs.contains("#[derive(Params)]"),
        "Generated lib.rs should contain #[derive(Params)]"
    );
}

#[test]
fn params_float_has_id_attribute() {
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(
        lib_rs.contains(r#"#[id = "gain"]"#),
        "Generated lib.rs should contain #[id = \"gain\"] for the gain param"
    );
}

#[test]
fn params_enum_generates_derive_enum() {
    let source = include_str!("../examples/filter.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(
        lib_rs.contains("#[derive(Enum,"),
        "Generated lib.rs should contain #[derive(Enum, ...)] for enum param types, got:\n{}",
        &lib_rs[..lib_rs.len().min(500)]
    );
}

#[test]
fn plugin_struct_has_arc_params() {
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(
        lib_rs.contains("params: Arc<PluginParams>"),
        "Plugin struct should have params: Arc<PluginParams>"
    );
}

#[test]
fn clap_features_map_correctly() {
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);
    // gain.muse has features [audio_effect, stereo, utility]
    assert!(
        lib_rs.contains("ClapFeature::AudioEffect"),
        "Should map audio_effect to ClapFeature::AudioEffect"
    );
    assert!(
        lib_rs.contains("ClapFeature::Stereo"),
        "Should map stereo to ClapFeature::Stereo"
    );
    assert!(
        lib_rs.contains("ClapFeature::Utility"),
        "Should map utility to ClapFeature::Utility"
    );
}

#[test]
fn vst3_class_id_is_16_bytes() {
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);
    // VST3_CLASS_ID should be a byte literal with exactly 16 bytes
    // gain.muse has vst3 id "MuseWarmGain1" (13 chars, padded to 16 with spaces)
    let marker = r#"const VST3_CLASS_ID: [u8; 16] = *b""#;
    let idx = lib_rs.find(marker).expect("should contain VST3_CLASS_ID");
    let after = &lib_rs[idx + marker.len()..];
    let end_quote = after.find('"').expect("should have closing quote");
    let class_id_str = &after[..end_quote];
    assert_eq!(
        class_id_str.len(),
        16,
        "VST3_CLASS_ID byte literal should be exactly 16 bytes, got {} bytes: {:?}",
        class_id_str.len(),
        class_id_str
    );
}

// ══════════════════════════════════════════════════════════════
// Integration tests: cargo check on generated crates
// ══════════════════════════════════════════════════════════════

#[test]
fn codegen_gain_cargo_check() {
    let source = include_str!("../examples/gain.muse");

    let tmp = std::env::temp_dir().join("muse-codegen-test-gain");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    eprintln!("=== Generated src/lib.rs ===\n{}\n=== END ===", lib_rs);

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_filter_cargo_check() {
    let source = include_str!("../examples/filter.muse");

    let tmp = std::env::temp_dir().join("muse-codegen-test-filter");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_multiband_cargo_check() {
    let source = include_str!("../examples/multiband.muse");

    let tmp = std::env::temp_dir().join("muse-codegen-test-multiband");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    assert_cargo_check(&crate_dir);
}

// ══════════════════════════════════════════════════════════════
// Integration test: instrument mode (synth) cargo check
// ══════════════════════════════════════════════════════════════

#[test]
fn codegen_synth_cargo_check() {
    let source = include_str!("../examples/synth.muse");

    let tmp = std::env::temp_dir().join("muse-codegen-test-synth");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    eprintln!("=== Generated synth src/lib.rs ===\n{}\n=== END ===", lib_rs);

    // Verify instrument-mode codegen markers
    assert!(lib_rs.contains("MidiConfig::Basic"), "Instrument should use MidiConfig::Basic");
    assert!(lib_rs.contains("ProcessStatus::KeepAlive"), "Instrument should use KeepAlive");
    assert!(lib_rs.contains("main_input_channels: None"), "Instrument should have no main input");
    assert!(lib_rs.contains("active_note: Option<u8>"), "Should have active_note field");
    assert!(lib_rs.contains("note_freq: f32"), "Should have note_freq field");
    assert!(lib_rs.contains("velocity: f32"), "Should have velocity field");
    assert!(lib_rs.contains("osc_state_0: OscState"), "Should have first oscillator state");
    assert!(lib_rs.contains("osc_state_1: OscState"), "Should have second oscillator state");
    assert!(lib_rs.contains("adsr_state: AdsrState"), "Should have ADSR state");
    assert!(lib_rs.contains("process_osc_saw"), "Should have saw oscillator function");
    assert!(lib_rs.contains("process_osc_square"), "Should have square oscillator function");
    assert!(lib_rs.contains("process_adsr"), "Should have ADSR function");
    assert!(lib_rs.contains("context.next_event()"), "Should have MIDI event processing");

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_synth_has_instrument_struct_fields() {
    let source = include_str!("../examples/synth.muse");
    let (_, lib_rs) = generate_code_strings(source);

    assert!(lib_rs.contains("osc_state_0: OscState"), "Missing osc_state_0");
    assert!(lib_rs.contains("osc_state_1: OscState"), "Missing osc_state_1");
    assert!(lib_rs.contains("adsr_state: AdsrState"), "Missing adsr_state");
    assert!(lib_rs.contains("active_note: Option<u8>"), "Missing active_note");
    assert!(lib_rs.contains("note_freq: f32"), "Missing note_freq");
    assert!(lib_rs.contains("velocity: f32"), "Missing velocity");
    assert!(lib_rs.contains("sample_rate: f32"), "Missing sample_rate");
}

#[test]
fn codegen_effect_unchanged_after_instrument_mode() {
    // Ensure gain.muse still generates identical effect-mode code
    let source = include_str!("../examples/gain.muse");
    let (_, lib_rs) = generate_code_strings(source);

    assert!(lib_rs.contains("MidiConfig::None"), "Effect should use MidiConfig::None");
    assert!(lib_rs.contains("ProcessStatus::Normal"), "Effect should use ProcessStatus::Normal");
    assert!(!lib_rs.contains("active_note"), "Effect should not have instrument fields");
    assert!(!lib_rs.contains("OscState"), "Effect should not have oscillator state");
    assert!(!lib_rs.contains("AdsrState"), "Effect should not have ADSR state");
    assert!(!lib_rs.contains("KeepAlive"), "Effect should not use KeepAlive");
    assert!(lib_rs.contains("main_input_channels: NonZeroU32::new(2)"), "Effect should have stereo input");
}

// ══════════════════════════════════════════════════════════════
// Diagnostic tests: E010 / E011 error codes and JSON format
// ══════════════════════════════════════════════════════════════

#[test]
fn codegen_missing_clap_id_produces_e010() {
    // Plugin missing vendor, clap, vst3 blocks should produce E010 diagnostics
    let source = r#"plugin "Bare" {
  input stereo
  output stereo
  process {
    input -> output
  }
}"#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");

    let tmp = std::env::temp_dir().join("muse-codegen-test-e010");
    let result = generate_plugin(&resolved, &registry, &tmp);

    assert!(result.is_err(), "expected codegen to fail for bare plugin");
    let diags = result.unwrap_err();

    // Should have E010 for missing vendor, clap, vst3
    let e010_count = diags.iter().filter(|d| d.code == "E010").count();
    assert!(
        e010_count >= 3,
        "expected at least 3 E010 diagnostics (vendor, clap, vst3), got {}: {:?}",
        e010_count,
        diags
    );

    // Each E010 should have a suggestion
    for d in &diags {
        if d.code == "E010" {
            assert!(
                d.suggestion.is_some(),
                "E010 diagnostic should include a suggestion: {:?}",
                d
            );
        }
    }
}

#[test]
fn codegen_diagnostic_json_format() {
    // Verify E010 diagnostics serialize correctly via diagnostics_to_json
    let source = r#"plugin "Bare" {
  input stereo
  output stereo
  process {
    input -> output
  }
}"#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty());
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");

    let tmp = std::env::temp_dir().join("muse-codegen-test-json-format");
    let result = generate_plugin(&resolved, &registry, &tmp);
    let diags = result.unwrap_err();

    // Serialize to JSON
    let json = diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("should be valid JSON array");

    assert!(!parsed.is_empty(), "should have diagnostics");

    for entry in &parsed {
        // Same contract as parse/resolve diagnostics
        let code = entry["code"].as_str().expect("code should be string");
        assert!(code.starts_with('E'), "error code should start with 'E'");

        let span = entry["span"].as_array().expect("span should be array");
        assert_eq!(span.len(), 2, "span should have 2 elements");

        assert!(entry["severity"].is_string(), "severity should be string");
        assert!(entry["message"].is_string(), "message should be string");
    }
}

#[test]
fn codegen_generate_plugin_returns_path() {
    let source = include_str!("../examples/gain.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-path");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    assert_eq!(crate_dir, tmp);
}

// ══════════════════════════════════════════════════════════════
// Unit tests: oscillator DSP helper generation
// ══════════════════════════════════════════════════════════════

#[test]
fn dsp_helpers_emit_osc_state_for_oscillator() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;

    let mut prims = HashSet::new();
    prims.insert(muse_lang::dsp::primitives::DspPrimitive::Oscillator(
        muse_lang::dsp::primitives::OscKind::Saw,
    ));

    let code = generate_dsp_helpers(&prims);
    assert!(
        code.contains("struct OscState"),
        "Should emit OscState struct when oscillator is used, got:\n{}",
        code
    );
    assert!(
        code.contains("fn process_osc_saw("),
        "Should emit process_osc_saw function, got:\n{}",
        code
    );
}

#[test]
fn dsp_helpers_emit_all_oscillator_variants() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;
    use muse_lang::dsp::primitives::{DspPrimitive, OscKind};

    let mut prims = HashSet::new();
    prims.insert(DspPrimitive::Oscillator(OscKind::Saw));
    prims.insert(DspPrimitive::Oscillator(OscKind::Square));
    prims.insert(DspPrimitive::Oscillator(OscKind::Sine));
    prims.insert(DspPrimitive::Oscillator(OscKind::Triangle));

    let code = generate_dsp_helpers(&prims);
    assert!(code.contains("fn process_osc_saw("), "Missing saw");
    assert!(code.contains("fn process_osc_square("), "Missing square");
    assert!(code.contains("fn process_osc_sine("), "Missing sine");
    assert!(code.contains("fn process_osc_triangle("), "Missing triangle");
    // OscState should appear exactly once
    assert!(code.contains("struct OscState"), "Missing OscState");
}

#[test]
fn dsp_helpers_emit_only_used_oscillator() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;
    use muse_lang::dsp::primitives::{DspPrimitive, OscKind};

    let mut prims = HashSet::new();
    prims.insert(DspPrimitive::Oscillator(OscKind::Sine));

    let code = generate_dsp_helpers(&prims);
    assert!(code.contains("fn process_osc_sine("), "Should emit sine");
    assert!(!code.contains("fn process_osc_saw("), "Should NOT emit saw");
    assert!(!code.contains("fn process_osc_square("), "Should NOT emit square");
}

// ══════════════════════════════════════════════════════════════
// Unit tests: ADSR envelope DSP helper generation
// ══════════════════════════════════════════════════════════════

#[test]
fn dsp_helpers_emit_adsr_state_and_function() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;
    use muse_lang::dsp::primitives::{DspPrimitive, EnvKind};

    let mut prims = HashSet::new();
    prims.insert(DspPrimitive::Envelope(EnvKind::Adsr));

    let code = generate_dsp_helpers(&prims);
    assert!(
        code.contains("enum AdsrStage"),
        "Should emit AdsrStage enum, got:\n{}",
        code
    );
    assert!(
        code.contains("struct AdsrState"),
        "Should emit AdsrState struct"
    );
    assert!(
        code.contains("fn process_adsr("),
        "Should emit process_adsr function"
    );
    // Verify all stages are present
    assert!(code.contains("Attack"), "AdsrStage should include Attack");
    assert!(code.contains("Decay"), "AdsrStage should include Decay");
    assert!(code.contains("Sustain"), "AdsrStage should include Sustain");
    assert!(code.contains("Release"), "AdsrStage should include Release");
    assert!(code.contains("Idle"), "AdsrStage should include Idle");
}

#[test]
fn dsp_helpers_no_adsr_when_not_used() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;
    use muse_lang::dsp::primitives::{DspPrimitive, FilterKind};

    let mut prims = HashSet::new();
    prims.insert(DspPrimitive::Filter(FilterKind::Lowpass));

    let code = generate_dsp_helpers(&prims);
    assert!(!code.contains("AdsrState"), "Should NOT emit ADSR when only filter is used");
    assert!(!code.contains("OscState"), "Should NOT emit oscillator when only filter is used");
}

// ══════════════════════════════════════════════════════════════
// Unit tests: MIDI event loop generation
// ══════════════════════════════════════════════════════════════

#[test]
fn midi_event_loop_generates_non_empty_code() {
    let code = muse_lang::codegen::midi::generate_midi_event_loop();
    assert!(!code.is_empty(), "MIDI event loop should not be empty");
    assert!(
        code.contains("NoteEvent::NoteOn"),
        "Should handle NoteOn events"
    );
    assert!(
        code.contains("NoteEvent::NoteOff"),
        "Should handle NoteOff events"
    );
    assert!(
        code.contains("self.active_note"),
        "Should use self.active_note for note tracking"
    );
    assert!(
        code.contains("self.note_freq"),
        "Should set self.note_freq from MIDI note"
    );
    assert!(
        code.contains("self.velocity"),
        "Should capture velocity from NoteOn into self.velocity"
    );
}

// ══════════════════════════════════════════════════════════════
// Regression: existing filter codegen still works
// ══════════════════════════════════════════════════════════════

#[test]
fn dsp_helpers_still_emit_biquad_for_filters() {
    use std::collections::HashSet;
    use muse_lang::codegen::dsp::generate_dsp_helpers;
    use muse_lang::dsp::primitives::{DspPrimitive, FilterKind};

    let mut prims = HashSet::new();
    prims.insert(DspPrimitive::Filter(FilterKind::Lowpass));
    prims.insert(DspPrimitive::Filter(FilterKind::Highpass));

    let code = generate_dsp_helpers(&prims);
    assert!(code.contains("struct BiquadState"), "Should still emit BiquadState");
    assert!(code.contains("fn process_biquad("), "Should still emit lowpass biquad");
    assert!(code.contains("fn process_biquad_highpass("), "Should still emit highpass biquad");
}
