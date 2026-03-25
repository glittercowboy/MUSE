//! Generates the process() method body from the AST's ProcessBlock.
//!
//! For T01 (gain.muse), the process block is the chain: `input -> gain(param.gain) -> output`.
//! This generates the inner loop body that reads smoothed params and applies DSP.

use crate::ast::{BinOp, Expr, PluginDef, PluginItem, ProcessBlock, Spanned, Statement};

/// Generate the process() body (the code that replaces `{PROCESS_BODY}` in the Plugin trait).
///
/// Returns the Rust code for the process method body (everything inside the `fn process(...)`).
pub fn generate_process(plugin: &PluginDef) -> String {
    let process_block = match find_process_block(plugin) {
        Some(pb) => pb,
        None => return "        ProcessStatus::Normal".to_string(),
    };

    // Analyze the chain to determine what DSP operations are needed
    let chain = analyze_chain(&process_block.body);

    match chain {
        ProcessChain::GainChain { param_path } => generate_gain_loop(&param_path),
        ProcessChain::Unknown => {
            // Fallback: passthrough
            "        ProcessStatus::Normal".to_string()
        }
    }
}

/// Find the process block in the plugin's items.
fn find_process_block(plugin: &PluginDef) -> Option<&ProcessBlock> {
    for (item, _) in &plugin.items {
        if let PluginItem::ProcessBlock(pb) = item {
            return Some(pb);
        }
    }
    None
}

/// Represents the recognized patterns we can generate code for.
enum ProcessChain {
    /// `input -> gain(param.X) -> output` — simple gain with one smoothed parameter
    GainChain { param_path: String },
    /// Unrecognized pattern
    Unknown,
}

/// Analyze the process block's statement list to recognize the chain pattern.
fn analyze_chain(stmts: &[Spanned<Statement>]) -> ProcessChain {
    // For gain.muse, there's a single expression statement: `input -> gain(param.gain) -> output`
    // This is a chain: Binary(Binary(input, Chain, gain(param.gain)), Chain, output)
    if stmts.len() != 1 {
        return ProcessChain::Unknown;
    }

    let (stmt, _) = &stmts[0];
    let expr = match stmt {
        Statement::Expr(e) => e,
        _ => return ProcessChain::Unknown,
    };

    // Try to match: input -> gain(param.X) -> output
    if let Some(param_name) = match_gain_chain(&expr.0) {
        return ProcessChain::GainChain {
            param_path: param_name,
        };
    }

    ProcessChain::Unknown
}

/// Try to match the pattern: input -> gain(param.X) -> output
/// Returns the param field name if matched.
fn match_gain_chain(expr: &Expr) -> Option<String> {
    // Outermost: Binary { left: (input -> gain(param.X)), Chain, right: output }
    let (left_chain, right_ident) = match_chain(expr)?;
    if !is_ident(right_ident, "output") {
        return None;
    }

    // left_chain: Binary { left: input, Chain, right: gain(param.X) }
    let (input_expr, gain_call) = match_chain(left_chain)?;
    if !is_ident(input_expr, "input") {
        return None;
    }

    // gain_call: FnCall { callee: "gain", args: [param.X] }
    match_gain_fn_call(gain_call)
}

/// Match a Binary Chain expression, returning (left_expr, right_expr).
fn match_chain(expr: &Expr) -> Option<(&Expr, &Expr)> {
    if let Expr::Binary {
        left,
        op: BinOp::Chain,
        right,
    } = expr
    {
        Some((&left.0, &right.0))
    } else {
        None
    }
}

/// Check if an expression is an identifier with the given name.
fn is_ident(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Ident(n) if n == name)
}

/// Match `gain(param.X)` and return the field name X.
fn match_gain_fn_call(expr: &Expr) -> Option<String> {
    if let Expr::FnCall { callee, args } = expr {
        if !is_ident(&callee.0, "gain") {
            return None;
        }
        if args.len() != 1 {
            return None;
        }
        // arg should be FieldAccess(Ident("param"), "X")
        if let Expr::FieldAccess(base, field) = &args[0].0 {
            if is_ident(&base.0, "param") {
                return Some(field.clone());
            }
        }
    }
    None
}

/// Generate the gain loop for `input -> gain(param.X) -> output`.
fn generate_gain_loop(param_name: &str) -> String {
    format!(
        r#"        for channel_samples in buffer.iter_samples() {{
            let gain = self.params.{param_name}.smoothed.next();
            for sample in channel_samples {{
                *sample *= gain;
            }}
        }}
        ProcessStatus::Normal"#,
    )
}
