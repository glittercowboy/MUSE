//! Generates the Cargo.toml for a nih-plug plugin crate.

use crate::ast::PluginDef;

/// Generate a Cargo.toml string for the plugin crate.
///
/// The package name is derived from the plugin name: lowercased, spaces → hyphens.
/// Depends on nih-plug from git (pinned commit for reproducibility).
pub fn generate_cargo_toml(plugin: &PluginDef) -> String {
    let pkg_name = plugin_name_to_package(&plugin.name);
    let version = extract_metadata(plugin, "version").unwrap_or_else(|| "0.1.0".to_string());

    format!(
        r#"[package]
name = "{pkg_name}"
version = "{version}"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
nih_plug = {{ git = "https://github.com/robbert-vdh/nih-plug.git", rev = "28b149ec4d" }}
"#,
    )
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
}
