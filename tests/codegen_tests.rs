//! Integration tests for code generation.
//!
//! These tests parse .muse files, resolve them, generate Rust/nih-plug crates,
//! and run `cargo check` to prove the generated code compiles.

use std::path::PathBuf;
use std::process::Command;

use muse_lang::{builtin_registry, generate_plugin, parse, resolve_plugin};

/// Helper: parse + resolve + codegen from source, return the generated crate path.
fn generate_from_source(source: &str, output_dir: &std::path::Path) -> PathBuf {
    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");
    generate_plugin(&resolved, &registry, output_dir).expect("codegen failed")
}

/// Helper: run `cargo check` on a generated crate and assert it succeeds.
fn assert_cargo_check(crate_dir: &std::path::Path) {
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(crate_dir)
        .output()
        .expect("failed to run cargo check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("=== cargo check FAILED ===");
        eprintln!("stdout:\n{}", stdout);
        eprintln!("stderr:\n{}", stderr);
        panic!(
            "cargo check failed with exit code {:?}",
            output.status.code()
        );
    }

    eprintln!("cargo check passed for {}", crate_dir.display());
}

#[test]
fn test_gain_muse_codegen_cargo_check() {
    let source = include_str!("../examples/gain.muse");

    // Use a stable temp directory so cargo can cache between runs
    let tmp = std::env::temp_dir().join("muse-codegen-test-gain");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    // Verify generated files exist
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    // Print generated code for debugging
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    eprintln!("=== Generated src/lib.rs ===\n{}\n=== END ===", lib_rs);

    let cargo_toml = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    eprintln!(
        "=== Generated Cargo.toml ===\n{}\n=== END ===",
        cargo_toml
    );

    // The real proof: cargo check against nih-plug
    assert_cargo_check(&crate_dir);
}

#[test]
fn test_codegen_missing_metadata_produces_diagnostics() {
    // A plugin missing vendor, clap, vst3 blocks should produce E010 diagnostics
    let source = r#"plugin "Bare" {
  input stereo
  output stereo
  process {
    input -> output
  }
}"#;

    let (ast, errors) = parse(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let ast = ast.expect("parse returned None");
    let registry = builtin_registry();
    let resolved = resolve_plugin(&ast, &registry).expect("resolve failed");

    let tmp = std::env::temp_dir().join("muse-codegen-test-bare");
    let result = generate_plugin(&resolved, &registry, &tmp);

    assert!(result.is_err(), "expected codegen to fail for bare plugin");
    let diags = result.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E010"),
        "expected E010 diagnostic, got: {:?}",
        diags
    );
    eprintln!(
        "Correctly rejected bare plugin with {} diagnostics",
        diags.len()
    );
}

#[test]
fn test_codegen_generate_plugin_returns_path() {
    let source = include_str!("../examples/gain.muse");
    let tmp = std::env::temp_dir().join("muse-codegen-test-path");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);
    assert_eq!(crate_dir, tmp);
}

#[test]
fn test_filter_muse_codegen_cargo_check() {
    let source = include_str!("../examples/filter.muse");

    // Use a stable temp directory so cargo can cache between runs
    let tmp = std::env::temp_dir().join("muse-codegen-test-filter");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    // Verify generated files exist
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    // Print generated code for debugging
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    eprintln!("=== Generated filter src/lib.rs ===\n{}\n=== END ===", lib_rs);

    // The real proof: cargo check against nih-plug
    assert_cargo_check(&crate_dir);
}

#[test]
fn test_multiband_muse_codegen_cargo_check() {
    let source = include_str!("../examples/multiband.muse");

    let tmp = std::env::temp_dir().join("muse-codegen-test-multiband");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).ok();
    }

    let crate_dir = generate_from_source(source, &tmp);

    // Verify generated files exist
    assert!(crate_dir.join("Cargo.toml").exists(), "Cargo.toml missing");
    assert!(crate_dir.join("src/lib.rs").exists(), "src/lib.rs missing");

    // Print generated code for debugging
    let lib_rs = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    eprintln!("=== Generated multiband src/lib.rs ===\n{}\n=== END ===", lib_rs);

    // The real proof: cargo check against nih-plug
    assert_cargo_check(&crate_dir);
}
