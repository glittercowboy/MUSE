//! Structured diagnostic system for the Muse compiler.
//!
//! Provides the `Diagnostic` struct with JSON serialization and ariadne rendering.
//! This is the primary observability surface for AI agents consuming compiler output —
//! all compiler errors flow through this struct as structured JSON.

use crate::span::Span;
use ariadne::{Color, Label, Report, ReportKind, Source};
use serde::Serialize;

/// Severity level for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A structured compiler diagnostic.
///
/// Every error, warning, or info message produced by the compiler is represented
/// as a `Diagnostic`. The JSON representation is the primary machine-readable
/// interface for tooling and AI agents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Error code, e.g. "E001", "E002".
    pub code: String,
    /// Byte-offset span into the source text.
    pub span: (usize, usize),
    /// Severity level.
    pub severity: Severity,
    /// Human-readable error message.
    pub message: String,
    /// Optional fix suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Diagnostic {
    /// Create a new error diagnostic.
    pub fn error(code: impl Into<String>, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            span: (span.start, span.end),
            severity: Severity::Error,
            message: message.into(),
            suggestion: None,
        }
    }

    /// Attach a suggestion to this diagnostic.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Serialize this diagnostic to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Diagnostic serialization failed")
    }
}

/// Serialize a slice of diagnostics to a JSON array string.
pub fn diagnostics_to_json(diagnostics: &[Diagnostic]) -> String {
    serde_json::to_string_pretty(diagnostics).expect("Diagnostics serialization failed")
}

/// Render diagnostics as human-readable output using ariadne.
///
/// Writes colored, annotated source snippets to stderr.
pub fn render_ariadne(diagnostics: &[Diagnostic], source: &str, filename: &str) {
    for diag in diagnostics {
        let kind = match diag.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
            Severity::Info => ReportKind::Advice,
        };

        let color = match diag.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
            Severity::Info => Color::Blue,
        };

        let mut builder =
            Report::build(kind, (filename, diag.span.0..diag.span.1))
                .with_code(&diag.code)
                .with_message(&diag.message)
                .with_label(
                    Label::new((filename, diag.span.0..diag.span.1))
                        .with_message(&diag.message)
                        .with_color(color),
                );

        if let Some(ref suggestion) = diag.suggestion {
            builder = builder.with_help(suggestion);
        }

        let report = builder.finish();
        // Print to stderr; ignore write errors (e.g. broken pipe)
        let _ = report.print((filename, Source::from(source)));
    }
}
