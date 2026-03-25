//! Muse compiler CLI.
//!
//! Usage:
//!   muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]
//!   muse check <file> [--format json]
//!
//! Exit codes:
//!   0 — success
//!   1 — compile/check error (diagnostics emitted)
//!   2 — I/O or usage error

use std::path::PathBuf;
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

            // Full build path — T02 will wire this up. For now, behave
            // like --no-build since build_plugin() doesn't exist yet.
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
    eprintln!("  2  I/O or usage error");
}
