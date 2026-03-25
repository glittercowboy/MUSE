//! Generates the `#[derive(Params)]` struct and its `Default` impl.

use crate::ast::{ParamDef, ParamOption, ParamType, PluginDef, PluginItem, SmoothingKind};

/// Generate the full `PluginParams` struct with `#[derive(Params)]` and `Default` impl.
pub fn generate_params(plugin: &PluginDef) -> String {
    let params = collect_params(plugin);
    if params.is_empty() {
        return generate_empty_params();
    }

    let mut out = String::new();

    // Struct definition
    out.push_str("#[derive(Params)]\nstruct PluginParams {\n");
    for p in &params {
        out.push_str(&format!("    #[id = \"{}\"]\n", p.name));
        out.push_str(&format!("    pub {}: {},\n", p.name, rust_param_type(p)));
    }
    out.push_str("}\n\n");

    // Default impl
    out.push_str("impl Default for PluginParams {\n");
    out.push_str("    fn default() -> Self {\n");
    out.push_str("        Self {\n");
    for p in &params {
        out.push_str(&generate_param_default(p));
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

/// Collect ParamDef references from the plugin AST.
fn collect_params(plugin: &PluginDef) -> Vec<&ParamDef> {
    plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::ParamDecl(p) = item {
                Some(p.as_ref())
            } else {
                None
            }
        })
        .collect()
}

/// Generate an empty params struct (for plugins with no parameters).
fn generate_empty_params() -> String {
    "#[derive(Params)]\nstruct PluginParams {}\n\nimpl Default for PluginParams {\n    fn default() -> Self {\n        Self {}\n    }\n}\n".to_string()
}

/// Map a ParamDef to its Rust field type string.
fn rust_param_type(param: &ParamDef) -> &'static str {
    match &param.param_type {
        ParamType::Float => "FloatParam",
        ParamType::Int => "IntParam",
        ParamType::Bool => "BoolParam",
        ParamType::Enum(_) => "FloatParam", // TODO: EnumParam in T02
    }
}

/// Check if this parameter has a "dB" unit — triggers the gain idiom.
fn is_db_param(param: &ParamDef) -> bool {
    param.options.iter().any(|(opt, _)| {
        matches!(opt, ParamOption::Unit(u) if u.eq_ignore_ascii_case("db"))
    })
}

/// Extract the smoothing option if present.
fn get_smoothing(param: &ParamDef) -> Option<(&SmoothingKind, f64)> {
    for (opt, _) in &param.options {
        if let ParamOption::Smoothing { kind, value } = opt {
            if let crate::ast::Expr::Number(n, _) = &value.0 {
                return Some((kind, *n));
            }
        }
    }
    None
}

/// Get the default value as f64, falling back to 0.0.
fn default_value(param: &ParamDef) -> f64 {
    param
        .default
        .as_ref()
        .and_then(|(expr, _)| {
            if let crate::ast::Expr::Number(n, _) = expr {
                Some(*n)
            } else {
                None
            }
        })
        .unwrap_or(0.0)
}

/// Get the range (min, max) as f64, falling back to (0.0, 1.0).
fn param_range(param: &ParamDef) -> (f64, f64) {
    param
        .range
        .as_ref()
        .map(|r| {
            let min = match &r.min.0 {
                crate::ast::Expr::Number(n, _) => *n,
                crate::ast::Expr::Unary { op: crate::ast::UnaryOp::Neg, operand } => {
                    if let crate::ast::Expr::Number(n, _) = &operand.0 {
                        -n
                    } else {
                        0.0
                    }
                }
                _ => 0.0,
            };
            let max = match &r.max.0 {
                crate::ast::Expr::Number(n, _) => *n,
                _ => 1.0,
            };
            (min, max)
        })
        .unwrap_or((0.0, 1.0))
}

/// Generate the default initializer for a single parameter field.
fn generate_param_default(param: &ParamDef) -> String {
    let name = &param.name;
    // Capitalize the display name
    let display_name = capitalize(name);
    let default_val = default_value(param);
    let (min, max) = param_range(param);

    match &param.param_type {
        ParamType::Float => {
            if is_db_param(param) {
                generate_db_gain_param(name, &display_name, default_val, min, max, param)
            } else {
                generate_float_param(name, &display_name, default_val, min, max, param)
            }
        }
        ParamType::Int => {
            let default_int = default_val as i32;
            let min_int = min as i32;
            let max_int = max as i32;
            let mut s = format!(
                "            {name}: IntParam::new(\n                \"{display_name}\",\n                {default_int},\n                IntRange::Linear {{ min: {min_int}, max: {max_int} }},\n            )",
            );
            if let Some((kind, ms)) = get_smoothing(param) {
                s.push_str(&format!(
                    "\n            .with_smoother(SmoothingStyle::{}({:.1}))",
                    smoothing_style(kind),
                    ms
                ));
            }
            s.push_str(",\n");
            s
        }
        ParamType::Bool => {
            let default_bool = default_val != 0.0;
            format!(
                "            {name}: BoolParam::new(\n                \"{display_name}\",\n                {default_bool},\n            ),\n",
            )
        }
        ParamType::Enum(_) => {
            // Placeholder — T02 handles enum params
            format!("            {name}: todo!(\"enum param\"),\n")
        }
    }
}

/// Generate a dB-gain parameter using nih-plug's gain idiom:
/// stored as linear gain, displayed in dB, using Skewed range with gain_skew_factor.
fn generate_db_gain_param(
    name: &str,
    display_name: &str,
    default_db: f64,
    min_db: f64,
    max_db: f64,
    param: &ParamDef,
) -> String {
    let mut s = format!(
        r#"            {name}: FloatParam::new(
                "{display_name}",
                util::db_to_gain({default_db:.1}),
                FloatRange::Skewed {{
                    min: util::db_to_gain({min_db:.1}),
                    max: util::db_to_gain({max_db:.1}),
                    factor: FloatRange::gain_skew_factor({min_db:.1}, {max_db:.1}),
                }},
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db())"#,
    );

    if let Some((kind, ms)) = get_smoothing(param) {
        s.push_str(&format!(
            "\n            .with_smoother(SmoothingStyle::{}({:.1}))",
            smoothing_style(kind),
            ms
        ));
    }
    s.push_str(",\n");
    s
}

/// Generate a plain float parameter (non-dB).
fn generate_float_param(
    name: &str,
    display_name: &str,
    default_val: f64,
    min: f64,
    max: f64,
    param: &ParamDef,
) -> String {
    let mut s = format!(
        "            {name}: FloatParam::new(\n                \"{display_name}\",\n                {default_val:.1},\n                FloatRange::Linear {{ min: {min:.1}, max: {max:.1} }},\n            )",
    );
    if let Some((kind, ms)) = get_smoothing(param) {
        s.push_str(&format!(
            "\n            .with_smoother(SmoothingStyle::{}({:.1}))",
            smoothing_style(kind),
            ms
        ));
    }
    s.push_str(",\n");
    s
}

/// Map SmoothingKind to nih-plug's SmoothingStyle variant name.
fn smoothing_style(kind: &SmoothingKind) -> &'static str {
    match kind {
        SmoothingKind::Linear => "Linear",
        SmoothingKind::Logarithmic => "Logarithmic",
        SmoothingKind::Exponential => "Exponential",
    }
}

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
