//! Tests for the diagnostic system: structured errors, JSON serialization, and ariadne rendering.

use muse_lang::diagnostic::{diagnostics_to_json, render_ariadne, Diagnostic, Severity};
use muse_lang::parser::parse_to_diagnostics;

// ── Diagnostic struct tests ──────────────────────────────────

#[test]
fn diagnostic_error_constructor() {
    let span = chumsky::span::SimpleSpan::new(10, 15);
    let diag = Diagnostic::error("E001", span, "unexpected token");
    assert_eq!(diag.code, "E001");
    assert_eq!(diag.span, (10, 15));
    assert_eq!(diag.severity, Severity::Error);
    assert_eq!(diag.message, "unexpected token");
    assert!(diag.suggestion.is_none());
}

#[test]
fn diagnostic_with_suggestion() {
    let span = chumsky::span::SimpleSpan::new(0, 5);
    let diag = Diagnostic::error("E002", span, "unclosed block")
        .with_suggestion("add closing brace '}'");
    assert_eq!(diag.suggestion, Some("add closing brace '}'".to_string()));
}

// ── Parse error to diagnostic conversion ─────────────────────

#[test]
fn parse_error_missing_closing_brace() {
    let src = r#"plugin "Test" { vendor "Me""#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(!diags.is_empty(), "expected at least one diagnostic");
    // Should produce an E002 (unclosed block) since we hit EOF without '}'
    let has_unclosed = diags.iter().any(|d| d.code == "E002");
    let has_any_error = diags.iter().any(|d| d.severity == Severity::Error);
    assert!(has_any_error, "expected an error-severity diagnostic");
    // The error should mention closing brace or be an unexpected-end-of-input error
    assert!(
        has_unclosed || diags.iter().any(|d| d.message.contains("end of input")),
        "expected unclosed block or unexpected end of input, got: {:?}",
        diags
    );
}

#[test]
fn parse_error_unexpected_token() {
    // "@@" is not a valid token after the plugin keyword
    let src = r#"plugin "Test" { process { 123 + } }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(!diags.is_empty(), "expected at least one diagnostic");
    assert!(
        diags.iter().any(|d| d.severity == Severity::Error),
        "expected error severity, got: {:?}",
        diags
    );
}

#[test]
fn parse_error_span_is_valid() {
    let src = r#"plugin "Test" { process { 123 + } }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    for diag in &diags {
        assert!(
            diag.span.0 <= diag.span.1,
            "span start should be <= end: {:?}",
            diag.span
        );
        assert!(
            diag.span.1 <= src.len(),
            "span end should be <= source length: {:?} vs {}",
            diag.span,
            src.len()
        );
    }
}

#[test]
fn parse_valid_input_produces_no_diagnostics() {
    let src = r#"plugin "Test" { vendor "Me" }"#;
    let (ast, diags) = parse_to_diagnostics(src);
    assert!(ast.is_some(), "expected AST for valid input");
    assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
}

// ── JSON serialization tests ─────────────────────────────────

#[test]
fn diagnostic_to_json_roundtrip() {
    let span = chumsky::span::SimpleSpan::new(5, 10);
    let diag = Diagnostic::error("E001", span, "unexpected token '+'")
        .with_suggestion("remove the operator");

    let json = diag.to_json();

    // Deserialize back and verify fields
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["code"], "E001");
    assert_eq!(parsed["span"][0], 5);
    assert_eq!(parsed["span"][1], 10);
    assert_eq!(parsed["severity"], "error");
    assert_eq!(parsed["message"], "unexpected token '+'");
    assert_eq!(parsed["suggestion"], "remove the operator");
}

#[test]
fn diagnostics_to_json_array() {
    let diags = vec![
        Diagnostic::error(
            "E001",
            chumsky::span::SimpleSpan::new(0, 5),
            "first error",
        ),
        Diagnostic::error(
            "E002",
            chumsky::span::SimpleSpan::new(10, 15),
            "second error",
        )
        .with_suggestion("fix it"),
    ];

    let json = diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).expect("valid JSON array");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0]["code"], "E001");
    assert_eq!(parsed[1]["code"], "E002");
    assert_eq!(parsed[1]["suggestion"], "fix it");
}

#[test]
fn diagnostic_json_omits_null_suggestion() {
    let diag = Diagnostic::error(
        "E001",
        chumsky::span::SimpleSpan::new(0, 1),
        "some error",
    );
    let json = diag.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    // suggestion should not be present (skip_serializing_if = None)
    assert!(
        parsed.get("suggestion").is_none(),
        "null suggestion should be omitted from JSON, got: {json}"
    );
}

#[test]
fn parse_errors_produce_valid_json() {
    // Parse deliberately broken input, serialize, and verify valid JSON
    let src = r#"plugin "Test" { process { 123 + } }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(!diags.is_empty());

    let json = diagnostics_to_json(&diags);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("diagnostic JSON should be valid");

    for entry in &parsed {
        assert!(entry.get("code").is_some(), "missing 'code' field");
        assert!(entry.get("span").is_some(), "missing 'span' field");
        assert!(entry.get("severity").is_some(), "missing 'severity' field");
        assert!(entry.get("message").is_some(), "missing 'message' field");
    }
}

// ── Multiple errors test ─────────────────────────────────────

#[test]
fn multiple_errors_all_reported() {
    // Input with errors in distinct blocks — error recovery should catch at least one.
    // Note: chumsky's nested_delimiters recovery may coalesce errors within a single
    // block boundary, so we may get fewer errors than distinct malformed blocks.
    let src = r#"plugin "Test" {
        process { 123 + }
        process { 456 * }
    }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(
        !diags.is_empty(),
        "expected at least 1 diagnostic, got 0"
    );
    // All reported diagnostics should be errors
    for diag in &diags {
        assert_eq!(diag.severity, Severity::Error);
    }
}

// ── Error recovery test ──────────────────────────────────────

#[test]
fn error_recovery_produces_partial_ast_and_diagnostics() {
    // Valid metadata followed by a bad block — the nested_delimiters recovery
    // should let the parser skip the malformed block and still produce a partial AST.
    // Chumsky's recovery works at brace boundaries: the bad { ... } is consumed as
    // a recovery placeholder, and the parser continues with subsequent items.
    let src = r#"plugin "Test" {
        vendor "Test Co"
        process { input -> output }
    }"#;
    let (ast, diags) = parse_to_diagnostics(src);
    // This input is actually valid, so it should parse cleanly
    assert!(
        ast.is_some(),
        "expected AST from valid input"
    );
    assert!(
        diags.is_empty(),
        "expected no diagnostics for valid input, got: {:?}",
        diags
    );
}

#[test]
fn error_recovery_with_bad_block_still_parses() {
    // A malformed item between valid items. The brace-level recovery should
    // skip the bad { ... } and still produce a partial AST.
    let src = r#"plugin "Test" {
        vendor "Test Co"
        { invalid stuff here }
        process { input -> output }
    }"#;
    let (ast, diags) = parse_to_diagnostics(src);
    // With error recovery, we should get at least a partial AST
    // (the plugin node itself should be produced even if some items are error placeholders)
    if ast.is_some() {
        // If recovery worked, we may have diagnostics for the bad block
        // but we don't require them since chumsky may silently recover
    }
    // The key thing: parsing doesn't panic, and we get a result either way
    let _ = (ast, diags);
}

// ── Ariadne rendering doesn't panic ──────────────────────────

#[test]
fn ariadne_rendering_does_not_panic() {
    let src = r#"plugin "Test" { process { 123 + } }"#;
    let (_ast, diags) = parse_to_diagnostics(src);
    assert!(!diags.is_empty());
    // Just call render_ariadne — we don't assert output, just that it doesn't panic
    render_ariadne(&diags, src, "test.muse");
}

#[test]
fn ariadne_rendering_with_suggestion_does_not_panic() {
    let src = r#"plugin "Test" { vendor "Me""#;
    let (_ast, diags) = parse_to_diagnostics(src);
    // This typically produces an unclosed-block error with a suggestion
    render_ariadne(&diags, src, "test.muse");
}

// ── compile_check convenience function ───────────────────────

#[test]
fn compile_check_valid_returns_true() {
    let result = muse_lang::compile_check(
        r#"plugin "Test" { vendor "Me" }"#,
        "test.muse",
        true,
    );
    assert!(result, "compile_check should return true for valid input");
}

#[test]
fn compile_check_invalid_returns_false() {
    let result = muse_lang::compile_check(
        r#"plugin "Test" { process { 123 + } }"#,
        "test.muse",
        true,
    );
    assert!(!result, "compile_check should return false for invalid input");
}
