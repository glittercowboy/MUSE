//! Generates the process() method body from the AST's ProcessBlock.
//!
//! Supports:
//! - Simple chains: `input -> gain(param.gain) -> output`
//! - Let bindings: `let filtered = input -> lowpass(param.cutoff, param.resonance)`
//! - If-expressions: `if param.drive > 0.0 { ... } else { ... }`
//! - Multi-DSP chains: lowpass, gain, tanh, mix
//! - Split/merge parallel routing: `split { branch1; branch2 } -> merge`

use std::collections::HashSet;

use crate::ast::{
    BinOp, ElseBody, Expr, PluginDef, PluginItem, ProcessBlock, Spanned, Statement, UnaryOp,
};
use crate::dsp::primitives::{DspPrimitive, EnvKind, OscKind};

pub const MAX_BLOCK_SIZE: usize = 64;

/// Information collected during process generation that downstream codegen needs.
pub struct ProcessInfo {
    pub used_primitives: HashSet<DspPrimitive>,
    pub branch_filters: Vec<(usize, usize, crate::dsp::primitives::FilterKind)>,
    pub needs_channel_idx: bool,
    pub diagnostics: Vec<crate::diagnostic::Diagnostic>,
    pub is_instrument: bool,
    pub voice_count: Option<u32>,
    pub oscillator_count: usize,
    pub has_adsr: bool,
    pub chorus_count: usize,
    pub compressor_count: usize,
    pub delay_count: usize,
}

pub fn generate_process(plugin: &PluginDef, voice_count: Option<u32>, unison_config: Option<&crate::codegen::CodegenUnisonConfig>) -> (String, ProcessInfo) {
    let is_instrument = find_midi_decl(plugin);

    let process_block = match find_process_block(plugin) {
        Some(pb) => pb,
        None => {
            return (
                "        ProcessStatus::Normal".to_string(),
                ProcessInfo {
                    used_primitives: HashSet::new(),
                    branch_filters: Vec::new(),
                    needs_channel_idx: false,
                    diagnostics: Vec::new(),
                    is_instrument: false,
                    voice_count,
                    oscillator_count: 0,
                    has_adsr: false,
                    chorus_count: 0,
                    compressor_count: 0,
                    delay_count: 0,
                },
            )
        }
    };

    let mut ctx = ProcessContext::new();
    ctx.is_instrument = is_instrument;
    ctx.is_polyphonic = is_instrument && voice_count.is_some();

    let mut stmt_lines: Vec<String> = Vec::new();
    for (i, (stmt, _)) in process_block.body.iter().enumerate() {
        let is_last = i == process_block.body.len() - 1;
        let lines = generate_statement(stmt, is_last, &mut ctx);
        stmt_lines.extend(lines);
    }

    let has_adsr = ctx
        .used_primitives
        .iter()
        .any(|p| matches!(p, DspPrimitive::Envelope(EnvKind::Adsr)));
    let oscillator_count = ctx.oscillator_counter;
    let chorus_count = ctx.chorus_counter;
    let compressor_count = ctx.compressor_counter;
    let delay_count = ctx.delay_counter;

    let process_body = if ctx.is_polyphonic {
        generate_polyphonic_process(&ctx.smoothed_params, &stmt_lines, unison_config)
    } else if is_instrument {
        generate_monophonic_instrument_process(&ctx.smoothed_params, &stmt_lines)
    } else {
        generate_effect_process(&ctx, &stmt_lines)
    };

    let needs_channel_idx = if ctx.is_polyphonic {
        false
    } else {
        ctx.used_primitives
            .iter()
            .any(|p| matches!(p, DspPrimitive::Filter(_)))
    };

    let info = ProcessInfo {
        used_primitives: ctx.used_primitives,
        branch_filters: ctx.branch_filters,
        needs_channel_idx,
        diagnostics: ctx.diagnostics,
        is_instrument,
        voice_count,
        oscillator_count,
        has_adsr,
        chorus_count,
        compressor_count,
        delay_count,
    };

    (process_body, info)
}

fn generate_monophonic_instrument_process(
    smoothed_params: &[String],
    stmt_lines: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("        let mut next_event = context.next_event();\n");
    out.push_str("        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");

    for param_name in smoothed_params {
        out.push_str(&format!(
            "            let {param_name} = self.params.{param_name}.smoothed.next();\n"
        ));
    }

    let midi_loop = crate::codegen::midi::generate_midi_event_loop();
    for line in midi_loop.lines() {
        out.push_str("            ");
        out.push_str(line);
        out.push('\n');
    }

    for line in stmt_lines {
        out.push_str("            ");
        out.push_str(line);
        out.push('\n');
    }

    out.push_str("        }\n");
    out.push_str("        ProcessStatus::KeepAlive");
    out
}

fn generate_polyphonic_process(smoothed_params: &[String], stmt_lines: &[String], unison_config: Option<&crate::codegen::CodegenUnisonConfig>) -> String {
    let mut out = String::new();
    out.push_str("        let num_samples = buffer.samples();\n");
    out.push_str("        let output = buffer.as_slice();\n");
    out.push_str("        let mut next_event = context.next_event();\n");
    out.push_str("        let mut block_start: usize = 0;\n");
    out.push_str("        let mut block_end: usize = MAX_BLOCK_SIZE.min(num_samples);\n");
    out.push_str("        while block_start < num_samples {\n");

    let poly_handler = crate::codegen::midi::generate_polyphonic_event_handler(unison_config);
    for line in poly_handler.lines() {
        out.push_str("            ");
        out.push_str(line);
        out.push('\n');
    }

    out.push_str("            let block_len = block_end - block_start;\n");
    out.push_str("            for channel in output.iter_mut() {\n");
    out.push_str("                channel[block_start..block_end].fill(0.0);\n");
    out.push_str("            }\n");

    for param_name in smoothed_params {
        out.push_str(&format!(
            "            let mut {param_name} = [0.0_f32; MAX_BLOCK_SIZE];\n"
        ));
        out.push_str(&format!(
            "            self.params.{param_name}.smoothed.next_block(&mut {param_name}, block_len);\n"
        ));
    }

    out.push_str("            let mut terminated_voices = Vec::new();\n");
    out.push_str("            for voice in self.voices.iter_mut().filter_map(|voice| voice.as_mut()) {\n");
    out.push_str("                for value_idx in 0..block_len {\n");
    out.push_str("                    let sample_idx = block_start + value_idx;\n");
    for line in stmt_lines {
        out.push_str("                    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("                }\n");
    out.push_str("                if voice.releasing && voice_is_silent(voice) {\n");
    out.push_str("                    terminated_voices.push((voice.voice_id, voice.channel, voice.note));\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("            for (voice_id, channel, note) in terminated_voices {\n");
    out.push_str("                context.send_event(NoteEvent::VoiceTerminated {\n");
    out.push_str("                    timing: block_end as u32,\n");
    out.push_str("                    voice_id: Some(voice_id),\n");
    out.push_str("                    channel,\n");
    out.push_str("                    note,\n");
    out.push_str("                });\n");
    out.push_str("                if let Some(idx) = self.get_voice_idx(voice_id) {\n");
    out.push_str("                    self.voices[idx] = None;\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("            block_start = block_end;\n");
    out.push_str("            block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);\n");
    out.push_str("        }\n");
    out.push_str("        ProcessStatus::Normal");
    out
}

fn generate_effect_process(ctx: &ProcessContext, stmt_lines: &[String]) -> String {
    let mut out = String::new();

    let needs_channel_idx = ctx
        .used_primitives
        .iter()
        .any(|p| matches!(p, DspPrimitive::Filter(_)));

    out.push_str("        for channel_samples in buffer.iter_samples() {\n");

    for param_name in &ctx.smoothed_params {
        out.push_str(&format!(
            "            let {param_name} = self.params.{param_name}.smoothed.next();\n"
        ));
    }

    if needs_channel_idx {
        out.push_str(
            "            for (channel_idx, sample) in channel_samples.into_iter().enumerate() {\n",
        );
    } else {
        out.push_str("            for sample in channel_samples {\n");
    }
    for line in stmt_lines {
        out.push_str("                ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        ProcessStatus::Normal");

    out
}

fn find_process_block(plugin: &PluginDef) -> Option<&ProcessBlock> {
    for (item, _) in &plugin.items {
        if let PluginItem::ProcessBlock(pb) = item {
            return Some(pb);
        }
    }
    None
}

fn find_midi_decl(plugin: &PluginDef) -> bool {
    plugin
        .items
        .iter()
        .any(|(item, _)| matches!(item, PluginItem::MidiDecl(_)))
}

struct ProcessContext {
    smoothed_params: Vec<String>,
    used_primitives: HashSet<DspPrimitive>,
    pending_lines: Vec<String>,
    split_branch_vars: Vec<String>,
    split_counter: usize,
    current_branch: Option<(usize, usize)>,
    branch_filters: Vec<(usize, usize, crate::dsp::primitives::FilterKind)>,
    diagnostics: Vec<crate::diagnostic::Diagnostic>,
    oscillator_counter: usize,
    chorus_counter: usize,
    compressor_counter: usize,
    delay_counter: usize,
    is_instrument: bool,
    is_polyphonic: bool,
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
            chorus_counter: 0,
            compressor_counter: 0,
            delay_counter: 0,
            is_instrument: false,
            is_polyphonic: false,
        }
    }

    fn use_smoothed_param(&mut self, name: &str) {
        if !self.smoothed_params.contains(&name.to_string()) {
            self.smoothed_params.push(name.to_string());
        }
    }

    fn drain_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_lines)
    }
}

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
            if let Some(output_lines) = extract_output_chain(&expr.0, ctx) {
                let mut lines = ctx.drain_pending();
                lines.extend(output_lines);
                lines
            } else {
                let code = generate_chain_value(&expr.0, ctx);
                let mut lines = ctx.drain_pending();
                if is_last_in_block {
                    lines.push(format!("{};", code));
                } else {
                    lines.push(format!("{};", code));
                }
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

fn extract_output_chain(expr: &Expr, ctx: &mut ProcessContext) -> Option<Vec<String>> {
    if let Expr::Binary {
        left,
        op: BinOp::Chain,
        right,
    } = expr
    {
        if matches!(&right.0, Expr::Ident(name) if name == "output") {
            let source = generate_chain_value(&left.0, ctx);
            if ctx.is_polyphonic {
                return Some(vec![
                    format!("let output_sample = {};", source),
                    "for channel in output.iter_mut() { channel[sample_idx] += output_sample; }".to_string(),
                ]);
            }
            if ctx.is_instrument {
                return Some(vec![
                    format!("let output_sample = {};", source),
                    "for sample in channel_samples { *sample = output_sample; }".to_string(),
                ]);
            }
            return Some(vec![format!("*sample = {};", source)]);
        }
    }
    None
}

fn generate_chain_value(expr: &Expr, ctx: &mut ProcessContext) -> String {
    if let Expr::Binary {
        left,
        op: BinOp::Chain,
        right,
    } = expr
    {
        if matches!(&right.0, Expr::Ident(name) if name == "output") {
            return generate_chain_value(&left.0, ctx);
        }
        if matches!(&right.0, Expr::Merge) {
            let _ = generate_chain_value(&left.0, ctx);
            if ctx.split_branch_vars.is_empty() {
                return "0.0_f32".to_string();
            }
            let sum_expr = format!("({})", ctx.split_branch_vars.join(" + "));
            ctx.split_branch_vars.clear();
            return sum_expr;
        }
        if let Expr::Split { branches } = &right.0 {
            let input_code = generate_chain_value(&left.0, ctx);
            return generate_split_branches(&input_code, branches, ctx);
        }
        let input_code = generate_chain_value(&left.0, ctx);
        return generate_dsp_call_with_input(&right.0, &input_code, ctx);
    }

    generate_expr(expr, ctx)
}

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
                "notch" => generate_filter_call(input_code, "notch", args, ctx),
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
                "fold" => {
                    ctx.used_primitives.insert(DspPrimitive::Fold);
                    let amount = generate_expr_as_param(&args[0].0, ctx);
                    format!("({} * {}).sin()", input_code, amount)
                }
                "bitcrush" => {
                    ctx.used_primitives.insert(DspPrimitive::Bitcrush);
                    let bits = generate_expr_as_param(&args[0].0, ctx);
                    format!(
                        "{{ let step = 2.0_f32.powi({} as i32); ({} * step).round() / step }}",
                        bits, input_code
                    )
                }
                "chorus" => generate_chorus_call_with_input(input_code, args, ctx),
                "compressor" => generate_compressor_call_with_input(input_code, args, ctx),
                "delay" => generate_delay_call_with_input(input_code, args, ctx),
                _ => format!("{}({})", fn_name, input_code),
            };
        }
    }
    generate_expr(expr, ctx)
}

fn biquad_state_field(
    ctx: &mut ProcessContext,
    filter_kind: crate::dsp::primitives::FilterKind,
) -> String {
    if let Some((split_id, branch_idx)) = ctx.current_branch {
        ctx.branch_filters.push((split_id, branch_idx, filter_kind));
        if ctx.is_instrument {
            format!("self.split{}_branch{}_biquad[0]", split_id, branch_idx)
        } else {
            format!("self.split{}_branch{}_biquad[channel_idx]", split_id, branch_idx)
        }
    } else if ctx.is_polyphonic {
        "voice.biquad_state".to_string()
    } else if ctx.is_instrument {
        "self.biquad_state[0]".to_string()
    } else {
        "self.biquad_state[channel_idx]".to_string()
    }
}

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
        "notch" => crate::dsp::primitives::FilterKind::Notch,
        _ => crate::dsp::primitives::FilterKind::Lowpass,
    };
    ctx.used_primitives.insert(DspPrimitive::Filter(filter_kind));

    let cutoff = generate_expr_as_param(&args[0].0, ctx);
    let resonance = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.707".to_string()
    };

    let state_field = biquad_state_field(ctx, filter_kind);

    let fn_name = match filter_name {
        "bandpass" => "process_biquad_bandpass",
        "highpass" => "process_biquad_highpass",
        "notch" => "process_biquad_notch",
        _ => "process_biquad",
    };

    format!(
        "{}(&mut {}, {}, {}, {}, self.sample_rate)",
        fn_name, state_field, input_code, cutoff, resonance
    )
}

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
        ctx.pending_lines
            .push(format!("let mut {} = {};", branch_var, input_code));
        ctx.current_branch = Some((split_id, branch_idx));

        for (stmt, _) in branch_stmts {
            match stmt {
                Statement::Expr(expr) => {
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

        ctx.current_branch = None;
        branch_vars.push(branch_var);
    }

    ctx.split_branch_vars = branch_vars.clone();
    branch_vars
        .last()
        .cloned()
        .unwrap_or_else(|| "0.0_f32".to_string())
}

fn generate_branch_chain(expr: &Expr, branch_var: &str, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Binary {
            left,
            op: BinOp::Chain,
            right,
        } => {
            let input = generate_branch_chain(&left.0, branch_var, ctx);
            generate_dsp_call_with_input(&right.0, &input, ctx)
        }
        Expr::FnCall { .. } => generate_dsp_call_with_input(expr, branch_var, ctx),
        Expr::Ident(name) if name == "input" => branch_var.to_string(),
        _ => generate_expr(expr, ctx),
    }
}

fn generate_expr(expr: &Expr, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Number(n, _) => format!("{:.1}_f32", n),
        Expr::Bool(b) => format!("{}", b),
        Expr::StringLit(s) => format!("\"{}\"", s),
        Expr::Ident(name) => match name.as_str() {
            "input" => {
                if ctx.is_polyphonic {
                    "0.0_f32".to_string()
                } else {
                    "*sample".to_string()
                }
            }
            _ => name.clone(),
        },
        Expr::FieldAccess(base, field) => {
            if let Expr::Ident(base_name) = &base.0 {
                if base_name == "param" {
                    ctx.use_smoothed_param(field);
                    if ctx.is_polyphonic {
                        return format!("{}[value_idx]", field);
                    }
                    return field.clone();
                }
                if base_name == "note" {
                    return match field.as_str() {
                        "pitch" => {
                            if ctx.is_polyphonic {
                                "voice.note_freq".to_string()
                            } else {
                                "self.note_freq".to_string()
                            }
                        }
                        "velocity" => {
                            if ctx.is_polyphonic {
                                "voice.velocity".to_string()
                            } else {
                                "self.velocity".to_string()
                            }
                        }
                        "gate" => {
                            if ctx.is_polyphonic {
                                "if !voice.releasing { 1.0_f32 } else { 0.0_f32 }".to_string()
                            } else {
                                "if self.active_note.is_some() { 1.0_f32 } else { 0.0_f32 }".to_string()
                            }
                        }
                        "pressure" => {
                            if ctx.is_polyphonic {
                                "voice.pressure".to_string()
                            } else {
                                "0.0_f32".to_string()
                            }
                        }
                        "bend" => {
                            if ctx.is_polyphonic {
                                "voice.tuning".to_string()
                            } else {
                                "0.0_f32".to_string()
                            }
                        }
                        "slide" => {
                            if ctx.is_polyphonic {
                                "voice.slide".to_string()
                            } else {
                                "0.0_f32".to_string()
                            }
                        }
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
                        let state_field =
                            biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Lowpass);
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
                        let state_field =
                            biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Bandpass);
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
                        let state_field =
                            biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Highpass);
                        return format!(
                            "process_biquad_highpass(&mut {}, *sample, {}, {}, self.sample_rate)",
                            state_field, cutoff, resonance
                        );
                    }
                    "notch" => {
                        ctx.used_primitives.insert(DspPrimitive::Filter(
                            crate::dsp::primitives::FilterKind::Notch,
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
                        let state_field =
                            biquad_state_field(ctx, crate::dsp::primitives::FilterKind::Notch);
                        return format!(
                            "process_biquad_notch(&mut {}, *sample, {}, {}, self.sample_rate)",
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
                    "saw" | "square" | "sine" | "triangle" => {
                        return generate_osc_call(fn_name, args, ctx)
                    }
                    "lfo" => return generate_lfo_call(args, ctx),
                    "pulse" => return generate_pulse_call(args, ctx),
                    "fold" => {
                        ctx.used_primitives.insert(DspPrimitive::Fold);
                        let amount = generate_expr_as_param(&args[0].0, ctx);
                        return format!("(*sample * {}).sin()", amount);
                    }
                    "bitcrush" => {
                        ctx.used_primitives.insert(DspPrimitive::Bitcrush);
                        let bits = generate_expr_as_param(&args[0].0, ctx);
                        return format!(
                            "{{ let step = 2.0_f32.powi({} as i32); (*sample * step).round() / step }}",
                            bits
                        );
                    }
                    "chorus" => return generate_chorus_call_with_input("*sample", args, ctx),
                    "compressor" => {
                        return generate_compressor_call_with_input("*sample", args, ctx)
                    }
                    "delay" => return generate_delay_call_with_input("*sample", args, ctx),
                    "adsr" => return generate_adsr_call(args, ctx),
                    "semitones_to_ratio" => {
                        ctx.used_primitives.insert(DspPrimitive::SemitonesToRatio);
                        let semitones = generate_expr(&args[0].0, ctx);
                        return format!("2.0_f32.powf({} / 12.0)", semitones);
                    }
                    _ => {}
                }
            }
            let callee_code = generate_expr(&callee.0, ctx);
            let args_code: Vec<String> = args.iter().map(|(a, _)| generate_expr(a, ctx)).collect();
            format!("{}({})", callee_code, args_code.join(", "))
        }
        Expr::Binary { left, op, right } => {
            if *op == BinOp::Chain {
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
                BinOp::Chain => "->",
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
        Expr::If {
            condition,
            then_body,
            then_expr,
            else_body,
        } => generate_if_expr(condition, then_body, then_expr, else_body.as_ref(), ctx),
        Expr::Grouped(inner) => {
            let inner_code = generate_expr(&inner.0, ctx);
            format!("({})", inner_code)
        }
        Expr::Split { branches } => {
            if ctx.is_polyphonic {
                generate_split_branches("0.0_f32", branches, ctx)
            } else {
                generate_split_branches("*sample", branches, ctx)
            }
        }
        Expr::Merge => {
            if ctx.split_branch_vars.is_empty() {
                "0.0_f32".to_string()
            } else {
                let sum = format!("({})", ctx.split_branch_vars.join(" + "));
                ctx.split_branch_vars.clear();
                sum
            }
        }
        _ => {
            ctx.diagnostics.push(
                crate::diagnostic::Diagnostic::error(
                    "E011",
                    crate::span::Span::new(0, 0),
                    format!(
                        "Unsupported expression in codegen: {:?}",
                        std::mem::discriminant(expr)
                    ),
                )
                .with_suggestion("This language construct is not yet supported in code generation"),
            );
            "0.0_f32 /* unsupported */".to_string()
        }
    }
}

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

    let state_target = if ctx.is_polyphonic {
        format!("voice.osc_state_{}", osc_idx)
    } else {
        format!("self.osc_state_{}", osc_idx)
    };

    format!(
        "{}(&mut {}, {}, self.sample_rate)",
        process_fn, state_target, freq
    )
}

fn generate_lfo_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    ctx.used_primitives.insert(DspPrimitive::Lfo);
    ctx.used_primitives
        .insert(DspPrimitive::Oscillator(crate::dsp::primitives::OscKind::Sine));

    let osc_idx = ctx.oscillator_counter;
    ctx.oscillator_counter += 1;

    let rate = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "1.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.osc_state_{}", osc_idx)
    } else {
        format!("self.osc_state_{}", osc_idx)
    };

    format!(
        "process_osc_sine(&mut {}, {}, self.sample_rate)",
        state_target, rate
    )
}

fn generate_pulse_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    ctx.used_primitives.insert(DspPrimitive::Pulse);

    let osc_idx = ctx.oscillator_counter;
    ctx.oscillator_counter += 1;

    let freq = if !args.is_empty() {
        generate_expr(&args[0].0, ctx)
    } else {
        "440.0_f32".to_string()
    };

    let width = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.osc_state_{}", osc_idx)
    } else {
        format!("self.osc_state_{}", osc_idx)
    };

    format!(
        "process_osc_pulse(&mut {}, {}, {}, self.sample_rate)",
        state_target, freq, width
    )
}

fn generate_adsr_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    ctx.used_primitives
        .insert(DspPrimitive::Envelope(EnvKind::Adsr));

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

    let gate = if ctx.is_polyphonic {
        "if !voice.releasing { 1.0_f32 } else { 0.0_f32 }"
    } else {
        "if self.active_note.is_some() { 1.0_f32 } else { 0.0_f32 }"
    };
    let state_target = if ctx.is_polyphonic {
        "voice.adsr_state"
    } else {
        "self.adsr_state"
    };

    format!(
        "process_adsr(&mut {}, {}, {}, {}, {}, {}, self.sample_rate)",
        state_target, gate, attack, decay, sustain, release
    )
}

fn generate_expr_as_param(expr: &Expr, ctx: &mut ProcessContext) -> String {
    if let Expr::FieldAccess(base, field) = expr {
        if let Expr::Ident(base_name) = &base.0 {
            if base_name == "param" {
                ctx.use_smoothed_param(field);
                if ctx.is_polyphonic {
                    return format!("{}[value_idx]", field);
                }
                return field.clone();
            }
        }
    }
    generate_expr(expr, ctx)
}

fn generate_chorus_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Chorus);

    let chorus_idx = ctx.chorus_counter;
    ctx.chorus_counter += 1;

    let rate = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "1.0_f32".to_string()
    };

    let depth = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.chorus_state_{}", chorus_idx)
    } else {
        format!("self.chorus_state_{}", chorus_idx)
    };

    format!(
        "process_chorus(&mut {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, rate, depth
    )
}

fn generate_compressor_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Compressor);

    let comp_idx = ctx.compressor_counter;
    ctx.compressor_counter += 1;

    let threshold = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let ratio = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "4.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.compressor_state_{}", comp_idx)
    } else {
        format!("self.compressor_state_{}", comp_idx)
    };

    format!(
        "process_compressor(&mut {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, threshold, ratio
    )
}

fn generate_delay_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Delay);

    let delay_idx = ctx.delay_counter;
    ctx.delay_counter += 1;

    // delay(time: Time) — time in seconds (unit literals like 0.5s are already converted)
    let delay_time = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.delay_state_{}", delay_idx)
    } else {
        format!("self.delay_state_{}", delay_idx)
    };

    format!(
        "process_delay(&mut {}, {}, {}, self.sample_rate)",
        state_target, input_code, delay_time
    )
}
