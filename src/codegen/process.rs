//! Generates the process() method body from the AST's ProcessBlock.
//!
//! Supports:
//! - Simple chains: `input -> gain(param.gain) -> output`
//! - Let bindings: `let filtered = input -> lowpass(param.cutoff, param.resonance)`
//! - If-expressions: `if param.drive > 0.0 { ... } else { ... }`
//! - Multi-DSP chains: lowpass, gain, tanh, mix
//! - Split/merge parallel routing: `split { branch1; branch2 } -> merge`

use std::collections::HashSet;

use crate::ast::{BinOp, ElseBody, Expr, PluginDef, PluginItem, ProcessBlock, Spanned, Statement, UnaryOp};
use crate::dsp::primitives::{DspPrimitive, EnvKind, OscKind};

/// Information collected during process generation that downstream codegen needs.
pub struct ProcessInfo {
    /// DSP primitives used (for state struct generation and DSP helpers).
    pub used_primitives: HashSet<DspPrimitive>,
    /// Per-branch filter uses: (split_id, branch_idx, FilterKind).
    /// Used by plugin.rs to generate per-branch biquad state fields.
    pub branch_filters: Vec<(usize, usize, crate::dsp::primitives::FilterKind)>,
    /// Whether any code needs per-channel indexing (e.g. filter state).
    pub needs_channel_idx: bool,
    /// Diagnostics collected during process generation (E011 unsupported constructs).
    pub diagnostics: Vec<crate::diagnostic::Diagnostic>,
    /// True when the plugin has a MidiDecl — triggers instrument mode codegen.
    pub is_instrument: bool,
    /// Number of oscillator state fields needed (one per oscillator call site).
    pub oscillator_count: usize,
    /// Whether ADSR envelope state is needed.
    pub has_adsr: bool,
}

/// Generate the process() body (the code that replaces `{PROCESS_BODY}` in the Plugin trait).
///
/// Also collects which DSP primitives are used so the caller can generate
/// appropriate state structs and helper functions.
pub fn generate_process(plugin: &PluginDef) -> (String, ProcessInfo) {
    let is_instrument = find_midi_decl(plugin);

    let process_block = match find_process_block(plugin) {
        Some(pb) => pb,
        None => return ("        ProcessStatus::Normal".to_string(), ProcessInfo {
            used_primitives: HashSet::new(),
            branch_filters: Vec::new(),
            needs_channel_idx: false,
            diagnostics: Vec::new(),
            is_instrument: false,
            oscillator_count: 0,
            has_adsr: false,
        }),
    };

    let mut ctx = ProcessContext::new();
    ctx.is_instrument = is_instrument;

    // First pass: collect all statements' code
    // All statements execute per-sample inside the inner loop.
    let mut stmt_lines: Vec<String> = Vec::new();
    for (i, (stmt, _)) in process_block.body.iter().enumerate() {
        let is_last = i == process_block.body.len() - 1;
        let lines = generate_statement(stmt, is_last, &mut ctx);
        stmt_lines.extend(lines);
    }

    let has_adsr = ctx.used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Envelope(EnvKind::Adsr))
    });
    let oscillator_count = ctx.oscillator_counter;

    if is_instrument {
        // ── Instrument mode: MIDI event loop, mono output, KeepAlive ──
        let mut out = String::new();

        out.push_str("        let mut next_event = context.next_event();\n");
        out.push_str("        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");

        // Per-sample parameter smoothing
        for param_name in &ctx.smoothed_params {
            out.push_str(&format!(
                "            let {param_name} = self.params.{param_name}.smoothed.next();\n"
            ));
        }

        // MIDI event loop (sample-accurate)
        let midi_loop = crate::codegen::midi::generate_midi_event_loop();
        for line in midi_loop.lines() {
            out.push_str("            ");
            out.push_str(line);
            out.push('\n');
        }

        // Process block statements — compute the mono output
        for line in &stmt_lines {
            out.push_str("            ");
            out.push_str(line);
            out.push('\n');
        }

        out.push_str("        }\n");
        out.push_str("        ProcessStatus::KeepAlive");

        let info = ProcessInfo {
            used_primitives: ctx.used_primitives,
            branch_filters: ctx.branch_filters,
            needs_channel_idx: false, // instrument mode doesn't use per-channel indexing
            diagnostics: ctx.diagnostics,
            is_instrument: true,
            oscillator_count,
            has_adsr,
        };
        (out, info)
    } else {
        // ── Effect mode: unchanged from original ──
        let mut out = String::new();

        let needs_channel_idx = ctx.used_primitives.iter().any(|p| {
            matches!(p, DspPrimitive::Filter(_))
        });

        if needs_channel_idx {
            out.push_str("        for (channel_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");
        } else {
            out.push_str("        for channel_samples in buffer.iter_samples() {\n");
        }

        // Per-sample parameter smoothing
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

        let info = ProcessInfo {
            used_primitives: ctx.used_primitives,
            branch_filters: ctx.branch_filters,
            needs_channel_idx,
            diagnostics: ctx.diagnostics,
            is_instrument: false,
            oscillator_count,
            has_adsr,
        };
        (out, info)
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

/// Check if the plugin has a MidiDecl item, indicating instrument mode.
fn find_midi_decl(plugin: &PluginDef) -> bool {
    plugin.items.iter().any(|(item, _)| matches!(item, PluginItem::MidiDecl(_)))
}

/// Tracks state during process body generation.
struct ProcessContext {
    /// Parameter fields that need `.smoothed.next()` calls.
    smoothed_params: Vec<String>,
    /// DSP primitives used (for state struct generation).
    used_primitives: HashSet<DspPrimitive>,
    /// Lines emitted as side-effects during expression generation (e.g. split branch processing).
    /// Drained into the statement output by generate_statement.
    pending_lines: Vec<String>,
    /// Names of the last split's branch result variables — consumed by merge.
    split_branch_vars: Vec<String>,
    /// Counter for generating unique split IDs when multiple splits exist.
    split_counter: usize,
    /// Current branch context: Some((split_id, branch_idx)) when inside a split branch.
    /// Controls per-branch state field naming in biquad calls.
    current_branch: Option<(usize, usize)>,
    /// Records (split_id, branch_idx, FilterKind) for all filter uses inside branches.
    /// Used by plugin.rs to generate per-branch state fields.
    branch_filters: Vec<(usize, usize, crate::dsp::primitives::FilterKind)>,
    /// Diagnostics collected during process generation (E011 unsupported constructs).
    diagnostics: Vec<crate::diagnostic::Diagnostic>,
    /// Counter for generating unique oscillator state field names (osc_state_0, osc_state_1, ...).
    oscillator_counter: usize,
    /// True when generating for an instrument plugin (affects output/input codegen).
    is_instrument: bool,
}

impl ProcessContext {
    fn new() -> Self {
        Self {
            smoothed_params: Vec::new(),
            used_primitives: HashSet::new(),
            pending_lines: Vec::new(),
            split_branch_vars: Vec::new(),
            split_counter: 0,
            current_branch: None,
            branch_filters: Vec::new(),
            diagnostics: Vec::new(),
            oscillator_counter: 0,
            is_instrument: false,
        }
    }

    /// Record that a smoothed parameter is used, avoiding duplicates.
    fn use_smoothed_param(&mut self, name: &str) {
        if !self.smoothed_params.contains(&name.to_string()) {
            self.smoothed_params.push(name.to_string());
        }
    }

    /// Drain pending side-effect lines into a target vec.
    fn drain_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_lines)
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
            let mut lines = ctx.drain_pending();
            lines.push(format!("let {} = {};", name, expr_code));
            lines
        }
        Statement::Expr(expr) => {
            // Check if this is a chain ending in `output` — that's a write to the output buffer
            if let Some(output_lines) = extract_output_chain(&expr.0, ctx) {
                let mut lines = ctx.drain_pending();
                lines.extend(output_lines);
                lines
            } else if is_last_in_block {
                // Last expression in block — might still need to become an output write
                let code = generate_chain_value(&expr.0, ctx);
                let mut lines = ctx.drain_pending();
                lines.push(format!("{};", code));
                lines
            } else {
                let code = generate_chain_value(&expr.0, ctx);
                let mut lines = ctx.drain_pending();
                lines.push(format!("{};", code));
                lines
            }
        }
        Statement::Assign { target, value } => {
            let expr_code = generate_chain_value(&value.0, ctx);
            let mut lines = ctx.drain_pending();
            lines.push(format!("{} = {};", target, expr_code));
            lines
        }
        Statement::Return(expr) => {
            let expr_code = generate_chain_value(&expr.0, ctx);
            let mut lines = ctx.drain_pending();
            lines.push(format!("return {};", expr_code));
            lines
        }
    }
}

/// Try to extract `X -> output` as a chain where the final step is writing to the output buffer.
/// Returns the generated code for X if this is an output chain, None otherwise.
fn extract_output_chain(expr: &Expr, ctx: &mut ProcessContext) -> Option<Vec<String>> {
    // Match: anything -> output
    if let Expr::Binary { left, op: BinOp::Chain, right } = expr {
        if matches!(&right.0, Expr::Ident(name) if name == "output") {
            let source = generate_chain_value(&left.0, ctx);
            if ctx.is_instrument {
                // Instrument mode: compute mono output, write to all channels
                return Some(vec![
                    format!("let output_sample = {};", source),
                    "for sample in channel_samples { *sample = output_sample; }".to_string(),
                ]);
            } else {
                return Some(vec![format!("*sample = {};", source)]);
            }
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
        // Check if right is Merge — sum the split branch variables
        if matches!(&right.0, Expr::Merge) {
            // The left side should have been a split (or chain ending in split)
            // which populated split_branch_vars. Just evaluate the left to trigger that.
            let _left_val = generate_chain_value(&left.0, ctx);
            // Sum all branch result variables
            if ctx.split_branch_vars.is_empty() {
                return "0.0_f32".to_string();
            }
            let sum_expr = format!("({})", ctx.split_branch_vars.join(" + "));
            ctx.split_branch_vars.clear();
            return sum_expr;
        }
        // Check if right is Split — generate parallel branches
        if let Expr::Split { branches } = &right.0 {
            let input_code = generate_chain_value(&left.0, ctx);
            return generate_split_branches(&input_code, branches, ctx);
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
                "lowpass" => generate_filter_call(input_code, "lowpass", args, ctx),
                "bandpass" => generate_filter_call(input_code, "bandpass", args, ctx),
                "highpass" => generate_filter_call(input_code, "highpass", args, ctx),
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

/// Get the biquad state field reference, accounting for branch context and instrument mode.
fn biquad_state_field(ctx: &mut ProcessContext, filter_kind: crate::dsp::primitives::FilterKind) -> String {
    if let Some((split_id, branch_idx)) = ctx.current_branch {
        ctx.branch_filters.push((split_id, branch_idx, filter_kind));
        if ctx.is_instrument {
            format!("self.split{}_branch{}_biquad[0]", split_id, branch_idx)
        } else {
            format!("self.split{}_branch{}_biquad[channel_idx]", split_id, branch_idx)
        }
    } else if ctx.is_instrument {
        "self.biquad_state[0]".to_string()
    } else {
        "self.biquad_state[channel_idx]".to_string()
    }
}

/// Generate a biquad filter call (lowpass, bandpass, highpass) with branch-aware state naming.
fn generate_filter_call(
    input_code: &str,
    filter_name: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    let filter_kind = match filter_name {
        "lowpass" => crate::dsp::primitives::FilterKind::Lowpass,
        "bandpass" => crate::dsp::primitives::FilterKind::Bandpass,
        "highpass" => crate::dsp::primitives::FilterKind::Highpass,
        _ => crate::dsp::primitives::FilterKind::Lowpass,
    };
    ctx.used_primitives.insert(DspPrimitive::Filter(filter_kind));

    let cutoff = generate_expr_as_param(&args[0].0, ctx);
    let resonance = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.707".to_string()
    };

    // Determine the state field name based on branch context and instrument mode
    let state_field = biquad_state_field(ctx, filter_kind);

    // Use filter-type-specific function for the correct coefficient formula
    let fn_name = match filter_name {
        "bandpass" => "process_biquad_bandpass",
        "highpass" => "process_biquad_highpass",
        _ => "process_biquad",
    };

    format!(
        "{}(&mut {}, {}, {}, {}, self.sample_rate)",
        fn_name, state_field, input_code, cutoff, resonance
    )
}

/// Generate code for parallel split branches.
///
/// Emits per-branch variable declarations and processing chains into pending_lines,
/// then stores the branch result variable names for merge to consume.
fn generate_split_branches(
    input_code: &str,
    branches: &[Vec<Spanned<Statement>>],
    ctx: &mut ProcessContext,
) -> String {
    let split_id = ctx.split_counter;
    ctx.split_counter += 1;

    let mut branch_vars = Vec::new();

    for (branch_idx, branch_stmts) in branches.iter().enumerate() {
        let branch_var = format!("split{}_branch{}", split_id, branch_idx);

        // Copy input to branch variable
        ctx.pending_lines.push(format!("let mut {} = {};", branch_var, input_code));

        // Set branch context for state field naming
        ctx.current_branch = Some((split_id, branch_idx));

        // Process each statement in the branch.
        // Each branch is typically a single chain expression like:
        //   lowpass(400Hz) -> gain(param.drive) -> tanh()
        // The chain's input is the branch variable, and its output replaces it.
        for (stmt, _) in branch_stmts {
            match stmt {
                Statement::Expr(expr) => {
                    // Generate the chain with the branch var as the implicit input
                    let result = generate_branch_chain(&expr.0, &branch_var, ctx);
                    let pending = ctx.drain_pending();
                    for line in pending {
                        ctx.pending_lines.push(line);
                    }
                    ctx.pending_lines.push(format!("{} = {};", branch_var, result));
                }
                Statement::Let { name, value } => {
                    let result = generate_branch_chain(&value.0, &branch_var, ctx);
                    let pending = ctx.drain_pending();
                    for line in pending {
                        ctx.pending_lines.push(line);
                    }
                    ctx.pending_lines.push(format!("let {} = {};", name, result));
                }
                _ => {}
            }
        }

        // Clear branch context
        ctx.current_branch = None;

        branch_vars.push(branch_var);
    }

    ctx.split_branch_vars = branch_vars.clone();

    // Return a placeholder — the actual value comes from merge
    // If someone chains directly off a split without merge, return the last branch
    branch_vars.last().cloned().unwrap_or_else(|| "0.0_f32".to_string())
}

/// Generate code for a chain expression within a split branch.
/// The branch variable serves as the implicit input (replaces `input`).
fn generate_branch_chain(expr: &Expr, branch_var: &str, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Binary { left, op: BinOp::Chain, right } => {
            let input = generate_branch_chain(&left.0, branch_var, ctx);
            generate_dsp_call_with_input(&right.0, &input, ctx)
        }
        Expr::FnCall { .. } => {
            // Standalone function call at the start of a branch chain —
            // the input is the branch variable
            generate_dsp_call_with_input(expr, branch_var, ctx)
        }
        Expr::Ident(name) if name == "input" => branch_var.to_string(),
        _ => generate_expr(expr, ctx),
    }
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
                if base_name == "note" {
                    return match field.as_str() {
                        "pitch" => "self.note_freq".to_string(),
                        "velocity" => "self.velocity".to_string(),
                        "gate" => "if self.active_note.is_some() { 1.0_f32 } else { 0.0_f32 }".to_string(),
                        _ => format!("self.{}", field),
                    };
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
                        let state_field = biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Lowpass);
                        return format!(
                            "process_biquad(&mut {}, *sample, {}, {}, self.sample_rate)",
                            state_field, cutoff, resonance
                        );
                    }
                    "bandpass" => {
                        ctx.used_primitives.insert(DspPrimitive::Filter(
                            crate::dsp::primitives::FilterKind::Bandpass,
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
                        let state_field = biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Bandpass);
                        return format!(
                            "process_biquad_bandpass(&mut {}, *sample, {}, {}, self.sample_rate)",
                            state_field, cutoff, resonance
                        );
                    }
                    "highpass" => {
                        ctx.used_primitives.insert(DspPrimitive::Filter(
                            crate::dsp::primitives::FilterKind::Highpass,
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
                        let state_field = biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Highpass);
                        return format!(
                            "process_biquad_highpass(&mut {}, *sample, {}, {}, self.sample_rate)",
                            state_field, cutoff, resonance
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
                    // ── Oscillators (standalone call) ──
                    "saw" | "square" | "sine" | "triangle" => {
                        return generate_osc_call(fn_name, args, ctx);
                    }
                    // ── ADSR envelope ──
                    "adsr" => {
                        return generate_adsr_call(args, ctx);
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
        Expr::Split { branches } => {
            // Standalone split (not chained from an input) — use *sample as input
            generate_split_branches("*sample", branches, ctx)
        }
        Expr::Merge => {
            // Standalone merge — sum whatever branch vars exist
            if ctx.split_branch_vars.is_empty() {
                "0.0_f32".to_string()
            } else {
                let sum = format!("({})", ctx.split_branch_vars.join(" + "));
                ctx.split_branch_vars.clear();
                sum
            }
        }
        _ => {
            ctx.diagnostics.push(crate::diagnostic::Diagnostic::error(
                "E011",
                crate::span::Span::new(0, 0),
                format!("Unsupported expression in codegen: {:?}", std::mem::discriminant(expr)),
            ).with_suggestion("This language construct is not yet supported in code generation"));
            "0.0_f32 /* unsupported */".to_string()
        }
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

/// Generate code for an oscillator call: `saw(freq)`, `square(freq)`, etc.
///
/// Each call site gets a unique state field (`self.osc_state_0`, `self.osc_state_1`, ...)
/// via the oscillator counter on ProcessContext.
fn generate_osc_call(fn_name: &str, args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    let osc_kind = match fn_name {
        "saw" => OscKind::Saw,
        "square" => OscKind::Square,
        "sine" => OscKind::Sine,
        "triangle" => OscKind::Triangle,
        _ => unreachable!(),
    };
    ctx.used_primitives.insert(DspPrimitive::Oscillator(osc_kind));

    let osc_idx = ctx.oscillator_counter;
    ctx.oscillator_counter += 1;

    let freq = if !args.is_empty() {
        generate_expr(&args[0].0, ctx)
    } else {
        "440.0_f32".to_string()
    };

    let process_fn = match fn_name {
        "saw" => "process_osc_saw",
        "square" => "process_osc_square",
        "sine" => "process_osc_sine",
        "triangle" => "process_osc_triangle",
        _ => unreachable!(),
    };

    format!(
        "{}(&mut self.osc_state_{}, {}, self.sample_rate)",
        process_fn, osc_idx, freq
    )
}

/// Generate code for an ADSR envelope call: `adsr(attack, decay, sustain, release)`.
///
/// Maps to `process_adsr(&mut self.adsr_state, gate, attack, decay, sustain, release, sample_rate)`.
fn generate_adsr_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    ctx.used_primitives.insert(DspPrimitive::Envelope(EnvKind::Adsr));

    let attack = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "10.0_f32".to_string()
    };
    let decay = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "100.0_f32".to_string()
    };
    let sustain = if args.len() > 2 {
        generate_expr_as_param(&args[2].0, ctx)
    } else {
        "0.5_f32".to_string()
    };
    let release = if args.len() > 3 {
        generate_expr_as_param(&args[3].0, ctx)
    } else {
        "200.0_f32".to_string()
    };

    // Gate is based on active_note state
    format!(
        "process_adsr(&mut self.adsr_state, if self.active_note.is_some() {{ 1.0_f32 }} else {{ 0.0_f32 }}, {}, {}, {}, {}, self.sample_rate)",
        attack, decay, sustain, release
    )
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
