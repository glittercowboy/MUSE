//! Generates the process() method body from the AST's ProcessBlock.
//!
//! Supports:
//! - Simple chains: `input -> gain(param.gain) -> output`
//! - Let bindings: `let filtered = input -> lowpass(param.cutoff, param.resonance)`
//! - If-expressions: `if param.drive > 0.0 { ... } else { ... }`
//! - Multi-DSP chains: lowpass, gain, tanh, mix

use std::collections::HashSet;

use crate::ast::{BinOp, ElseBody, Expr, PluginDef, PluginItem, ProcessBlock, Spanned, Statement, UnaryOp};
use crate::dsp::primitives::DspPrimitive;

/// Generate the process() body (the code that replaces `{PROCESS_BODY}` in the Plugin trait).
///
/// Also collects which DSP primitives are used so the caller can generate
/// appropriate state structs and helper functions.
pub fn generate_process(plugin: &PluginDef) -> (String, HashSet<DspPrimitive>) {
    let process_block = match find_process_block(plugin) {
        Some(pb) => pb,
        None => return ("        ProcessStatus::Normal".to_string(), HashSet::new()),
    };

    let mut ctx = ProcessContext::new();

    // First pass: collect all statements' code
    // All statements execute per-sample inside the inner loop.
    let mut stmt_lines: Vec<String> = Vec::new();
    for (i, (stmt, _)) in process_block.body.iter().enumerate() {
        let is_last = i == process_block.body.len() - 1;
        let lines = generate_statement(stmt, is_last, &mut ctx);
        stmt_lines.extend(lines);
    }

    // Build the full process body
    let mut out = String::new();

    // Use enumerate() only when we need per-channel state (e.g. biquad filter)
    let needs_channel_idx = ctx.used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(_))
    });

    if needs_channel_idx {
        out.push_str("        for (channel_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");
    } else {
        out.push_str("        for channel_samples in buffer.iter_samples() {\n");
    }

    // Per-sample parameter smoothing — read all smoothed params once per sample
    // These must be inside the outer loop so they advance each sample.
    for param_name in &ctx.smoothed_params {
        out.push_str(&format!(
            "            let {param_name} = self.params.{param_name}.smoothed.next();\n"
        ));
    }

    // Inner per-sample loop
    out.push_str("            for sample in channel_samples {\n");
    for line in &stmt_lines {
        out.push_str("                ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        ProcessStatus::Normal");

    (out, ctx.used_primitives)
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

/// Tracks state during process body generation.
struct ProcessContext {
    /// Parameter fields that need `.smoothed.next()` calls.
    smoothed_params: Vec<String>,
    /// DSP primitives used (for state struct generation).
    used_primitives: HashSet<DspPrimitive>,
}

impl ProcessContext {
    fn new() -> Self {
        Self {
            smoothed_params: Vec::new(),
            used_primitives: HashSet::new(),
        }
    }

    /// Record that a smoothed parameter is used, avoiding duplicates.
    fn use_smoothed_param(&mut self, name: &str) {
        if !self.smoothed_params.contains(&name.to_string()) {
            self.smoothed_params.push(name.to_string());
        }
    }
}

/// Generate code lines for a single statement.
/// Returns one or more lines (without trailing newlines).
/// `is_last_in_block` is true for the final statement in the process block —
/// if it's a chain ending in `-> output`, it writes to the sample buffer.
fn generate_statement(
    stmt: &Statement,
    is_last_in_block: bool,
    ctx: &mut ProcessContext,
) -> Vec<String> {
    match stmt {
        Statement::Let { name, value } => {
            let expr_code = generate_chain_value(&value.0, ctx);
            vec![format!("let {} = {};", name, expr_code)]
        }
        Statement::Expr(expr) => {
            // Check if this is a chain ending in `output` — that's a write to the output buffer
            if let Some(source) = extract_output_chain(&expr.0, ctx) {
                vec![format!("*sample = {};", source)]
            } else if is_last_in_block {
                // Last expression in block — might still need to become an output write
                let code = generate_chain_value(&expr.0, ctx);
                vec![format!("{};", code)]
            } else {
                let code = generate_chain_value(&expr.0, ctx);
                vec![format!("{};", code)]
            }
        }
        Statement::Assign { target, value } => {
            let expr_code = generate_chain_value(&value.0, ctx);
            vec![format!("{} = {};", target, expr_code)]
        }
        Statement::Return(expr) => {
            let expr_code = generate_chain_value(&expr.0, ctx);
            vec![format!("return {};", expr_code)]
        }
    }
}

/// Try to extract `X -> output` as a chain where the final step is writing to the output buffer.
/// Returns the generated code for X if this is an output chain, None otherwise.
fn extract_output_chain(expr: &Expr, ctx: &mut ProcessContext) -> Option<String> {
    // Match: anything -> output
    if let Expr::Binary { left, op: BinOp::Chain, right } = expr {
        if matches!(&right.0, Expr::Ident(name) if name == "output") {
            return Some(generate_chain_value(&left.0, ctx));
        }
    }
    None
}

/// Generate the value produced by a chain expression.
/// Handles nested chains like `input -> lowpass(cutoff, res)` and
/// `mix(input, shaped) -> gain(param.mix)`.
fn generate_chain_value(expr: &Expr, ctx: &mut ProcessContext) -> String {
    // Check for chain: left -> right (where right is a DSP function call applied to the chain input)
    if let Expr::Binary { left, op: BinOp::Chain, right } = expr {
        // Check if right is `output` — if so, generate the left side as the value
        if matches!(&right.0, Expr::Ident(name) if name == "output") {
            return generate_chain_value(&left.0, ctx);
        }
        let input_code = generate_chain_value(&left.0, ctx);
        return generate_dsp_call_with_input(&right.0, &input_code, ctx);
    }

    // Not a chain — generate as a regular expression
    generate_expr(expr, ctx)
}

/// Generate a DSP function call where the input signal is piped in via the chain operator.
fn generate_dsp_call_with_input(expr: &Expr, input_code: &str, ctx: &mut ProcessContext) -> String {
    if let Expr::FnCall { callee, args } = expr {
        if let Expr::Ident(fn_name) = &callee.0 {
            return match fn_name.as_str() {
                "gain" => {
                    ctx.used_primitives.insert(DspPrimitive::Gain);
                    let amount = generate_expr_as_param(&args[0].0, ctx);
                    format!("{} * {}", input_code, amount)
                }
                "lowpass" => {
                    ctx.used_primitives.insert(DspPrimitive::Filter(
                        crate::dsp::primitives::FilterKind::Lowpass,
                    ));
                    let cutoff = generate_expr_as_param(&args[0].0, ctx);
                    let resonance = if args.len() > 1 {
                        generate_expr_as_param(&args[1].0, ctx)
                    } else {
                        "0.707".to_string()
                    };
                    format!(
                        "process_biquad(&mut self.biquad_state[channel_idx], {}, {}, {}, self.sample_rate)",
                        input_code, cutoff, resonance
                    )
                }
                "tanh" => {
                    ctx.used_primitives.insert(DspPrimitive::Tanh);
                    format!("({}).tanh()", input_code)
                }
                "mix" => {
                    ctx.used_primitives.insert(DspPrimitive::Mix);
                    if !args.is_empty() {
                        let other = generate_expr(&args[0].0, ctx);
                        format!("({} + {}) * 0.5", input_code, other)
                    } else {
                        input_code.to_string()
                    }
                }
                _ => format!("{}({})", fn_name, input_code),
            };
        }
    }
    // Not a function call — might be just an ident
    generate_expr(expr, ctx)
}

/// Generate code for an expression.
fn generate_expr(expr: &Expr, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Number(n, _) => {
            format!("{:.1}_f32", n)
        }
        Expr::Bool(b) => format!("{}", b),
        Expr::StringLit(s) => format!("\"{}\"", s),
        Expr::Ident(name) => {
            match name.as_str() {
                "input" => "*sample".to_string(),
                _ => name.clone(),
            }
        }
        Expr::FieldAccess(base, field) => {
            if let Expr::Ident(base_name) = &base.0 {
                if base_name == "param" {
                    ctx.use_smoothed_param(field);
                    return field.clone();
                }
            }
            format!("{}.{}", generate_expr(&base.0, ctx), field)
        }
        Expr::FnCall { callee, args } => {
            if let Expr::Ident(fn_name) = &callee.0 {
                match fn_name.as_str() {
                    "mix" => {
                        // mix(dry, wet) as a standalone call (not chained)
                        ctx.used_primitives.insert(DspPrimitive::Mix);
                        if args.len() >= 2 {
                            let dry = generate_chain_value(&args[0].0, ctx);
                            let wet = generate_chain_value(&args[1].0, ctx);
                            return format!("({} + {}) * 0.5", dry, wet);
                        }
                    }
                    "gain" => {
                        ctx.used_primitives.insert(DspPrimitive::Gain);
                        if !args.is_empty() {
                            let amount = generate_expr_as_param(&args[0].0, ctx);
                            return format!("*sample * {}", amount);
                        }
                    }
                    "lowpass" => {
                        ctx.used_primitives.insert(DspPrimitive::Filter(
                            crate::dsp::primitives::FilterKind::Lowpass,
                        ));
                        let cutoff = if !args.is_empty() {
                            generate_expr_as_param(&args[0].0, ctx)
                        } else {
                            "1000.0".to_string()
                        };
                        let resonance = if args.len() > 1 {
                            generate_expr_as_param(&args[1].0, ctx)
                        } else {
                            "0.707".to_string()
                        };
                        return format!(
                            "process_biquad(&mut self.biquad_state[channel_idx], *sample, {}, {}, self.sample_rate)",
                            cutoff, resonance
                        );
                    }
                    "tanh" => {
                        ctx.used_primitives.insert(DspPrimitive::Tanh);
                        if !args.is_empty() {
                            let input = generate_expr(&args[0].0, ctx);
                            return format!("({}).tanh()", input);
                        }
                        return "0.0_f32.tanh()".to_string();
                    }
                    _ => {}
                }
            }
            // Generic function call
            let callee_code = generate_expr(&callee.0, ctx);
            let args_code: Vec<String> = args.iter().map(|(a, _)| generate_expr(a, ctx)).collect();
            format!("{}({})", callee_code, args_code.join(", "))
        }
        Expr::Binary { left, op, right } => {
            if *op == BinOp::Chain {
                // Handle chain expressions
                return generate_chain_value(expr, ctx);
            }
            let left_code = generate_expr(&left.0, ctx);
            let right_code = generate_expr(&right.0, ctx);
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "%",
                BinOp::Eq => "==",
                BinOp::NotEq => "!=",
                BinOp::Lt => "<",
                BinOp::Gt => ">",
                BinOp::LtEq => "<=",
                BinOp::GtEq => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::Chain => "->", // handled above
            };
            format!("{} {} {}", left_code, op_str, right_code)
        }
        Expr::Unary { op, operand } => {
            let operand_code = generate_expr(&operand.0, ctx);
            match op {
                UnaryOp::Neg => format!("-{}", operand_code),
                UnaryOp::Not => format!("!{}", operand_code),
            }
        }
        Expr::If { condition, then_body, then_expr, else_body } => {
            generate_if_expr(condition, then_body, then_expr, else_body.as_ref(), ctx)
        }
        Expr::Grouped(inner) => {
            let inner_code = generate_expr(&inner.0, ctx);
            format!("({})", inner_code)
        }
        _ => "todo!()".to_string(),
    }
}

/// Generate an if-expression that produces a value.
/// The result is a single-line or indented multi-line block suitable
/// for use inside a let binding or as a value expression.
fn generate_if_expr(
    condition: &Spanned<Expr>,
    then_body: &[Spanned<Statement>],
    then_expr: &Spanned<Expr>,
    else_body: Option<&ElseBody>,
    ctx: &mut ProcessContext,
) -> String {
    let cond_code = generate_expr(&condition.0, ctx);

    let mut then_lines = Vec::new();
    for (stmt, _) in then_body {
        let lines = generate_statement(stmt, false, ctx);
        then_lines.extend(lines);
    }
    let then_value = generate_chain_value(&then_expr.0, ctx);
    then_lines.push(then_value);

    let then_block = then_lines
        .iter()
        .map(|l| format!("    {}", l))
        .collect::<Vec<_>>()
        .join("\n                ");

    let mut s = format!("if {} {{\n                {}\n                }}", cond_code, then_block);

    if let Some((else_stmts, else_expr)) = else_body {
        let mut else_lines = Vec::new();
        for (stmt, _) in else_stmts {
            let lines = generate_statement(stmt, false, ctx);
            else_lines.extend(lines);
        }
        let else_value = generate_chain_value(&else_expr.0, ctx);
        else_lines.push(else_value);

        let else_block = else_lines
            .iter()
            .map(|l| format!("    {}", l))
            .collect::<Vec<_>>()
            .join("\n                ");
        s.push_str(&format!(" else {{\n                {}\n                }}", else_block));
    }

    s
}

/// Generate a parameter expression — handles `param.X` field access by using the smoothed local.
fn generate_expr_as_param(expr: &Expr, ctx: &mut ProcessContext) -> String {
    if let Expr::FieldAccess(base, field) = expr {
        if let Expr::Ident(base_name) = &base.0 {
            if base_name == "param" {
                ctx.use_smoothed_param(field);
                return field.clone();
            }
        }
    }
    generate_expr(expr, ctx)
}
