//! Muse compiler CLI.
//!
//! Usage:
//!   muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]
//!   muse build <file> [--output-dir <dir>] [--format json]
//!   muse check <file> [--format json]
//!   muse test <file> [--output-dir <dir>] [--format json]
//!   muse preview <file> [--format json]
//!
//! Exit codes:
//!   0 — success
//!   1 — compile/check/test error (diagnostics emitted)
//!   2 — build error (cargo build failed)

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use muse_lang::{compile, compile_check, preview_html, diagnostics_to_json, render_ariadne, build_plugin, assemble_clap_bundle, assemble_vst3_bundle, codesign_bundle};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(2);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "build" => cmd_build(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "test" => cmd_test(&args[2..]),
        "preview" => cmd_preview(&args[2..]),
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

// ── muse build ───────────────────────────────────────────────────────────────

/// Parsed CLI options for the build subcommand.
struct BuildOpts {
    file: PathBuf,
    output_dir: PathBuf,
    json_format: bool,
}

fn parse_build_args(args: &[String]) -> Result<BuildOpts, String> {
    if args.is_empty() {
        return Err("muse build: missing <file> argument".into());
    }

    let mut file: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut json_format = false;

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

    let file = file.ok_or("muse build: missing <file> argument")?;
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    Ok(BuildOpts {
        file,
        output_dir,
        json_format,
    })
}

fn cmd_build(args: &[String]) {
    let opts = match parse_build_args(args) {
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

    // Phase 1: compile (.muse → Rust crate)
    let t0 = Instant::now();
    let result = match compile(&source, &filename, &opts.output_dir) {
        Ok(r) => r,
        Err(diags) => {
            if opts.json_format {
                // Compilation errors are structured diagnostics, not phase errors
                println!("{}", diagnostics_to_json(&diags));
            } else {
                render_ariadne(&diags, &source, &filename);
            }
            process::exit(1);
        }
    };
    let compile_ms = t0.elapsed().as_millis();

    // Phase 2: cargo build (Rust crate → dylib)
    let t1 = Instant::now();
    let build_output = match build_plugin(&result.crate_dir, &result.package_name) {
        Ok(o) => o,
        Err(e) => {
            if opts.json_format {
                let json = serde_json::json!({
                    "status": "error",
                    "phase": "cargo_build",
                    "message": e.to_string(),
                    "cargo_stderr": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!("muse: build failed: {e}");
            }
            process::exit(2);
        }
    };
    let cargo_build_ms = t1.elapsed().as_millis();

    // Phase 3: CLAP bundle assembly
    let t2 = Instant::now();
    let clap_bundle = match assemble_clap_bundle(
        &opts.output_dir,
        &build_output.dylib_path,
        &result.plugin_name,
        &result.clap_id,
        &result.version,
    ) {
        Ok(p) => p,
        Err(e) => {
            if opts.json_format {
                let json = serde_json::json!({
                    "status": "error",
                    "phase": "clap_bundle",
                    "message": e,
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!("muse: CLAP bundle assembly failed: {e}");
            }
            process::exit(2);
        }
    };
    let clap_bundle_ms = t2.elapsed().as_millis();

    // Phase 4: VST3 bundle assembly
    let t3 = Instant::now();
    let vst3_bundle = match assemble_vst3_bundle(
        &opts.output_dir,
        &build_output.dylib_path,
        &result.plugin_name,
        &result.vst3_id,
        &result.version,
    ) {
        Ok(p) => p,
        Err(e) => {
            if opts.json_format {
                let json = serde_json::json!({
                    "status": "error",
                    "phase": "vst3_bundle",
                    "message": e,
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!("muse: VST3 bundle assembly failed: {e}");
            }
            process::exit(2);
        }
    };
    let vst3_bundle_ms = t3.elapsed().as_millis();

    // Phase 5: codesign CLAP bundle
    let t4 = Instant::now();
    if let Err(e) = codesign_bundle(&clap_bundle) {
        if opts.json_format {
            let json = serde_json::json!({
                "status": "error",
                "phase": "codesign_clap",
                "message": e,
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
        } else {
            eprintln!("muse: CLAP codesign failed: {e}");
        }
        process::exit(2);
    }
    let codesign_clap_ms = t4.elapsed().as_millis();

    // Phase 6: codesign VST3 bundle
    let t5 = Instant::now();
    if let Err(e) = codesign_bundle(&vst3_bundle) {
        if opts.json_format {
            let json = serde_json::json!({
                "status": "error",
                "phase": "codesign_vst3",
                "message": e,
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
        } else {
            eprintln!("muse: VST3 codesign failed: {e}");
        }
        process::exit(2);
    }
    let codesign_vst3_ms = t5.elapsed().as_millis();

    // Compute artifact sizes (binary inside bundle is >99% of bundle size)
    let clap_binary = clap_bundle.join("Contents").join("MacOS").join(&result.plugin_name);
    let vst3_binary = vst3_bundle.join("Contents").join("MacOS").join(&result.plugin_name);
    let clap_size = std::fs::metadata(&clap_binary).map(|m| m.len()).unwrap_or(0);
    let vst3_size = std::fs::metadata(&vst3_binary).map(|m| m.len()).unwrap_or(0);

    if opts.json_format {
        let json = serde_json::json!({
            "status": "ok",
            "plugin_name": result.plugin_name,
            "package_name": result.package_name,
            "phases": {
                "compile": { "duration_ms": compile_ms },
                "cargo_build": { "duration_ms": cargo_build_ms },
                "clap_bundle": { "duration_ms": clap_bundle_ms },
                "vst3_bundle": { "duration_ms": vst3_bundle_ms },
                "codesign_clap": { "duration_ms": codesign_clap_ms },
                "codesign_vst3": { "duration_ms": codesign_vst3_ms },
            },
            "artifacts": {
                "clap": {
                    "path": format!("{}.clap", result.plugin_name),
                    "size_bytes": clap_size,
                },
                "vst3": {
                    "path": format!("{}.vst3", result.plugin_name),
                    "size_bytes": vst3_size,
                },
                "crate_dir": result.crate_dir.display().to_string(),
            },
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else {
        eprintln!(
            "Built '{}' → {}.clap + {}.vst3",
            result.plugin_name, result.plugin_name, result.plugin_name
        );
    }
    process::exit(0);
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
            let build_output = match build_plugin(&result.crate_dir, &result.package_name) {
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
                &build_output.dylib_path,
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

// ── muse test ────────────────────────────────────────────────────────────────

/// Parsed CLI options for the test subcommand.
struct TestOpts {
    file: PathBuf,
    output_dir: PathBuf,
    json_format: bool,
}

fn parse_test_args(args: &[String]) -> Result<TestOpts, String> {
    if args.is_empty() {
        return Err("muse test: missing <file> argument".into());
    }

    let mut file: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut json_format = false;

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

    let file = file.ok_or("muse test: missing <file> argument")?;
    let output_dir = output_dir.unwrap_or_else(|| std::env::temp_dir().join("muse-test"));

    Ok(TestOpts {
        file,
        output_dir,
        json_format,
    })
}

/// A single test result parsed from cargo test output.
#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    /// Populated from MUSE_TEST_FAIL:{json} for failed assertions.
    assertion: Option<String>,
    expected: Option<String>,
    actual: Option<String>,
}

/// Aggregate results from a cargo test run.
#[derive(Debug)]
struct TestRunResults {
    tests: Vec<TestResult>,
    passed: usize,
    failed: usize,
}

fn cmd_test(args: &[String]) {
    let opts = match parse_test_args(args) {
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

    // Compile the .muse file to a Rust crate (no native build — tests run in debug)
    let result = match compile(&source, &filename, &opts.output_dir) {
        Ok(r) => r,
        Err(diags) => {
            if opts.json_format {
                println!("{}", diagnostics_to_json(&diags));
            } else {
                render_ariadne(&diags, &source, &filename);
            }
            process::exit(1);
        }
    };

    // Check that the generated code contains tests
    let lib_rs = result.crate_dir.join("src").join("lib.rs");
    match std::fs::read_to_string(&lib_rs) {
        Ok(contents) => {
            if !contents.contains("#[test]") {
                if opts.json_format {
                    let json = serde_json::json!({
                        "status": "error",
                        "message": "no test blocks found in source file",
                        "file": filename,
                    });
                    println!("{}", serde_json::to_string_pretty(&json).unwrap());
                } else {
                    eprintln!("muse: no test blocks found in '{}'", opts.file.display());
                }
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("muse: failed to read generated lib.rs: {e}");
            process::exit(2);
        }
    }

    // Run cargo test in the generated crate (debug mode for speed)
    let output = match std::process::Command::new("cargo")
        .args(["test", "--", "--nocapture"])
        .current_dir(&result.crate_dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            if opts.json_format {
                let json = serde_json::json!({
                    "status": "error",
                    "phase": "build",
                    "message": format!("failed to invoke cargo test: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!("muse: failed to invoke cargo test: {e}");
            }
            process::exit(2);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // If cargo itself failed to compile the generated crate, that's a build error
    if !output.status.success() {
        // Check if this is a compile error (no test results at all) vs test failure
        let has_test_results = stdout.lines().any(|l| l.starts_with("test "));
        if !has_test_results {
            if opts.json_format {
                let json = serde_json::json!({
                    "status": "error",
                    "phase": "build",
                    "message": stderr.trim(),
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                eprintln!("muse: cargo test build failed:\n{}", stderr.trim());
            }
            process::exit(2);
        }
    }

    // Parse cargo test output
    let results = parse_cargo_test_output(&stdout, &stderr);

    if opts.json_format {
        print_json_results(&results, &filename);
    } else {
        print_human_results(&results);
    }

    if results.failed > 0 {
        process::exit(1);
    } else {
        process::exit(0);
    }
}

/// Parse cargo test stdout/stderr into structured test results.
fn parse_cargo_test_output(stdout: &str, stderr: &str) -> TestRunResults {
    let mut tests = Vec::new();

    // Collect MUSE_TEST_FAIL entries from stderr for enriching failure details.
    // The format is: MUSE_TEST_FAIL:{"test":"...","assertion":"...","expected":"...","actual":"..."}
    let mut fail_details: std::collections::HashMap<String, (String, String, String)> =
        std::collections::HashMap::new();

    // Scan both stdout and stderr for MUSE_TEST_FAIL markers (--nocapture sends
    // panic messages to stdout for the test runner)
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(json_start) = line.find("MUSE_TEST_FAIL:") {
            let json_str = &line[json_start + "MUSE_TEST_FAIL:".len()..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(name) = val.get("test").and_then(|v| v.as_str()) {
                    let assertion = val
                        .get("assertion")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let expected = val
                        .get("expected")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let actual = val
                        .get("actual")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    fail_details.insert(name.to_string(), (assertion, expected, actual));
                }
            }
        }
    }

    // Parse test result lines from stdout: "test tests::test_<name> ... ok" or "... FAILED"
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with("test ") {
            continue;
        }

        // Format: "test tests::test_<sanitized_name> ... ok" or "... FAILED"
        let is_ok = line.ends_with("... ok");
        let is_failed = line.ends_with("... FAILED");
        if !is_ok && !is_failed {
            continue;
        }

        // Extract the test function name
        let name_part = if is_ok {
            line.strip_prefix("test ")
                .and_then(|s| s.strip_suffix(" ... ok"))
        } else {
            line.strip_prefix("test ")
                .and_then(|s| s.strip_suffix(" ... FAILED"))
        };

        let Some(raw_name) = name_part else {
            continue;
        };

        // Convert "tests::test_silence_in_produces_silence_out" → "silence in produces silence out"
        let human_name = raw_name
            .strip_prefix("tests::test_")
            .unwrap_or(raw_name)
            .replace('_', " ");

        if is_ok {
            tests.push(TestResult {
                name: human_name,
                passed: true,
                assertion: None,
                expected: None,
                actual: None,
            });
        } else {
            // Look up structured failure details
            let details = fail_details.get(&human_name);
            tests.push(TestResult {
                name: human_name,
                passed: false,
                assertion: details.map(|(a, _, _)| a.clone()),
                expected: details.map(|(_, e, _)| e.clone()),
                actual: details.map(|(_, _, a)| a.clone()),
            });
        }
    }

    let passed = tests.iter().filter(|t| t.passed).count();
    let failed = tests.iter().filter(|t| !t.passed).count();

    TestRunResults {
        tests,
        passed,
        failed,
    }
}

/// Print test results as structured JSON.
fn print_json_results(results: &TestRunResults, file: &str) {
    let status = if results.failed > 0 { "fail" } else { "ok" };
    let total = results.passed + results.failed;

    let tests_json: Vec<serde_json::Value> = results
        .tests
        .iter()
        .map(|t| {
            let mut obj = serde_json::json!({
                "name": t.name,
                "result": if t.passed { "pass" } else { "fail" },
            });
            if !t.passed {
                if let Some(ref a) = t.assertion {
                    obj["assertion"] = serde_json::Value::String(a.clone());
                }
                if let Some(ref e) = t.expected {
                    obj["expected"] = serde_json::Value::String(e.clone());
                }
                if let Some(ref a) = t.actual {
                    obj["actual"] = serde_json::Value::String(a.clone());
                }
            }
            obj
        })
        .collect();

    let json = serde_json::json!({
        "status": status,
        "file": file,
        "tests": tests_json,
        "passed": results.passed,
        "failed": results.failed,
        "total": total,
    });

    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}

/// Print test results in human-readable format.
fn print_human_results(results: &TestRunResults) {
    let total = results.passed + results.failed;

    for t in &results.tests {
        if t.passed {
            eprintln!("  ✓ {}", t.name);
        } else {
            eprintln!("  ✗ {}", t.name);
            if let Some(ref assertion) = t.assertion {
                let actual_str = t.actual.as_deref().unwrap_or("?");
                eprintln!("    assertion failed: {} (actual: {})", assertion, actual_str);
            }
        }
    }

    eprintln!();
    eprintln!("  {} passed, {} failed, {} total", results.passed, results.failed, total);
}

// ── muse preview ─────────────────────────────────────────────────────────────

/// Parsed CLI options for the preview subcommand.
struct PreviewOpts {
    file: PathBuf,
    json_format: bool,
}

fn parse_preview_args(args: &[String]) -> Result<PreviewOpts, String> {
    if args.is_empty() {
        return Err("muse preview: missing <file> argument".into());
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

    let file = file.ok_or("muse preview: missing <file> argument")?;

    Ok(PreviewOpts { file, json_format })
}

#[cfg(target_os = "macos")]
fn cmd_preview(args: &[String]) {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowStyleMask, NSBackingStoreType};
    use objc2_foundation::{NSRect, NSPoint, NSSize, NSString};
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

    let opts = match parse_preview_args(args) {
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

    let html = match preview_html(&source, &filename) {
        Ok(h) => h,
        Err(diags) => {
            if opts.json_format {
                println!("{}", diagnostics_to_json(&diags));
            } else {
                render_ariadne(&diags, &source, &filename);
            }
            process::exit(1);
        }
    };

    // Extract GUI size from AST for window dimensions
    let (width, height) = extract_gui_size(&source);

    // ── Native macOS window with WKWebView ──
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    let app = NSApplication::sharedApplication(mtm);

    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable;

    let frame = NSRect::new(
        NSPoint::new(200.0, 200.0),
        NSSize::new(width as f64, height as f64),
    );

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    let plugin_name = opts.file.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Muse Preview".to_string());
    let title = NSString::from_str(&format!("{plugin_name} — Muse Preview"));
    window.setTitle(&title);

    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    let webview_frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width as f64, height as f64),
    );
    let webview = unsafe {
        WKWebView::initWithFrame_configuration(
            WKWebView::alloc(mtm),
            webview_frame,
            &config,
        )
    };

    // Load the HTML string into the webview
    let html_ns = NSString::from_str(&html);
    let base_url: Option<&objc2_foundation::NSURL> = None;
    unsafe { webview.loadHTMLString_baseURL(&html_ns, base_url) };

    // WKWebView → NSView via into_super (WKWebView > NSView > NSResponder > NSObject)
    let webview_view: objc2::rc::Retained<objc2_app_kit::NSView> =
        objc2::rc::Retained::into_super(webview);
    window.setContentView(Some(&webview_view));
    window.center();
    window.makeKeyAndOrderFront(None);

    app.run();
}

#[cfg(not(target_os = "macos"))]
fn cmd_preview(_args: &[String]) {
    eprintln!("muse: preview is only supported on macOS");
    process::exit(2);
}

/// Extract GUI size from source by parsing the AST.
/// Returns (width, height), defaulting to (600, 400).
fn extract_gui_size(source: &str) -> (u32, u32) {
    let (ast, errors) = muse_lang::parse(source);
    if !errors.is_empty() {
        return (600, 400);
    }
    let Some(plugin) = ast else {
        return (600, 400);
    };
    match muse_lang::codegen::gui::find_gui_block(&plugin) {
        Some(gui) => muse_lang::codegen::gui::gui_size(gui),
        None => (600, 400),
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  muse compile <file> [--output-dir <dir>] [--format json] [--no-build] [--release]");
    eprintln!("  muse build <file> [--output-dir <dir>] [--format json]");
    eprintln!("  muse check <file> [--format json]");
    eprintln!("  muse test <file> [--output-dir <dir>] [--format json]");
    eprintln!("  muse preview <file> [--format json]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile    Parse, resolve, and generate a Rust/nih-plug crate from a .muse file");
    eprintln!("  build      Compile, build, bundle (CLAP + VST3), and codesign a .muse plugin");
    eprintln!("  check      Parse and resolve a .muse file, reporting any errors");
    eprintln!("  test       Compile and run in-language test blocks, reporting pass/fail results");
    eprintln!("  preview    Open a native preview window showing the plugin GUI (macOS only)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --output-dir <dir>  Directory to place generated crate/bundles (default: current dir)");
    eprintln!("  --format json       Output structured JSON (telemetry for build, diagnostics for others)");
    eprintln!("  --no-build          Generate Rust crate only, skip cargo build (compile only)");
    eprintln!("  --release           Build in release mode (default: debug)");
    eprintln!();
    eprintln!("Exit codes:");
    eprintln!("  0  Success");
    eprintln!("  1  Compile/check/test error (diagnostics emitted)");
    eprintln!("  2  Build or I/O error");
}
