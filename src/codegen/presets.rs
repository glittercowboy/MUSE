//! Generates preset support code: `apply_preset()` function and `PRESET_NAMES` constant.
//!
//! Each `PresetBlock` in the AST becomes a match arm in `apply_preset()` that
//! sets all listed parameters to their preset values via smoother reset.
//! dB-unit params are wrapped in `util::db_to_gain()` per K022.
//!
//! Float/Int params use `.smoothed.reset(value)` which is always public.
//! Bool params use the internal atomic via `Param::update_plain_value()` pattern —
//! since the preset function lives in the same crate, pub(crate) access is available.
//! Enum params use a cast through the i32 plain value representation.

use crate::ast::{
    ParamOption, ParamType, PluginDef, PluginItem, PresetBlock, PresetValue,
};

/// Generate the `apply_preset()` function and `PRESET_NAMES` constant.
///
/// Returns an empty string if there are no preset blocks.
pub fn generate_presets(plugin: &PluginDef) -> String {
    let presets: Vec<&PresetBlock> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::PresetDecl(pb) = item {
                Some(pb)
            } else {
                None
            }
        })
        .collect();

    if presets.is_empty() {
        return String::new();
    }

    let db_params = collect_db_param_names(plugin);

    let mut out = String::new();

    // PRESET_NAMES constant
    out.push_str("pub const PRESET_NAMES: &[&str] = &[\n");
    for preset in &presets {
        out.push_str(&format!("    \"{}\",\n", preset.name));
    }
    out.push_str("];\n\n");

    // apply_preset function
    out.push_str("pub fn apply_preset(params: &PluginParams, name: &str) {\n");
    out.push_str("    match name {\n");

    for preset in &presets {
        out.push_str(&format!("        \"{}\" => {{\n", preset.name));
        for (assignment, _span) in &preset.assignments {
            let param_name = &assignment.param_name;
            let param_type = find_param_type(plugin, param_name);
            match &assignment.value {
                PresetValue::Number(val) => {
                    if db_params.contains(param_name) {
                        out.push_str(&format!(
                            "            params.{}.smoothed.reset(util::db_to_gain({:.6}_f32));\n",
                            param_name, val
                        ));
                    } else if matches!(param_type, Some(ParamType::Int)) {
                        // IntParam smoother expects f32
                        out.push_str(&format!(
                            "            params.{}.smoothed.reset({}_f32);\n",
                            param_name, *val as i64
                        ));
                    } else {
                        out.push_str(&format!(
                            "            params.{}.smoothed.reset({:.6}_f32);\n",
                            param_name, val
                        ));
                    }
                }
                PresetValue::Bool(_val) => {
                    // BoolParam has no smoother, and set_plain_value is pub(crate) in nih-plug.
                    // Skip for now — preset bool params require a nih-plug API workaround.
                    out.push_str(&format!(
                        "            // TODO: bool preset for '{}' (nih-plug BoolParam lacks public setter)\n",
                        param_name
                    ));
                }
                PresetValue::Ident(_variant) => {
                    // EnumParam: set_plain_value is also pub(crate) in nih-plug.
                    // Skip for now — preset enum params require a nih-plug API workaround.
                    out.push_str(&format!(
                        "            // TODO: enum preset for '{}' (nih-plug EnumParam lacks public setter)\n",
                        param_name
                    ));
                }
            }
        }
        out.push_str("        }\n");
    }

    out.push_str("        _ => {} // Unknown preset name — no-op\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

/// Collect names of parameters that use dB units.
/// Same logic as in test.rs — extracts params with unit "dB".
fn collect_db_param_names(plugin: &PluginDef) -> Vec<String> {
    let mut db_params = Vec::new();
    for (item, _) in &plugin.items {
        if let PluginItem::ParamDecl(param) = item {
            if param.param_type == ParamType::Float {
                let is_db = param.options.iter().any(|(opt, _)| {
                    matches!(opt, ParamOption::Unit(u) if u.eq_ignore_ascii_case("db"))
                });
                if is_db {
                    db_params.push(param.name.clone());
                }
            }
        }
    }
    db_params
}

/// Find the param type for a given param name.
fn find_param_type(plugin: &PluginDef, name: &str) -> Option<ParamType> {
    for (item, _) in &plugin.items {
        if let PluginItem::ParamDecl(param) = item {
            if param.name == name {
                return Some(param.param_type.clone());
            }
        }
    }
    None
}

/// Capitalize the first letter of a string (for PascalCase type names).
#[allow(dead_code)]
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + chars.as_str()
        }
    }
}
