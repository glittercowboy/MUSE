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

/// Full compilation pipeline: parse → resolve → generate Rust/nih-plug crate.
///
/// Takes source code, a filename (for diagnostics), and an output directory.
/// On success, returns the path to the generated crate directory.
/// On failure, returns structured diagnostics (parse, resolve, or codegen errors).
///
/// This runs the complete pipeline. For parse+resolve checking without codegen,
/// use [`compile_check()`].
pub fn compile(
    source: &str,
    _filename: &str,
    output_dir: &std::path::Path,
) -> Result<std::path::PathBuf, Vec<Diagnostic>> {
    let (ast, parse_diags) = parse_to_diagnostics(source);

    if !parse_diags.is_empty() {
        return Err(parse_diags);
    }

    let Some(plugin) = ast else {
        return Err(vec![Diagnostic::error(
            "E001",
            span::Span::new(0, source.len()),
            "Failed to produce AST from source",
        )]);
    };

    let registry = dsp::builtin_registry();
    let resolved = resolve::resolve_plugin(&plugin, &registry)?;

    codegen::generate_plugin(&resolved, &registry, output_dir)
}

/// Convenience function: parse source and emit diagnostics.
///
/// - If `json_output` is true, prints JSON diagnostics to stdout.
/// - Otherwise, renders human-readable ariadne diagnostics to stderr.
///
/// Returns `true` if the source parsed without errors.
/// For full compilation to Rust/nih-plug, use [`compile()`].
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
