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
pub use codegen::cargo::plugin_name_to_package;

/// Metadata returned by a successful [`compile()`] run.
///
/// Contains everything downstream tools (CLI, bundler) need to locate
/// the generated crate and assemble a CLAP/VST3 bundle.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Path to the generated Rust crate directory.
    pub crate_dir: std::path::PathBuf,
    /// Plugin display name as declared in source (e.g. "Warm Gain").
    pub plugin_name: String,
    /// Cargo package name derived from plugin name (e.g. "warm-gain").
    pub package_name: String,
    /// CLAP plugin ID (e.g. "dev.museaudio.warm-gain").
    pub clap_id: String,
    /// Plugin version from metadata (defaults to "0.1.0").
    pub version: String,
}

/// Full compilation pipeline: parse → resolve → generate Rust/nih-plug crate.
///
/// Takes source code, a filename (for diagnostics), and an output directory.
/// On success, returns a [`CompileResult`] with the generated crate path and plugin metadata.
/// The crate is placed in `output_dir/<package_name>/`.
/// On failure, returns structured diagnostics (parse, resolve, or codegen errors).
///
/// This runs the complete pipeline. For parse+resolve checking without codegen,
/// use [`compile_check()`].
pub fn compile(
    source: &str,
    _filename: &str,
    output_dir: &std::path::Path,
) -> Result<CompileResult, Vec<Diagnostic>> {
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

    // Extract metadata before resolve consumes the plugin reference
    let plugin_name = plugin.name.clone();
    let package_name = codegen::cargo::plugin_name_to_package(&plugin_name);
    let clap_id = extract_clap_id(&plugin).unwrap_or_default();
    let version = extract_version(&plugin).unwrap_or_else(|| "0.1.0".to_string());

    let registry = dsp::builtin_registry();
    let resolved = resolve::resolve_plugin(&plugin, &registry)?;

    let crate_dir = output_dir.join(&package_name);
    codegen::generate_plugin(&resolved, &registry, &crate_dir)?;

    Ok(CompileResult {
        crate_dir,
        plugin_name,
        package_name,
        clap_id,
        version,
    })
}

/// Extract the CLAP ID from a plugin's AST.
fn extract_clap_id(plugin: &ast::PluginDef) -> Option<String> {
    for (item, _) in &plugin.items {
        if let ast::PluginItem::FormatBlock(ast::FormatBlock::Clap(clap)) = item {
            for (clap_item, _) in &clap.items {
                if let ast::ClapItem::Id(id) = clap_item {
                    return Some(id.clone());
                }
            }
        }
    }
    None
}

/// Extract the version string from a plugin's metadata.
fn extract_version(plugin: &ast::PluginDef) -> Option<String> {
    for (item, _) in &plugin.items {
        if let ast::PluginItem::Metadata(meta) = item {
            if meta.key == ast::MetadataKey::Version {
                return match &meta.value {
                    ast::MetadataValue::StringVal(s) => Some(s.clone()),
                    ast::MetadataValue::Identifier(s) => Some(s.clone()),
                };
            }
        }
    }
    None
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
