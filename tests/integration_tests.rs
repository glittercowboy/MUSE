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
