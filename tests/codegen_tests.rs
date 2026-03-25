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

#[test]
fn mpe_synth_cargo_check() {
    let source = include_str!("../examples/mpe_synth.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-mpe-synth");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Verify MPE expression fields in Voice struct
    assert!(lib_rs.contains("pressure: f32"), "Voice struct should have pressure field");
    assert!(lib_rs.contains("tuning: f32"), "Voice struct should have tuning field");
    assert!(lib_rs.contains("slide: f32"), "Voice struct should have slide field");

    // Verify MPE expression event handlers
    assert!(lib_rs.contains("PolyPressure"), "Should handle PolyPressure events");
    assert!(lib_rs.contains("PolyTuning"), "Should handle PolyTuning events");
    assert!(lib_rs.contains("PolyBrightness"), "Should handle PolyBrightness events");

    // Verify MPE field access in process block maps to voice fields
    assert!(lib_rs.contains("voice.pressure"), "note.pressure should map to voice.pressure");

    assert_cargo_check(&crate_dir);
}

#[test]
fn unison_synth_cargo_check() {
    let source = include_str!("../examples/unison_synth.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-unison-synth");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Verify UNISON_MAX constant
    assert!(lib_rs.contains("const UNISON_MAX: i32 = 16;"), "Should have UNISON_MAX constant");

    // Verify unison voice allocation in NoteOn
    assert!(lib_rs.contains("detuned_freq"), "NoteOn should compute detuned frequencies");
    assert!(lib_rs.contains("unison_vid"), "NoteOn should derive unison voice IDs");
    assert!(lib_rs.contains("UNISON_MAX"), "NoteOn should use UNISON_MAX for voice ID derivation");

    assert_cargo_check(&crate_dir);
}

#[test]
fn note_on_codegen_contains_note_event_and_vecdeque() {
    let source = r#"
    plugin "Test Synth" {
        vendor "Test"
        input mono
        output stereo

        clap {
            id "dev.test.synth"
            description "Test synth"
            features [instrument, stereo]
        }

        vst3 {
            id "TestSynth00001"
            subcategories [Instrument, Synth]
        }

        midi {
            note {
                let freq = note.pitch
                let vel = note.velocity
                let gate = note.gate
            }
        }

        param attack: float = 10.0 in 0.5..5000.0 { unit "ms" }
        param decay: float = 200.0 in 1.0..5000.0 { unit "ms" }
        param sustain: float = 0.7 in 0.0..1.0
        param release: float = 300.0 in 1.0..10000.0 { unit "ms" }

        process {
            let env = adsr(param.attack, param.decay, param.sustain, param.release)
            saw(note.pitch) -> gain(env) -> output
        }

        test "note produces sound" {
            note on 69 0.8 at 0
            note off 69 at 4096
            input silence 8192 samples
            assert output.rms > -20.0
        }
    }
    "#;

    let (_, lib_rs) = generate_code_strings(source);

    assert!(
        lib_rs.contains("VecDeque"),
        "Generated code should use VecDeque for event queue"
    );
    assert!(
        lib_rs.contains("NoteEvent::NoteOn"),
        "Generated code should contain NoteEvent::NoteOn for injected events"
    );
    assert!(
        lib_rs.contains("NoteEvent::NoteOff"),
        "Generated code should contain NoteEvent::NoteOff for injected events"
    );
    assert!(
        lib_rs.contains("push_back"),
        "Generated code should push events to the VecDeque"
    );
    assert!(
        lib_rs.contains("pop_front"),
        "Generated code should pop events from the VecDeque via next_event"
    );
}

#[test]
fn safety_assert_codegen_contains_nan_denormal_inf_checks() {
    let source = r#"
    plugin "Safe Plugin" {
        vendor "Test"
        input stereo
        output stereo

        clap {
            id "dev.test.safe"
            description "Safety test"
            features [audio_effect, stereo]
        }

        vst3 {
            id "SafePlugin00001"
            subcategories [Fx]
        }

        process { input }

        test "safety checks" {
            input silence 512 samples
            assert no_nan
            assert no_denormal
            assert no_inf
        }
    }
    "#;

    let (_, lib_rs) = generate_code_strings(source);

    assert!(
        lib_rs.contains("is_nan()"),
        "Generated code should contain is_nan() check"
    );
    assert!(
        lib_rs.contains("MIN_POSITIVE"),
        "Generated code should contain MIN_POSITIVE for denormal detection"
    );
    assert!(
        lib_rs.contains("is_infinite()"),
        "Generated code should contain is_infinite() check"
    );
    assert!(
        lib_rs.contains("MUSE_TEST_FAIL"),
        "Generated code should emit MUSE_TEST_FAIL on failure"
    );
}

#[test]
fn temporal_assertion_codegen_contains_range_slice() {
    let source = r#"
    plugin "Temporal Plugin" {
        vendor "Test"
        input stereo
        output stereo

        clap {
            id "dev.test.temporal"
            description "Temporal test"
            features [audio_effect, stereo]
        }

        vst3 {
            id "TempoPlugin00001"
            subcategories [Fx]
        }

        process { input }

        test "temporal checks" {
            input silence 1024 samples
            assert output.rms_in 0..256 > -10.0
            assert output.peak_in 256..512 < -60.0
        }
    }
    "#;

    let (_, lib_rs) = generate_code_strings(source);

    assert!(
        lib_rs.contains("output[0][0..256]"),
        "Generated code should slice output for rms_in range"
    );
    assert!(
        lib_rs.contains("output[0][256..512]"),
        "Generated code should slice output for peak_in range"
    );
    assert!(
        lib_rs.contains("compute_rms"),
        "Generated code should compute RMS on the sliced range"
    );
    assert!(
        lib_rs.contains("compute_peak"),
        "Generated code should compute peak on the sliced range"
    );
}

#[test]
fn frequency_assert_codegen_has_fft_helper_and_rustfft() {
    let source = r#"
    plugin "FFT Plugin" {
        vendor "Test"
        input stereo
        output stereo

        clap {
            id "dev.test.fft"
            description "FFT test"
            features [audio_effect, stereo]
        }

        vst3 {
            id "FFTTestPlugin001"
            subcategories [Fx]
        }

        process { input }

        test "spectral test" {
            input sine 440 Hz 4096 samples
            assert frequency 440Hz > -20.0
        }
    }
    "#;

    let (cargo_toml, lib_rs) = generate_code_strings(source);

    // Cargo.toml should include rustfft dev-dependency
    assert!(
        cargo_toml.contains("rustfft"),
        "Cargo.toml should contain rustfft dev-dependency when frequency assertions present"
    );
    assert!(
        cargo_toml.contains("[dev-dependencies]"),
        "Cargo.toml should have [dev-dependencies] section"
    );

    // lib.rs should contain the FFT helper and assertion
    assert!(
        lib_rs.contains("compute_magnitude_at_freq"),
        "Generated code should contain compute_magnitude_at_freq helper"
    );
    assert!(
        lib_rs.contains("FftPlanner"),
        "Generated code should use FftPlanner from rustfft"
    );
    assert!(
        lib_rs.contains("use rustfft"),
        "Generated code should import rustfft"
    );
}

#[test]
fn no_fft_when_no_frequency_assertions() {
    let source = r#"
    plugin "Plain Plugin" {
        vendor "Test"
        input stereo
        output stereo

        clap {
            id "dev.test.plain"
            description "Plain test"
            features [audio_effect, stereo]
        }

        vst3 {
            id "PlainPlugin00001"
            subcategories [Fx]
        }

        process { input }

        test "basic test" {
            input silence 512 samples
            assert output.rms < -60.0
        }
    }
    "#;

    let (cargo_toml, lib_rs) = generate_code_strings(source);

    // Cargo.toml should NOT include rustfft when no frequency assertions
    assert!(
        !cargo_toml.contains("rustfft"),
        "Cargo.toml should not contain rustfft when no frequency assertions"
    );
    assert!(
        !cargo_toml.contains("[dev-dependencies]"),
        "Cargo.toml should not have [dev-dependencies] when not needed"
    );

    // lib.rs should NOT contain FFT helper
    assert!(
        !lib_rs.contains("compute_magnitude_at_freq"),
        "Generated code should not contain FFT helper when no frequency assertions"
    );
}

#[test]
#[ignore = "requires cargo check — run with --include-ignored"]
fn preset_gain_cargo_check() {
    let source = include_str!("../examples/preset_gain.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-preset-gain-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert_cargo_check(&crate_dir);
}

#[test]
fn preset_codegen_contains_apply_preset_and_names() {
    let source = include_str!("../examples/preset_gain.muse");
    let (_cargo_toml, lib_rs) = generate_code_strings(source);

    assert!(
        lib_rs.contains("pub const PRESET_NAMES"),
        "Generated code should contain PRESET_NAMES constant"
    );
    assert!(
        lib_rs.contains("pub fn apply_preset"),
        "Generated code should contain apply_preset function"
    );
    assert!(
        lib_rs.contains(r#""Unity""#),
        "Generated code should contain Unity preset name"
    );
    assert!(
        lib_rs.contains(r#""Boost""#),
        "Generated code should contain Boost preset name"
    );
    assert!(
        lib_rs.contains(r#""Cut""#),
        "Generated code should contain Cut preset name"
    );
    assert!(
        lib_rs.contains("db_to_gain"),
        "Generated code should wrap dB param in db_to_gain"
    );
}

#[test]
fn preset_test_codegen_calls_apply_preset() {
    let source = include_str!("../examples/preset_gain.muse");
    let (_cargo_toml, lib_rs) = generate_code_strings(source);

    assert!(
        lib_rs.contains(r#"apply_preset(&plugin.params, "Unity")"#),
        "Test code should call apply_preset for Unity preset"
    );
    assert!(
        lib_rs.contains(r#"apply_preset(&plugin.params, "Boost")"#),
        "Test code should call apply_preset for Boost preset"
    );
    assert!(
        lib_rs.contains(r#"apply_preset(&plugin.params, "Cut")"#),
        "Test code should call apply_preset for Cut preset"
    );
}

#[test]
fn no_preset_code_when_no_presets() {
    let source = include_str!("../examples/gain.muse");
    let (_cargo_toml, lib_rs) = generate_code_strings(source);

    assert!(
        !lib_rs.contains("PRESET_NAMES"),
        "Generated code should not contain PRESET_NAMES when no presets defined"
    );
    assert!(
        !lib_rs.contains("apply_preset"),
        "Generated code should not contain apply_preset when no presets defined"
    );
}

#[test]
#[ignore = "requires cargo check — run with --include-ignored"]
fn gui_gain_cargo_check() {
    let source = include_str!("../examples/gui_gain.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-gui-gain-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);

    // Verify GUI-specific output structure
    assert!(crate_dir.join("assets/editor.html").exists(), "assets/editor.html missing");

    let cargo_toml = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("objc2"), "Cargo.toml should contain objc2 dependency");
    assert!(cargo_toml.contains("objc2-web-kit"), "Cargo.toml should contain objc2-web-kit dependency");
    assert!(cargo_toml.contains("serde_json"), "Cargo.toml should contain serde_json dependency");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("mod editor"), "lib.rs should contain editor module");
    assert!(lib_rs.contains("define_class!"), "lib.rs should contain define_class! macro");
    assert!(lib_rs.contains("WKWebView"), "lib.rs should reference WKWebView");
    assert!(lib_rs.contains("fn editor("), "lib.rs should contain editor() method");
    assert!(lib_rs.contains("paramBridge"), "lib.rs should set up paramBridge IPC handler");

    // Verify the HTML includes the param name
    let html = std::fs::read_to_string(crate_dir.join("assets/editor.html")).unwrap();
    assert!(html.contains("gain"), "editor.html should reference the gain param");
    assert!(html.contains("#E8A87C"), "editor.html should contain the accent color");

    assert_cargo_check(&crate_dir);
}

#[test]
#[ignore = "requires cargo check — run with --include-ignored"]
fn gui_layout_cargo_check() {
    let source = include_str!("../examples/gui_layout.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-gui-layout-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);

    // ── Verify GUI-specific output structure ──
    assert!(
        crate_dir.join("assets/editor.html").exists(),
        "assets/editor.html missing"
    );

    // ── Tier 2 structural assertions on HTML ──
    let html = std::fs::read_to_string(crate_dir.join("assets/editor.html")).unwrap();

    // Must have Tier 2 layout structure
    assert!(
        html.contains("class=\"tier2-root\""),
        "Tier 2 HTML should contain tier2-root container"
    );
    assert!(
        html.contains("layout-vertical"),
        "Tier 2 HTML should contain layout-vertical class"
    );
    assert!(
        html.contains("layout-horizontal"),
        "Tier 2 HTML should contain layout-horizontal class"
    );
    assert!(
        html.contains("class=\"panel-title\""),
        "Tier 2 HTML should contain panel-title elements"
    );
    assert!(
        html.contains("Controls"),
        "Tier 2 HTML should contain 'Controls' panel title"
    );

    // Must NOT have Tier 1 flat knob grid in the body (only in CSS as a rule)
    // The Tier 1 pattern is <div class="knob-grid"> — Tier 2 uses tier2-root instead
    assert!(
        !html.contains("<div class=\"knob-grid\">"),
        "Tier 2 HTML should NOT contain knob-grid div element"
    );

    // Must reference both params
    assert!(html.contains("data-param=\"gain\""), "Should reference gain param");
    assert!(html.contains("data-param=\"mix\""), "Should reference mix param");

    // Label widget present
    assert!(
        html.contains("class=\"label-widget\""),
        "Tier 2 HTML should contain label widget"
    );

    // ── Editor module assertions ──
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("evaluateJavaScript"),
        "Editor should contain evaluateJavaScript for Rust→JS sync"
    );
    // Editor dimensions should match size 700 450
    assert!(
        lib_rs.contains("AtomicU32::new(700)"),
        "Editor width should be 700"
    );
    assert!(
        lib_rs.contains("AtomicU32::new(450)"),
        "Editor height should be 450"
    );

    // ── Full cargo check ──
    assert_cargo_check(&crate_dir);
}

#[test]
#[ignore = "requires cargo check — run with --include-ignored"]
fn gui_styled_cargo_check() {
    let source = include_str!("../examples/gui_styled.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-gui-styled-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);

    // ── Verify GUI-specific output structure ──
    assert!(
        crate_dir.join("assets/editor.html").exists(),
        "assets/editor.html missing"
    );

    // ── Tier 3 structural assertions on HTML ──
    let html = std::fs::read_to_string(crate_dir.join("assets/editor.html")).unwrap();

    // Must have Tier 2 layout structure (Tier 3 builds on Tier 2)
    assert!(
        html.contains("class=\"tier2-root\""),
        "Tier 3 HTML should contain tier2-root container"
    );
    assert!(
        html.contains("layout-vertical"),
        "Tier 3 HTML should contain layout-vertical class"
    );

    // Must NOT have Tier 1 flat knob grid in the body
    assert!(
        !html.contains("<div class=\"knob-grid\">"),
        "Tier 3 HTML should NOT contain knob-grid div element"
    );

    // ── Tier 3: custom CSS injection ──
    assert!(
        html.contains("/* --- Custom CSS --- */"),
        "Tier 3 HTML should contain custom CSS comment marker"
    );
    assert!(
        html.contains("drop-shadow"),
        "Tier 3 CSS should contain drop-shadow from custom CSS"
    );
    assert!(
        html.contains("linear-gradient"),
        "Tier 3 CSS should contain linear-gradient from custom CSS"
    );

    // ── Widget props: class and style ──
    assert!(
        html.contains("hero-knob"),
        "Tier 3 HTML should contain hero-knob class from widget prop"
    );
    assert!(
        html.contains("data-style=\"vintage\""),
        "Tier 3 HTML should contain vintage data-style from widget prop"
    );

    // ── Slider widget present ──
    assert!(
        html.contains("class=\"slider-cell\""),
        "Tier 3 HTML should contain slider widget"
    );
    assert!(
        html.contains("id=\"slider-mix\""),
        "Tier 3 HTML should contain slider for mix param"
    );

    // Must reference both params
    assert!(html.contains("data-param=\"gain\""), "Should reference gain param");
    assert!(html.contains("data-param=\"mix\""), "Should reference mix param");

    // ── Editor module assertions ──
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("evaluateJavaScript"),
        "Editor should contain evaluateJavaScript for Rust→JS sync"
    );
    // Editor dimensions should match size 800 500
    assert!(
        lib_rs.contains("AtomicU32::new(800)"),
        "Editor width should be 800"
    );
    assert!(
        lib_rs.contains("AtomicU32::new(500)"),
        "Editor height should be 500"
    );

    // ── Full cargo check ──
    assert_cargo_check(&crate_dir);
}
