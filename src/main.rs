//! Muse compiler CLI.
//!
//! Usage:
//!   muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]
//!   muse check <file> [--format json]
//!
//! Exit codes:
//!   0 — success
//!   1 — compile/check error (diagnostics emitted)
//!   2 — build error (cargo build failed)

use std::path::{Path, PathBuf};
use std::process;

use muse_lang::{compile, compile_check, diagnostics_to_json, render_ariadne};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(2);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "--help" | "-h" | "help" => {
            print_usage();
            process::exit(0);
        }
        other => {
            eprintln!("muse: unknown command '{other}'");
            eprintln!();
            print_usage();
            process::exit(2);
        }
    }
}

/// Parsed CLI options for the compile subcommand.
struct CompileOpts {
    file: PathBuf,
    output_dir: PathBuf,
    json_format: bool,
    no_build: bool,
    #[allow(dead_code)]
    release: bool,
}

/// Parsed CLI options for the check subcommand.
struct CheckOpts {
    file: PathBuf,
    json_format: bool,
}

fn parse_compile_args(args: &[String]) -> Result<CompileOpts, String> {
    if args.is_empty() {
        return Err("muse compile: missing <file> argument".into());
    }

    let mut file: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut json_format = false;
    let mut no_build = false;
    let mut release = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--output-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output-dir requires a value".into());
                }
                output_dir = Some(PathBuf::from(&args[i]));
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err("--format requires a value".into());
                }
                match args[i].as_str() {
                    "json" => json_format = true,
                    other => return Err(format!("unknown format '{other}', expected 'json'")),
                }
            }
            "--no-build" => no_build = true,
            "--release" => release = true,
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option '{arg}'"));
            }
            _ => {
                if file.is_some() {
                    return Err(format!("unexpected argument '{}'", args[i]));
                }
                file = Some(PathBuf::from(&args[i]));
            }
        }
        i += 1;
    }

    let file = file.ok_or("muse compile: missing <file> argument")?;

    // Default output directory: current working directory
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    Ok(CompileOpts {
        file,
        output_dir,
        json_format,
        no_build,
        release,
    })
}

fn parse_check_args(args: &[String]) -> Result<CheckOpts, String> {
    if args.is_empty() {
        return Err("muse check: missing <file> argument".into());
    }

    let mut file: Option<PathBuf> = None;
    let mut json_format = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err("--format requires a value".into());
                }
                match args[i].as_str() {
                    "json" => json_format = true,
                    other => return Err(format!("unknown format '{other}', expected 'json'")),
                }
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option '{arg}'"));
            }
            _ => {
                if file.is_some() {
                    return Err(format!("unexpected argument '{}'", args[i]));
                }
                file = Some(PathBuf::from(&args[i]));
            }
        }
        i += 1;
    }

    let file = file.ok_or("muse check: missing <file> argument")?;

    Ok(CheckOpts { file, json_format })
}

fn cmd_compile(args: &[String]) {
    let opts = match parse_compile_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("muse: {e}");
            process::exit(2);
        }
    };

    let source = match std::fs::read_to_string(&opts.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("muse: cannot read '{}': {e}", opts.file.display());
            process::exit(2);
        }
    };

    let filename = opts.file.display().to_string();

    match compile(&source, &filename, &opts.output_dir) {
        Ok(result) => {
            if opts.no_build {
                // --no-build: just report the generated crate location
                if opts.json_format {
                    let json = serde_json::json!({
                        "status": "ok",
                        "plugin_name": result.plugin_name,
                        "package_name": result.package_name,
                        "clap_id": result.clap_id,
                        "version": result.version,
                        "crate_dir": result.crate_dir.display().to_string(),
                    });
                    println!("{}", serde_json::to_string_pretty(&json).unwrap());
                } else {
                    eprintln!(
                        "Generated crate for '{}' at {}",
                        result.plugin_name,
                        result.crate_dir.display()
                    );
                }
                process::exit(0);
            }

            // Full build: cargo build → bundle assembly
            let dylib_path = match build_plugin(&result.crate_dir, &result.package_name) {
                Ok(p) => p,
                Err(e) => {
                    if opts.json_format {
                        let json = serde_json::json!({
                            "status": "error",
                            "phase": "build",
                            "message": e,
                        });
                        println!("{}", serde_json::to_string_pretty(&json).unwrap());
                    } else {
                        eprintln!("muse: build failed: {e}");
                    }
                    process::exit(2);
                }
            };

            let bundle_path = match assemble_clap_bundle(
                &opts.output_dir,
                &dylib_path,
                &result.plugin_name,
                &result.clap_id,
                &result.version,
            ) {
                Ok(p) => p,
                Err(e) => {
                    if opts.json_format {
                        let json = serde_json::json!({
                            "status": "error",
                            "phase": "bundle",
                            "message": e,
                        });
                        println!("{}", serde_json::to_string_pretty(&json).unwrap());
                    } else {
                        eprintln!("muse: bundle assembly failed: {e}");
                    }
                    process::exit(2);
                }
            };

            if opts.json_format {
                let json = serde_json::json!({
                    "status": "ok",
                    "plugin_name": result.plugin_name,
                    "package_name": result.package_name,
                    "clap_id": result.clap_id,
                    "version": result.version,
                    "crate_dir": result.crate_dir.display().to_string(),
                    "bundle_path": bundle_path.display().to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!(
                    "Built '{}' → {}",
                    result.plugin_name,
                    bundle_path.display()
                );
            }
            process::exit(0);
        }
        Err(diags) => {
            if opts.json_format {
                println!("{}", diagnostics_to_json(&diags));
            } else {
                render_ariadne(&diags, &source, &filename);
            }
            process::exit(1);
        }
    }
}

fn cmd_check(args: &[String]) {
    let opts = match parse_check_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("muse: {e}");
            process::exit(2);
        }
    };

    let source = match std::fs::read_to_string(&opts.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("muse: cannot read '{}': {e}", opts.file.display());
            process::exit(2);
        }
    };

    let filename = opts.file.display().to_string();
    let ok = compile_check(&source, &filename, opts.json_format);

    if ok {
        if opts.json_format {
            println!(r#"{{"status":"ok"}}"#);
        }
        process::exit(0);
    } else {
        process::exit(1);
    }
}

/// Run `cargo build --release` in the generated crate directory.
///
/// Returns the path to the built cdylib on success. On macOS the cdylib
/// is at `<crate_dir>/target/release/lib<underscored_name>.dylib`.
fn build_plugin(crate_dir: &Path, package_name: &str) -> Result<PathBuf, String> {
    let status = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(crate_dir)
        .status()
        .map_err(|e| format!("failed to invoke cargo: {e}"))?;

    if !status.success() {
        return Err(format!(
            "cargo build exited with {}",
            status.code().map_or("signal".to_string(), |c| c.to_string())
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

    Ok(dylib)
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
fn assemble_clap_bundle(
    output_dir: &Path,
    dylib_path: &Path,
    plugin_name: &str,
    clap_id: &str,
    version: &str,
) -> Result<PathBuf, String> {
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

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]");
    eprintln!("  muse check <file> [--format json]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile    Parse, resolve, and generate a Rust/nih-plug crate from a .muse file");
    eprintln!("  check      Parse and resolve a .muse file, reporting any errors");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --output-dir <dir>  Directory to place generated crate (default: current dir)");
    eprintln!("  --format json       Output structured JSON diagnostics instead of human-readable");
    eprintln!("  --no-build          Generate Rust crate only, skip cargo build");
    eprintln!("  --release           Build in release mode (default: debug)");
    eprintln!();
    eprintln!("Exit codes:");
    eprintln!("  0  Success");
    eprintln!("  1  Compile/check error (diagnostics emitted)");
    eprintln!("  2  Build or I/O error");
}
