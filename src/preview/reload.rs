//! Hot-reload orchestrator for live preview.
//!
//! `ReloadPipeline` manages the compile → cargo build → codesign → load cycle.
//! On initial build it returns a fresh `HostPlugin`. On reload, it preserves
//! parameter state across the swap.

use super::host_plugin::HostPlugin;
use crate::{compile, diagnostics_to_json, render_ariadne, CompileResult, Diagnostic};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Errors that can occur during the reload pipeline.
#[derive(Debug)]
pub enum ReloadError {
    CompileError(Vec<Diagnostic>),
    BuildError(String),
    CodesignError(String),
    LoadError(String),
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReloadError::CompileError(diags) => {
                write!(f, "compile error: {} diagnostic(s)", diags.len())
            }
            ReloadError::BuildError(msg) => write!(f, "build error: {msg}"),
            ReloadError::CodesignError(msg) => write!(f, "codesign error: {msg}"),
            ReloadError::LoadError(msg) => write!(f, "load error: {msg}"),
        }
    }
}

/// Manages the compile→build→codesign→load pipeline for hot-reload.
pub struct ReloadPipeline {
    /// Temp directory for generated crates. Persists across reloads so
    /// incremental compilation speeds up subsequent builds.
    output_dir: PathBuf,
}

impl ReloadPipeline {
    pub fn new() -> Result<Self, String> {
        let output_dir = std::env::temp_dir().join("muse-preview");
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("failed to create preview temp dir: {e}"))?;
        Ok(Self { output_dir })
    }

    /// Run the full pipeline from scratch: compile → cargo build --features preview → codesign → load.
    pub fn initial_build(
        &self,
        source_path: &Path,
        sample_rate: f32,
    ) -> Result<(HostPlugin, CompileResult), ReloadError> {
        let source = std::fs::read_to_string(source_path).map_err(|e| {
            ReloadError::BuildError(format!(
                "cannot read '{}': {e}",
                source_path.display()
            ))
        })?;
        let filename = source_path.display().to_string();

        // Phase 1: compile .muse → Rust crate
        let t0 = Instant::now();
        let result =
            compile(&source, &filename, &self.output_dir).map_err(ReloadError::CompileError)?;
        let compile_ms = t0.elapsed().as_millis();

        // Phase 2: cargo build --features preview (debug mode for speed)
        let t1 = Instant::now();
        let dylib_path =
            self.cargo_build_preview(&result.crate_dir, &result.package_name)?;
        let build_ms = t1.elapsed().as_millis();

        // Phase 3: codesign the dylib (required on Apple Silicon)
        let t2 = Instant::now();
        self.codesign_dylib(&dylib_path)?;
        let sign_ms = t2.elapsed().as_millis();

        // Phase 4: load the dylib
        let t3 = Instant::now();
        let plugin =
            HostPlugin::load(&dylib_path, sample_rate).map_err(ReloadError::LoadError)?;
        let load_ms = t3.elapsed().as_millis();

        eprintln!(
            "[muse preview] built '{}' (compile: {compile_ms}ms, cargo: {build_ms}ms, sign: {sign_ms}ms, load: {load_ms}ms)",
            result.plugin_name
        );

        Ok((plugin, result))
    }

    /// Reload: recompile, rebuild, codesign, and load a new plugin.
    ///
    /// `param_snapshot` is an optional set of `(index, value)` pairs captured
    /// from the currently-running plugin *before* calling this method.
    /// If the new plugin has the same param count, the snapshot is restored;
    /// otherwise defaults are used.
    pub fn reload(
        &self,
        source_path: &Path,
        sample_rate: f32,
        param_snapshot: Option<(u32, Vec<(u32, f32)>)>,
    ) -> Result<HostPlugin, ReloadError> {
        let source = std::fs::read_to_string(source_path).map_err(|e| {
            ReloadError::BuildError(format!(
                "cannot read '{}': {e}",
                source_path.display()
            ))
        })?;
        let filename = source_path.display().to_string();

        // Phase 1: compile
        let t0 = Instant::now();
        let result =
            compile(&source, &filename, &self.output_dir).map_err(ReloadError::CompileError)?;
        let compile_ms = t0.elapsed().as_millis();

        // Phase 2: cargo build --features preview
        let t1 = Instant::now();
        let dylib_path =
            self.cargo_build_preview(&result.crate_dir, &result.package_name)?;
        let build_ms = t1.elapsed().as_millis();

        // Phase 3: codesign
        let t2 = Instant::now();
        self.codesign_dylib(&dylib_path)?;
        let sign_ms = t2.elapsed().as_millis();

        // Phase 4: load
        let t3 = Instant::now();
        let new_plugin =
            HostPlugin::load(&dylib_path, sample_rate).map_err(ReloadError::LoadError)?;
        let load_ms = t3.elapsed().as_millis();

        // Phase 5: restore parameter state
        if let Some((old_count, snapshot)) = param_snapshot {
            if new_plugin.param_count() == old_count {
                new_plugin.restore_params(&snapshot);
            } else {
                eprintln!(
                    "[muse preview] param count changed ({old_count} → {}), using defaults",
                    new_plugin.param_count()
                );
            }
        }

        eprintln!(
            "[muse preview] reloaded (compile: {compile_ms}ms, cargo: {build_ms}ms, sign: {sign_ms}ms, load: {load_ms}ms)",
        );

        Ok(new_plugin)
    }

    /// Run `cargo build --features preview` in the generated crate (debug mode).
    ///
    /// Returns the path to the built dylib.
    fn cargo_build_preview(
        &self,
        crate_dir: &Path,
        package_name: &str,
    ) -> Result<PathBuf, ReloadError> {
        let output = std::process::Command::new("cargo")
            .args(["build", "--features", "preview"])
            .current_dir(crate_dir)
            .output()
            .map_err(|e| {
                ReloadError::BuildError(format!("failed to invoke cargo: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ReloadError::BuildError(format!(
                "cargo build failed:\n{stderr}"
            )));
        }

        // Debug mode dylib: target/debug/lib<name>.dylib
        let lib_name = package_name.replace('-', "_");
        let dylib = crate_dir
            .join("target")
            .join("debug")
            .join(format!("lib{lib_name}.dylib"));

        if !dylib.exists() {
            return Err(ReloadError::BuildError(format!(
                "expected dylib not found at {}",
                dylib.display()
            )));
        }

        Ok(dylib)
    }

    /// Ad-hoc codesign a dylib (required on Apple Silicon).
    fn codesign_dylib(&self, dylib_path: &Path) -> Result<(), ReloadError> {
        let output = std::process::Command::new("codesign")
            .args([
                "--force",
                "--sign",
                "-",
                &dylib_path.display().to_string(),
            ])
            .output()
            .map_err(|e| {
                ReloadError::CodesignError(format!("failed to invoke codesign: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ReloadError::CodesignError(format!(
                "codesign failed: {stderr}"
            )));
        }

        Ok(())
    }

    /// Format a ReloadError for display.
    /// For compile errors, uses ariadne with the source text.
    pub fn format_error(
        &self,
        error: &ReloadError,
        source_path: &Path,
        json_format: bool,
    ) {
        match error {
            ReloadError::CompileError(diags) => {
                if json_format {
                    let json = serde_json::json!({
                        "event": "error",
                        "phase": "compile",
                        "diagnostics": diagnostics_to_json(diags),
                    });
                    println!("{}", serde_json::to_string(&json).unwrap());
                } else {
                    // Re-read source for ariadne rendering
                    if let Ok(source) = std::fs::read_to_string(source_path) {
                        let filename = source_path.display().to_string();
                        render_ariadne(diags, &source, &filename);
                    } else {
                        eprintln!(
                            "[muse preview] compile error: {} diagnostic(s)",
                            diags.len()
                        );
                        for d in diags {
                            eprintln!("  {}: {}", d.code, d.message);
                        }
                    }
                }
            }
            other => {
                if json_format {
                    let json = serde_json::json!({
                        "event": "error",
                        "message": other.to_string(),
                    });
                    println!("{}", serde_json::to_string(&json).unwrap());
                } else {
                    eprintln!("[muse preview] {other}");
                }
            }
        }
    }
}
