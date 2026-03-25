//! Generates the Cargo.toml for a nih-plug plugin crate.

use crate::ast::PluginDef;

/// Generate a Cargo.toml string for the plugin crate.
///
/// The package name is derived from the plugin name: lowercased, spaces → hyphens.
/// Depends on nih-plug from git (pinned commit for reproducibility).
/// When `needs_fft` is true, adds rustfft as a dev-dependency for spectral assertions.
/// When `has_gui` is true, adds objc2, objc2-foundation, objc2-app-kit, objc2-web-kit,
/// block2, and serde_json dependencies for WebKit-based editor embedding.
pub fn generate_cargo_toml(plugin: &PluginDef, needs_fft: bool, has_gui: bool) -> String {
    let pkg_name = plugin_name_to_package(&plugin.name);
    let version = extract_metadata(plugin, "version").unwrap_or_else(|| "0.1.0".to_string());

    let mut toml = format!(
        r#"[package]
name = "{pkg_name}"
version = "{version}"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
nih_plug = {{ git = "https://github.com/robbert-vdh/nih-plug.git", rev = "28b149ec4d" }}
"#,
    );

    if has_gui {
        toml.push_str("objc2 = \"0.6\"\n");
        toml.push_str("objc2-foundation = { version = \"0.3\", features = [\"NSString\", \"NSObject\", \"NSURL\"] }\n");
        toml.push_str("objc2-app-kit = { version = \"0.3\", features = [\"NSView\", \"NSResponder\"] }\n");
        toml.push_str("objc2-web-kit = { version = \"0.3\", features = [\"WKWebView\", \"WKWebViewConfiguration\", \"WKUserContentController\", \"WKScriptMessageHandler\", \"WKScriptMessage\"] }\n");
        toml.push_str("block2 = \"0.6\"\n");
        toml.push_str("serde_json = \"1\"\n");
    }

    if needs_fft {
        toml.push_str("\n[dev-dependencies]\nrustfft = \"6\"\n");
    }

    toml.push_str("\n[features]\npreview = []\n");

    toml
}

/// Convert a plugin display name to a Cargo package name.
///
/// "Warm Gain" → "warm-gain"
pub fn plugin_name_to_package(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Extract a metadata string value from the plugin's items.
fn extract_metadata(plugin: &PluginDef, key: &str) -> Option<String> {
    use crate::ast::{MetadataKey, MetadataValue, PluginItem};

    let target_key = match key {
        "vendor" => MetadataKey::Vendor,
        "version" => MetadataKey::Version,
        "url" => MetadataKey::Url,
        "email" => MetadataKey::Email,
        _ => return None,
    };

    for (item, _span) in &plugin.items {
        if let PluginItem::Metadata(meta) = item {
            if meta.key == target_key {
                return match &meta.value {
                    MetadataValue::StringVal(s) => Some(s.clone()),
                    MetadataValue::Identifier(s) => Some(s.clone()),
                };
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_name_to_package() {
        assert_eq!(plugin_name_to_package("Warm Gain"), "warm-gain");
        assert_eq!(plugin_name_to_package("Multi-Band Compressor"), "multi-band-compressor");
        assert_eq!(plugin_name_to_package("SimpleGain"), "simplegain");
    }

    #[test]
    fn test_cargo_toml_with_gui_deps() {
        use crate::ast::*;
        use crate::span::Span;

        let plugin = PluginDef {
            name: "Test Plugin".to_string(),
            items: vec![],
            span: Span::new(0, 0),
        };
        let toml = generate_cargo_toml(&plugin, false, true);
        assert!(toml.contains("objc2 = \"0.6\""));
        assert!(toml.contains("objc2-foundation"));
        assert!(toml.contains("objc2-app-kit"));
        assert!(toml.contains("objc2-web-kit"));
        assert!(toml.contains("WKWebView"));
        assert!(toml.contains("WKScriptMessageHandler"));
        assert!(toml.contains("block2 = \"0.6\""));
        assert!(toml.contains("serde_json = \"1\""));
    }

    #[test]
    fn test_cargo_toml_without_gui() {
        use crate::ast::*;
        use crate::span::Span;

        let plugin = PluginDef {
            name: "Test Plugin".to_string(),
            items: vec![],
            span: Span::new(0, 0),
        };
        let toml = generate_cargo_toml(&plugin, false, false);
        assert!(!toml.contains("objc2"));
        assert!(!toml.contains("serde_json"));
    }
}
