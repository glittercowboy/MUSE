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
    /// VST3 plugin ID (e.g. "dev.museaudio.warm-gain").
    pub vst3_id: String,
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
    let vst3_id = extract_vst3_id(&plugin).unwrap_or_default();
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

    // macOS cdylib naming: lib<name>.dylib where <name> has hyphens → underscores
    let lib_name = package_name.replace('-', "_");
    let dylib = crate_dir
        .join("target")
        .join("release")
        .join(format!("lib{lib_name}.dylib"));

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
