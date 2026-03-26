//! Parser integration tests.
//!
//! Tests the full pipeline: source → lex → parse → AST.

use muse_lang::ast::*;
use muse_lang::parser::parse;

// ── Example file tests ───────────────────────────────────────

#[test]
fn parse_gain_example() {
    let source = include_str!("../examples/gain.muse");
    let (ast, errors) = parse(source);

    assert!(
        errors.is_empty(),
        "Expected no errors parsing gain.muse, got: {:?}",
        errors
    );
    let plugin = ast.expect("Expected AST from gain.muse");

    assert_eq!(plugin.name, "Warm Gain");

    // Count item types
    let metadata_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::Metadata(_))).count();
    let format_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::FormatBlock(_))).count();
    let io_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::IoDecl(_))).count();
    let param_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::ParamDecl(_))).count();
    let process_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::ProcessBlock(_))).count();

    assert_eq!(metadata_count, 5, "Expected 5 metadata fields");
    assert_eq!(format_count, 2, "Expected 2 format blocks (clap + vst3)");
    assert_eq!(io_count, 2, "Expected 2 I/O declarations");
    assert_eq!(param_count, 1, "Expected 1 param");
    assert_eq!(process_count, 1, "Expected 1 process block");

    // Check param details
    let param = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ParamDecl(p) = item { Some(p) } else { None }
    }).unwrap();
    assert_eq!(param.name, "gain");
    assert_eq!(param.param_type, ParamType::Float);
    assert!(param.default.is_some());
    assert!(param.range.is_some());
    assert!(!param.options.is_empty(), "gain param should have smoothing and unit options");
}

#[test]
fn parse_filter_example() {
    let source = include_str!("../examples/filter.muse");
    let (ast, errors) = parse(source);

    assert!(
        errors.is_empty(),
        "Expected no errors parsing filter.muse, got: {:?}",
        errors
    );
    let plugin = ast.expect("Expected AST from filter.muse");

    assert_eq!(plugin.name, "Velvet Filter");

    let param_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::ParamDecl(_))).count();
    assert_eq!(param_count, 5, "Expected 5 params (cutoff, resonance, mode, drive, mix)");

    // Check enum param
    let mode_param = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ParamDecl(p) = item {
            if p.name == "mode" { Some(p) } else { None }
        } else {
            None
        }
    }).unwrap();
    assert!(matches!(mode_param.param_type, ParamType::Enum(_)));
    if let ParamType::Enum(variants) = &mode_param.param_type {
        assert_eq!(variants, &["lowpass", "highpass", "bandpass", "notch"]);
    }

    // Process block should exist and contain if-expression
    let process = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ProcessBlock(p) = item { Some(p) } else { None }
    }).unwrap();
    assert!(!process.body.is_empty(), "process block should have statements");

    // The process block should contain a let with an if expression
    let has_if = process.body.iter().any(|(stmt, _)| {
        if let Statement::Let { value, .. } = stmt {
            matches!(value.0, Expr::If { .. })
        } else {
            false
        }
    });
    assert!(has_if, "process block should contain a let with if expression");
}

#[test]
fn parse_synth_example() {
    let source = include_str!("../examples/synth.muse");
    let (ast, errors) = parse(source);

    assert!(
        errors.is_empty(),
        "Expected no errors parsing synth.muse, got: {:?}",
        errors
    );
    let plugin = ast.expect("Expected AST from synth.muse");

    assert_eq!(plugin.name, "Glass Synth");

    let param_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::ParamDecl(_))).count();
    assert_eq!(param_count, 8, "Expected 8 params");

    // Should have MIDI declaration
    let midi_count = plugin.items.iter().filter(|(item, _)| matches!(item, PluginItem::MidiDecl(_))).count();
    assert_eq!(midi_count, 1, "Expected 1 MIDI declaration");

    // Check MIDI has note handler
    let midi = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::MidiDecl(m) = item { Some(m) } else { None }
    }).unwrap();
    let has_note_handler = midi.items.iter().any(|(item, _)| matches!(item, MidiItem::NoteHandler(_)));
    assert!(has_note_handler, "MIDI block should have a note handler");

    // Process block with signal chains
    let process = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ProcessBlock(p) = item { Some(p) } else { None }
    }).unwrap();
    assert!(process.body.len() >= 4, "process block should have multiple statements");
}

// ── Expression parsing tests ─────────────────────────────────

/// Helper: parse a minimal plugin wrapping an expression in the process block.
fn parse_process_expr(expr_source: &str) -> Spanned<Expr> {
    let source = format!(
        r#"plugin "Test" {{
  input mono
  output mono
  process {{
    {}
  }}
}}"#,
        expr_source
    );
    let (ast, errors) = parse(&source);
    assert!(
        errors.is_empty(),
        "Parse errors for '{}': {:?}",
        expr_source,
        errors
    );
    let plugin = ast.unwrap();
    let process = plugin.items.into_iter().find_map(|(item, _)| {
        if let PluginItem::ProcessBlock(p) = item { Some(p) } else { None }
    }).unwrap();
    assert!(!process.body.is_empty(), "process body should not be empty");
    if let (Statement::Expr(e), _) = process.body.into_iter().last().unwrap() {
        e
    } else {
        panic!("Expected expression statement")
    }
}

#[test]
fn parse_number_literal() {
    let expr = parse_process_expr("42.0");
    assert!(matches!(expr.0, Expr::Number(n, None) if (n - 42.0).abs() < f64::EPSILON));
}

#[test]
fn parse_number_with_unit() {
    let expr = parse_process_expr("440Hz");
    assert!(matches!(expr.0, Expr::Number(n, Some(UnitSuffix::Hz)) if (n - 440.0).abs() < f64::EPSILON));
}

#[test]
fn parse_number_with_ms_unit() {
    let expr = parse_process_expr("50ms");
    assert!(matches!(expr.0, Expr::Number(n, Some(UnitSuffix::Ms)) if (n - 50.0).abs() < f64::EPSILON));
}

#[test]
fn parse_string_literal() {
    let expr = parse_process_expr(r#""hello world""#);
    assert!(matches!(expr.0, Expr::StringLit(ref s) if s == "hello world"));
}

#[test]
fn parse_bool_literals() {
    let t = parse_process_expr("true");
    assert!(matches!(t.0, Expr::Bool(true)));

    let f = parse_process_expr("false");
    assert!(matches!(f.0, Expr::Bool(false)));
}

#[test]
fn parse_identifier() {
    let expr = parse_process_expr("my_var");
    assert!(matches!(expr.0, Expr::Ident(ref s) if s == "my_var"));
}

#[test]
fn parse_field_access() {
    let expr = parse_process_expr("param.gain");
    match &expr.0 {
        Expr::FieldAccess(base, field) => {
            assert!(matches!(base.0, Expr::Ident(ref s) if s == "param"));
            assert_eq!(field, "gain");
        }
        other => panic!("Expected FieldAccess, got {:?}", other),
    }
}

#[test]
fn parse_function_call() {
    let expr = parse_process_expr("gain(1.0)");
    match &expr.0 {
        Expr::FnCall { callee, args } => {
            assert!(matches!(callee.0, Expr::Ident(ref s) if s == "gain"));
            assert_eq!(args.len(), 1);
        }
        other => panic!("Expected FnCall, got {:?}", other),
    }
}

#[test]
fn parse_function_call_multiple_args() {
    let expr = parse_process_expr("lowpass(200Hz, 0.5)");
    match &expr.0 {
        Expr::FnCall { callee, args } => {
            assert!(matches!(callee.0, Expr::Ident(ref s) if s == "lowpass"));
            assert_eq!(args.len(), 2);
        }
        other => panic!("Expected FnCall, got {:?}", other),
    }
}

#[test]
fn parse_binary_arithmetic() {
    let expr = parse_process_expr("1.0 + 2.0 * 3.0");
    // Should be 1.0 + (2.0 * 3.0) due to precedence
    match &expr.0 {
        Expr::Binary { op: BinOp::Add, left, right } => {
            assert!(matches!(left.0, Expr::Number(n, _) if (n - 1.0).abs() < f64::EPSILON));
            assert!(matches!(right.0, Expr::Binary { op: BinOp::Mul, .. }));
        }
        other => panic!("Expected Binary Add, got {:?}", other),
    }
}

#[test]
fn parse_signal_chain() {
    let expr = parse_process_expr("input -> gain(1.0) -> output");
    // Should be (input -> gain(1.0)) -> output
    match &expr.0 {
        Expr::Binary { op: BinOp::Chain, left, right } => {
            assert!(matches!(right.0, Expr::Ident(ref s) if s == "output"));
            assert!(matches!(left.0, Expr::Binary { op: BinOp::Chain, .. }));
        }
        other => panic!("Expected Binary Chain, got {:?}", other),
    }
}

#[test]
fn parse_unary_negation() {
    let expr = parse_process_expr("-1.0");
    match &expr.0 {
        Expr::Unary { op: UnaryOp::Neg, operand } => {
            assert!(matches!(operand.0, Expr::Number(n, _) if (n - 1.0).abs() < f64::EPSILON));
        }
        other => panic!("Expected Unary Neg, got {:?}", other),
    }
}

#[test]
fn parse_comparison() {
    let expr = parse_process_expr("1.0 > 0.0");
    assert!(matches!(expr.0, Expr::Binary { op: BinOp::Gt, .. }));
}

#[test]
fn parse_logical_and() {
    let expr = parse_process_expr("true && false");
    assert!(matches!(expr.0, Expr::Binary { op: BinOp::And, .. }));
}

#[test]
fn parse_grouped_expression() {
    let expr = parse_process_expr("(1.0 + 2.0) * 3.0");
    match &expr.0 {
        Expr::Binary { op: BinOp::Mul, left, .. } => {
            assert!(matches!(left.0, Expr::Grouped(_)));
        }
        other => panic!("Expected Binary Mul with grouped left, got {:?}", other),
    }
}

#[test]
fn parse_chained_field_access() {
    let expr = parse_process_expr("note.pitch");
    match &expr.0 {
        Expr::FieldAccess(base, field) => {
            assert!(matches!(base.0, Expr::Ident(ref s) if s == "note"));
            assert_eq!(field, "pitch");
        }
        other => panic!("Expected FieldAccess, got {:?}", other),
    }
}

#[test]
fn parse_voices_declaration() {
    let source = r#"plugin "Test" {
  midi {
    note {
      let f = note.pitch
    }
  }
  voices 8
  process {
    sine(440Hz) -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let voice = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::VoiceDecl(v) = item { Some(v) } else { None }
    }).unwrap();

    assert_eq!(voice.count, 8);
}

#[test]
fn parse_voices_in_full_synth() {
    let source = r#"plugin "Poly Synth" {
  vendor "Muse Audio"
  version "0.1.0"
  input mono
  output stereo
  midi {
    note {
      let f = note.pitch
    }
  }
  voices 8
  param gain: float = 0.5 in 0.0..1.0
  process {
    sine(note.pitch) -> gain(param.gain) -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    assert!(plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::MidiDecl(_))));
    assert!(plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::VoiceDecl(_))));
    assert!(plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::ProcessBlock(_))));
}

// ── Parameter declaration tests ──────────────────────────────

fn parse_param(param_source: &str) -> ParamDef {
    let source = format!(
        r#"plugin "Test" {{
  input mono
  output mono
  {}
  process {{
    input -> output
  }}
}}"#,
        param_source
    );
    let (ast, errors) = parse(&source);
    assert!(
        errors.is_empty(),
        "Parse errors for '{}': {:?}",
        param_source,
        errors
    );
    let plugin = ast.unwrap();
    *plugin.items.into_iter().find_map(|(item, _)| {
        if let PluginItem::ParamDecl(p) = item { Some(p) } else { None }
    }).unwrap()
}

#[test]
fn parse_simple_float_param() {
    let p = parse_param("param volume: float = 0.5 in 0.0..1.0");
    assert_eq!(p.name, "volume");
    assert_eq!(p.param_type, ParamType::Float);
    assert!(p.default.is_some());
    assert!(p.range.is_some());
    assert!(p.options.is_empty());
}

#[test]
fn parse_bool_param() {
    let p = parse_param("param bypass: bool = false");
    assert_eq!(p.name, "bypass");
    assert_eq!(p.param_type, ParamType::Bool);
    assert!(p.default.is_some());
    assert!(p.range.is_none());
}

#[test]
fn parse_enum_param() {
    let p = parse_param("param mode: enum [lowpass, highpass, bandpass] = lowpass");
    assert_eq!(p.name, "mode");
    if let ParamType::Enum(variants) = &p.param_type {
        assert_eq!(variants, &["lowpass", "highpass", "bandpass"]);
    } else {
        panic!("Expected Enum param type");
    }
}

#[test]
fn parse_param_with_body() {
    let p = parse_param(r#"param gain: float = 0.0 in -30.0..30.0 {
    smoothing logarithmic 50ms
    unit "dB"
  }"#);
    assert_eq!(p.name, "gain");
    assert_eq!(p.options.len(), 2);

    let has_smoothing = p.options.iter().any(|(opt, _)| matches!(opt, ParamOption::Smoothing { .. }));
    let has_unit = p.options.iter().any(|(opt, _)| matches!(opt, ParamOption::Unit(_)));
    assert!(has_smoothing, "Should have smoothing option");
    assert!(has_unit, "Should have unit option");
}

// ── Metadata tests ───────────────────────────────────────────

#[test]
fn parse_metadata_fields() {
    let source = r#"plugin "Test" {
  vendor "Test Vendor"
  version "1.0.0"
  category effect
  input mono
  output mono
  process {
    input -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let metadata: Vec<_> = plugin.items.iter().filter_map(|(item, _)| {
        if let PluginItem::Metadata(m) = item { Some(m) } else { None }
    }).collect();

    assert_eq!(metadata.len(), 3);
    assert!(metadata.iter().any(|m| m.key == MetadataKey::Vendor));
    assert!(metadata.iter().any(|m| m.key == MetadataKey::Version));
    assert!(metadata.iter().any(|m| m.key == MetadataKey::Category));
}

// ── Error recovery tests ─────────────────────────────────────

#[test]
fn parse_malformed_input_produces_errors() {
    let source = r#"plugin "Bad" {
  param : float
  process {
    input -> output
  }
}"#;
    let (ast, errors) = parse(source);
    // Should produce errors but potentially still have a partial AST
    assert!(!errors.is_empty(), "Expected parse errors for malformed input");
    // The AST might be Some with partial recovery, or None
    // Either way, we got errors — that's the important thing
    let _ = ast;
}

#[test]
fn parse_empty_process_block() {
    let source = r#"plugin "Empty" {
  input mono
  output mono
  process {
  }
}"#;
    let (ast, errors) = parse(source);
    // Empty process block is valid syntax
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();
    let process = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ProcessBlock(p) = item { Some(p) } else { None }
    }).unwrap();
    assert!(process.body.is_empty());
}

// ── Precedence tests ─────────────────────────────────────────

#[test]
fn chain_has_lowest_precedence() {
    // `input -> gain(1.0 + 2.0)` should parse as `input -> gain(1.0 + 2.0)`
    // not as `(input -> gain(1.0)) + 2.0`
    let expr = parse_process_expr("input -> gain(1.0 + 2.0)");
    match &expr.0 {
        Expr::Binary { op: BinOp::Chain, left, right } => {
            assert!(matches!(left.0, Expr::Ident(ref s) if s == "input"));
            assert!(matches!(right.0, Expr::FnCall { .. }));
        }
        other => panic!("Expected Chain, got {:?}", other),
    }
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    let expr = parse_process_expr("2.0 + 3.0 * 4.0");
    match &expr.0 {
        Expr::Binary { op: BinOp::Add, right, .. } => {
            assert!(matches!(right.0, Expr::Binary { op: BinOp::Mul, .. }));
        }
        other => panic!("Expected Add with Mul right child, got {:?}", other),
    }
}

#[test]
fn field_access_binds_tightest() {
    // param.gain should bind tighter than anything
    let expr = parse_process_expr("param.gain + 1.0");
    match &expr.0 {
        Expr::Binary { op: BinOp::Add, left, .. } => {
            assert!(matches!(left.0, Expr::FieldAccess(_, _)));
        }
        other => panic!("Expected Add with FieldAccess left child, got {:?}", other),
    }
}

// ── If expression tests ──────────────────────────────────────

#[test]
fn parse_if_expression() {
    let source = r#"plugin "Test" {
  input mono
  output mono
  param x: float = 0.0 in 0.0..1.0
  process {
    let y = if true {
      1.0
    } else {
      0.0
    }
    y -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();
    let process = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::ProcessBlock(p) = item { Some(p) } else { None }
    }).unwrap();

    // First statement should be a let with an if expression
    let first_stmt = &process.body[0].0;
    match first_stmt {
        Statement::Let { name, value } => {
            assert_eq!(name, "y");
            assert!(matches!(value.0, Expr::If { .. }));
        }
        other => panic!("Expected Let statement, got {:?}", other),
    }
}

// ── I/O declaration tests ────────────────────────────────────

#[test]
fn parse_io_declarations() {
    let source = r#"plugin "Test" {
  input stereo
  output mono
  process {
    input -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let ios: Vec<_> = plugin.items.iter().filter_map(|(item, _)| {
        if let PluginItem::IoDecl(io) = item { Some(io) } else { None }
    }).collect();

    assert_eq!(ios.len(), 2);
    assert_eq!(ios[0].direction, IoDirection::Input);
    assert_eq!(ios[0].channels, ChannelSpec::Stereo);
    assert_eq!(ios[1].direction, IoDirection::Output);
    assert_eq!(ios[1].channels, ChannelSpec::Mono);
}

// ── Format block tests ───────────────────────────────────────

#[test]
fn parse_clap_block() {
    let source = r#"plugin "Test" {
  input mono
  output mono
  clap {
    id "com.test.plugin"
    description "A test plugin"
    features [audio_effect, stereo]
  }
  process {
    input -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let clap = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::FormatBlock(FormatBlock::Clap(c)) = item { Some(c) } else { None }
    }).unwrap();

    assert_eq!(clap.items.len(), 3);
}

#[test]
fn parse_vst3_block() {
    let source = r#"plugin "Test" {
  input mono
  output mono
  vst3 {
    id "TestPlugin1"
    subcategories [Fx, Dynamics]
  }
  process {
    input -> output
  }
}"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let vst3 = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::FormatBlock(FormatBlock::Vst3(v)) = item { Some(v) } else { None }
    }).unwrap();

    assert_eq!(vst3.items.len(), 2);
}

// ── Signal routing expression tests ──────────────────────────

#[test]
fn split_basic_parses() {
    let expr = parse_process_expr(
        "split { lowpass(400Hz) highpass(4000Hz) }",
    );
    match &expr.0 {
        Expr::Split { branches } => {
            assert_eq!(branches.len(), 2, "Expected 2 branches, got {}", branches.len());
            // Each branch is a single expression-statement
            assert_eq!(branches[0].len(), 1);
            assert_eq!(branches[1].len(), 1);
            // First branch: lowpass(400Hz)
            match &branches[0][0].0 {
                Statement::Expr((Expr::FnCall { callee, args }, _)) => {
                    assert!(matches!(callee.0, Expr::Ident(ref s) if s == "lowpass"));
                    assert_eq!(args.len(), 1);
                }
                other => panic!("Expected FnCall in first branch, got {:?}", other),
            }
            // Second branch: highpass(4000Hz)
            match &branches[1][0].0 {
                Statement::Expr((Expr::FnCall { callee, args }, _)) => {
                    assert!(matches!(callee.0, Expr::Ident(ref s) if s == "highpass"));
                    assert_eq!(args.len(), 1);
                }
                other => panic!("Expected FnCall in second branch, got {:?}", other),
            }
        }
        other => panic!("Expected Split, got {:?}", other),
    }
}

#[test]
fn merge_as_expression() {
    let expr = parse_process_expr("merge");
    assert!(
        matches!(expr.0, Expr::Merge),
        "Expected Expr::Merge, got {:?}",
        expr.0
    );
}

#[test]
fn feedback_basic_parses() {
    let expr = parse_process_expr(
        "feedback { delay(100ms) -> lowpass(2000Hz) }",
    );
    match &expr.0 {
        Expr::Feedback { body } => {
            assert_eq!(body.len(), 1, "Expected 1 statement in feedback body");
            // The body statement should be an expression containing a chain
            match &body[0].0 {
                Statement::Expr((Expr::Binary { op: BinOp::Chain, .. }, _)) => {}
                other => panic!("Expected chain expression in feedback body, got {:?}", other),
            }
        }
        other => panic!("Expected Feedback, got {:?}", other),
    }
}

#[test]
fn split_merge_chain() {
    // input -> split { lowpass(400Hz) highpass(4000Hz) } -> merge -> output
    let expr = parse_process_expr(
        "input -> split { lowpass(400Hz) highpass(4000Hz) } -> merge -> output",
    );
    // Structure: (((input -> split{...}) -> merge) -> output)
    match &expr.0 {
        Expr::Binary { op: BinOp::Chain, left, right } => {
            // right = output
            assert!(matches!(right.0, Expr::Ident(ref s) if s == "output"));
            // left = (input -> split{...}) -> merge
            match &left.0 {
                Expr::Binary { op: BinOp::Chain, left: inner_left, right: inner_right } => {
                    // inner_right = merge
                    assert!(matches!(inner_right.0, Expr::Merge));
                    // inner_left = input -> split{...}
                    match &inner_left.0 {
                        Expr::Binary { op: BinOp::Chain, left: chain_left, right: chain_right } => {
                            assert!(matches!(chain_left.0, Expr::Ident(ref s) if s == "input"));
                            assert!(matches!(chain_right.0, Expr::Split { .. }));
                        }
                        other => panic!("Expected inner chain, got {:?}", other),
                    }
                }
                other => panic!("Expected chain with merge, got {:?}", other),
            }
        }
        other => panic!("Expected top-level chain, got {:?}", other),
    }
}

#[test]
fn feedback_in_chain() {
    // input -> feedback { delay(100ms) } -> output
    let expr = parse_process_expr(
        "input -> feedback { delay(100ms) } -> output",
    );
    // Structure: ((input -> feedback{...}) -> output)
    match &expr.0 {
        Expr::Binary { op: BinOp::Chain, left, right } => {
            assert!(matches!(right.0, Expr::Ident(ref s) if s == "output"));
            match &left.0 {
                Expr::Binary { op: BinOp::Chain, left: inner_left, right: inner_right } => {
                    assert!(matches!(inner_left.0, Expr::Ident(ref s) if s == "input"));
                    match &inner_right.0 {
                        Expr::Feedback { body } => {
                            assert_eq!(body.len(), 1);
                        }
                        other => panic!("Expected Feedback, got {:?}", other),
                    }
                }
                other => panic!("Expected inner chain, got {:?}", other),
            }
        }
        other => panic!("Expected chain, got {:?}", other),
    }
}

#[test]
fn nested_split() {
    // split inside a split branch should parse
    let expr = parse_process_expr(
        "split { split { lowpass(400Hz) highpass(4000Hz) } bandpass(1000Hz) }",
    );
    match &expr.0 {
        Expr::Split { branches } => {
            assert_eq!(branches.len(), 2, "Expected 2 branches in outer split");
            // First branch: inner split
            match &branches[0][0].0 {
                Statement::Expr((Expr::Split { branches: inner }, _)) => {
                    assert_eq!(inner.len(), 2, "Expected 2 branches in inner split");
                }
                other => panic!("Expected inner Split in first branch, got {:?}", other),
            }
            // Second branch: bandpass call
            match &branches[1][0].0 {
                Statement::Expr((Expr::FnCall { callee, .. }, _)) => {
                    assert!(matches!(callee.0, Expr::Ident(ref s) if s == "bandpass"));
                }
                other => panic!("Expected FnCall in second branch, got {:?}", other),
            }
        }
        other => panic!("Expected Split, got {:?}", other),
    }
}

// ── Test block parser tests ──────────────────────────────────

#[test]
fn test_block_basic_parse() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo

        param gain: float = 0.0 in -30.0..30.0

        process {
            input
        }

        test "silence produces silence" {
            input silence 512 samples
            set param.gain = 0.0
            assert output.rms < -120.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.expect("Should produce AST");

    // Find the test block
    let test_blocks: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::TestBlock(tb) = item {
                Some(tb)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(test_blocks.len(), 1);
    assert_eq!(test_blocks[0].name, "silence produces silence");
    assert_eq!(test_blocks[0].statements.len(), 3);

    // Check input statement
    match &test_blocks[0].statements[0].0 {
        TestStatement::Input(input) => {
            assert_eq!(input.signal, TestSignal::Silence);
            assert_eq!(input.sample_count, 512);
        }
        other => panic!("Expected Input, got {:?}", other),
    }

    // Check set statement
    match &test_blocks[0].statements[1].0 {
        TestStatement::Set(set) => {
            assert_eq!(set.param_path, "gain");
            assert!((set.value - 0.0).abs() < f64::EPSILON);
        }
        other => panic!("Expected Set, got {:?}", other),
    }

    // Check assert statement
    match &test_blocks[0].statements[2].0 {
        TestStatement::Assert(assertion) => {
            assert_eq!(assertion.property, TestProperty::OutputRms);
            assert_eq!(assertion.op, TestOp::LessThan);
            assert!((assertion.value - (-120.0)).abs() < f64::EPSILON);
        }
        other => panic!("Expected Assert, got {:?}", other),
    }
}

#[test]
fn test_block_sine_signal() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "sine input" {
            input sine 440Hz 1024 samples
            assert output.peak > 0.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).unwrap();

    assert_eq!(tb.name, "sine input");
    match &tb.statements[0].0 {
        TestStatement::Input(input) => {
            assert_eq!(input.signal, TestSignal::Sine { frequency: 440.0 });
            assert_eq!(input.sample_count, 1024);
        }
        other => panic!("Expected Input with sine, got {:?}", other),
    }
    match &tb.statements[1].0 {
        TestStatement::Assert(a) => {
            assert_eq!(a.property, TestProperty::OutputPeak);
            assert_eq!(a.op, TestOp::GreaterThan);
        }
        other => panic!("Expected Assert, got {:?}", other),
    }
}

#[test]
fn test_block_impulse_signal() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "impulse response" {
            input impulse 256 samples
            assert output.peak > 0.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).unwrap();

    match &tb.statements[0].0 {
        TestStatement::Input(input) => {
            assert_eq!(input.signal, TestSignal::Impulse);
            assert_eq!(input.sample_count, 256);
        }
        other => panic!("Expected Input with impulse, got {:?}", other),
    }
}

#[test]
fn test_block_multiple_tests() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        param gain: float = 0.0 in -30.0..30.0
        process { input }

        test "first test" {
            input silence 512 samples
            assert output.rms < -120.0
        }

        test "second test" {
            input sine 440Hz 1024 samples
            set param.gain = 6.0
            assert output.peak > 1.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let test_blocks: Vec<_> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
        })
        .collect();
    assert_eq!(test_blocks.len(), 2);
    assert_eq!(test_blocks[0].name, "first test");
    assert_eq!(test_blocks[1].name, "second test");
}

#[test]
fn test_block_approx_equal_op() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "approx test" {
            input sine 440Hz 1024 samples
            assert output.rms ~= 0.707
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).unwrap();

    match &tb.statements[1].0 {
        TestStatement::Assert(a) => {
            assert_eq!(a.op, TestOp::ApproxEqual);
            assert!((a.value - 0.707).abs() < 0.001);
        }
        other => panic!("Expected Assert with ~=, got {:?}", other),
    }
}

#[test]
fn test_block_input_property() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "check input properties" {
            input sine 1000Hz 512 samples
            assert input.rms > 0.0
            assert input.peak > 0.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).unwrap();

    assert_eq!(tb.statements.len(), 3);
    match &tb.statements[1].0 {
        TestStatement::Assert(a) => assert_eq!(a.property, TestProperty::InputRms),
        other => panic!("Expected Assert input.rms, got {:?}", other),
    }
    match &tb.statements[2].0 {
        TestStatement::Assert(a) => assert_eq!(a.property, TestProperty::InputPeak),
        other => panic!("Expected Assert input.peak, got {:?}", other),
    }
}

#[test]
fn test_block_negative_value() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "negative dB value" {
            input silence 512 samples
            assert output.rms < -120.0dB
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).unwrap();

    match &tb.statements[1].0 {
        TestStatement::Assert(a) => {
            assert!((a.value - (-120.0)).abs() < f64::EPSILON);
        }
        other => panic!("Expected Assert, got {:?}", other),
    }
}

#[test]
fn parse_unison_block() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input mono
        output stereo
        voices 8
        unison {
            count 3
            detune 15
        }
        midi {
            note {
                let freq = note.pitch
            }
        }
        process { input -> output }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let unison = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::UnisonDecl(u) = item { Some(u) } else { None }
    });

    let unison = unison.expect("Should have a UnisonDecl item");
    assert_eq!(unison.count, 3);
    assert!((unison.detune_cents - 15.0).abs() < f64::EPSILON);
}

#[test]
fn parse_unison_block_float_detune() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input mono
        output stereo
        voices 8
        unison {
            count 4
            detune 7.5
        }
        midi {
            note {
                let freq = note.pitch
            }
        }
        process { input -> output }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let unison = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::UnisonDecl(u) = item { Some(u) } else { None }
    }).expect("Should have a UnisonDecl item");
    assert_eq!(unison.count, 4);
    assert!((unison.detune_cents - 7.5).abs() < f64::EPSILON);
}

#[test]
fn note_on_parsed() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "note on test" {
            note on 69 0.8 at 0
            input silence 8192 samples
            assert output.rms > -20.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    match &tb.statements[0].0 {
        TestStatement::NoteOn { note, velocity, timing } => {
            assert_eq!(*note, 69);
            assert!((*velocity - 0.8).abs() < f64::EPSILON);
            assert_eq!(*timing, 0);
        }
        other => panic!("Expected NoteOn, got {:?}", other),
    }
}

#[test]
fn note_off_parsed() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "note off test" {
            note on 60 0.5 at 0
            note off 60 at 4096
            input silence 8192 samples
            assert output.rms > -60.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    assert_eq!(tb.statements.len(), 4);

    match &tb.statements[0].0 {
        TestStatement::NoteOn { note, velocity, timing } => {
            assert_eq!(*note, 60);
            assert!((*velocity - 0.5).abs() < f64::EPSILON);
            assert_eq!(*timing, 0);
        }
        other => panic!("Expected NoteOn, got {:?}", other),
    }

    match &tb.statements[1].0 {
        TestStatement::NoteOff { note, timing } => {
            assert_eq!(*note, 60);
            assert_eq!(*timing, 4096);
        }
        other => panic!("Expected NoteOff, got {:?}", other),
    }
}

#[test]
fn safety_assert_no_nan_parsed() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "safety test" {
            input silence 512 samples
            assert no_nan
            assert no_denormal
            assert no_inf
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    assert_eq!(tb.statements.len(), 4);

    match &tb.statements[1].0 {
        TestStatement::SafetyAssert(SafetyCheck::NoNan) => {}
        other => panic!("Expected SafetyAssert(NoNan), got {:?}", other),
    }
    match &tb.statements[2].0 {
        TestStatement::SafetyAssert(SafetyCheck::NoDenormal) => {}
        other => panic!("Expected SafetyAssert(NoDenormal), got {:?}", other),
    }
    match &tb.statements[3].0 {
        TestStatement::SafetyAssert(SafetyCheck::NoInf) => {}
        other => panic!("Expected SafetyAssert(NoInf), got {:?}", other),
    }
}

#[test]
fn temporal_rms_in_parsed() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "temporal test" {
            input silence 1024 samples
            assert output.rms_in 0..256 > -10.0
            assert output.peak_in 256..512 < -60.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    assert_eq!(tb.statements.len(), 3);

    match &tb.statements[1].0 {
        TestStatement::Assert(assert) => {
            assert_eq!(assert.property, TestProperty::OutputRmsIn(0, 256));
            assert_eq!(assert.op, TestOp::GreaterThan);
            assert!(assert.value < 0.0);
        }
        other => panic!("Expected Assert with OutputRmsIn, got {:?}", other),
    }

    match &tb.statements[2].0 {
        TestStatement::Assert(assert) => {
            assert_eq!(assert.property, TestProperty::OutputPeakIn(256, 512));
            assert_eq!(assert.op, TestOp::LessThan);
        }
        other => panic!("Expected Assert with OutputPeakIn, got {:?}", other),
    }
}

#[test]
fn frequency_assert_parsed() {
    let source = r#"
    plugin "TestPlugin" {
        vendor "Test"
        input stereo
        output stereo
        process { input }

        test "spectral test" {
            input sine 440 Hz 4096 samples
            assert frequency 440Hz > -20.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    assert_eq!(tb.statements.len(), 2);

    match &tb.statements[1].0 {
        TestStatement::Assert(assert) => {
            assert_eq!(assert.property, TestProperty::Frequency(440.0));
            assert_eq!(assert.op, TestOp::GreaterThan);
            assert!((assert.value - (-20.0)).abs() < f64::EPSILON);
        }
        other => panic!("Expected Assert with Frequency, got {:?}", other),
    }
}

// ── Preset block parsing ─────────────────────────────────────

#[test]
fn parse_single_preset_block() {
    let source = r#"
    plugin "Test" {
        vendor "Test"
        input stereo
        output stereo
        param gain: float = 0.0 in -30.0..30.0
        process { input }

        preset "Default" {
            gain = 0.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let preset = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::PresetDecl(p) = item { Some(p) } else { None }
    }).expect("Should have a PresetDecl");

    assert_eq!(preset.name, "Default");
    assert_eq!(preset.assignments.len(), 1);
    assert_eq!(preset.assignments[0].0.param_name, "gain");
    assert_eq!(preset.assignments[0].0.value, PresetValue::Number(0.0));
}

#[test]
fn parse_multiple_preset_blocks() {
    let source = r#"
    plugin "Test" {
        vendor "Test"
        input stereo
        output stereo
        param gain: float = 0.0 in -30.0..30.0
        param mix: float = 1.0 in 0.0..1.0
        process { input }

        preset "Clean" {
            gain = 0.0
            mix = 1.0
        }

        preset "Heavy" {
            gain = -12.0
            mix = 0.5
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let presets: Vec<_> = plugin.items.iter().filter_map(|(item, _)| {
        if let PluginItem::PresetDecl(p) = item { Some(p) } else { None }
    }).collect();

    assert_eq!(presets.len(), 2);
    assert_eq!(presets[0].name, "Clean");
    assert_eq!(presets[0].assignments.len(), 2);
    assert_eq!(presets[1].name, "Heavy");
    assert_eq!(presets[1].assignments.len(), 2);
    // Verify negative number parsing
    assert_eq!(presets[1].assignments[0].0.value, PresetValue::Number(-12.0));
}

#[test]
fn parse_preset_with_bool_and_ident_values() {
    let source = r#"
    plugin "Test" {
        vendor "Test"
        input stereo
        output stereo
        param bypass: bool = false
        param mode: enum [lowpass, highpass, bandpass]
        param gain: float = 0.0 in -30.0..30.0
        process { input }

        preset "Special" {
            bypass = true
            mode = highpass
            gain = -6.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let preset = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::PresetDecl(p) = item { Some(p) } else { None }
    }).expect("Should have a PresetDecl");

    assert_eq!(preset.name, "Special");
    assert_eq!(preset.assignments.len(), 3);
    assert_eq!(preset.assignments[0].0.value, PresetValue::Bool(true));
    assert_eq!(preset.assignments[1].0.value, PresetValue::Ident("highpass".into()));
    assert_eq!(preset.assignments[2].0.value, PresetValue::Number(-6.0));
}

#[test]
fn parse_set_preset_in_test_block() {
    let source = r#"
    plugin "Test" {
        vendor "Test"
        input stereo
        output stereo
        param gain: float = 0.0 in -30.0..30.0
        process { input }

        preset "Loud" {
            gain = 6.0
        }

        test "preset test" {
            set preset "Loud"
            input silence 512 samples
            assert output.peak == 0.0
        }
    }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let tb = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item { Some(tb) } else { None }
    }).expect("Should have a TestBlock");

    assert_eq!(tb.statements.len(), 3);
    match &tb.statements[0].0 {
        TestStatement::SetPreset { name } => assert_eq!(name, "Loud"),
        other => panic!("Expected SetPreset, got {:?}", other),
    }
}


// ── GUI block parser tests ───────────────────────────────────

#[test]
fn gui_block_with_theme_and_accent() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        theme dark
        accent "#E8A87C"
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 2);
    match &gui.items[0].0 {
        GuiItem::Theme(t) => assert_eq!(t, "dark"),
        other => panic!("Expected Theme, got {:?}", other),
    }
    match &gui.items[1].0 {
        GuiItem::Accent(a) => assert_eq!(a, "#E8A87C"),
        other => panic!("Expected Accent, got {:?}", other),
    }
}

#[test]
fn gui_block_theme_only() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        theme light
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Theme(t) => assert_eq!(t, "light"),
        other => panic!("Expected Theme, got {:?}", other),
    }
}

#[test]
fn gui_block_empty() {
    let source = r#"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui { }
}
"#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 0);
}

#[test]
fn gui_block_accent_only() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        accent "#FFF"
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Accent(a) => assert_eq!(a, "#FFF"),
        other => panic!("Expected Accent, got {:?}", other),
    }
}

// ── Tier 2/3 GUI parser tests ────────────────────────────────

#[test]
fn gui_block_size_item() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        size 700 450
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Size(w, h) => {
            assert_eq!(*w, 700);
            assert_eq!(*h, 450);
        }
        other => panic!("Expected Size, got {:?}", other),
    }
}

#[test]
fn gui_block_css_item() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        css ".my-class { color: red; }"
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Css(s) => assert_eq!(s, ".my-class { color: red; }"),
        other => panic!("Expected Css, got {:?}", other),
    }
}

#[test]
fn gui_block_layout_with_nested_widgets() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    param mix : float = 0.5 in 0.0..1.0
    process { input }
    gui {
        layout horizontal {
            knob gain
            knob mix
        }
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Layout(layout) => {
            assert_eq!(layout.direction, LayoutDirection::Horizontal);
            assert_eq!(layout.children.len(), 2);
            match &layout.children[0].0 {
                GuiItem::Widget(w) => {
                    assert_eq!(w.widget_type, WidgetType::Knob);
                    assert_eq!(w.param_name, Some("gain".to_string()));
                }
                other => panic!("Expected Widget, got {:?}", other),
            }
            match &layout.children[1].0 {
                GuiItem::Widget(w) => {
                    assert_eq!(w.widget_type, WidgetType::Knob);
                    assert_eq!(w.param_name, Some("mix".to_string()));
                }
                other => panic!("Expected Widget, got {:?}", other),
            }
        }
        other => panic!("Expected Layout, got {:?}", other),
    }
}

#[test]
fn gui_block_panel_with_title() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    process { input }
    gui {
        panel "Controls" {
            knob gain
            label "Output Level"
        }
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Panel(panel) => {
            assert_eq!(panel.title, "Controls");
            assert_eq!(panel.children.len(), 2);
            // First child: knob
            match &panel.children[0].0 {
                GuiItem::Widget(w) => {
                    assert_eq!(w.widget_type, WidgetType::Knob);
                    assert_eq!(w.param_name, Some("gain".to_string()));
                }
                other => panic!("Expected Widget(Knob), got {:?}", other),
            }
            // Second child: label
            match &panel.children[1].0 {
                GuiItem::Widget(w) => {
                    assert_eq!(w.widget_type, WidgetType::Label);
                    assert_eq!(w.param_name, None);
                    assert_eq!(w.label_text, Some("Output Level".to_string()));
                }
                other => panic!("Expected Widget(Label), got {:?}", other),
            }
        }
        other => panic!("Expected Panel, got {:?}", other),
    }
}

#[test]
fn gui_block_widget_with_style_and_class() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    process { input }
    gui {
        knob gain { style "vintage" class "hero-knob" label "Volume" }
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Widget(w) => {
            assert_eq!(w.widget_type, WidgetType::Knob);
            assert_eq!(w.param_name, Some("gain".to_string()));
            assert_eq!(w.props.len(), 3);
            assert_eq!(w.props[0], WidgetProp::Style("vintage".to_string()));
            assert_eq!(w.props[1], WidgetProp::Class("hero-knob".to_string()));
            assert_eq!(w.props[2], WidgetProp::Label("Volume".to_string()));
        }
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_nested_layout_in_panel() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    param mix : float = 0.5 in 0.0..1.0
    process { input }
    gui {
        theme dark
        accent "#E8A87C"
        size 700 450
        layout vertical {
            panel "Main" {
                layout horizontal {
                    knob gain
                    slider mix
                }
            }
            label "Status"
        }
        css ".custom { opacity: 0.8; }"
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    // theme, accent, size, layout, css = 5 items
    assert_eq!(gui.items.len(), 5);

    // Verify the nested structure: layout > panel > layout > [knob, slider]
    match &gui.items[3].0 {
        GuiItem::Layout(outer) => {
            assert_eq!(outer.direction, LayoutDirection::Vertical);
            assert_eq!(outer.children.len(), 2); // panel + label

            match &outer.children[0].0 {
                GuiItem::Panel(panel) => {
                    assert_eq!(panel.title, "Main");
                    assert_eq!(panel.children.len(), 1); // inner layout

                    match &panel.children[0].0 {
                        GuiItem::Layout(inner) => {
                            assert_eq!(inner.direction, LayoutDirection::Horizontal);
                            assert_eq!(inner.children.len(), 2); // knob + slider
                            match &inner.children[0].0 {
                                GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::Knob),
                                other => panic!("Expected Knob, got {:?}", other),
                            }
                            match &inner.children[1].0 {
                                GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::Slider),
                                other => panic!("Expected Slider, got {:?}", other),
                            }
                        }
                        other => panic!("Expected inner Layout, got {:?}", other),
                    }
                }
                other => panic!("Expected Panel, got {:?}", other),
            }

            // Second child of outer layout: label
            match &outer.children[1].0 {
                GuiItem::Widget(w) => {
                    assert_eq!(w.widget_type, WidgetType::Label);
                    assert_eq!(w.label_text, Some("Status".to_string()));
                }
                other => panic!("Expected Label, got {:?}", other),
            }
        }
        other => panic!("Expected outer Layout, got {:?}", other),
    }

    // Verify css
    match &gui.items[4].0 {
        GuiItem::Css(s) => assert_eq!(s, ".custom { opacity: 0.8; }"),
        other => panic!("Expected Css, got {:?}", other),
    }
}

#[test]
fn gui_block_all_widget_types() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    param level : float = 0.0 in -60.0..0.0
    param bypass : bool = false
    process { input }
    gui {
        knob gain
        slider gain
        meter level
        switch bypass
        value gain
        label "Info"
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 6);

    let types: Vec<_> = gui.items.iter().map(|(item, _)| match item {
        GuiItem::Widget(w) => w.widget_type.clone(),
        other => panic!("Expected Widget, got {:?}", other),
    }).collect();

    assert_eq!(types, vec![
        WidgetType::Knob,
        WidgetType::Slider,
        WidgetType::Meter,
        WidgetType::Switch,
        WidgetType::Value,
        WidgetType::Label,
    ]);
}

#[test]
fn gui_block_xy_pad_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param freq : float = 0.5 in 0.0..1.0
    param res : float = 0.5 in 0.0..1.0
    process { input }
    gui {
        xy_pad freq res
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Widget(w) => {
            assert_eq!(w.widget_type, WidgetType::XyPad);
            assert_eq!(w.param_name, Some("freq".to_string()));
            assert_eq!(w.param_name_y, Some("res".to_string()));
        }
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_xy_pad_with_props() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param x : float = 0.5 in 0.0..1.0
    param y : float = 0.5 in 0.0..1.0
    process { input }
    gui {
        xy_pad x y { style "large" class "main-pad" }
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => {
            assert_eq!(w.widget_type, WidgetType::XyPad);
            assert_eq!(w.param_name, Some("x".to_string()));
            assert_eq!(w.param_name_y, Some("y".to_string()));
            assert!(w.props.iter().any(|p| matches!(p, WidgetProp::Style(s) if s == "large")));
            assert!(w.props.iter().any(|p| matches!(p, WidgetProp::Class(c) if c == "main-pad")));
        }
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_spectrum_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        spectrum
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 1);
    match &gui.items[0].0 {
        GuiItem::Widget(w) => {
            assert_eq!(w.widget_type, WidgetType::Spectrum);
            assert_eq!(w.param_name, None);
            assert_eq!(w.param_name_y, None);
        }
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_waveform_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        waveform
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::Waveform),
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_envelope_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        envelope
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::Envelope),
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_eq_curve_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        eq_curve
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::EqCurve),
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_reduction_widget() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        reduction
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => assert_eq!(w.widget_type, WidgetType::Reduction),
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_vis_widget_with_props() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    process { input }
    gui {
        spectrum { class "analyzer" style "large" }
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    match &gui.items[0].0 {
        GuiItem::Widget(w) => {
            assert_eq!(w.widget_type, WidgetType::Spectrum);
            assert!(w.props.iter().any(|p| matches!(p, WidgetProp::Class(c) if c == "analyzer")));
            assert!(w.props.iter().any(|p| matches!(p, WidgetProp::Style(s) if s == "large")));
        }
        other => panic!("Expected Widget, got {:?}", other),
    }
}

#[test]
fn gui_block_all_widget_types_including_advanced() {
    let source = r##"
plugin "Test" {
    vendor "Test"
    input stereo
    output stereo
    param gain : float = 0.0 in -30.0..30.0
    param level : float = 0.0 in -60.0..0.0
    param bypass : bool = false
    param freq : float = 0.5 in 0.0..1.0
    param res : float = 0.5 in 0.0..1.0
    process { input }
    gui {
        knob gain
        slider gain
        meter level
        switch bypass
        value gain
        label "Info"
        xy_pad freq res
        spectrum
        waveform
        envelope
        eq_curve
        reduction
    }
}
"##;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    let plugin = ast.unwrap();

    let gui = plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::GuiDecl(gui) = item { Some(gui) } else { None }
    }).expect("Should have a GuiDecl");

    assert_eq!(gui.items.len(), 12);

    let types: Vec<_> = gui.items.iter().map(|(item, _)| match item {
        GuiItem::Widget(w) => w.widget_type.clone(),
        other => panic!("Expected Widget, got {:?}", other),
    }).collect();

    assert_eq!(types, vec![
        WidgetType::Knob,
        WidgetType::Slider,
        WidgetType::Meter,
        WidgetType::Switch,
        WidgetType::Value,
        WidgetType::Label,
        WidgetType::XyPad,
        WidgetType::Spectrum,
        WidgetType::Waveform,
        WidgetType::Envelope,
        WidgetType::EqCurve,
        WidgetType::Reduction,
    ]);
}

// ── Sample declaration tests ─────────────────────────────────

#[test]
fn parse_sample_declaration() {
    let source = r#"
        plugin "Test" {
            sample kick "samples/kick.wav"
        }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no parse errors, got: {:?}", errors);
    let plugin = ast.expect("Expected AST");

    let sample_items: Vec<_> = plugin.items.iter()
        .filter_map(|(item, _)| match item {
            PluginItem::SampleDecl(decl) => Some(decl),
            _ => None,
        })
        .collect();

    assert_eq!(sample_items.len(), 1);
    assert_eq!(sample_items[0].name, "kick");
    assert_eq!(sample_items[0].path, "samples/kick.wav");
}

#[test]
fn parse_multiple_sample_declarations() {
    let source = r#"
        plugin "Test" {
            sample kick "samples/kick.wav"
            sample snare "samples/snare.wav"
            sample hihat "samples/hihat.wav"
        }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no parse errors, got: {:?}", errors);
    let plugin = ast.expect("Expected AST");

    let sample_items: Vec<_> = plugin.items.iter()
        .filter_map(|(item, _)| match item {
            PluginItem::SampleDecl(decl) => Some(decl),
            _ => None,
        })
        .collect();

    assert_eq!(sample_items.len(), 3);
    assert_eq!(sample_items[0].name, "kick");
    assert_eq!(sample_items[1].name, "snare");
    assert_eq!(sample_items[2].name, "hihat");
}

#[test]
fn parse_sample_with_process_block() {
    let source = r#"
        plugin "Drum Machine" {
            input mono
            output stereo

            sample kick "samples/kick.wav"

            process {
                play(kick) -> output
            }
        }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no parse errors, got: {:?}", errors);
    let plugin = ast.expect("Expected AST");

    let has_sample = plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::SampleDecl(_)));
    let has_process = plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::ProcessBlock(_)));
    assert!(has_sample, "Expected sample declaration");
    assert!(has_process, "Expected process block");
}

// ── Wavetable parser tests ───────────────────────────────────

#[test]
fn parse_wavetable_declaration() {
    let source = r#"
        plugin "Test" {
            wavetable wt "samples/test.wav"
        }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no parse errors, got: {:?}", errors);
    let plugin = ast.expect("Expected AST");

    let wt_items: Vec<_> = plugin.items.iter()
        .filter_map(|(item, _)| match item {
            PluginItem::WavetableDecl(decl) => Some(decl),
            _ => None,
        })
        .collect();

    assert_eq!(wt_items.len(), 1);
    assert_eq!(wt_items[0].name, "wt");
    assert_eq!(wt_items[0].path, "samples/test.wav");
    assert_eq!(wt_items[0].frame_size, 2048);
}

#[test]
fn parse_multiple_wavetable_declarations() {
    let source = r#"
        plugin "Test" {
            wavetable saw "samples/saw.wav"
            wavetable square "samples/square.wav"
        }
    "#;
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no parse errors, got: {:?}", errors);
    let plugin = ast.expect("Expected AST");

    let wt_items: Vec<_> = plugin.items.iter()
        .filter_map(|(item, _)| match item {
            PluginItem::WavetableDecl(decl) => Some(decl),
            _ => None,
        })
        .collect();

    assert_eq!(wt_items.len(), 2);
    assert_eq!(wt_items[0].name, "saw");
    assert_eq!(wt_items[1].name, "square");
}
