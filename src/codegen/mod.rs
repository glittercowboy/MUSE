//! Code generation orchestrator for Muse → Rust/nih-plug.
//!
//! The `generate_plugin()` function takes a resolved plugin AST and produces
//! a complete, compilable Rust crate in the specified output directory.

pub mod cargo;
pub mod dsp;
pub mod params;
pub mod plugin;
pub mod process;

use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::PluginDef;
use crate::diagnostic::Diagnostic;
use crate::resolve::ResolvedPlugin;
use crate::span::Span;

/// Generate a complete Rust/nih-plug crate from a resolved plugin.
///
/// On success, returns the path to the generated crate directory.
/// On failure, returns structured diagnostics with E010+ codes.
///
/// The generated crate is a standalone Rust project with:
/// - `Cargo.toml` — package config with nih-plug dependency
/// - `src/lib.rs` — plugin struct, params, trait impls, process body, export macros
pub fn generate_plugin(
    resolved: &ResolvedPlugin,
    _registry: &crate::dsp::primitives::DspRegistry,
    output_dir: &Path,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let plugin = resolved.plugin;
    let mut diagnostics = Vec::new();

    // Validate required metadata for codegen
    validate_codegen_requirements(plugin, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    // Generate all code fragments
    let cargo_toml = cargo::generate_cargo_toml(plugin);
    let params_code = params::generate_params(plugin);

    // Generate process body — also collects which DSP primitives are used
    let (process_body, used_primitives) = process::generate_process(plugin);

    // Generate DSP helpers based on which primitives are actually used
    let dsp_helpers = dsp::generate_dsp_helpers(&used_primitives);

    // Generate plugin struct with DSP state fields based on used primitives
    let plugin_code = plugin::generate_plugin_struct(plugin, &used_primitives);

    // Replace the process body placeholder in the plugin code
    let plugin_code = plugin_code.replace("{PROCESS_BODY}", &process_body);

    // Assemble the full lib.rs
    let lib_rs = assemble_lib_rs(&params_code, &dsp_helpers, &plugin_code);

    // Write files to disk
    let crate_dir = output_dir.to_path_buf();
    let src_dir = crate_dir.join("src");

    fs::create_dir_all(&src_dir).map_err(|e| {
        vec![Diagnostic::error(
            "E010",
            Span::new(0, 0),
            format!("Failed to create output directory: {}", e),
        )]
    })?;

    fs::write(crate_dir.join("Cargo.toml"), &cargo_toml).map_err(|e| {
        vec![Diagnostic::error(
            "E010",
            Span::new(0, 0),
            format!("Failed to write Cargo.toml: {}", e),
        )]
    })?;

    fs::write(src_dir.join("lib.rs"), &lib_rs).map_err(|e| {
        vec![Diagnostic::error(
            "E010",
            Span::new(0, 0),
            format!("Failed to write src/lib.rs: {}", e),
        )]
    })?;

    Ok(crate_dir)
}

/// Validate that all metadata required for codegen is present.
fn validate_codegen_requirements(plugin: &PluginDef, diagnostics: &mut Vec<Diagnostic>) {
    use crate::ast::{MetadataKey, PluginItem, FormatBlock};

    let mut has_vendor = false;
    let mut has_clap = false;
    let mut has_vst3 = false;
    let mut has_io_in = false;
    let mut has_io_out = false;
    let mut has_process = false;

    for (item, _) in &plugin.items {
        match item {
            PluginItem::Metadata(m) if m.key == MetadataKey::Vendor => has_vendor = true,
            PluginItem::FormatBlock(FormatBlock::Clap(_)) => has_clap = true,
            PluginItem::FormatBlock(FormatBlock::Vst3(_)) => has_vst3 = true,
            PluginItem::IoDecl(io) => match io.direction {
                crate::ast::IoDirection::Input => has_io_in = true,
                crate::ast::IoDirection::Output => has_io_out = true,
            },
            PluginItem::ProcessBlock(_) => has_process = true,
            _ => {}
        }
    }

    if !has_vendor {
        diagnostics.push(
            Diagnostic::error("E010", plugin.span, "Missing required 'vendor' metadata for code generation")
                .with_suggestion("Add: vendor \"Your Name\""),
        );
    }
    if !has_clap {
        diagnostics.push(
            Diagnostic::error("E010", plugin.span, "Missing required 'clap' block for code generation")
                .with_suggestion("Add a clap { id \"...\" description \"...\" features [...] } block"),
        );
    }
    if !has_vst3 {
        diagnostics.push(
            Diagnostic::error("E010", plugin.span, "Missing required 'vst3' block for code generation")
                .with_suggestion("Add a vst3 { id \"...\" subcategories [...] } block"),
        );
    }
    if !has_io_in || !has_io_out {
        diagnostics.push(
            Diagnostic::error("E010", plugin.span, "Missing input/output declarations for code generation")
                .with_suggestion("Add: input stereo / output stereo"),
        );
    }
    if !has_process {
        diagnostics.push(
            Diagnostic::error("E010", plugin.span, "Missing process block for code generation")
                .with_suggestion("Add: process { input -> ... -> output }"),
        );
    }
}

/// Assemble the final lib.rs content from generated fragments.
fn assemble_lib_rs(params_code: &str, dsp_helpers: &str, plugin_code: &str) -> String {
    let mut out = String::new();

    // Prelude imports
    out.push_str("use nih_plug::prelude::*;\nuse std::sync::Arc;\n\n");

    // Params struct
    out.push_str(params_code);
    out.push('\n');

    // DSP helpers (may be empty)
    if !dsp_helpers.is_empty() {
        out.push_str(dsp_helpers);
        out.push('\n');
    }

    // Plugin struct + trait impls + export macros
    out.push_str(plugin_code);

    out
}
