pub mod token;
pub mod span;
pub mod ast;
pub mod parser;
pub mod diagnostic;
pub mod types;
pub mod dsp;
pub mod resolve;
pub mod codegen;

// Re-export primary public API
pub use ast::PluginDef;
pub use diagnostic::{Diagnostic, Severity, diagnostics_to_json, render_ariadne};
pub use parser::{parse, parse_to_diagnostics};
pub use resolve::{ResolvedPlugin, resolve_plugin};
pub use dsp::{DspRegistry, builtin_registry};
pub use types::DspType;
pub use codegen::generate_plugin;

/// Convenience function: parse source and emit diagnostics.
///
/// - If `json_output` is true, prints JSON diagnostics to stdout.
/// - Otherwise, renders human-readable ariadne diagnostics to stderr.
///
/// Returns `true` if the source parsed without errors.
pub fn compile_check(source: &str, filename: &str, json_output: bool) -> bool {
    let (ast, parse_diags) = parse_to_diagnostics(source);

    if !parse_diags.is_empty() {
        if json_output {
            println!("{}", diagnostics_to_json(&parse_diags));
        } else {
            render_ariadne(&parse_diags, source, filename);
        }
        return false;
    }

    let Some(plugin) = ast else { return false };

    let registry = dsp::builtin_registry();
    match resolve::resolve_plugin(&plugin, &registry) {
        Ok(_resolved) => true,
        Err(resolve_diags) => {
            if json_output {
                println!("{}", diagnostics_to_json(&resolve_diags));
            } else {
                render_ariadne(&resolve_diags, source, filename);
            }
            false
        }
    }
}
