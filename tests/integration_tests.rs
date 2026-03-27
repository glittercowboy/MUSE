//! End-to-end integration tests for the Muse parser.
//!
//! These tests read actual .muse example files, parse them, and verify
//! the AST structure matches what's written in the source.

use muse_lang::ast::*;
use muse_lang::diagnostic::diagnostics_to_json;
use muse_lang::parser::parse_to_diagnostics;
use muse_lang::{resolve_plugin, builtin_registry};

/// Helper: read an example file and parse it, asserting zero errors.
fn parse_example(filename: &str) -> PluginDef {
    let path = format!("examples/{filename}");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let (ast, diags) = parse_to_diagnostics(&source);
    assert!(
        diags.is_empty(),
        "{filename} should parse without errors, got: {}",
        diagnostics_to_json(&diags)
    );
    ast.unwrap_or_else(|| panic!("{filename} should produce an AST"))
}

// ── gain.muse round-trip ─────────────────────────────────────

#[test]
fn gain_plugin_name() {
    let plugin = parse_example("gain.muse");
    assert_eq!(plugin.name, "Warm Gain");
}

#[test]
fn gain_vendor_metadata() {
    let plugin = parse_example("gain.muse");
    let vendor = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::Metadata(m) if m.key == MetadataKey::Vendor => Some(&m.value),
        _ => None,
    });
    assert_eq!(
        vendor,
        Some(&MetadataValue::StringVal("Muse Audio".to_string()))
    );
}

#[test]
fn gain_has_one_param() {
    let plugin = parse_example("gain.muse");
    let params: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| match item {
            PluginItem::ParamDecl(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(params.len(), 1, "gain.muse should have 1 param");
    assert_eq!(params[0].name, "gain");
    assert_eq!(params[0].param_type, ParamType::Float);
}

#[test]
fn gain_has_process_block() {
    let plugin = parse_example("gain.muse");
    let process_count = plugin
        .items
        .iter()
        .filter(|(item, _)| matches!(item, PluginItem::ProcessBlock(_)))
        .count();
    assert_eq!(process_count, 1, "gain.muse should have 1 process block");
}

#[test]
fn gain_process_contains_signal_chain() {
    let plugin = parse_example("gain.muse");
    let process = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ProcessBlock(p) => Some(p),
        _ => None,
    });
    let process = process.expect("should have process block");
    // The process body should have at least one statement containing a chain operator
    assert!(
        !process.body.is_empty(),
        "process block body should not be empty"
    );
    // Verify the expression contains a Binary(Chain)
    let has_chain = process.body.iter().any(|(stmt, _)| match stmt {
        Statement::Expr((Expr::Binary { op: BinOp::Chain, .. }, _)) => true,
        _ => false,
    });
    assert!(has_chain, "process block should contain a signal chain expression");
}

#[test]
fn gain_io_declarations() {
    let plugin = parse_example("gain.muse");
    let io_decls: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| match item {
            PluginItem::IoDecl(io) => Some(io),
            _ => None,
        })
        .collect();
    assert_eq!(io_decls.len(), 2, "gain.muse should have input + output");
    assert_eq!(io_decls[0].direction, IoDirection::Input);
    assert_eq!(io_decls[0].channels, ChannelSpec::Stereo);
    assert_eq!(io_decls[1].direction, IoDirection::Output);
    assert_eq!(io_decls[1].channels, ChannelSpec::Stereo);
}

#[test]
fn gain_clap_block() {
    let plugin = parse_example("gain.muse");
    let clap = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::FormatBlock(FormatBlock::Clap(c)) => Some(c),
        _ => None,
    });
    let clap = clap.expect("gain.muse should have a clap block");
    // Should have id, description, and features
    let has_id = clap.items.iter().any(|(item, _)| matches!(item, ClapItem::Id(_)));
    let has_desc = clap.items.iter().any(|(item, _)| matches!(item, ClapItem::Description(_)));
    let has_features = clap.items.iter().any(|(item, _)| matches!(item, ClapItem::Features(_)));
    assert!(has_id, "clap block should have an id");
    assert!(has_desc, "clap block should have a description");
    assert!(has_features, "clap block should have features");
}

// ── filter.muse round-trip ───────────────────────────────────

#[test]
fn filter_plugin_name() {
    let plugin = parse_example("filter.muse");
    assert_eq!(plugin.name, "Velvet Filter");
}

#[test]
fn filter_multiple_params() {
    let plugin = parse_example("filter.muse");
    let params: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| match item {
            PluginItem::ParamDecl(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(params.len(), 5, "filter.muse should have 5 params");

    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["cutoff", "resonance", "mode", "drive", "mix"]);
}

#[test]
fn filter_has_enum_param() {
    let plugin = parse_example("filter.muse");
    let mode_param = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ParamDecl(p) if p.name == "mode" => Some(p),
        _ => None,
    });
    let mode = mode_param.expect("filter.muse should have a 'mode' param");
    match &mode.param_type {
        ParamType::Enum(variants) => {
            assert_eq!(
                variants,
                &["lowpass", "highpass", "bandpass", "notch"],
                "mode variants should match"
            );
        }
        other => panic!("mode param should be Enum, got {:?}", other),
    }
}

#[test]
fn filter_param_ranges() {
    let plugin = parse_example("filter.muse");
    let cutoff = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ParamDecl(p) if p.name == "cutoff" => Some(p),
        _ => None,
    });
    let cutoff = cutoff.expect("should have cutoff param");
    assert!(cutoff.range.is_some(), "cutoff should have a range");
    let range = cutoff.range.as_ref().unwrap();
    // min should be 20.0, max should be 20000.0
    match &range.min.0 {
        Expr::Number(n, _) => assert_eq!(*n, 20.0),
        other => panic!("expected number for range min, got {:?}", other),
    }
    match &range.max.0 {
        Expr::Number(n, _) => assert_eq!(*n, 20000.0),
        other => panic!("expected number for range max, got {:?}", other),
    }
}

#[test]
fn filter_process_has_if_expression() {
    let plugin = parse_example("filter.muse");
    let process = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ProcessBlock(p) => Some(p),
        _ => None,
    });
    let process = process.expect("should have process block");
    // The filter's process block has `let shaped = if param.drive > 0.0 { ... } else { ... }`
    let has_if = process.body.iter().any(|(stmt, _)| match stmt {
        Statement::Let { value: (Expr::If { .. }, _), .. } => true,
        _ => false,
    });
    assert!(has_if, "filter process should contain an if expression");
}

// ── synth.muse round-trip ────────────────────────────────────

#[test]
fn synth_plugin_name() {
    let plugin = parse_example("synth.muse");
    assert_eq!(plugin.name, "Glass Synth");
}

#[test]
fn synth_has_midi_declaration() {
    let plugin = parse_example("synth.muse");
    let midi_count = plugin
        .items
        .iter()
        .filter(|(item, _)| matches!(item, PluginItem::MidiDecl(_)))
        .count();
    assert_eq!(midi_count, 1, "synth.muse should have 1 midi block");
}

#[test]
fn synth_midi_has_note_handler() {
    let plugin = parse_example("synth.muse");
    let midi = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::MidiDecl(m) => Some(m),
        _ => None,
    });
    let midi = midi.expect("should have midi block");
    let has_note = midi
        .items
        .iter()
        .any(|(item, _)| matches!(item, MidiItem::NoteHandler(_)));
    assert!(has_note, "midi block should have a note handler");
}

#[test]
fn synth_note_handler_body() {
    let plugin = parse_example("synth.muse");
    let midi = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::MidiDecl(m) => Some(m),
        _ => None,
    });
    let midi = midi.expect("should have midi block");
    let note_handler = midi.items.iter().find_map(|(item, _)| match item {
        MidiItem::NoteHandler(body) => Some(body),
        _ => None,
    });
    let body = note_handler.expect("should have note handler");
    // The note handler has: let freq = note.pitch, let vel = note.velocity, let gate = note.gate
    assert_eq!(body.len(), 3, "note handler should have 3 statements");
    // First statement: let freq = note.pitch
    match &body[0].0 {
        Statement::Let { name, .. } => assert_eq!(name, "freq"),
        other => panic!("expected Let statement, got {:?}", other),
    }
}

#[test]
fn synth_category_instrument() {
    let plugin = parse_example("synth.muse");
    let category = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::Metadata(m) if m.key == MetadataKey::Category => Some(&m.value),
        _ => None,
    });
    assert_eq!(
        category,
        Some(&MetadataValue::Identifier("instrument".to_string())),
        "synth.muse should have category instrument"
    );
}

#[test]
fn synth_many_params() {
    let plugin = parse_example("synth.muse");
    let params: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| match item {
            PluginItem::ParamDecl(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(params.len(), 8, "synth.muse should have 8 params");
    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["attack", "decay", "sustain", "release", "cutoff", "resonance", "osc_mix", "volume"]
    );
}

#[test]
fn synth_io_mono_in_stereo_out() {
    let plugin = parse_example("synth.muse");
    let io_decls: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| match item {
            PluginItem::IoDecl(io) => Some(io),
            _ => None,
        })
        .collect();
    assert_eq!(io_decls.len(), 2);
    assert_eq!(io_decls[0].direction, IoDirection::Input);
    assert_eq!(io_decls[0].channels, ChannelSpec::Mono);
    assert_eq!(io_decls[1].direction, IoDirection::Output);
    assert_eq!(io_decls[1].channels, ChannelSpec::Stereo);
}

// ── JSON diagnostic output from broken input ─────────────────

#[test]
fn json_diagnostic_output_from_broken_input() {
    let src = r#"plugin "Test" { process { 123 + } }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(!diags.is_empty(), "broken input should produce diagnostics");

    let json = diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("should be valid JSON");
    assert!(!parsed.is_empty());

    for entry in &parsed {
        let code = entry["code"].as_str().expect("code should be string");
        assert!(
            code.starts_with('E'),
            "error code should start with 'E', got: {code}"
        );

        let span = entry["span"].as_array().expect("span should be array");
        assert_eq!(span.len(), 2, "span should have 2 elements");
        let start = span[0].as_u64().expect("span[0] should be number");
        let end = span[1].as_u64().expect("span[1] should be number");
        assert!(start <= end, "span start <= end");
    }
}

// ── Round-trip: parse → no errors → verify AST ──────────────

#[test]
fn all_examples_parse_without_errors() {
    for filename in &["gain.muse", "filter.muse", "synth.muse", "multiband.muse"] {
        let path = format!("examples/{filename}");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
        let (ast, diags) = parse_to_diagnostics(&source);
        assert!(
            diags.is_empty(),
            "{filename} should parse cleanly, got {} errors: {}",
            diags.len(),
            diagnostics_to_json(&diags)
        );
        assert!(ast.is_some(), "{filename} should produce an AST");
    }
}

#[test]
fn gain_param_default_value() {
    let plugin = parse_example("gain.muse");
    let gain_param = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ParamDecl(p) if p.name == "gain" => Some(p),
        _ => None,
    });
    let param = gain_param.expect("should have gain param");
    match &param.default {
        Some((Expr::Number(n, _), _)) => assert_eq!(*n, 0.0),
        other => panic!("expected default 0.0, got {:?}", other),
    }
}

#[test]
fn gain_param_range() {
    let plugin = parse_example("gain.muse");
    let gain_param = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::ParamDecl(p) if p.name == "gain" => Some(p),
        _ => None,
    });
    let param = gain_param.expect("should have gain param");
    let range = param.range.as_ref().expect("gain should have range");
    match (&range.min.0, &range.max.0) {
        (Expr::Unary { op: UnaryOp::Neg, operand }, Expr::Number(max, _)) => {
            // min is -30.0 (unary neg + 30.0)
            match &operand.0 {
                Expr::Number(n, _) => assert_eq!(*n, 30.0),
                other => panic!("expected number inside unary neg, got {:?}", other),
            }
            assert_eq!(*max, 30.0);
        }
        other => panic!("expected range -30.0..30.0, got {:?}", other),
    }
}

#[test]
fn gain_vst3_block() {
    let plugin = parse_example("gain.muse");
    let vst3 = plugin.items.iter().find_map(|(item, _)| match item {
        PluginItem::FormatBlock(FormatBlock::Vst3(v)) => Some(v),
        _ => None,
    });
    let vst3 = vst3.expect("gain.muse should have a vst3 block");
    let has_id = vst3.items.iter().any(|(item, _)| match item {
        Vst3Item::Id(id) => id == "MuseWarmGain1",
        _ => false,
    });
    assert!(has_id, "vst3 block should have id 'MuseWarmGain1'");
}

// ── Resolve integration: parse → resolve → verify types ──────

/// Helper: read an example file, parse it, then resolve it against the builtin registry.
fn resolve_example(filename: &str) -> muse_lang::ResolvedPlugin<'static> {
    let path = format!("examples/{filename}");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    // Leak source to get 'static lifetime for the test helper
    let source: &'static str = Box::leak(source.into_boxed_str());
    let (ast, diags) = parse_to_diagnostics(source);
    assert!(
        diags.is_empty(),
        "{filename} should parse without errors, got: {}",
        diagnostics_to_json(&diags)
    );
    let plugin = ast.unwrap_or_else(|| panic!("{filename} should produce an AST"));
    // Leak the plugin too so we can return ResolvedPlugin with 'static
    let plugin: &'static PluginDef = Box::leak(Box::new(plugin));
    let registry = builtin_registry();
    resolve_plugin(plugin, &registry)
        .unwrap_or_else(|diags| {
            panic!(
                "{filename} should resolve without errors, got: {}",
                diagnostics_to_json(&diags)
            )
        })
}

#[test]
fn resolve_gain_example() {
    let resolved = resolve_example("gain.muse");
    assert!(
        !resolved.type_map.is_empty(),
        "gain.muse resolve should produce a non-empty type map"
    );
}

#[test]
fn resolve_filter_example() {
    let resolved = resolve_example("filter.muse");
    assert!(
        !resolved.type_map.is_empty(),
        "filter.muse resolve should produce a non-empty type map"
    );
}

#[test]
fn resolve_synth_example() {
    let resolved = resolve_example("synth.muse");
    assert!(
        !resolved.type_map.is_empty(),
        "synth.muse resolve should produce a non-empty type map"
    );
}

#[test]
fn resolve_error_json_format() {
    // Use an unknown function to trigger E003
    let src = r#"plugin "Test" {
  input stereo
  output stereo
  process {
    input -> frobnicator(440Hz) -> output
  }
}"#;
    let (ast, parse_diags) = parse_to_diagnostics(src);
    assert!(parse_diags.is_empty(), "should parse without errors");
    let plugin = ast.expect("should produce an AST");
    let registry = builtin_registry();
    let resolve_diags = resolve_plugin(&plugin, &registry).unwrap_err();

    let json = diagnostics_to_json(&resolve_diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("should be valid JSON");
    assert!(!parsed.is_empty(), "should have at least one diagnostic");

    for entry in &parsed {
        // Verify diagnostic contract: code, span, severity, message
        assert!(entry["code"].is_string(), "diagnostic should have 'code' string");
        assert!(entry["span"].is_array(), "diagnostic should have 'span' array");
        assert!(entry["severity"].is_string(), "diagnostic should have 'severity' string");
        assert!(entry["message"].is_string(), "diagnostic should have 'message' string");

        let code = entry["code"].as_str().unwrap();
        assert!(
            code.starts_with('E'),
            "error code should start with 'E', got: {code}"
        );

        let span = entry["span"].as_array().unwrap();
        assert_eq!(span.len(), 2, "span should have 2 elements");
    }
}

#[test]
fn compile_check_with_resolve() {
    // compile_check should catch parse errors
    let parse_err_src = r#"plugin "Test" { process { 123 + } }"#;
    assert!(
        !muse_lang::compile_check(parse_err_src, "test.muse", true),
        "compile_check should return false for parse errors"
    );

    // compile_check should catch resolve errors (unknown function)
    let resolve_err_src = r#"plugin "Test" {
  input stereo
  output stereo
  process {
    input -> blorp(440Hz) -> output
  }
}"#;
    assert!(
        !muse_lang::compile_check(resolve_err_src, "test.muse", true),
        "compile_check should return false for resolve errors"
    );

    // compile_check should pass for valid source
    let valid_src = std::fs::read_to_string("examples/gain.muse")
        .expect("should read gain.muse");
    assert!(
        muse_lang::compile_check(&valid_src, "gain.muse", false),
        "compile_check should return true for valid source"
    );
}

// ── Multiband example integration ────────────────────────────

#[test]
fn resolve_multiband_example() {
    let resolved = resolve_example("multiband.muse");
    assert!(
        !resolved.type_map.is_empty(),
        "multiband.muse resolve should produce a non-empty type map"
    );
}

#[test]
fn routing_error_json_format() {
    // Use merge without split to trigger E008 routing error
    let src = r#"plugin "Test" {
  input stereo
  output stereo
  process {
    input -> merge -> output
  }
}"#;
    let (ast, parse_diags) = parse_to_diagnostics(src);
    assert!(parse_diags.is_empty(), "should parse without errors");
    let plugin = ast.expect("should produce an AST");
    let registry = builtin_registry();
    let resolve_diags = resolve_plugin(&plugin, &registry).unwrap_err();

    let json = diagnostics_to_json(&resolve_diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("should be valid JSON");
    assert!(!parsed.is_empty(), "should have at least one diagnostic");

    // Find the E008 entry
    let e008 = parsed
        .iter()
        .find(|e| e["code"].as_str() == Some("E008"))
        .expect("should contain E008 diagnostic");

    // Verify it follows the diagnostic contract
    assert!(e008["span"].is_array(), "E008 should have span array");
    assert_eq!(e008["severity"].as_str(), Some("error"), "E008 should be error severity");
    assert!(
        e008["message"]
            .as_str()
            .unwrap()
            .contains("merge without preceding split"),
        "E008 message should describe the routing error"
    );
    assert!(
        e008["suggestion"].is_string(),
        "E008 should include a suggestion"
    );
}

#[test]
fn compile_check_catches_routing_errors() {
    // E007: split without merge
    let split_no_merge = r#"plugin "Test" {
  input stereo
  output stereo
  process {
    input -> split {
      lowpass(400Hz)
      highpass(4000Hz)
    } -> output
  }
}"#;
    assert!(
        !muse_lang::compile_check(split_no_merge, "test.muse", true),
        "compile_check should return false for split without merge"
    );

    // E008: merge without split
    let merge_no_split = r#"plugin "Test" {
  input stereo
  output stereo
  process {
    input -> merge -> output
  }
}"#;
    assert!(
        !muse_lang::compile_check(merge_no_split, "test.muse", true),
        "compile_check should return false for merge without split"
    );

    // Valid: multiband.muse should pass
    let valid_src = std::fs::read_to_string("examples/multiband.muse")
        .expect("should read multiband.muse");
    assert!(
        muse_lang::compile_check(&valid_src, "multiband.muse", false),
        "compile_check should return true for valid multiband source"
    );
}

// ── compile() pipeline integration ───────────────────────────

#[test]
fn compile_synth_pipeline() {
    let source = std::fs::read_to_string("examples/synth.muse")
        .expect("should read synth.muse");

    let tmp = std::env::temp_dir().join("muse-compile-test-synth-pipeline");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let result = muse_lang::compile(&source, "synth.muse", &tmp);
    assert!(result.is_ok(), "compile() should succeed for synth.muse: {:?}", result.err());

    let cr = result.unwrap();
    let crate_dir = &cr.crate_dir;
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml should exist");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs should exist");

    // Verify the generated code contains instrument-specific patterns
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("MidiConfig::Basic"), "compile() output should have MidiConfig::Basic");
    assert!(lib_rs.contains("OscState"), "compile() output should have OscState");
    assert!(lib_rs.contains("AdsrState"), "compile() output should have AdsrState");
}

#[test]
fn compile_function_produces_output() {
    let source = std::fs::read_to_string("examples/gain.muse")
        .expect("should read gain.muse");

    let tmp = std::env::temp_dir().join("muse-compile-test-output");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let result = muse_lang::compile(&source, "gain.muse", &tmp);
    assert!(result.is_ok(), "compile() should succeed for gain.muse: {:?}", result.err());

    let cr = result.unwrap();
    let crate_dir = &cr.crate_dir;
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml should exist");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs should exist");
}

#[test]
fn compile_with_parse_error_produces_diagnostics() {
    let source = r#"plugin "Test" { process { 123 + } }"#;
    let tmp = std::env::temp_dir().join("muse-compile-test-parse-err");

    let result = muse_lang::compile(source, "test.muse", &tmp);
    assert!(result.is_err(), "compile() should fail for broken source");

    let diags = result.unwrap_err();
    assert!(!diags.is_empty(), "should produce parse diagnostics");
    // Verify the diagnostics are valid JSON-serializable
    let json = muse_lang::diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("diagnostics should be valid JSON");
    assert!(!parsed.is_empty());
}

#[test]
fn compile_with_codegen_error_produces_json() {
    // Plugin that parses and resolves but fails codegen (missing vendor/clap/vst3)
    let source = r#"plugin "Bare" {
  input stereo
  output stereo
  process {
    input -> output
  }
}"#;
    let tmp = std::env::temp_dir().join("muse-compile-test-codegen-err");

    let result = muse_lang::compile(source, "test.muse", &tmp);
    assert!(result.is_err(), "compile() should fail for bare plugin");

    let diags = result.unwrap_err();
    let json = muse_lang::diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("diagnostics should be valid JSON");

    // Should contain E010 codegen errors
    let has_e010 = parsed.iter().any(|e| e["code"].as_str() == Some("E010"));
    assert!(has_e010, "compile() codegen errors should include E010, got: {}", json);
}

// ── End-to-end: gain.muse → .clap → clap-validator ──────────

/// Full pipeline proof: compile gain.muse via the CLI binary, then validate
/// the resulting .clap bundle with clap-validator.
///
/// Marked `#[ignore]` because it takes 10+ seconds (full cargo build + validator).
/// Run with: `cargo test --test integration_tests compile_gain_to_clap_and_validate -- --ignored`
#[test]
#[ignore]
fn compile_gain_to_clap_and_validate() {
    use std::process::Command;

    let out_dir = std::env::temp_dir().join(format!("muse-e2e-test-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).ok();
    }
    std::fs::create_dir_all(&out_dir).expect("create temp output dir");

    // Step 1: Run `cargo run -- compile examples/gain.muse --output-dir <tmp>`
    // We shell out to the binary because build_plugin/assemble_clap_bundle live in main.rs.
    let compile_output = Command::new(env!("CARGO"))
        .args(["run", "--", "compile", "examples/gain.muse", "--output-dir"])
        .arg(&out_dir)
        .output()
        .expect("failed to invoke cargo run");

    assert!(
        compile_output.status.success(),
        "muse compile should exit 0, got {}\nstderr: {}",
        compile_output.status,
        String::from_utf8_lossy(&compile_output.stderr)
    );

    // Step 2: Verify the .clap bundle structure exists
    let bundle_dir = out_dir.join("Warm Gain.clap");
    assert!(
        bundle_dir.is_dir(),
        "bundle directory should exist at {}",
        bundle_dir.display()
    );
    assert!(
        bundle_dir.join("Contents/Info.plist").is_file(),
        "Info.plist should exist"
    );
    assert!(
        bundle_dir.join("Contents/MacOS/Warm Gain").is_file(),
        "binary should exist in MacOS/"
    );

    // Step 3: Run clap-validator (skip gracefully if not installed)
    let validator = match which_clap_validator() {
        Some(path) => path,
        None => {
            eprintln!("clap-validator not found on PATH — skipping validation step");
            // Clean up
            std::fs::remove_dir_all(&out_dir).ok();
            return;
        }
    };

    let validate_output = Command::new(&validator)
        .arg("validate")
        .arg(&bundle_dir)
        .output()
        .expect("failed to invoke clap-validator");

    let stdout = String::from_utf8_lossy(&validate_output.stdout);
    let stderr = String::from_utf8_lossy(&validate_output.stderr);

    assert!(
        validate_output.status.success(),
        "clap-validator should exit 0, got {}\nstdout: {stdout}\nstderr: {stderr}",
        validate_output.status
    );

    // Verify 0 failures in output — parse the summary line "N tests run, M passed, F failed, ..."
    // The summary always contains "0 failed" when everything passes.
    assert!(
        stdout.contains("0 failed"),
        "clap-validator should report 0 failures, output:\n{stdout}"
    );

    // Clean up
    std::fs::remove_dir_all(&out_dir).ok();
}

// ── Deterministic output (R016) — fast variant ──────────────

/// R016 primary validation: compile the same source twice and assert the
/// generated Rust crate is byte-identical. This proves no random IDs,
/// timestamp injection, or HashMap iteration order issues affect codegen.
#[test]
fn deterministic_output_no_build() {
    let source = std::fs::read_to_string("examples/gain.muse")
        .expect("should read gain.muse");

    let tmp_a = std::env::temp_dir().join(format!("muse-det-a-{}", std::process::id()));
    let tmp_b = std::env::temp_dir().join(format!("muse-det-b-{}", std::process::id()));

    // Clean up any prior runs
    for d in [&tmp_a, &tmp_b] {
        if d.exists() {
            std::fs::remove_dir_all(d).ok();
        }
    }

    let result_a = muse_lang::compile(&source, "gain.muse", &tmp_a)
        .expect("first compile should succeed");
    let result_b = muse_lang::compile(&source, "gain.muse", &tmp_b)
        .expect("second compile should succeed");

    let lib_rs_a = std::fs::read_to_string(result_a.crate_dir.join("src/lib.rs"))
        .expect("should read first lib.rs");
    let lib_rs_b = std::fs::read_to_string(result_b.crate_dir.join("src/lib.rs"))
        .expect("should read second lib.rs");

    assert_eq!(lib_rs_a, lib_rs_b, "codegen output must be deterministic (R016)");

    let cargo_a = std::fs::read_to_string(result_a.crate_dir.join("Cargo.toml"))
        .expect("should read first Cargo.toml");
    let cargo_b = std::fs::read_to_string(result_b.crate_dir.join("Cargo.toml"))
        .expect("should read second Cargo.toml");

    assert_eq!(cargo_a, cargo_b, "Cargo.toml must be deterministic (R016)");

    // Clean up
    std::fs::remove_dir_all(&tmp_a).ok();
    std::fs::remove_dir_all(&tmp_b).ok();
}

// ── Deterministic output (R016) — full build variant ─────────

/// R016 ignored variant: same as the fast test but runs through compile().
/// Separated so the fast test suite stays fast.
#[test]
#[ignore]
fn deterministic_output_produces_identical_lib_rs() {
    let source = std::fs::read_to_string("examples/gain.muse")
        .expect("should read gain.muse");

    let tmp_a = std::env::temp_dir().join(format!("muse-det-ign-a-{}", std::process::id()));
    let tmp_b = std::env::temp_dir().join(format!("muse-det-ign-b-{}", std::process::id()));

    for d in [&tmp_a, &tmp_b] {
        if d.exists() {
            std::fs::remove_dir_all(d).ok();
        }
    }

    let result_a = muse_lang::compile(&source, "gain.muse", &tmp_a)
        .expect("first compile should succeed");
    let result_b = muse_lang::compile(&source, "gain.muse", &tmp_b)
        .expect("second compile should succeed");

    let lib_rs_a = std::fs::read_to_string(result_a.crate_dir.join("src/lib.rs"))
        .expect("should read first lib.rs");
    let lib_rs_b = std::fs::read_to_string(result_b.crate_dir.join("src/lib.rs"))
        .expect("should read second lib.rs");

    assert_eq!(lib_rs_a, lib_rs_b, "codegen output must be deterministic (R016)");

    std::fs::remove_dir_all(&tmp_a).ok();
    std::fs::remove_dir_all(&tmp_b).ok();
}

// ── muse build E2E: dual-format bundles + telemetry (R010) ──

/// Full `muse build` E2E: build gain.muse to both .clap and .vst3, verify
/// bundle structures, codesign, and structured JSON telemetry.
#[test]
#[ignore]
fn build_gain_to_clap_and_vst3() {
    use std::process::Command;

    let out_dir = std::env::temp_dir().join(format!("muse-build-e2e-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).ok();
    }
    std::fs::create_dir_all(&out_dir).expect("create temp output dir");

    // Run `muse build` with JSON telemetry
    let output = Command::new(env!("CARGO"))
        .args(["run", "--", "build", "examples/gain.muse", "--output-dir"])
        .arg(&out_dir)
        .args(["--format", "json"])
        .output()
        .expect("failed to invoke cargo run");

    assert!(
        output.status.success(),
        "muse build should exit 0, got {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // Parse JSON telemetry
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nraw: {stdout}"));

    assert_eq!(json["status"].as_str(), Some("ok"), "status should be 'ok'");
    assert_eq!(json["plugin_name"].as_str(), Some("Warm Gain"));

    // Verify all 6 phase keys exist
    let phases = &json["phases"];
    for key in &["compile", "cargo_build", "clap_bundle", "vst3_bundle", "codesign_clap", "codesign_vst3"] {
        assert!(
            phases[key].is_object(),
            "phases.{key} should be an object, got: {:?}",
            phases[key]
        );
        assert!(
            phases[key]["duration_ms"].is_number(),
            "phases.{key}.duration_ms should be a number"
        );
    }

    // Verify artifact sizes
    let clap_size = json["artifacts"]["clap"]["size_bytes"].as_u64()
        .expect("clap size_bytes should be a number");
    let vst3_size = json["artifacts"]["vst3"]["size_bytes"].as_u64()
        .expect("vst3 size_bytes should be a number");
    assert!(clap_size > 0, "clap size_bytes should be > 0, got {clap_size}");
    assert!(vst3_size > 0, "vst3 size_bytes should be > 0, got {vst3_size}");

    // Verify .clap bundle structure
    let clap_bundle = out_dir.join("Warm Gain.clap");
    assert!(clap_bundle.is_dir(), ".clap bundle should exist");
    assert!(
        clap_bundle.join("Contents/MacOS/Warm Gain").is_file(),
        "CLAP binary should exist"
    );

    // Verify .vst3 bundle structure
    let vst3_bundle = out_dir.join("Warm Gain.vst3");
    assert!(vst3_bundle.is_dir(), ".vst3 bundle should exist");
    assert!(
        vst3_bundle.join("Contents/MacOS/Warm Gain").is_file(),
        "VST3 binary should exist"
    );
    assert!(
        vst3_bundle.join("Contents/PkgInfo").is_file(),
        "VST3 PkgInfo should exist"
    );
    assert!(
        vst3_bundle.join("Contents/Info.plist").is_file(),
        "VST3 Info.plist should exist"
    );

    // Verify codesign on both bundles
    let clap_cs = Command::new("codesign")
        .args(["-v"])
        .arg(&clap_bundle)
        .output()
        .expect("codesign check");
    assert!(
        clap_cs.status.success(),
        "CLAP bundle codesign should verify: {}",
        String::from_utf8_lossy(&clap_cs.stderr)
    );

    let vst3_cs = Command::new("codesign")
        .args(["-v"])
        .arg(&vst3_bundle)
        .output()
        .expect("codesign check");
    assert!(
        vst3_cs.status.success(),
        "VST3 bundle codesign should verify: {}",
        String::from_utf8_lossy(&vst3_cs.stderr)
    );

    // Clean up
    std::fs::remove_dir_all(&out_dir).ok();
}

// ── clap-validator state tests (R013) ────────────────────────

/// Validates R013 (state save/restore): builds a .clap bundle and runs
/// clap-validator which includes state persistence tests in its suite.
/// Skips gracefully if clap-validator is not installed.
#[test]
#[ignore]
fn clap_validator_state_tests() {
    use std::process::Command;

    let validator = match which_clap_validator() {
        Some(path) => path,
        None => {
            eprintln!("clap-validator not found on PATH — skipping R013 validation");
            return;
        }
    };

    let out_dir = std::env::temp_dir().join(format!("muse-r013-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).ok();
    }
    std::fs::create_dir_all(&out_dir).expect("create temp output dir");

    // Build the .clap bundle via muse build
    let build_output = Command::new(env!("CARGO"))
        .args(["run", "--", "build", "examples/gain.muse", "--output-dir"])
        .arg(&out_dir)
        .output()
        .expect("failed to invoke cargo run");

    assert!(
        build_output.status.success(),
        "muse build should succeed: {}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let clap_bundle = out_dir.join("Warm Gain.clap");
    assert!(clap_bundle.is_dir(), ".clap bundle should exist for validation");

    // Run clap-validator
    let validate_output = Command::new(&validator)
        .arg("validate")
        .arg(&clap_bundle)
        .output()
        .expect("failed to invoke clap-validator");

    let stdout = String::from_utf8_lossy(&validate_output.stdout);
    let stderr = String::from_utf8_lossy(&validate_output.stderr);

    assert!(
        validate_output.status.success(),
        "clap-validator should exit 0, got {}\nstdout: {stdout}\nstderr: {stderr}",
        validate_output.status
    );

    assert!(
        stdout.contains("0 failed"),
        "clap-validator should report 0 failures (R013 state tests included), output:\n{stdout}"
    );

    // Clean up
    std::fs::remove_dir_all(&out_dir).ok();
}

// ── Build error produces structured JSON (R017) ──────────────

/// Verifies that build errors are reported as structured JSON suitable
/// for AI agent consumption (R017).
#[test]
#[ignore]
fn build_error_produces_structured_json() {
    use std::process::Command;

    // Create a broken .muse file that will fail at compile phase
    let tmp = std::env::temp_dir().join(format!("muse-err-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let broken_file = tmp.join("broken.muse");
    std::fs::write(&broken_file, r#"plugin "Broken" { process { 123 + } }"#)
        .expect("write broken muse file");

    let output = Command::new(env!("CARGO"))
        .args(["run", "--", "build"])
        .arg(&broken_file)
        .args(["--output-dir", tmp.to_str().unwrap()])
        .args(["--format", "json"])
        .output()
        .expect("failed to invoke cargo run");

    assert!(
        !output.status.success(),
        "muse build should fail for broken input, got exit {}",
        output.status
    );

    // The compile phase emits diagnostics as a JSON array
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("error output should be valid JSON: {e}\nraw: {stdout}"));

    // Compile-phase errors are a diagnostics array (not the phase-error object)
    // Each entry has code, message, span, severity — structured for agent consumption
    let diags = json.as_array()
        .expect("compile error JSON should be an array of diagnostics");
    assert!(!diags.is_empty(), "should have at least one diagnostic");

    for entry in diags {
        assert!(entry["code"].is_string(), "diagnostic should have 'code'");
        assert!(entry["message"].is_string(), "diagnostic should have 'message'");
        assert!(entry["span"].is_array(), "diagnostic should have 'span'");
        assert!(entry["severity"].is_string(), "diagnostic should have 'severity'");
    }

    // Clean up
    std::fs::remove_dir_all(&tmp).ok();
}

/// Find clap-validator on PATH, returning None if not installed.
fn which_clap_validator() -> Option<std::path::PathBuf> {
    // Check common locations
    for name in &["clap-validator"] {
        if let Ok(output) = std::process::Command::new("which")
            .arg(name)
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(std::path::PathBuf::from(path));
                }
            }
        }
    }
    None
}

#[test]
fn check_named_bus_plugin() {
    let source = r#"
plugin "Bus Test" {
    vendor "Test"
    input main stereo
    input sidechain stereo
    output stereo
    output fx_send mono

    param mix: float = 1.0 in 0.0..1.0

    clap {
        id "dev.test.bus-test"
        description "Named bus test"
        features [audio_effect, stereo]
    }

    vst3 {
        id "TestBusPlugin12345"
        subcategories [Fx]
    }

    process {
        input -> gain(param.mix) -> output
    }
}
"#;
    let (ast, errors) = muse_lang::parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse should return AST");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry);
    assert!(resolved.is_ok(), "resolve should succeed for named bus plugin, got: {:?}", resolved.err());
}

// ── Import system integration tests ────────────────────────────

#[test]
fn import_demo_parses_ok() {
    let plugin = parse_example("import_demo.muse");
    assert_eq!(plugin.name, "Import Demo");

    // Should have a UseDecl
    let use_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::UseDecl(_))).count();
    assert_eq!(use_count, 1);
}

#[test]
fn saturation_lib_parses_ok() {
    let plugin = parse_example("lib/saturation.muse");
    assert_eq!(plugin.name, "Saturation Lib");

    // Should have FnDef items
    let fn_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::FnDef(_))).count();
    assert_eq!(fn_count, 2, "Expected 2 fn definitions in saturation lib");
}

#[test]
fn import_resolves_with_compile_check() {
    // Use compile_check which resolves imports via lib.rs pipeline
    let source = std::fs::read_to_string("examples/import_demo.muse")
        .expect("failed to read import_demo.muse");
    let ok = muse_lang::compile_check(&source, "examples/import_demo.muse", false);
    assert!(ok, "import_demo.muse should pass compile_check (import resolution + resolve)");
}

#[test]
fn import_nonexistent_file_errors() {
    let source = r#"
plugin "Test" {
  vendor "Test" version "0.1.0" category utility
  clap { id "test" description "test" features [audio_effect] }
  vst3 { id "test" subcategories [Fx] }
  input stereo output stereo

  use "nonexistent/file.muse" expose something

  process { input -> output }
}
"#;
    // compile_check returns false when there are import errors
    let ok = muse_lang::compile_check(source, "tests/fake.muse", false);
    assert!(!ok, "Importing nonexistent file should fail compile_check");
}

#[test]
fn import_nonexistent_name_errors() {
    // Try to import a name that doesn't exist in the target file
    let source = r#"
plugin "Test" {
  vendor "Test" version "0.1.0" category utility
  clap { id "test" description "test" features [audio_effect] }
  vst3 { id "test" subcategories [Fx] }
  input stereo output stereo

  use "lib/saturation.muse" expose nonexistent_fn

  process { input -> output }
}
"#;
    let ok = muse_lang::compile_check(source, "examples/test.muse", false);
    assert!(!ok, "Importing nonexistent name should fail compile_check");
}

#[test]
fn import_with_as_alias() {
    // Parse a plugin that uses `as` alias and verify the fns get prefixed
    let source = std::fs::read_to_string("examples/lib/saturation.muse")
        .expect("failed to read saturation lib");
    let (target_ast, _) = parse_to_diagnostics(&source);
    let target_plugin = target_ast.unwrap();

    // Verify the lib has fn definitions
    let fn_names: Vec<String> = target_plugin.items.iter().filter_map(|(item, _)| {
        if let PluginItem::FnDef(f) = item { Some(f.name.clone()) } else { None }
    }).collect();
    assert!(fn_names.contains(&"warm_saturate".to_string()));
    assert!(fn_names.contains(&"hard_clip_saturate".to_string()));
}

#[test]
fn import_as_alias_resolves() {
    let source = std::fs::read_to_string("examples/import_as_demo.muse")
        .expect("failed to read import_as_demo.muse");
    let ok = muse_lang::compile_check(&source, "examples/import_as_demo.muse", false);
    assert!(ok, "import_as_demo.muse should pass compile_check with 'as' alias import");
}
