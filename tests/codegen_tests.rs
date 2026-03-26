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
    generate_plugin(&resolved, &registry, output_dir, None).expect("codegen failed")
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
    let result = generate_plugin(&resolved, &registry, &tmp, None);

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
    let result = generate_plugin(&resolved, &registry, &tmp, None);
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

#[test]
#[ignore = "requires cargo check — run with --include-ignored"]
fn gui_spectrum_cargo_check() {
    let source = include_str!("../examples/gui_spectrum.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-gui-spectrum-{}",
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

    // ── HTML structural assertions ──
    let html = std::fs::read_to_string(crate_dir.join("assets/editor.html")).unwrap();

    // Tier 2 layout structure
    assert!(
        html.contains("class=\"tier2-root\""),
        "HTML should contain tier2-root container"
    );

    // Spectrum widget present
    assert!(
        html.contains("spectrum-display"),
        "HTML should contain spectrum-display element"
    );

    // XY pad widget present
    assert!(
        html.contains("xy-pad"),
        "HTML should contain xy-pad element"
    );
    assert!(
        html.contains("data-param-x=\"freq\""),
        "XY pad should bind X axis to freq param"
    );
    assert!(
        html.contains("data-param-y=\"resonance\""),
        "XY pad should bind Y axis to resonance param"
    );

    // Standard knob present
    assert!(
        html.contains("data-param=\"gain\""),
        "HTML should contain gain knob"
    );

    // JS initialization present
    assert!(
        html.contains("MuseSpectrum"),
        "HTML should contain MuseSpectrum JS class"
    );
    assert!(
        html.contains("MuseXyPad"),
        "HTML should contain MuseXyPad JS class"
    );

    // Editor dimensions from gui block (800x550)
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("AtomicU32::new(800)"),
        "Editor width should be 800"
    );
    assert!(
        lib_rs.contains("AtomicU32::new(550)"),
        "Editor height should be 550"
    );

    // ── Full cargo check ──
    assert_cargo_check(&crate_dir);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Preview C-ABI exports
// ═══════════════════════════════════════════════════════════════════════════════

fn assert_cargo_check_features(crate_dir: &std::path::Path, features: &str) {
    let output = Command::new("cargo")
        .arg("check")
        .arg("--features")
        .arg(features)
        .current_dir(crate_dir)
        .output()
        .expect("failed to run cargo check --features");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("=== cargo check --features {} FAILED ===", features);
        eprintln!("stdout:\n{}", stdout);
        eprintln!("stderr:\n{}", stderr);
        panic!(
            "cargo check --features {} failed with exit code {:?}",
            features,
            output.status.code()
        );
    }

    eprintln!(
        "cargo check --features {} passed for {}",
        features,
        crate_dir.display()
    );
}

#[test]
fn preview_exports_in_generated_code() {
    let source = include_str!("../examples/gain.muse");
    let (cargo_toml, lib_rs) = generate_code_strings(source);

    // Cargo.toml has the preview feature
    assert!(
        cargo_toml.contains("[features]"),
        "Cargo.toml should contain [features] section"
    );
    assert!(
        cargo_toml.contains("preview = []"),
        "Cargo.toml should contain preview feature"
    );

    // Feature gate
    assert!(
        lib_rs.contains("#[cfg(feature = \"preview\")]"),
        "lib.rs should contain #[cfg(feature = \"preview\")]"
    );

    // All 12 extern "C" functions
    let expected_fns = [
        "fn muse_preview_create(sample_rate: f32) -> *mut u8",
        "fn muse_preview_destroy(ptr: *mut u8)",
        "fn muse_preview_process(",
        "fn muse_preview_get_param_count() -> u32",
        "fn muse_preview_get_param_name(index: u32, buf: *mut u8, buf_len: u32) -> u32",
        "fn muse_preview_get_param_default(index: u32) -> f32",
        "fn muse_preview_set_param(ptr: *mut u8, index: u32, value: f32)",
        "fn muse_preview_get_param(ptr: *mut u8, index: u32) -> f32",
        "fn muse_preview_get_num_channels() -> u32",
        "fn muse_preview_note_on(ptr: *mut u8, note: u8, velocity: f32)",
        "fn muse_preview_note_off(ptr: *mut u8, note: u8)",
        "fn muse_preview_is_instrument() -> bool",
    ];

    for func_sig in &expected_fns {
        assert!(
            lib_rs.contains(func_sig),
            "lib.rs should contain '{}'\n\nGenerated lib.rs tail:\n{}",
            func_sig,
            &lib_rs[lib_rs.len().saturating_sub(2000)..],
        );
    }

    // All functions have #[no_mangle]
    let no_mangle_count = lib_rs.matches("#[no_mangle]").count();
    assert!(
        no_mangle_count >= 12,
        "Expected at least 12 #[no_mangle] attributes in preview module, found {}",
        no_mangle_count
    );

    // Param count is 1 for gain
    assert!(
        lib_rs.contains("fn muse_preview_get_param_count() -> u32 {\n        1\n    }"),
        "Param count should be 1 for gain plugin"
    );

    // Channel count is 2 for stereo
    assert!(
        lib_rs.contains("fn muse_preview_get_num_channels() -> u32 {\n        2\n    }"),
        "Channel count should be 2 for stereo plugin"
    );

    // Param name contains "gain"
    assert!(
        lib_rs.contains("0 => \"gain\""),
        "Param name at index 0 should be \"gain\""
    );

    // Effect plugin: is_instrument returns false
    assert!(
        lib_rs.contains("fn muse_preview_is_instrument() -> bool {\n        false\n    }"),
        "Effect plugin is_instrument should return false"
    );

    // Effect plugin: note_on is a no-op (uses let _ = to suppress unused warnings)
    assert!(
        lib_rs.contains("let _ = (ptr, note, velocity)"),
        "Effect plugin note_on should be a no-op"
    );

    // Effect plugin: note_off is a no-op
    assert!(
        lib_rs.contains("let _ = (ptr, note)"),
        "Effect plugin note_off should be a no-op"
    );
}

#[test]
fn preview_midi_instrument_codegen() {
    let source = include_str!("../examples/synth.muse");
    let (_, lib_rs) = generate_code_strings(source);

    // Instrument plugin: is_instrument returns true
    assert!(
        lib_rs.contains("fn muse_preview_is_instrument() -> bool {\n        true\n    }"),
        "Instrument plugin is_instrument should return true"
    );

    // Instrument plugin: note_on pushes NoteEvent::NoteOn
    assert!(
        lib_rs.contains("NoteEvent::NoteOn {"),
        "Instrument plugin note_on should push NoteEvent::NoteOn\n\nGenerated tail:\n{}",
        &lib_rs[lib_rs.len().saturating_sub(2000)..],
    );

    // Instrument plugin: note_off pushes NoteEvent::NoteOff
    assert!(
        lib_rs.contains("NoteEvent::NoteOff {"),
        "Instrument plugin note_off should push NoteEvent::NoteOff"
    );

    // note_on accesses the instance's context event queue
    assert!(
        lib_rs.contains("instance.ctx.events.push_back(NoteEvent::NoteOn"),
        "Instrument note_on should push into instance.ctx.events"
    );
}

#[test]
fn preview_cargo_check_gain() {
    let source = include_str!("../examples/gain.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-preview-gain");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);

    // Standard check still passes
    assert_cargo_check(&crate_dir);

    // Preview feature check passes
    assert_cargo_check_features(&crate_dir, "preview");
}

#[test]
fn preview_cargo_check_filter() {
    let source = include_str!("../examples/filter.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-preview-filter");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert_cargo_check_features(&crate_dir, "preview");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Preview C-ABI round-trip (requires cargo build)
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a generated crate with `--features preview`, codesign the dylib,
/// and return the path to the loadable .dylib.
#[cfg(target_os = "macos")]
fn build_preview_dylib(source: &str, test_name: &str) -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("muse-preview-test-{test_name}"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);

    // Read package name from generated Cargo.toml
    let cargo_toml_content = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    let package_name = cargo_toml_content
        .lines()
        .find(|l| l.starts_with("name = "))
        .and_then(|l| l.strip_prefix("name = \""))
        .and_then(|l| l.strip_suffix('"'))
        .expect("could not extract package name from Cargo.toml")
        .to_string();

    // cargo build --features preview (debug mode)
    let output = Command::new("cargo")
        .args(["build", "--features", "preview"])
        .current_dir(&crate_dir)
        .output()
        .expect("failed to run cargo build --features preview");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("cargo build --features preview failed:\n{stderr}");
    }

    // Locate the dylib
    let lib_name = package_name.replace('-', "_");
    let dylib = crate_dir
        .join("target/debug")
        .join(format!("lib{lib_name}.dylib"));
    assert!(dylib.exists(), "dylib not found at {}", dylib.display());

    // Codesign (required on Apple Silicon)
    let sign = Command::new("codesign")
        .args(["--force", "--sign", "-", &dylib.display().to_string()])
        .output()
        .expect("failed to run codesign");
    if !sign.status.success() {
        let stderr = String::from_utf8_lossy(&sign.stderr);
        panic!("codesign failed: {stderr}");
    }

    (crate_dir, dylib)
}

#[test]
#[ignore = "requires cargo build — run with --include-ignored"]
#[cfg(target_os = "macos")]
fn preview_cabi_round_trip() {
    use muse_lang::preview::host_plugin::HostPlugin;

    let source = include_str!("../examples/gain.muse");
    let (_crate_dir, dylib) = build_preview_dylib(source, "cabi-round-trip");

    // Load the dylib via HostPlugin (which resolves all 9 C-ABI symbols)
    let plugin = HostPlugin::load(&dylib, 44100.0)
        .expect("HostPlugin::load failed — C-ABI symbols missing or create returned null");

    // Param count: gain.muse has 1 param ("gain")
    assert_eq!(plugin.param_count(), 1, "gain.muse should have 1 param");

    // Channel count: stereo = 2
    assert_eq!(plugin.num_channels(), 2, "gain.muse should be stereo (2 channels)");

    // Param name
    let name = plugin.param_name(0);
    assert_eq!(name, "gain", "first param should be named 'gain'");

    // Param default: gain.muse default is 0.0 dB
    let default_val = plugin.param_default(0);
    assert!(
        (default_val - 0.0).abs() < 0.001,
        "gain default should be 0.0 dB, got {default_val}"
    );

    // Param set/get round-trip
    plugin.set_param(0, 6.0);
    let readback = plugin.get_param(0);
    assert!(
        (readback - 6.0).abs() < 0.01,
        "set_param(0, 6.0) then get_param(0) should round-trip, got {readback}"
    );

    // Process silence through the plugin at 0 dB gain — output should be near-zero
    plugin.set_param(0, 0.0); // 0 dB = unity gain
    let num_samples = 512;
    let silence = vec![0.0_f32; num_samples];
    let inputs: Vec<&[f32]> = vec![&silence, &silence]; // stereo silence
    let mut out_l = vec![0.0_f32; num_samples];
    let mut out_r = vec![0.0_f32; num_samples];
    let mut outputs: Vec<&mut [f32]> = vec![&mut out_l, &mut out_r];
    plugin.process(&inputs, &mut outputs);

    // Silence in at unity gain → silence out
    let max_abs = out_l
        .iter()
        .chain(out_r.iter())
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_abs < 0.001,
        "processing silence at 0 dB gain should produce near-zero output, got peak {max_abs}"
    );

    // Snapshot/restore round-trip
    plugin.set_param(0, -12.0);
    let snapshot = plugin.snapshot_params();
    assert_eq!(snapshot.len(), 1);
    assert!((snapshot[0].1 - (-12.0)).abs() < 0.01);

    eprintln!("preview_cabi_round_trip: all assertions passed");
    // HostPlugin::drop calls muse_preview_destroy automatically
}

#[test]
#[ignore = "requires cargo build — run with --include-ignored"]
#[cfg(target_os = "macos")]
fn preview_instrument_midi_round_trip() {
    use muse_lang::preview::host_plugin::HostPlugin;

    let source = include_str!("../examples/synth.muse");
    let (_crate_dir, dylib) = build_preview_dylib(source, "midi-round-trip");

    // Load the synth plugin
    let plugin = HostPlugin::load(&dylib, 44100.0)
        .expect("HostPlugin::load failed for synth.muse");

    // Verify it's an instrument
    assert!(
        plugin.is_instrument(),
        "synth.muse should report as instrument"
    );

    // Process a buffer WITHOUT any note — should be silent
    let num_samples = 1024;
    let plugin_channels = plugin.num_channels() as usize;
    assert!(plugin_channels >= 1, "plugin should have at least 1 channel");

    let silence = vec![0.0_f32; num_samples];
    let inputs: Vec<&[f32]> = (0..plugin_channels).map(|_| silence.as_slice()).collect();

    let mut out_bufs: Vec<Vec<f32>> = (0..plugin_channels)
        .map(|_| vec![0.0_f32; num_samples])
        .collect();
    let mut outputs: Vec<&mut [f32]> = out_bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
    plugin.process(&inputs, &mut outputs);

    let silent_peak = out_bufs
        .iter()
        .flat_map(|ch| ch.iter())
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    eprintln!("no-note peak: {silent_peak}");
    // Instrument with no note should produce near-silence (ADSR at zero)
    assert!(
        silent_peak < 0.01,
        "instrument with no note should be near-silent, got peak {silent_peak}"
    );

    // Send NoteOn via the C-ABI (simulating what the MIDI callback would do)
    plugin.note_on(69, 0.8); // A4, velocity 0.8

    // Process several buffers to let the oscillator + envelope ramp up
    let mut max_signal = 0.0_f32;
    for _ in 0..8 {
        let mut out_bufs: Vec<Vec<f32>> = (0..plugin_channels)
            .map(|_| vec![0.0_f32; num_samples])
            .collect();
        let mut outputs: Vec<&mut [f32]> = out_bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
        plugin.process(&inputs, &mut outputs);

        let peak = out_bufs
            .iter()
            .flat_map(|ch| ch.iter())
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        max_signal = max_signal.max(peak);
    }

    eprintln!("note-on peak after 8 buffers: {max_signal}");
    assert!(
        max_signal > 0.001,
        "instrument should produce non-silent output after NoteOn, got peak {max_signal}"
    );

    // Send NoteOff
    plugin.note_off(69);

    // Process more buffers — signal should decay (ADSR release)
    // We just verify it doesn't crash; full decay depends on release time
    for _ in 0..4 {
        let mut out_bufs: Vec<Vec<f32>> = (0..plugin_channels)
            .map(|_| vec![0.0_f32; num_samples])
            .collect();
        let mut outputs: Vec<&mut [f32]> = out_bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
        plugin.process(&inputs, &mut outputs);
    }

    eprintln!("preview_instrument_midi_round_trip: all assertions passed");
}

#[test]
fn codegen_delay_cargo_check() {
    let source = r#"plugin "Echo FX" {
    vendor "Test Audio"
    version "0.1.0"
    url "https://test.dev"
    email "test@test.dev"
    category effect

    clap {
        id "dev.test.echo-fx"
        description "Simple echo effect"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestEchoFX00001"
        subcategories [Fx, Delay]
    }

    input stereo
    output stereo

    param time: float = 0.5 in 0.01..5.0 {
        unit "s"
    }

    param mix_amt: float = 0.3 in 0.0..0.95

    process {
        input -> delay(param.time) -> gain(param.mix_amt) -> output
    }
}"#;

    let tmp = std::env::temp_dir().join("muse-codegen-test-delay");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    // Verify generated code contains delay-specific structures
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("struct DelayLine"),
        "Generated code should contain DelayLine struct"
    );
    assert!(
        lib_rs.contains("process_delay("),
        "Generated code should contain process_delay function"
    );
    assert!(
        lib_rs.contains("delay_state_0"),
        "Generated code should contain delay_state_0 field"
    );
    assert!(
        lib_rs.contains(".allocate("),
        "Generated code should contain allocate() call in initialize()"
    );

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_allpass_cargo_check() {
    let source = r#"plugin "Phaser FX" {
    vendor "Test Audio"
    version "0.1.0"
    url "https://test.dev"
    email "test@test.dev"
    category effect

    clap {
        id "dev.test.phaser-fx"
        description "Simple phaser using chained allpass stages"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestPhaserFX0001"
        subcategories [Fx, Modulation]
    }

    input stereo
    output stereo

    param time_val: float = 0.005 in 0.001..0.05 {
        unit "s"
    }

    param feedback_amt: float = 0.7 in 0.0..0.95

    process {
        input -> allpass(param.time_val, param.feedback_amt) -> allpass(param.time_val, param.feedback_amt) -> output
    }
}"#;

    let tmp = std::env::temp_dir().join("muse-codegen-test-allpass");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("struct DelayLine"),
        "Generated code should contain DelayLine struct"
    );
    assert!(
        lib_rs.contains("process_allpass("),
        "Generated code should contain process_allpass function"
    );
    assert!(
        lib_rs.contains("delay_state_0"),
        "Generated code should contain delay_state_0 field"
    );
    assert!(
        lib_rs.contains("delay_state_1"),
        "Two allpass calls should produce delay_state_1"
    );
    assert!(
        lib_rs.contains(".allocate("),
        "Generated code should contain allocate() call in initialize()"
    );

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_comb_cargo_check() {
    let source = r#"plugin "Comb FX" {
    vendor "Test Audio"
    version "0.1.0"
    url "https://test.dev"
    email "test@test.dev"
    category effect

    clap {
        id "dev.test.comb-fx"
        description "Comb filter effect"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestCombFilter01"
        subcategories [Fx, Filter]
    }

    input stereo
    output stereo

    param time_val: float = 0.01 in 0.001..0.1 {
        unit "s"
    }

    param feedback_amt: float = 0.8 in 0.0..0.99

    process {
        input -> comb(param.time_val, param.feedback_amt) -> output
    }
}"#;

    let tmp = std::env::temp_dir().join("muse-codegen-test-comb");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("struct DelayLine"),
        "Generated code should contain DelayLine struct"
    );
    assert!(
        lib_rs.contains("process_comb("),
        "Generated code should contain process_comb function"
    );
    assert!(
        lib_rs.contains("delay_state_0"),
        "Generated code should contain delay_state_0 field"
    );
    assert!(
        lib_rs.contains(".allocate("),
        "Generated code should contain allocate() call in initialize()"
    );

    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_mod_delay_cargo_check() {
    let source = r#"plugin "ModDelay FX" {
    vendor "Test Audio"
    version "0.1.0"
    url "https://test.dev"
    email "test@test.dev"
    category effect

    clap {
        id "dev.test.mod-delay-fx"
        description "Modulated delay effect"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestModDelay0001"
        subcategories [Fx, Delay]
    }

    input stereo
    output stereo

    param time_val: float = 0.3 in 0.01..2.0 {
        unit "s"
    }

    param depth_val: float = 0.5 in 0.0..1.0
    param rate_val: float = 0.5 in 0.1..10.0

    process {
        input -> mod_delay(param.time_val, param.depth_val, param.rate_val) -> output
    }
}"#;

    let tmp = std::env::temp_dir().join("muse-codegen-test-mod-delay");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("struct DelayLine"),
        "Generated code should contain DelayLine struct"
    );
    assert!(
        lib_rs.contains("process_mod_delay("),
        "Generated code should contain process_mod_delay function"
    );
    assert!(
        lib_rs.contains("lfo_phase"),
        "DelayLine should contain lfo_phase field for mod_delay"
    );
    assert!(
        lib_rs.contains("delay_state_0"),
        "Generated code should contain delay_state_0 field"
    );
    assert!(
        lib_rs.contains(".allocate("),
        "Generated code should contain allocate() call in initialize()"
    );

    assert_cargo_check(&crate_dir);
}

#[test]
fn echo_example_cargo_check() {
    let source = include_str!("../examples/echo.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-echo-example");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Verify delay codegen markers
    assert!(lib_rs.contains("struct DelayLine"), "Should emit DelayLine struct");
    assert!(lib_rs.contains("process_delay("), "Should emit process_delay function");
    assert!(lib_rs.contains("delay_state_0"), "Should emit delay state field");
    assert!(lib_rs.contains(".allocate("), "Should call allocate() in initialize()");

    assert_cargo_check(&crate_dir);
}

#[test]
fn phaser_example_cargo_check() {
    let source = include_str!("../examples/phaser.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-phaser-example");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Verify allpass codegen markers — four chained allpass stages need four delay states
    assert!(lib_rs.contains("struct DelayLine"), "Should emit DelayLine struct");
    assert!(lib_rs.contains("process_allpass("), "Should emit process_allpass function");
    assert!(lib_rs.contains("delay_state_0"), "Should emit first delay state");
    assert!(lib_rs.contains("delay_state_3"), "Should emit fourth delay state for 4 allpass stages");

    assert_cargo_check(&crate_dir);
}

// ═══════════════════════════════════════════════════════════════════════════════
// EQ / Shelving filter codegen
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_peak_eq_cargo_check() {
    let source = include_str!("../examples/parametric_eq.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-parametric-eq");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");
    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_eq_contains_expected_structures() {
    let source = include_str!("../examples/parametric_eq.muse");
    let (_, lib_rs) = generate_code_strings(source);

    // BiquadState struct must be emitted
    assert!(
        lib_rs.contains("struct BiquadState"),
        "Generated code should contain BiquadState struct"
    );

    // Four chained EQ calls → four independent eq_biquad_state fields
    assert!(
        lib_rs.contains("eq_biquad_state_0"),
        "Generated code should contain eq_biquad_state_0 for low_shelf"
    );
    assert!(
        lib_rs.contains("eq_biquad_state_1"),
        "Generated code should contain eq_biquad_state_1 for first peak_eq"
    );
    assert!(
        lib_rs.contains("eq_biquad_state_2"),
        "Generated code should contain eq_biquad_state_2 for second peak_eq"
    );
    assert!(
        lib_rs.contains("eq_biquad_state_3"),
        "Generated code should contain eq_biquad_state_3 for high_shelf"
    );

    // Process functions for each EQ type
    assert!(
        lib_rs.contains("process_biquad_low_shelf("),
        "Generated code should contain process_biquad_low_shelf function"
    );
    assert!(
        lib_rs.contains("process_biquad_peak_eq("),
        "Generated code should contain process_biquad_peak_eq function"
    );
    assert!(
        lib_rs.contains("process_biquad_high_shelf("),
        "Generated code should contain process_biquad_high_shelf function"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dynamics: gate codegen
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn codegen_gate_cargo_check() {
    let source = include_str!("../examples/gate.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-gate");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    let crate_dir = generate_from_source(source, &tmp);
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");
    assert_cargo_check(&crate_dir);
}

#[test]
fn codegen_gate_contains_expected_structures() {
    let source = include_str!("../examples/gate.muse");
    let (_, lib_rs) = generate_code_strings(source);

    // GateState struct must be emitted
    assert!(
        lib_rs.contains("struct GateState"),
        "Generated code should contain GateState struct"
    );

    // Process function for gate
    assert!(
        lib_rs.contains("process_gate("),
        "Generated code should contain process_gate function"
    );

    // Per-call-site state field
    assert!(
        lib_rs.contains("gate_state_0"),
        "Generated code should contain gate_state_0 field"
    );
}

// ── Soft Clip + DC Block codegen integration ─────────────────

#[test]
fn soft_clip_dc_block_codegen_compiles() {
    let source = include_str!("fixtures/soft_clip_dc_block.muse");
    let tmp = std::env::temp_dir().join(format!(
        "muse-codegen-test-soft-clip-dc-block-{}",
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
fn soft_clip_dc_block_codegen_contains_structs() {
    let source = include_str!("fixtures/soft_clip_dc_block.muse");
    let (_, lib_rs) = generate_code_strings(source);

    // DC block state and processing
    assert!(
        lib_rs.contains("struct DcBlockState"),
        "Generated code should contain DcBlockState struct"
    );
    assert!(
        lib_rs.contains("process_dc_block("),
        "Generated code should contain process_dc_block function"
    );
    assert!(
        lib_rs.contains("dc_block_state_0"),
        "Generated code should contain dc_block_state_0 field"
    );

    // Soft clip inline math
    assert!(
        lib_rs.contains("__x / (1.0 + __x.abs())"),
        "Generated code should contain soft clip formula"
    );
}
#[test]
fn sample_codegen_contains_include_bytes_and_hound_decode() {
    // Test that codegen for a sample-based plugin contains the expected code patterns.
    // We use the lower-level codegen API directly to avoid needing actual WAV files.
    use muse_lang::codegen::SampleInfo;

    let source = r#"
        plugin "Drum Kit" {
            vendor "Test"
            version "0.1.0"
            category instrument

            clap {
                id "dev.test.drumkit"
                description "test"
                features [instrument, stereo]
            }
            vst3 {
                id "TestDrumKit0001"
                subcategories [Instrument]
            }

            input mono
            output stereo
            voices 4

            sample kick "samples/kick.wav"

            midi {
                note {
                    let num = note.number
                }
            }

            process {
                play(kick) -> output
            }
        }
    "#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let plugin = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&plugin, &registry).expect("resolve failed");

    // Create fake SampleInfo with a dummy absolute path
    let sample_infos = vec![SampleInfo {
        name: "kick".to_string(),
        path: "samples/kick.wav".to_string(),
        absolute_path: "/fake/path/samples/kick.wav".to_string(),
        embed: true,
    }];

    let tmp = std::env::temp_dir().join(format!(
        "muse-sample-codegen-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let result = generate_plugin(&resolved, &registry, &tmp, None);
    // This will fail because samples/kick.wav doesn't exist — expected.
    // Instead, test via the lower-level APIs directly.
    drop(result);

    // Test the process codegen directly
    let voice_count = Some(4u32);
    let (process_body, process_info) = muse_lang::codegen::process::generate_process(
        &plugin, voice_count, None, &sample_infos, &[],
    );

    // Verify play codegen was generated
    assert!(process_info.play_call_count > 0, "Expected play_call_count > 0");
    assert!(process_body.contains("play_active_"), "Process body should contain play_active state");
    assert!(process_body.contains("play_pos_"), "Process body should contain play_pos state");
    assert!(process_body.contains("self.sample_kick"), "Process body should reference sample_kick buffer");

    // Test the plugin struct codegen
    let plugin_code = muse_lang::codegen::plugin::generate_plugin_struct(&plugin, &process_info, &sample_infos, &[]);
    assert!(plugin_code.contains("sample_kick: Vec<f32>"), "Plugin struct should have sample_kick: Vec<f32>");
    assert!(plugin_code.contains("sample_kick_rate: u32"), "Plugin struct should have sample_kick_rate: u32");
    assert!(plugin_code.contains("play_pos_0: f32"), "Voice struct should have play_pos_0: f32");
    assert!(plugin_code.contains("play_active_0: bool"), "Voice struct should have play_active_0: bool");

    // Test initialize() contains hound decode
    assert!(plugin_code.contains("hound::WavReader::new"), "initialize() should decode WAV with hound");
    assert!(plugin_code.contains("SAMPLE_KICK_DATA"), "initialize() should reference SAMPLE_KICK_DATA const");
    assert!(plugin_code.contains("hound::SampleFormat::Float"), "initialize() should handle float WAV format");
    assert!(plugin_code.contains("hound::SampleFormat::Int"), "initialize() should handle int WAV format");

    // Test Cargo.toml generation
    let cargo_toml = muse_lang::codegen::cargo::generate_cargo_toml(&plugin, false, false, true);
    assert!(cargo_toml.contains("hound = \"3.5\""), "Cargo.toml should contain hound dependency");

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn sample_codegen_no_hound_when_no_samples() {
    let cargo_toml = muse_lang::codegen::cargo::generate_cargo_toml(
        &muse_lang::ast::PluginDef {
            name: "Test".to_string(),
            items: vec![],
            span: muse_lang::span::Span::new(0, 0),
        },
        false,
        false,
        false,
    );
    assert!(!cargo_toml.contains("hound"), "Cargo.toml should NOT contain hound when no samples");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Wavetable codegen unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn wavetable_codegen_contains_include_bytes_and_decode() {
    use muse_lang::codegen::WavetableInfo;

    let source = r#"
        plugin "WT Synth" {
            vendor "Test"
            version "0.1.0"
            category instrument

            clap {
                id "dev.test.wtsynth"
                description "test"
                features [instrument, stereo, synthesizer]
            }
            vst3 {
                id "TestWtSynth0001"
                subcategories [Instrument, Synth]
            }

            input mono
            output stereo
            voices 8

            wavetable wt "samples/saw_stack.wav"

            param position: float = 0.0 in 0.0..1.0 {
                display "percentage"
            }

            midi {
                note {
                    let freq = note.pitch
                    let vel = note.velocity
                    let gate = note.gate
                }
            }

            process {
                let snd = wavetable_osc(wt, note.pitch, param.position)
                snd -> gain(note.velocity) -> output
            }
        }
    "#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let plugin = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&plugin, &registry).expect("resolve failed");

    // Create fake WavetableInfo with a dummy absolute path
    let wavetable_infos = vec![WavetableInfo {
        name: "wt".to_string(),
        path: "samples/saw_stack.wav".to_string(),
        absolute_path: "/fake/path/samples/saw_stack.wav".to_string(),
        frame_size: 2048,
        embed: true,
    }];

    let tmp = std::env::temp_dir().join(format!(
        "muse-wavetable-codegen-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    // generate_plugin will fail because the WAV doesn't exist at the fake path —
    // test via lower-level APIs directly (same pattern as sample_codegen test).
    let result = generate_plugin(&resolved, &registry, &tmp, None);
    drop(result);

    // Test the process codegen directly
    let voice_count = Some(8u32);
    let (process_body, process_info) = muse_lang::codegen::process::generate_process(
        &plugin, voice_count, None, &[], &wavetable_infos,
    );

    // Verify wavetable_osc codegen was generated
    assert!(process_info.wt_osc_call_count > 0, "Expected wt_osc_call_count > 0");
    assert!(process_body.contains("process_wavetable_osc"), "Process body should contain process_wavetable_osc call");
    assert!(process_body.contains("wt_osc_state_0"), "Process body should contain wt_osc_state_0");
    assert!(process_body.contains("self.wavetable_wt"), "Process body should reference wavetable_wt buffer");

    // Test the plugin struct codegen
    let plugin_code = muse_lang::codegen::plugin::generate_plugin_struct(&plugin, &process_info, &[], &wavetable_infos);
    assert!(plugin_code.contains("wavetable_wt: Vec<f32>"), "Plugin struct should have wavetable_wt: Vec<f32>");
    assert!(plugin_code.contains("wavetable_wt_frame_size: usize"), "Plugin struct should have wavetable_wt_frame_size: usize");
    assert!(plugin_code.contains("wavetable_wt_frame_count: usize"), "Plugin struct should have wavetable_wt_frame_count: usize");
    assert!(plugin_code.contains("wt_osc_state_0: WtOscState"), "Voice struct should have wt_osc_state_0: WtOscState");

    // Test initialize() contains hound decode
    assert!(plugin_code.contains("hound::WavReader::new"), "initialize() should decode WAV with hound");
    assert!(plugin_code.contains("WAVETABLE_WT_DATA"), "initialize() should reference WAVETABLE_WT_DATA const");

    // Test Cargo.toml generation (has_samples = true triggers hound dep)
    let cargo_toml = muse_lang::codegen::cargo::generate_cargo_toml(&plugin, false, false, true);
    assert!(cargo_toml.contains("hound"), "Cargo.toml should contain hound dependency when wavetables present");

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn wavetable_codegen_no_extra_deps_when_no_wavetables() {
    // A simple effect plugin with no wavetables should not contain any wavetable-related code.
    let source = r#"
        plugin "Clean FX" {
            vendor "Test"
            version "0.1.0"
            category effect

            clap {
                id "dev.test.cleanfx"
                description "test"
                features [audio_effect, stereo]
            }
            vst3 {
                id "TestCleanFX0001"
                subcategories [Fx]
            }

            input stereo
            output stereo

            param gain: float = 0.0 in -60.0..12.0 { unit "dB" }

            process {
                input -> gain(param.gain) -> output
            }
        }
    "#;

    let (_, lib_rs) = generate_code_strings(source);

    assert!(!lib_rs.contains("WAVETABLE_"), "No WAVETABLE_ consts when no wavetables declared");
    assert!(!lib_rs.contains("wavetable_"), "No wavetable_ fields when no wavetables declared");
    assert!(!lib_rs.contains("WtOscState"), "No WtOscState when no wavetables declared");
    assert!(!lib_rs.contains("process_wavetable_osc"), "No process_wavetable_osc when no wavetables declared");
}

#[test]
fn loop_codegen_contains_wraparound() {
    use muse_lang::codegen::SampleInfo;

    let source = r#"
        plugin "LoopTest" {
            input mono
            output stereo

            midi {
                note {
                    let num = note.number
                }
            }

            sample pad "samples/kick.wav"

            process {
                loop(pad) -> output
            }
        }
    "#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let plugin = ast.expect("parse returned None");
    let registry = builtin_registry();
    let _resolved = resolve_plugin(&plugin, &registry).expect("resolve failed");

    let sample_infos = vec![SampleInfo {
        name: "pad".to_string(),
        path: "samples/kick.wav".to_string(),
        absolute_path: "/fake/path/samples/kick.wav".to_string(),
        embed: true,
    }];

    let voice_count = Some(4u32);
    let (process_body, process_info) = muse_lang::codegen::process::generate_process(
        &plugin, voice_count, None, &sample_infos, &[],
    );

    // Verify loop codegen was generated
    assert!(process_info.loop_call_count > 0, "Expected loop_call_count > 0");
    assert!(process_body.contains("loop_active_"), "Process body should contain loop_active state");
    assert!(process_body.contains("loop_pos_"), "Process body should contain loop_pos state");

    // Key assertion: loop uses wraparound (pos = 0.0) NOT deactivation (active = false)
    // The play() pattern sets `play_active_N = false` on buffer end — loop must NOT do that.
    // Instead, loop wraps position back to 0.0.
    assert!(process_body.contains("loop_pos_0 = 0.0"), "Loop should wrap position to 0.0 (not deactivate)");
    // The loop body should NOT contain `loop_active_0 = false`
    assert!(!process_body.contains("loop_active_0 = false"), "Loop should NOT deactivate on buffer end — should wrap instead");

    // Test the plugin struct codegen
    let plugin_code = muse_lang::codegen::plugin::generate_plugin_struct(&plugin, &process_info, &sample_infos, &[]);
    assert!(plugin_code.contains("loop_pos_0: f32"), "Voice struct should have loop_pos_0: f32");
    assert!(plugin_code.contains("loop_active_0: bool"), "Voice struct should have loop_active_0: bool");
}

#[test]
fn codegen_aux_input_ports() {
    let source = r#"
plugin "Sidechain Comp" {
    vendor "Test"
    input main stereo
    input sidechain stereo
    output stereo

    param gain: float = 0.0 in -24.0..24.0 {
        unit "dB"
    }

    clap {
        id "dev.test.sidechain-comp"
        description "Test sidechain"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestSidechain1234"
        subcategories [Fx, Dynamics]
    }

    process {
        input -> gain(param.gain) -> output
    }
}
"#;
    let (_, lib_rs) = generate_code_strings(source);

    // Verify aux_input_ports is generated with the sidechain bus
    assert!(
        lib_rs.contains("aux_input_ports"),
        "Generated code should contain aux_input_ports for sidechain bus"
    );
    assert!(
        lib_rs.contains("new_nonzero_u32(2)"),
        "Sidechain stereo bus should use new_nonzero_u32(2)"
    );
    // Verify PortNames with sidechain name
    assert!(
        lib_rs.contains("Sidechain"),
        "Generated code should contain sidechain port name"
    );
    assert!(
        lib_rs.contains("PortNames"),
        "Generated code should contain PortNames for aux bus names"
    );
    // Main bus should still be present
    assert!(
        lib_rs.contains("main_input_channels: NonZeroU32::new(2)"),
        "Main stereo input should still be declared"
    );
}
