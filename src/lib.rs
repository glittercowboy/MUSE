pub mod token;
pub mod span;
pub mod ast;
pub mod parser;
pub mod diagnostic;
pub mod types;
pub mod dsp;
pub mod resolve;
pub mod codegen;
pub mod preview;

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
    /// VST3 plugin ID (e.g. "dev.museaudio.warm-gain").
    pub vst3_id: String,
    /// Plugin version from metadata (defaults to "0.1.0").
    pub version: String,
}

/// Resolve `use` declarations in a parsed plugin, injecting imported `FnDef`
/// items into the AST before semantic analysis.
///
/// For each `UseDecl`, reads the target .muse file relative to `source_dir`,
/// parses it, extracts `FnDef` items, and either filters by `expose` list or
/// prefixes names with the `as` alias. Detects circular imports via the
/// `import_stack` set of canonical paths.
fn resolve_imports(
    plugin: &mut ast::PluginDef,
    source_dir: &std::path::Path,
    import_stack: &mut std::collections::HashSet<std::path::PathBuf>,
) -> Result<(), Vec<Diagnostic>> {
    use std::collections::HashSet;

    // Collect UseDecl items (indices + data) first to avoid borrow issues
    let use_decls: Vec<ast::UseDecl> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let ast::PluginItem::UseDecl(decl) = item {
                Some(decl.clone())
            } else {
                None
            }
        })
        .collect();

    if use_decls.is_empty() {
        return Ok(());
    }

    let mut imported_fns: Vec<(ast::FnDef, span::Span)> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for decl in &use_decls {
        let target_path = source_dir.join(&decl.path);
        let canonical = match target_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                diagnostics.push(Diagnostic::error(
                    "E018",
                    decl.span,
                    format!("import file not found: '{}'", decl.path),
                ).with_suggestion("Check the path is relative to the current file's directory."));
                continue;
            }
        };

        // Circular import detection
        if import_stack.contains(&canonical) {
            diagnostics.push(Diagnostic::error(
                "E018",
                decl.span,
                format!("circular import detected: '{}'", decl.path),
            ).with_suggestion("Break the circular dependency between files."));
            continue;
        }

        let target_source = match std::fs::read_to_string(&target_path) {
            Ok(s) => s,
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    "E018",
                    decl.span,
                    format!("cannot read import file '{}': {}", decl.path, e),
                ));
                continue;
            }
        };

        let (target_ast, target_parse_diags) = parse_to_diagnostics(&target_source);
        if !target_parse_diags.is_empty() {
            diagnostics.push(Diagnostic::error(
                "E018",
                decl.span,
                format!("parse errors in imported file '{}'", decl.path),
            ));
            continue;
        }

        let Some(mut target_plugin) = target_ast else {
            diagnostics.push(Diagnostic::error(
                "E018",
                decl.span,
                format!("failed to parse imported file '{}'", decl.path),
            ));
            continue;
        };

        // Recursively resolve imports in the target file
        let target_dir = target_path.parent().unwrap_or(std::path::Path::new("."));
        import_stack.insert(canonical);
        if let Err(mut errs) = resolve_imports(&mut target_plugin, target_dir, import_stack) {
            diagnostics.append(&mut errs);
            continue;
        }

        // Extract FnDef items from the target plugin
        let all_fns: Vec<ast::FnDef> = target_plugin
            .items
            .iter()
            .filter_map(|(item, _)| {
                if let ast::PluginItem::FnDef(fn_def) = item {
                    Some(fn_def.clone())
                } else {
                    None
                }
            })
            .collect();

        let available_names: HashSet<&str> = all_fns.iter().map(|f| f.name.as_str()).collect();

        if let Some(ref alias) = decl.alias {
            // `use "..." as namespace` — import all fns with prefix
            for mut fn_def in all_fns {
                fn_def.name = format!("{}_{}", alias, fn_def.name);
                imported_fns.push((fn_def, decl.span));
            }
        } else {
            // `use "..." expose name1, name2` — import only specified fns
            for name in &decl.expose {
                if !available_names.contains(name.as_str()) {
                    diagnostics.push(Diagnostic::error(
                        "E018",
                        decl.span,
                        format!(
                            "name '{}' not found in '{}' — available functions: {}",
                            name,
                            decl.path,
                            if available_names.is_empty() {
                                "(none)".to_string()
                            } else {
                                available_names.iter().copied().collect::<Vec<_>>().join(", ")
                            }
                        ),
                    ));
                    continue;
                }
                if let Some(fn_def) = all_fns.iter().find(|f| f.name == *name) {
                    imported_fns.push((fn_def.clone(), decl.span));
                }
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    // Inject imported FnDefs into the plugin's items
    for (fn_def, span) in imported_fns {
        plugin.items.push((ast::PluginItem::FnDef(fn_def), span));
    }

    Ok(())
}

/// Full compilation pipeline: parse → resolve imports → resolve → generate Rust/nih-plug crate.
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

    let Some(mut plugin) = ast else {
        return Err(vec![Diagnostic::error(
            "E001",
            span::Span::new(0, source.len()),
            "Failed to produce AST from source",
        )]);
    };

    // Resolve imports before semantic analysis
    let source_dir = std::path::Path::new(_filename)
        .parent()
        .map(|p| if p.as_os_str().is_empty() { std::path::Path::new(".") } else { p })
        .unwrap_or(std::path::Path::new("."));
    let mut import_stack = std::collections::HashSet::new();
    if let Ok(canonical) = std::path::Path::new(_filename).canonicalize() {
        import_stack.insert(canonical);
    }
    resolve_imports(&mut plugin, source_dir, &mut import_stack)?;

    let plugin = plugin; // rebind as immutable

    // Extract metadata before resolve consumes the plugin reference
    let plugin_name = plugin.name.clone();
    let package_name = codegen::cargo::plugin_name_to_package(&plugin_name);
    let clap_id = extract_clap_id(&plugin).unwrap_or_default();
    let vst3_id = extract_vst3_id(&plugin).unwrap_or_default();
    let version = extract_version(&plugin).unwrap_or_else(|| "0.1.0".to_string());

    let registry = dsp::builtin_registry();
    let resolved = resolve::resolve_plugin(&plugin, &registry)?;

    // Compute source directory for resolving relative sample paths
    let source_dir = std::path::Path::new(_filename)
        .parent()
        .map(|p| if p.as_os_str().is_empty() { std::path::Path::new(".") } else { p })
        .unwrap_or(std::path::Path::new("."));

    let crate_dir = output_dir.join(&package_name);
    codegen::generate_plugin(&resolved, &registry, &crate_dir, Some(source_dir))?;

    Ok(CompileResult {
        crate_dir,
        plugin_name,
        package_name,
        clap_id,
        vst3_id,
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

/// Extract the VST3 ID from a plugin's AST.
pub fn extract_vst3_id(plugin: &ast::PluginDef) -> Option<String> {
    for (item, _) in &plugin.items {
        if let ast::PluginItem::FormatBlock(ast::FormatBlock::Vst3(vst3)) = item {
            for (vst3_item, _) in &vst3.items {
                if let ast::Vst3Item::Id(id) = vst3_item {
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

/// Output from a successful [`build_plugin()`] run.
#[derive(Debug, Clone)]
pub struct BuildOutput {
    /// Path to the built cdylib (`.dylib` on macOS).
    pub dylib_path: std::path::PathBuf,
    /// Captured cargo stderr (contains warnings even on success).
    pub stderr: String,
}

/// Run `cargo build --release` in the generated crate directory.
///
/// Returns a [`BuildOutput`] containing the dylib path and captured cargo
/// stderr on success. On failure, returns an error string that includes
/// the cargo stderr text for diagnostic visibility.
pub fn build_plugin(crate_dir: &std::path::Path, package_name: &str) -> Result<BuildOutput, String> {
    let output = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(crate_dir)
        .output()
        .map_err(|e| format!("failed to invoke cargo: {e}"))?;

    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!(
            "cargo build exited with {}\n{}",
            output.status.code().map_or("signal".to_string(), |c| c.to_string()),
            stderr_text,
        ));
    }

    // cdylib naming is platform-specific:
    //   macOS:  lib<name>.dylib
    //   Linux:  lib<name>.so
    //   Windows: <name>.dll
    let lib_name = package_name.replace('-', "_");
    let dylib = crate_dir
        .join("target")
        .join("release")
        .join(format!(
            "{}{lib_name}.{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_EXTENSION
        ));

    if !dylib.exists() {
        return Err(format!("expected dylib not found at {}", dylib.display()));
    }

    Ok(BuildOutput {
        dylib_path: dylib,
        stderr: stderr_text,
    })
}

/// Assemble a macOS .clap bundle directory from a built cdylib.
///
/// Creates:
/// ```text
/// <output_dir>/<Plugin Display Name>.clap/
///   Contents/
///     Info.plist
///     MacOS/
///       <Plugin Display Name>    ← renamed dylib
/// ```
pub fn assemble_clap_bundle(
    output_dir: &std::path::Path,
    dylib_path: &std::path::Path,
    plugin_name: &str,
    clap_id: &str,
    version: &str,
) -> Result<std::path::PathBuf, String> {
    let bundle_dir = output_dir.join(format!("{plugin_name}.clap"));
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");

    std::fs::create_dir_all(&macos_dir)
        .map_err(|e| format!("failed to create bundle directory: {e}"))?;

    // Copy dylib → Contents/MacOS/<Plugin Display Name>
    let binary_dest = macos_dir.join(plugin_name);
    std::fs::copy(dylib_path, &binary_dest)
        .map_err(|e| format!("failed to copy dylib to bundle: {e}"))?;

    // Generate Info.plist
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{plugin_name}</string>
    <key>CFBundleExecutable</key>
    <string>{plugin_name}</string>
    <key>CFBundleIdentifier</key>
    <string>{clap_id}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
</dict>
</plist>
"#
    );
    std::fs::write(contents_dir.join("Info.plist"), plist)
        .map_err(|e| format!("failed to write Info.plist: {e}"))?;

    Ok(bundle_dir)
}

/// Assemble a macOS .vst3 bundle directory from a built cdylib.
///
/// Creates:
/// ```text
/// <output_dir>/<Plugin Display Name>.vst3/
///   Contents/
///     PkgInfo          ← "BNDL????" (8 bytes, nih-plug convention)
///     Info.plist
///     MacOS/
///       <Plugin Display Name>    ← renamed dylib
/// ```
pub fn assemble_vst3_bundle(
    output_dir: &std::path::Path,
    dylib_path: &std::path::Path,
    plugin_name: &str,
    vst3_id: &str,
    version: &str,
) -> Result<std::path::PathBuf, String> {
    let bundle_dir = output_dir.join(format!("{plugin_name}.vst3"));
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");

    std::fs::create_dir_all(&macos_dir)
        .map_err(|e| format!("failed to create VST3 bundle directory: {e}"))?;

    // Copy dylib → Contents/MacOS/<Plugin Display Name>
    let binary_dest = macos_dir.join(plugin_name);
    std::fs::copy(dylib_path, &binary_dest)
        .map_err(|e| format!("failed to copy dylib to VST3 bundle: {e}"))?;

    // Write PkgInfo (nih-plug convention)
    std::fs::write(contents_dir.join("PkgInfo"), b"BNDL????")
        .map_err(|e| format!("failed to write PkgInfo: {e}"))?;

    // Generate Info.plist
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{plugin_name}</string>
    <key>CFBundleExecutable</key>
    <string>{plugin_name}</string>
    <key>CFBundleIdentifier</key>
    <string>{vst3_id}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
</dict>
</plist>
"#
    );
    std::fs::write(contents_dir.join("Info.plist"), plist)
        .map_err(|e| format!("failed to write VST3 Info.plist: {e}"))?;

    Ok(bundle_dir)
}

/// Ad-hoc codesign a macOS bundle (CLAP or VST3).
///
/// Required on Apple Silicon — unsigned bundles won't load in hosts.
/// Uses `codesign --force --sign -` for ad-hoc signing (no developer identity).
pub fn codesign_bundle(bundle_path: &std::path::Path) -> Result<(), String> {
    let output = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-", &bundle_path.display().to_string()])
        .output()
        .map_err(|e| format!("failed to invoke codesign: {e}"))?;

    if !output.status.success() {
        let stderr_text = String::from_utf8_lossy(&output.stderr);
        return Err(format!("codesign failed: {stderr_text}"));
    }

    Ok(())
}

/// Parse and resolve a `.muse` source file and return the GUI editor HTML.
///
/// Runs the pipeline through parse → resolve → `generate_editor_html()` without
/// performing full Rust code generation. Useful for previewing the GUI layout
/// in a standalone viewer.
///
/// Returns `Ok(html_string)` on success, or `Err(diagnostics)` if parse or
/// resolve fails. Returns `Err` with a single diagnostic if the source has no
/// `gui` block.
pub fn preview_html(source: &str, _filename: &str) -> Result<String, Vec<Diagnostic>> {
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

    // Check for gui block before resolve (fast failure path)
    if codegen::gui::find_gui_block(&plugin).is_none() {
        return Err(vec![Diagnostic::error(
            "E010",
            span::Span::new(0, source.len()),
            "No gui block found — nothing to preview",
        )]);
    }

    let registry = dsp::builtin_registry();
    let _resolved = resolve::resolve_plugin(&plugin, &registry)?;

    // Generate the editor HTML directly from the AST
    let html = codegen::gui::generate_editor_html(&plugin);
    Ok(html)
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

    let Some(mut plugin) = ast else { return false };

    // Resolve imports
    let source_dir = std::path::Path::new(filename)
        .parent()
        .map(|p| if p.as_os_str().is_empty() { std::path::Path::new(".") } else { p })
        .unwrap_or(std::path::Path::new("."));
    let mut import_stack = std::collections::HashSet::new();
    if let Ok(canonical) = std::path::Path::new(filename).canonicalize() {
        import_stack.insert(canonical);
    }
    if let Err(import_diags) = resolve_imports(&mut plugin, source_dir, &mut import_stack) {
        if json_output {
            println!("{}", diagnostics_to_json(&import_diags));
        } else {
            render_ariadne(&import_diags, source, filename);
        }
        return false;
    }

    let plugin = plugin;
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
