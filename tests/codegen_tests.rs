//! Integration tests for code generation.
//!
//! These tests parse .muse files, resolve them, generate Rust/nih-plug crates,
//! and run `cargo check` to prove the generated code compiles.
//! Also includes unit tests for generated code structure and diagnostic tests.

use std::path::PathBuf;
use std::process::Command;

use muse_lang::{builtin_registry, diagnostics_to_json, generate_plugin, parse, resolve_plugin};

fn generate_from_source(source: &str, output_dir: &std::path::Path) -> PathBuf {
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");
    generate_plugin(&resolved, &registry, output_dir).expect("codegen failed")
}

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

#[test]
fn cargo_toml_contains_cdylib() {
    let source = include_str!("../examples/gain.muse");
    let (cargo_toml, _) = generate_code_strings(source);
    assert!(
        cargo_toml.contains(r#"crate-type = ["cdylib", "lib"]"#),
        "Cargo.toml should contain cdylib + lib crate-type, got:\n{}",
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
    assert_cargo_check(&crate_dir);
}

#[test]
fn poly_synth_cargo_check() {
    let source = r#"plugin "Glass Synth" {
  vendor   "Muse Audio"
  version  "0.1.0"
  url      "https://museaudio.dev"
  email    "hello@museaudio.dev"
  category instrument

  clap {
    id          "dev.museaudio.glass-synth"
    description "A crystalline subtractive synthesizer"
    features    [instrument, stereo, synthesizer]
  }

  vst3 {
    id              "MuseGlassSyn1"
    subcategories   [Instrument, Synth]
  }

  input  mono
  output stereo
  voices 8

  midi {
    note {
      let freq = note.pitch
      let vel = note.velocity
      let gate = note.gate
    }
  }

  param attack: float = 10.0 in 0.5..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param decay: float = 200.0 in 1.0..5000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param sustain: float = 0.7 in 0.0..1.0 {
    display "percentage"
  }

  param release: float = 300.0 in 1.0..10000.0 {
    smoothing linear 5ms
    unit "ms"
  }

  param cutoff: float = 4000.0 in 20.0..20000.0 {
    smoothing logarithmic 15ms
    unit "Hz"
  }

  param resonance: float = 0.3 in 0.0..1.0 {
    smoothing linear 10ms
  }

  param osc_mix: float = 0.5 in 0.0..1.0 {
    display "percentage"
  }

  param volume: float = -6.0 in -60.0..0.0 {
    unit "dB"
  }

  process {
    let env = adsr(param.attack, param.decay, param.sustain, param.release)
    let osc1 = saw(note.pitch)
    let osc2 = square(note.pitch)
    let tone = mix(osc1, osc2) -> gain(param.osc_mix)
    tone -> lowpass(param.cutoff, param.resonance) -> gain(env) -> gain(param.volume) -> output
  }
}"#;

    let tmp = std::env::temp_dir().join("muse-codegen-test-poly-synth");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    assert!(lib_rs.contains("struct Voice"), "Should emit Voice struct");
    assert!(lib_rs.contains("voices: [Option<Voice>; 8]"), "Should allocate 8 voices");
    assert!(lib_rs.contains("next_internal_voice_id: u64"), "Should track voice age");
    assert!(lib_rs.contains("ProcessStatus::Normal"), "Poly process should return Normal");
    assert!(lib_rs.contains("VoiceTerminated"), "Should send VoiceTerminated events");
    assert!(lib_rs.contains("MAX_BLOCK_SIZE"), "Should use block-based rendering");
    assert!(lib_rs.contains("CLAP_POLY_MODULATION_CONFIG"), "Should emit CLAP poly config");

    assert_cargo_check(&crate_dir);
}

#[test]
fn mono_synth_unchanged() {
    let source = include_str!("../examples/synth.muse");
    let (_, lib_rs) = generate_code_strings(source);

    assert!(lib_rs.contains("MidiConfig::Basic"), "Instrument should use MidiConfig::Basic");
    assert!(lib_rs.contains("ProcessStatus::KeepAlive"), "Mono instrument should keep KeepAlive");
    assert!(lib_rs.contains("active_note: Option<u8>"), "Mono should keep active_note");
    assert!(!lib_rs.contains("struct Voice"), "Mono synth should not emit Voice struct");
    assert!(!lib_rs.contains("CLAP_POLY_MODULATION_CONFIG"), "Mono synth should not emit CLAP poly config");
}

#[test]
fn poly_synth_example_cargo_check() {
    let source = include_str!("../examples/poly_synth.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-poly-synth-example");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Verify polyphonic codegen markers
    assert!(lib_rs.contains("struct Voice"), "Should emit Voice struct");
    assert!(lib_rs.contains("voices: [Option<Voice>; 8]"), "Should allocate 8 voices");
    assert!(lib_rs.contains("VoiceTerminated"), "Should send VoiceTerminated events");
    assert!(lib_rs.contains("MAX_BLOCK_SIZE"), "Should use block-based rendering");
    assert!(lib_rs.contains("CLAP_POLY_MODULATION_CONFIG"), "Should emit CLAP poly config");

    assert_cargo_check(&crate_dir);
}

#[test]
fn poly_codegen_contains_voice_struct() {
    let source = include_str!("../tests/fixtures/poly_synth_voice_decl.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(lib_rs.contains("struct Voice"), "Missing Voice struct");
    assert!(lib_rs.contains("voice.note_freq"), "Poly code should address per-voice note frequency");
}

#[test]
fn poly_codegen_contains_block_loop() {
    let source = include_str!("../tests/fixtures/poly_synth_voice_decl.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(lib_rs.contains("block_start"), "Missing block_start loop");
    assert!(lib_rs.contains("block_end"), "Missing block_end loop");
    assert!(lib_rs.contains("smoothed.next_block"), "Missing block parameter smoothing");
}

#[test]
fn poly_codegen_contains_voice_terminated() {
    let source = include_str!("../tests/fixtures/poly_synth_voice_decl.muse");
    let (_, lib_rs) = generate_code_strings(source);
    assert!(lib_rs.contains("NoteEvent::VoiceTerminated"), "Missing VoiceTerminated event emission");
    assert!(lib_rs.contains("compute_fallback_voice_id"), "Missing fallback voice ID helper");
}

#[test]
fn codegen_missing_clap_id_produces_e010() {
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

    let e010_count = diags.iter().filter(|d| d.code == "E010").count();
    assert!(
        e010_count >= 3,
        "expected at least 3 E010 diagnostics (vendor, clap, vst3), got {}: {:?}",
        e010_count,
        diags
    );

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

    let json = diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("should be valid JSON array");

    assert!(!parsed.is_empty(), "should have diagnostics");

    for entry in &parsed {
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
