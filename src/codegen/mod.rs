//! Code generation orchestrator for Muse → Rust/nih-plug.
//!
//! The `generate_plugin()` function takes a resolved plugin AST and produces
//! a complete, compilable Rust crate in the specified output directory.

pub mod cargo;
pub mod dsp;
pub mod editor;
pub mod gui;
pub mod midi;
pub mod params;
pub mod plugin;
pub mod presets;
pub mod process;
pub mod test;

use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{PluginDef, PluginItem, TestProperty, TestStatement};
use crate::diagnostic::Diagnostic;
use crate::resolve::ResolvedPlugin;
use crate::span::Span;

/// Codegen-side unison configuration extracted from the AST.
#[derive(Debug, Clone)]
pub struct CodegenUnisonConfig {
    pub count: u32,
    pub detune_cents: f64,
}

pub fn generate_plugin(
    resolved: &ResolvedPlugin,
    _registry: &crate::dsp::primitives::DspRegistry,
    output_dir: &Path,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let plugin = resolved.plugin;
    let mut diagnostics = Vec::new();

    validate_codegen_requirements(plugin, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let needs_fft = has_frequency_assertions(plugin);
    let has_gui = gui::find_gui_block(plugin).is_some();
    let cargo_toml = cargo::generate_cargo_toml(plugin, needs_fft, has_gui);
    let params_code = params::generate_params(plugin);
    let preset_code = presets::generate_presets(plugin);
    let voice_count = find_voice_count(plugin);
    let unison_config = find_unison_config(plugin);
    let (process_body, process_info) = process::generate_process(plugin, voice_count, unison_config.as_ref());

    if !process_info.diagnostics.is_empty() {
        return Err(process_info.diagnostics);
    }

    let dsp_helpers = dsp::generate_dsp_helpers(&process_info.used_primitives);
    let plugin_code = plugin::generate_plugin_struct(plugin, &process_info);
    let plugin_code = plugin_code.replace("{PROCESS_BODY}", &process_body);

    let mut lib_rs = assemble_lib_rs(&params_code, &preset_code, &dsp_helpers, &plugin_code, voice_count.is_some(), unison_config.as_ref());

    let test_module = test::generate_test_module(plugin, &process_info);
    if !test_module.is_empty() {
        lib_rs.push_str(&test_module);
    }

    // GUI editor module: generate HTML assets and Rust editor code
    let mut editor_html: Option<String> = None;
    if let Some(gui_block) = gui::find_gui_block(plugin) {
        let html = gui::generate_editor_html(plugin);
        let editor_module = editor::generate_editor_module(plugin, gui_block);
        lib_rs.push_str(&editor_module);
        editor_html = Some(html);
    }

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

    // Write GUI editor assets if present
    if let Some(html) = editor_html {
        let assets_dir = crate_dir.join("assets");
        fs::create_dir_all(&assets_dir).map_err(|e| {
            vec![Diagnostic::error(
                "E010",
                Span::new(0, 0),
                format!("Failed to create assets directory: {}", e),
            )]
        })?;
        fs::write(assets_dir.join("editor.html"), &html).map_err(|e| {
            vec![Diagnostic::error(
                "E010",
                Span::new(0, 0),
                format!("Failed to write assets/editor.html: {}", e),
            )]
        })?;
    }

    Ok(crate_dir)
}

fn find_voice_count(plugin: &PluginDef) -> Option<u32> {
    plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::VoiceDecl(voice) = item {
            Some(voice.count)
        } else {
            None
        }
    })
}

fn find_unison_config(plugin: &PluginDef) -> Option<CodegenUnisonConfig> {
    plugin.items.iter().find_map(|(item, _)| {
        if let PluginItem::UnisonDecl(unison) = item {
            Some(CodegenUnisonConfig {
                count: unison.count,
                detune_cents: unison.detune_cents,
            })
        } else {
            None
        }
    })
}

fn validate_codegen_requirements(plugin: &PluginDef, diagnostics: &mut Vec<Diagnostic>) {
    use crate::ast::{FormatBlock, MetadataKey, PluginItem};

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
            Diagnostic::error(
                "E010",
                plugin.span,
                "Missing required 'vendor' metadata for code generation",
            )
            .with_suggestion("Add: vendor \"Your Name\""),
        );
    }
    if !has_clap {
        diagnostics.push(
            Diagnostic::error(
                "E010",
                plugin.span,
                "Missing required 'clap' block for code generation",
            )
            .with_suggestion("Add a clap { id \"...\" description \"...\" features [...] } block"),
        );
    }
    if !has_vst3 {
        diagnostics.push(
            Diagnostic::error(
                "E010",
                plugin.span,
                "Missing required 'vst3' block for code generation",
            )
            .with_suggestion("Add a vst3 { id \"...\" subcategories [...] } block"),
        );
    }
    if !has_io_in || !has_io_out {
        diagnostics.push(
            Diagnostic::error(
                "E010",
                plugin.span,
                "Missing input/output declarations for code generation",
            )
            .with_suggestion("Add: input stereo / output stereo"),
        );
    }
    if !has_process {
        diagnostics.push(
            Diagnostic::error(
                "E010",
                plugin.span,
                "Missing process block for code generation",
            )
            .with_suggestion("Add: process { input -> ... -> output }"),
        );
    }
}

fn assemble_lib_rs(
    params_code: &str,
    preset_code: &str,
    dsp_helpers: &str,
    plugin_code: &str,
    include_poly_helpers: bool,
    unison_config: Option<&CodegenUnisonConfig>,
) -> String {
    let mut out = String::new();

    out.push_str(
        "use nih_plug::prelude::*;\nuse nih_plug::params::FloatParam;\nuse nih_plug::params::IntParam;\nuse nih_plug::params::BoolParam;\nuse nih_plug::params::EnumParam;\nuse nih_plug::params::Params;\nuse nih_plug::params::range::{FloatRange, IntRange};\nuse nih_plug::params::smoothing::SmoothingStyle;\nuse nih_plug::formatters;\nuse nih_plug::util;\nuse nih_plug::{nih_export_clap, nih_export_vst3};\nuse std::sync::Arc;\n\n",
    );
    out.push_str(params_code);
    out.push('\n');

    if !preset_code.is_empty() {
        out.push_str(preset_code);
        out.push('\n');
    }

    out.push_str("const MAX_BLOCK_SIZE: usize = 64;\n\n");

    if unison_config.is_some() {
        out.push_str("const UNISON_MAX: i32 = 16;\n\n");
    }

    if !dsp_helpers.is_empty() {
        out.push_str(dsp_helpers);
        out.push('\n');
    }

    if include_poly_helpers {
        out.push_str(
            "fn voice_is_silent(voice: &Voice) -> bool {\n    if let Some(level) = voice_adsr_level(voice) {\n        level <= 0.0001\n    } else {\n        false\n    }\n}\n\n",
        );
        out.push_str(
            "fn voice_adsr_level(voice: &Voice) -> Option<f32> {\n    Some(voice.adsr_state.level)\n}\n\n",
        );
    }

    out.push_str(plugin_code);
    out
}

/// Check if any test block contains a frequency assertion (requiring rustfft dev-dependency).
fn has_frequency_assertions(plugin: &PluginDef) -> bool {
    plugin.items.iter().any(|(item, _)| {
        if let PluginItem::TestBlock(tb) = item {
            tb.statements.iter().any(|(stmt, _)| {
                matches!(
                    stmt,
                    TestStatement::Assert(a) if matches!(a.property, TestProperty::Frequency(_))
                )
            })
        } else {
            false
        }
    })
}
