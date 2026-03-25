pub mod token;
pub mod span;
pub mod ast;
pub mod parser;
pub mod diagnostic;
pub mod types;
pub mod dsp;

// Re-export primary public API
pub use ast::PluginDef;
pub use diagnostic::{Diagnostic, Severity, diagnostics_to_json, render_ariadne};
pub use parser::{parse, parse_to_diagnostics};

/// Convenience function: parse source and emit diagnostics.
///
/// - If `json_output` is true, prints JSON diagnostics to stdout.
/// - Otherwise, renders human-readable ariadne diagnostics to stderr.
///
/// Returns `true` if the source parsed without errors.
pub fn compile_check(source: &str, filename: &str, json_output: bool) -> bool {
    let (ast, diagnostics) = parse_to_diagnostics(source);

    if diagnostics.is_empty() {
        return ast.is_some();
    }

    if json_output {
        println!("{}", diagnostics_to_json(&diagnostics));
    } else {
        render_ariadne(&diagnostics, source, filename);
    }

    false
}
