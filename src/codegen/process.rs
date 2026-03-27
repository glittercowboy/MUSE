//! Generates the process() method body from the AST's ProcessBlock.
//!
//! Supports:
//! - Simple chains: `input -> gain(param.gain) -> output`
//! - Let bindings: `let filtered = input -> lowpass(param.cutoff, param.resonance)`
//! - If-expressions: `if param.drive > 0.0 { ... } else { ... }`
//! - Multi-DSP chains: lowpass, gain, tanh, mix
//! - Split/merge parallel routing: `split { branch1; branch2 } -> merge`

use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, ElseBody, Expr, FnDef, IoDirection, PluginDef, PluginItem, ProcessBlock, Spanned,
    Statement, UnaryOp,
};
use crate::codegen::SampleInfo;
use crate::codegen::WavetableInfo;
use crate::dsp::primitives::{DspPrimitive, EnvKind, EqKind, OscKind};

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
    pub reverb_count: usize,
    pub delay_count: usize,
    pub eq_biquad_count: usize,
    pub rms_count: usize,
    pub peak_follow_count: usize,
    pub gate_count: usize,
    pub dc_block_count: usize,
    pub sample_hold_count: usize,
    pub pattern_count: usize,
    /// For each pattern instance: (index, values as Vec<f64>)
    pub pattern_values: Vec<(usize, Vec<f64>)>,
    pub play_call_count: usize,
    pub wt_osc_call_count: usize,
    pub loop_call_count: usize,
    pub needs_transport: bool,
    pub oversample_count: usize,
    /// (index, factor) pairs for each oversample block
    pub oversample_factors: Vec<(usize, u32)>,
    /// State variable declarations: (name, type, default_expr_code)
    pub state_decls: Vec<(String, crate::ast::StateType, String)>,
    /// Whether `sample_rate` is referenced directly in expressions
    pub uses_sample_rate_directly: bool,
}

/// A (field_prefix, rust_type, count) descriptor for generating DSP state fields.
pub struct StateSlot<'a> {
    pub prefix: &'a str,
    pub type_name: &'a str,
    pub count: usize,
}

impl ProcessInfo {
    /// Returns descriptors for the "simple" indexed DSP state fields —
    /// those that follow the `{prefix}_{i}: {Type}` pattern.
    /// Special cases (delay, eq_biquad, play, loop) are handled separately.
    pub fn simple_state_slots(&self) -> Vec<StateSlot<'_>> {
        [
            ("osc_state",          "OscState",           self.oscillator_count),
            ("chorus_state",       "ChorusState",        self.chorus_count),
            ("compressor_state",   "CompressorState",    self.compressor_count),
            ("rms_state",          "RmsState",           self.rms_count),
            ("peak_follow_state",  "PeakFollowState",    self.peak_follow_count),
            ("gate_state",         "GateState",          self.gate_count),
            ("dc_block_state",     "DcBlockState",       self.dc_block_count),
            ("sample_hold_state",  "SampleAndHoldState", self.sample_hold_count),
            ("wt_osc_state",       "WtOscState",         self.wt_osc_call_count),
            ("reverb_state",       "ReverbState",        self.reverb_count),
        ].into_iter()
         .filter(|(_, _, count)| *count > 0)
         .map(|(prefix, type_name, count)| StateSlot { prefix, type_name, count })
         .collect()
    }

    /// Returns true if any DSP primitive requires knowing sample_rate at runtime.
    pub fn needs_sample_rate(&self, needs_any_biquad: bool) -> bool {
        needs_any_biquad || self.is_instrument || self.oscillator_count > 0
            || self.chorus_count > 0 || self.compressor_count > 0 || self.delay_count > 0
            || self.eq_biquad_count > 0 || self.rms_count > 0 || self.peak_follow_count > 0
            || self.gate_count > 0 || self.wt_osc_call_count > 0 || self.reverb_count > 0
            || self.needs_transport || self.uses_sample_rate_directly
    }
}

/// Emit struct field declarations for each slot: `    {prefix}_{i}: {Type},\n`
pub fn emit_state_fields(slots: &[StateSlot<'_>], indent: &str) -> String {
    let mut out = String::new();
    for slot in slots {
        for i in 0..slot.count {
            out.push_str(&format!("{indent}{}_{}: {},\n", slot.prefix, i, slot.type_name));
        }
    }
    out
}

/// Emit default initializers for each slot: `    {prefix}_{i}: {Type}::default(),\n`
pub fn emit_state_defaults(slots: &[StateSlot<'_>], indent: &str) -> String {
    let mut out = String::new();
    for slot in slots {
        for i in 0..slot.count {
            out.push_str(&format!("{indent}{}_{}: {}::default(),\n", slot.prefix, i, slot.type_name));
        }
    }
    out
}

/// Emit play/loop position+active fields: `    {kind}_pos_{i}: f32,\n    {kind}_active_{i}: bool,\n`
pub fn emit_playback_fields(kind: &str, count: usize, indent: &str) -> String {
    let mut out = String::new();
    for i in 0..count {
        out.push_str(&format!("{indent}{kind}_pos_{i}: f32,\n"));
        out.push_str(&format!("{indent}{kind}_active_{i}: bool,\n"));
    }
    out
}

/// Emit play/loop default initializers.
pub fn emit_playback_defaults(kind: &str, count: usize, indent: &str, active_default: &str) -> String {
    let mut out = String::new();
    for i in 0..count {
        out.push_str(&format!("{indent}{kind}_pos_{i}: 0.0,\n"));
        out.push_str(&format!("{indent}{kind}_active_{i}: {active_default},\n"));
    }
    out
}


pub fn generate_process(plugin: &PluginDef, voice_count: Option<u32>, unison_config: Option<&crate::codegen::CodegenUnisonConfig>, sample_infos: &[SampleInfo], wavetable_infos: &[WavetableInfo]) -> (String, ProcessInfo) {
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
                    reverb_count: 0,
                    delay_count: 0,
                    eq_biquad_count: 0,
                    rms_count: 0,
                    peak_follow_count: 0,
                    gate_count: 0,
                    dc_block_count: 0,
                    sample_hold_count: 0,
                    pattern_count: 0,
                    pattern_values: Vec::new(),
                    play_call_count: 0,
                    wt_osc_call_count: 0,
                    loop_call_count: 0,
                    needs_transport: false,
                    oversample_count: 0,
                    oversample_factors: Vec::new(),
                    state_decls: Vec::new(),
                    uses_sample_rate_directly: false,
                },
            )
        }
    };

    let mut ctx = ProcessContext::new(sample_infos, wavetable_infos);
    ctx.is_instrument = is_instrument;
    ctx.is_polyphonic = is_instrument && voice_count.is_some();

    // Collect user-defined function declarations
    for (item, _) in &plugin.items {
        if let PluginItem::FnDef(fn_def) = item {
            ctx.fn_defs.insert(fn_def.name.clone(), fn_def.clone());
        }
    }

    // Build aux input/output maps from IoDecl items (same 3-way classification as extract_plugin_info)
    {
        let mut aux_in_idx: usize = 0;
        let mut aux_out_idx: usize = 0;
        for (item, _) in &plugin.items {
            if let PluginItem::IoDecl(io) = item {
                let effective_name = io.name.as_deref().unwrap_or("main");
                if effective_name != "main" {
                    match io.direction {
                        IoDirection::Input => {
                            ctx.aux_input_map.insert(effective_name.to_string(), aux_in_idx);
                            aux_in_idx += 1;
                        }
                        IoDirection::Output => {
                            ctx.aux_output_map.insert(effective_name.to_string(), aux_out_idx);
                            aux_out_idx += 1;
                        }
                    }
                }
            }
        }
    }

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
    let reverb_count = ctx.reverb_counter;
    let delay_count = ctx.delay_counter;
    let eq_biquad_count = ctx.eq_biquad_counter;
    let rms_count = ctx.rms_counter;
    let peak_follow_count = ctx.peak_follow_counter;
    let gate_count = ctx.gate_counter;
    let dc_block_count = ctx.dc_block_counter;
    let sample_hold_count = ctx.sample_hold_counter;
    let pattern_count = ctx.pattern_counter;

    let mut process_body = String::new();

    // Inject transport bindings when beat/tempo features are used
    if ctx.needs_transport {
        process_body.push_str("        let transport = context.transport();\n");
        process_body.push_str("        let tempo = transport.tempo.unwrap_or(120.0) as f32;\n");
    }

    process_body.push_str(&if ctx.is_polyphonic {
        generate_polyphonic_process(&ctx.smoothed_params, &stmt_lines, unison_config)
    } else if is_instrument {
        generate_monophonic_instrument_process(&ctx.smoothed_params, &stmt_lines, ctx.play_counter, ctx.wt_osc_counter, ctx.loop_counter)
    } else {
        generate_effect_process(&ctx, &stmt_lines)
    });

    let needs_channel_idx = if ctx.is_polyphonic {
        false
    } else {
        ctx.used_primitives
            .iter()
            .any(|p| matches!(p, DspPrimitive::Filter(_) | DspPrimitive::EqFilter(_)))
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
        reverb_count,
        delay_count,
        eq_biquad_count,
        rms_count,
        peak_follow_count,
        gate_count,
        dc_block_count,
        sample_hold_count,
        pattern_count,
        pattern_values: ctx.pattern_values.clone(),
        play_call_count: ctx.play_counter,
        wt_osc_call_count: ctx.wt_osc_counter,
        loop_call_count: ctx.loop_counter,
        needs_transport: ctx.needs_transport,
        oversample_count: ctx.oversample_counter,
        oversample_factors: ctx.oversample_factors,
        state_decls: ctx.state_decls.clone(),
        uses_sample_rate_directly: ctx.uses_sample_rate_directly,
    };

    (process_body, info)
}

fn generate_monophonic_instrument_process(
    smoothed_params: &[String],
    stmt_lines: &[String],
    play_call_count: usize,
    wt_osc_call_count: usize,
    loop_call_count: usize,
) -> String {
    let mut out = String::new();
    out.push_str("        let mut next_event = context.next_event();\n");
    out.push_str("        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");

    for param_name in smoothed_params {
        out.push_str(&format!(
            "            let {param_name} = self.params.{param_name}.smoothed.next();\n"
        ));
    }

    let midi_loop = crate::codegen::midi::generate_midi_event_loop(play_call_count, wt_osc_call_count, loop_call_count);
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
        .any(|p| matches!(p, DspPrimitive::Filter(_) | DspPrimitive::EqFilter(_)));

    let has_aux_inputs = !ctx.aux_input_map.is_empty();

    // Bind aux input slices BEFORE the outer loop to avoid borrow conflicts
    if has_aux_inputs {
        // Collect and sort by index for deterministic ordering
        let mut aux_entries: Vec<(&String, &usize)> = ctx.aux_input_map.iter().collect();
        aux_entries.sort_by_key(|(_, idx)| **idx);
        for (name, idx) in &aux_entries {
            out.push_str(&format!(
                "        let {}_slices = aux.inputs[{}].as_slice_immutable();\n",
                name, idx
            ));
        }
    }

    if has_aux_inputs {
        out.push_str("        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {\n");
    } else {
        out.push_str("        for channel_samples in buffer.iter_samples() {\n");
    }

    // Emit per-sample aux reads at top of outer loop body
    if has_aux_inputs {
        let mut aux_entries: Vec<(&String, &usize)> = ctx.aux_input_map.iter().collect();
        aux_entries.sort_by_key(|(_, idx)| **idx);
        for (name, _) in &aux_entries {
            out.push_str(&format!(
                "            let {}_sample = {}_slices[0][sample_idx];\n",
                name, name
            ));
        }
    }

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

struct ProcessContext<'a> {
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
    reverb_counter: usize,
    delay_counter: usize,
    eq_biquad_counter: usize,
    rms_counter: usize,
    peak_follow_counter: usize,
    gate_counter: usize,
    dc_block_counter: usize,
    sample_hold_counter: usize,
    pattern_counter: usize,
    pattern_values: Vec<(usize, Vec<f64>)>,
    play_counter: usize,
    wt_osc_counter: usize,
    loop_counter: usize,
    oversample_counter: usize,
    oversample_factors: Vec<(usize, u32)>,
    _sample_infos: &'a [SampleInfo],
    _wavetable_infos: &'a [WavetableInfo],
    is_instrument: bool,
    is_polyphonic: bool,
    aux_input_map: HashMap<String, usize>,
    aux_output_map: HashMap<String, usize>,
    needs_transport: bool,
    /// User-defined function declarations for inlining at call sites.
    fn_defs: HashMap<String, FnDef>,
    /// State declarations collected during process generation.
    state_decls: Vec<(String, crate::ast::StateType, String)>,
    /// Set of declared state variable names (for codegen prefix lookup).
    state_var_names: HashSet<String>,
    /// Whether `sample_rate` is referenced directly in expressions.
    uses_sample_rate_directly: bool,
}

impl<'a> ProcessContext<'a> {
    fn new(sample_infos: &'a [SampleInfo], wavetable_infos: &'a [WavetableInfo]) -> Self {
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
            reverb_counter: 0,
            delay_counter: 0,
            eq_biquad_counter: 0,
            rms_counter: 0,
            peak_follow_counter: 0,
            gate_counter: 0,
            dc_block_counter: 0,
            sample_hold_counter: 0,
            pattern_counter: 0,
            pattern_values: Vec::new(),
            play_counter: 0,
            wt_osc_counter: 0,
            loop_counter: 0,
            oversample_counter: 0,
            oversample_factors: Vec::new(),
            _sample_infos: sample_infos,
            _wavetable_infos: wavetable_infos,
            is_instrument: false,
            is_polyphonic: false,
            aux_input_map: HashMap::new(),
            aux_output_map: HashMap::new(),
            needs_transport: false,
            fn_defs: HashMap::new(),
            state_decls: Vec::new(),
            state_var_names: HashSet::new(),
            uses_sample_rate_directly: false,
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
        Statement::StateDecl { name, state_type, default } => {
            // State declarations don't generate per-sample code — they become struct fields.
            // We record them in the context so plugin.rs can generate the fields.
            let default_code = generate_expr(&default.0, ctx);
            ctx.state_decls.push((name.clone(), state_type.clone(), default_code));
            ctx.state_var_names.insert(name.clone());
            Vec::new()
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
            // State variable assignments use the appropriate prefix
            let prefix = if ctx.state_var_names.contains(target) {
                if ctx.is_polyphonic {
                    format!("voice.state_{}", target)
                } else {
                    format!("self.state_{}", target)
                }
            } else {
                target.clone()
            };
            lines.push(format!("{} = {};", prefix, expr_code));
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
        if let Expr::Oversample { factor, body } = &right.0 {
            let input_code = generate_chain_value(&left.0, ctx);
            return generate_oversample_block(&input_code, *factor, body, ctx);
        }
        let input_code = generate_chain_value(&left.0, ctx);
        return generate_dsp_call_with_input(&right.0, &input_code, ctx);
    }

    generate_expr(expr, ctx)
}

fn generate_dsp_call_with_input(expr: &Expr, input_code: &str, ctx: &mut ProcessContext) -> String {
    if let Expr::FnCall { callee, args } = expr {
        if let Expr::Ident(fn_name) = &callee.0 {
            // Check user-defined functions first — inline the body
            if let Some(fn_def) = ctx.fn_defs.get(fn_name).cloned() {
                return generate_user_fn_inline(input_code, &fn_def, args, ctx);
            }
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
                "reverb" => generate_reverb_call_with_input(input_code, args, ctx),
                "delay" => generate_delay_call_with_input(input_code, args, ctx),
                "mod_delay" => generate_mod_delay_call_with_input(input_code, args, ctx),
                "allpass" => generate_allpass_call_with_input(input_code, args, ctx),
                "comb" => generate_comb_call_with_input(input_code, args, ctx),
                "peak_eq" => generate_eq_call_with_input(input_code, "peak_eq", args, ctx),
                "low_shelf" => generate_eq_call_with_input(input_code, "low_shelf", args, ctx),
                "high_shelf" => generate_eq_call_with_input(input_code, "high_shelf", args, ctx),
                "rms" => generate_rms_call_with_input(input_code, args, ctx),
                "peak_follow" => generate_peak_follow_call_with_input(input_code, args, ctx),
                "gate" => generate_gate_call_with_input(input_code, args, ctx),
                "soft_clip" => {
                    ctx.used_primitives.insert(DspPrimitive::SoftClip);
                    let drive = generate_expr_as_param(&args[0].0, ctx);
                    format!("{{ let __x = {} * {}; __x / (1.0 + __x.abs()) }}", input_code, drive)
                }
                "dc_block" => generate_dc_block_call_with_input(input_code, args, ctx),
                "sample_and_hold" => generate_sample_hold_call_with_input(input_code, args, ctx),
                "play" => generate_play_call(args, ctx),
                "loop" => generate_loop_call(args, ctx),
                "wavetable_osc" => generate_wavetable_osc_call(args, ctx),
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

/// Generate inlined code for a user-defined function call in chain context.
///
/// For `fn saturate(amt, cutoff) -> processor { gain(amt) -> tanh() -> lowpass(cutoff) }`
/// called as `input -> saturate(param.drive, 2000Hz)`, generates the body chain
/// with arguments bound to local variables and input_code piped through.
fn generate_user_fn_inline(
    input_code: &str,
    fn_def: &FnDef,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    // Generate argument expressions and bind them to fn param names via pending lines
    let mut param_bindings: Vec<(String, String)> = Vec::new();
    for (i, param) in fn_def.params.iter().enumerate() {
        let arg_code = if i < args.len() {
            generate_expr_as_param(&args[i].0, ctx)
        } else {
            "0.0_f32".to_string()
        };
        param_bindings.push((param.name.clone(), arg_code));
    }

    // Emit param bindings as pending lines
    for (name, code) in &param_bindings {
        ctx.pending_lines.push(format!("let {} = {};", name, code));
    }

    // Generate the body. The body is a list of statements; the last one is the result.
    // For single-expression bodies (the common case), we pipe input_code through the chain.
    if fn_def.body.len() == 1 {
        if let Statement::Expr(ref body_expr) = fn_def.body[0].0 {
            return generate_fn_body_chain(&body_expr.0, input_code, ctx);
        }
    }

    // Multi-statement body: emit intermediate statements as pending lines,
    // and use the last expression as the result.
    let last_idx = fn_def.body.len().saturating_sub(1);
    for (i, (stmt, _)) in fn_def.body.iter().enumerate() {
        if i < last_idx {
            let lines = generate_statement(stmt, false, ctx);
            ctx.pending_lines.extend(lines);
        }
    }

    // Last statement — generate as chain from input
    if let Some((last_stmt, _)) = fn_def.body.last() {
        match last_stmt {
            Statement::Expr(ref body_expr) => {
                return generate_fn_body_chain(&body_expr.0, input_code, ctx);
            }
            Statement::Return(ref body_expr) => {
                return generate_fn_body_chain(&body_expr.0, input_code, ctx);
            }
            _ => {}
        }
    }

    // Fallback: just return the input unchanged
    input_code.to_string()
}

/// Generate a chain expression from a user fn body, routing input_code as the starting signal.
///
/// Similar to `generate_branch_chain` but for user fn bodies.
fn generate_fn_body_chain(expr: &Expr, input_code: &str, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Binary {
            left,
            op: BinOp::Chain,
            right,
        } => {
            let input = generate_fn_body_chain(&left.0, input_code, ctx);
            generate_dsp_call_with_input(&right.0, &input, ctx)
        }
        Expr::FnCall { .. } => generate_dsp_call_with_input(expr, input_code, ctx),
        Expr::Ident(name) if name == "input" => input_code.to_string(),
        _ => generate_expr(expr, ctx),
    }
}

/// Generate code for an oversample block: upsample, process body N times, downsample.
fn generate_oversample_block(
    input_code: &str,
    factor: u32,
    body: &[Spanned<Statement>],
    ctx: &mut ProcessContext,
) -> String {
    let os_idx = ctx.oversample_counter;
    ctx.oversample_counter += 1;
    ctx.oversample_factors.push((os_idx, factor));
    ctx.used_primitives.insert(DspPrimitive::Oversample);

    // Generate body chain code using a temporary input variable
    // The body statements form a chain; the last expression-statement's value is the output
    let body_input_var = format!("__os_in_{}", os_idx);

    // Generate the body by processing each statement, collecting lines
    let mut body_lines: Vec<String> = Vec::new();
    let mut last_expr_code = body_input_var.clone();
    for (stmt, _) in body {
        match stmt {
            Statement::Expr(expr) => {
                // Generate chain with our oversample input
                last_expr_code = generate_dsp_call_with_input_or_chain(&expr.0, &last_expr_code, ctx);
            }
            Statement::Let { name, value } => {
                let val_code = generate_expr(&value.0, ctx);
                body_lines.push(format!("let {} = {};", name, val_code));
            }
            _ => {}
        }
    }

    // Generate the oversample processing loop
    let state_name = format!("self.oversample_state_{}", os_idx);
    format!(
        "{{ \
let __os_input = {input}; \
let __os_up = {state}.upsample(__os_input); \
let mut __os_result = 0.0_f32; \
for __os_i in 0..{factor}usize {{ \
let {body_in} = __os_up[__os_i]; \
{body_extra}\
let __os_processed = {body_out}; \
__os_result = {state}.downsample_accumulate(__os_processed, __os_i); \
}} \
__os_result \
}}",
        input = input_code,
        state = state_name,
        factor = factor,
        body_in = body_input_var,
        body_extra = if body_lines.is_empty() {
            String::new()
        } else {
            body_lines.join(" ") + " "
        },
        body_out = last_expr_code,
    )
}

/// Helper: either generate a chain value or a DSP call with input depending on expression type.
fn generate_dsp_call_with_input_or_chain(expr: &Expr, input_code: &str, ctx: &mut ProcessContext) -> String {
    if let Expr::Binary { left, op: BinOp::Chain, right } = expr {
        // It's a chain — recursively process
        let left_code = generate_dsp_call_with_input_or_chain(&left.0, input_code, ctx);
        generate_dsp_call_with_input_or_chain(&right.0, &left_code, ctx)
    } else {
        generate_dsp_call_with_input(expr, input_code, ctx)
    }
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
        Expr::Ident(name) if ctx.aux_input_map.contains_key(name) => {
            format!("{}_sample", name)
        }
        _ => generate_expr(expr, ctx),
    }
}

fn generate_expr(expr: &Expr, ctx: &mut ProcessContext) -> String {
    match expr {
        Expr::Number(n, Some(crate::ast::UnitSuffix::Beat)) => {
            ctx.needs_transport = true;
            // Convert beats to samples: beats * 60.0 / tempo * sample_rate
            format!("({:.1}_f32 * 60.0 / tempo * self.sample_rate)", n)
        }
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
            "tempo" => {
                ctx.needs_transport = true;
                "tempo".to_string()
            }
            "beat_position" => {
                ctx.needs_transport = true;
                "(transport.pos_beats().unwrap_or(0.0)) as f32".to_string()
            }
            "sample_rate" => {
                ctx.uses_sample_rate_directly = true;
                "self.sample_rate".to_string()
            }
            _ => {
                if ctx.state_var_names.contains(name) {
                    if ctx.is_polyphonic {
                        format!("voice.state_{}", name)
                    } else {
                        format!("self.state_{}", name)
                    }
                } else if ctx.aux_input_map.contains_key(name) {
                    format!("{}_sample", name)
                } else {
                    name.clone()
                }
            }
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
                        "number" => {
                            if ctx.is_polyphonic {
                                "voice.note as f32".to_string()
                            } else {
                                "self.active_note.unwrap_or(0) as f32".to_string()
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
                    "reverb" => {
                        return generate_reverb_call_with_input("*sample", args, ctx)
                    }
                    "delay" => return generate_delay_call_with_input("*sample", args, ctx),
                    "mod_delay" => return generate_mod_delay_call_with_input("*sample", args, ctx),
                    "allpass" => return generate_allpass_call_with_input("*sample", args, ctx),
                    "comb" => return generate_comb_call_with_input("*sample", args, ctx),
                    "peak_eq" => return generate_eq_call_with_input("*sample", "peak_eq", args, ctx),
                    "low_shelf" => return generate_eq_call_with_input("*sample", "low_shelf", args, ctx),
                    "high_shelf" => return generate_eq_call_with_input("*sample", "high_shelf", args, ctx),
                    "rms" => return generate_rms_call_with_input("*sample", args, ctx),
                    "peak_follow" => return generate_peak_follow_call_with_input("*sample", args, ctx),
                    "gate" => return generate_gate_call_with_input("*sample", args, ctx),
                    "soft_clip" => {
                        ctx.used_primitives.insert(DspPrimitive::SoftClip);
                        let drive = generate_expr_as_param(&args[0].0, ctx);
                        return format!("{{ let __x = *sample * {}; __x / (1.0 + __x.abs()) }}", drive);
                    }
                    "dc_block" => return generate_dc_block_call_with_input("*sample", args, ctx),
                    "sample_and_hold" => return generate_sample_hold_call_with_input("*sample", args, ctx),
                    "crossfade" => {
                        ctx.used_primitives.insert(DspPrimitive::Crossfade);
                        let a = generate_chain_value(&args[0].0, ctx);
                        let b = generate_chain_value(&args[1].0, ctx);
                        let mix = generate_expr_as_param(&args[2].0, ctx);
                        return format!("{{ let __a = {}; let __b = {}; let __m = {}; __a * (1.0 - __m).sqrt() + __b * __m.sqrt() }}", a, b, mix);
                    }
                    "adsr" => return generate_adsr_call(args, ctx),
                    "play" => return generate_play_call(args, ctx),
                    "loop" => return generate_loop_call(args, ctx),
                    "wavetable_osc" => return generate_wavetable_osc_call(args, ctx),
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
        Expr::Pattern { values, rate } => {
            generate_pattern_expr(values, rate, ctx)
        }
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
        Expr::Oversample { factor, body } => {
            // Standalone oversample (no input chain) — use *sample as input
            let input = if ctx.is_polyphonic {
                "0.0_f32"
            } else {
                "*sample"
            };
            generate_oversample_block(input, *factor, body, ctx)
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

/// Generate code for a wavetable_osc() call with table name, pitch, and position arguments.
///
/// Each call-site gets its own WtOscState (phase accumulator). The wavetable data,
/// frame_size, and frame_count are read from plugin struct fields populated in initialize().
fn generate_wavetable_osc_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    let wt_idx = ctx.wt_osc_counter;
    ctx.wt_osc_counter += 1;
    ctx.used_primitives.insert(DspPrimitive::WavetableOsc);

    // Extract wavetable name from the first argument (an Ident)
    let wt_name = if let Expr::Ident(name) = &args[0].0 {
        name.clone()
    } else {
        "unknown".to_string()
    };

    let pitch = generate_expr(&args[1].0, ctx);
    let position = generate_expr_as_param(&args[2].0, ctx);

    let state_field = if ctx.is_polyphonic {
        format!("voice.wt_osc_state_{}", wt_idx)
    } else {
        format!("self.wt_osc_state_{}", wt_idx)
    };

    format!(
        "process_wavetable_osc(&mut {}, &self.wavetable_{}, self.wavetable_{}_frame_size, self.wavetable_{}_frame_count, {}, {}, self.sample_rate)",
        state_field, wt_name, wt_name, wt_name, pitch, position
    )
}

/// Generate code for a play() call with a sample name argument.
///
/// Each play() call-site gets its own playback state (play_pos_N, play_active_N).
/// On each sample tick:
/// 1. If play_active_N is true, read from sample buffer at play_pos_N
/// 2. Advance position by 1.0
/// 3. If position >= buffer length, set play_active_N = false
/// 4. Return 0.0 when not playing
///
/// NoteOn handling (setting play_active = true, play_pos = 0.0) is done
/// in the MIDI event loop, which sets ALL play call-sites to active.
/// The note.number dispatch (if/else) ensures only the matching play() executes.
fn generate_play_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    let play_idx = ctx.play_counter;
    ctx.play_counter += 1;
    ctx.used_primitives.insert(DspPrimitive::Play);

    // Extract sample name from the first argument
    let sample_name = if let Expr::Ident(name) = &args[0].0 {
        name.clone()
    } else {
        "unknown".to_string()
    };

    let pos_field = if ctx.is_polyphonic {
        format!("voice.play_pos_{}", play_idx)
    } else {
        format!("self.play_pos_{}", play_idx)
    };
    let active_field = if ctx.is_polyphonic {
        format!("voice.play_active_{}", play_idx)
    } else {
        format!("self.play_active_{}", play_idx)
    };
    let buffer_field = if ctx.is_polyphonic {
        // In polyphonic mode, sample buffers are on self (shared), not per-voice
        format!("self.sample_{}", sample_name)
    } else {
        format!("self.sample_{}", sample_name)
    };

    format!(
        "if {} {{ let __pos = {} as usize; if __pos < {}.len() {{ let __s = {}[__pos]; {} += 1.0; __s }} else {{ {} = false; 0.0_f32 }} }} else {{ 0.0_f32 }}",
        active_field, pos_field, buffer_field, buffer_field, pos_field, active_field
    )
}

/// Generate code for loop() DSP primitive — continuous wraparound playback.
///
/// On each sample tick:
/// 1. If loop_active_N is true, read from sample buffer at loop_pos_N
/// 2. Advance position by 1.0
/// 3. If position >= buffer length, wrap position to 0.0 (or start for ranged)
/// 4. Return 0.0 when not active
///
/// 3-arg variant loop(sample, start, end) wraps within [start, end) range.
fn generate_loop_call(args: &[Spanned<Expr>], ctx: &mut ProcessContext) -> String {
    let loop_idx = ctx.loop_counter;
    ctx.loop_counter += 1;
    ctx.used_primitives.insert(DspPrimitive::Loop);

    // Extract sample name from the first argument
    let sample_name = if let Expr::Ident(name) = &args[0].0 {
        name.clone()
    } else {
        "unknown".to_string()
    };

    let pos_field = if ctx.is_polyphonic {
        format!("voice.loop_pos_{}", loop_idx)
    } else {
        format!("self.loop_pos_{}", loop_idx)
    };
    let active_field = if ctx.is_polyphonic {
        format!("voice.loop_active_{}", loop_idx)
    } else {
        format!("self.loop_active_{}", loop_idx)
    };
    let buffer_field = if ctx.is_polyphonic {
        format!("self.sample_{}", sample_name)
    } else {
        format!("self.sample_{}", sample_name)
    };

    if args.len() == 3 {
        // 3-arg: loop(sample, start, end) — wrap within [start, end) range
        let start_expr = generate_expr(&args[1].0, ctx);
        let end_expr = generate_expr(&args[2].0, ctx);
        format!(
            "if {} {{ let __pos = {} as usize; let __start = ({}) as usize; let __end = ({}) as usize; if __pos < {}.len() && __pos < __end {{ let __s = {}[__pos]; {} += 1.0; if {} as usize >= __end {{ {} = __start as f32; }} __s }} else {{ {} = __start as f32; 0.0_f32 }} }} else {{ 0.0_f32 }}",
            active_field, pos_field, start_expr, end_expr, buffer_field, buffer_field, pos_field, pos_field, pos_field, pos_field
        )
    } else {
        // 1-arg: loop(sample) — wrap to beginning
        format!(
            "if {} {{ let __pos = {} as usize; if __pos < {}.len() {{ let __s = {}[__pos]; {} += 1.0; if {} as usize >= {}.len() {{ {} = 0.0; }} __s }} else {{ {} = 0.0; 0.0_f32 }} }} else {{ 0.0_f32 }}",
            active_field, pos_field, buffer_field, buffer_field, pos_field, pos_field, buffer_field, pos_field, pos_field
        )
    }
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

fn generate_reverb_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Reverb);

    let reverb_idx = ctx.reverb_counter;
    ctx.reverb_counter += 1;

    let size = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let decay = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "2.0_f32".to_string()
    };

    let damping = if args.len() > 2 {
        generate_expr_as_param(&args[2].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let mix = if args.len() > 3 {
        generate_expr_as_param(&args[3].0, ctx)
    } else {
        "0.3_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.reverb_state_{}", reverb_idx)
    } else {
        format!("self.reverb_state_{}", reverb_idx)
    };

    format!(
        "process_reverb(&mut {}, {}, {}, {}, {}, {})",
        state_target, input_code, size, decay, damping, mix
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

    // Delay state is always on self (not per-voice) — see plugin.rs delay_state generation
    let state_target = format!("self.delay_state_{}", delay_idx);

    format!(
        "process_delay(&mut {}, {}, {}, self.sample_rate)",
        state_target, input_code, delay_time
    )
}

fn generate_mod_delay_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::ModDelay);

    let delay_idx = ctx.delay_counter;
    ctx.delay_counter += 1;

    let delay_time = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let depth = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.5_f32".to_string()
    };

    let rate = if args.len() > 2 {
        generate_expr_as_param(&args[2].0, ctx)
    } else {
        "1.0_f32".to_string()
    };

    // Delay state is always on self (not per-voice) — see plugin.rs delay_state generation
    let state_target = format!("self.delay_state_{}", delay_idx);

    format!(
        "process_mod_delay(&mut {}, {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, delay_time, depth, rate
    )
}

fn generate_allpass_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Allpass);

    let delay_idx = ctx.delay_counter;
    ctx.delay_counter += 1;

    let delay_time = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.01_f32".to_string()
    };

    let feedback = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.7_f32".to_string()
    };

    // Delay state is always on self (not per-voice) — see plugin.rs delay_state generation
    let state_target = format!("self.delay_state_{}", delay_idx);

    format!(
        "process_allpass(&mut {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, delay_time, feedback
    )
}

fn generate_comb_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Comb);

    let delay_idx = ctx.delay_counter;
    ctx.delay_counter += 1;

    let delay_time = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.01_f32".to_string()
    };

    let feedback = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.7_f32".to_string()
    };

    // Delay state is always on self (not per-voice) — see plugin.rs delay_state generation
    let state_target = format!("self.delay_state_{}", delay_idx);

    format!(
        "process_comb(&mut {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, delay_time, feedback
    )
}

fn generate_eq_call_with_input(
    input_code: &str,
    eq_name: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    let eq_kind = match eq_name {
        "peak_eq" => EqKind::PeakEq,
        "low_shelf" => EqKind::LowShelf,
        "high_shelf" => EqKind::HighShelf,
        _ => EqKind::PeakEq,
    };
    ctx.used_primitives.insert(DspPrimitive::EqFilter(eq_kind));

    let eq_idx = ctx.eq_biquad_counter;
    ctx.eq_biquad_counter += 1;

    let freq = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "1000.0_f32".to_string()
    };

    let gain_db = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "0.0_f32".to_string()
    };

    let q = if args.len() > 2 {
        generate_expr_as_param(&args[2].0, ctx)
    } else {
        "0.707_f32".to_string()
    };

    let state_field = if ctx.is_polyphonic {
        format!("voice.eq_biquad_state_{}", eq_idx)
    } else if ctx.is_instrument {
        format!("self.eq_biquad_state_{}[0]", eq_idx)
    } else {
        format!("self.eq_biquad_state_{}[channel_idx]", eq_idx)
    };

    let fn_name = match eq_name {
        "peak_eq" => "process_biquad_peak_eq",
        "low_shelf" => "process_biquad_low_shelf",
        "high_shelf" => "process_biquad_high_shelf",
        _ => "process_biquad_peak_eq",
    };

    format!(
        "{}(&mut {}, {}, {}, {}, {}, self.sample_rate)",
        fn_name, state_field, input_code, freq, gain_db, q
    )
}

fn generate_rms_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Rms);

    let rms_idx = ctx.rms_counter;
    ctx.rms_counter += 1;

    let window_ms = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "10.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.rms_state_{}", rms_idx)
    } else {
        format!("self.rms_state_{}", rms_idx)
    };

    format!(
        "process_rms(&mut {}, {}, {}, self.sample_rate)",
        state_target, input_code, window_ms
    )
}

fn generate_peak_follow_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::PeakFollow);

    let pf_idx = ctx.peak_follow_counter;
    ctx.peak_follow_counter += 1;

    let attack_ms = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "1.0_f32".to_string()
    };

    let release_ms = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "100.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.peak_follow_state_{}", pf_idx)
    } else {
        format!("self.peak_follow_state_{}", pf_idx)
    };

    format!(
        "process_peak_follow(&mut {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, attack_ms, release_ms
    )
}

fn generate_gate_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::Gate);

    let gate_idx = ctx.gate_counter;
    ctx.gate_counter += 1;

    let threshold_db = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "-40.0_f32".to_string()
    };

    let attack_ms = if args.len() > 1 {
        generate_expr_as_param(&args[1].0, ctx)
    } else {
        "1.0_f32".to_string()
    };

    let release_ms = if args.len() > 2 {
        generate_expr_as_param(&args[2].0, ctx)
    } else {
        "50.0_f32".to_string()
    };

    let hold_ms = if args.len() > 3 {
        generate_expr_as_param(&args[3].0, ctx)
    } else {
        "10.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.gate_state_{}", gate_idx)
    } else {
        format!("self.gate_state_{}", gate_idx)
    };

    format!(
        "process_gate(&mut {}, {}, {}, {}, {}, {}, self.sample_rate)",
        state_target, input_code, threshold_db, attack_ms, release_ms, hold_ms
    )
}

fn generate_dc_block_call_with_input(
    input_code: &str,
    _args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::DcBlock);

    let dc_idx = ctx.dc_block_counter;
    ctx.dc_block_counter += 1;

    let state_target = if ctx.is_polyphonic {
        format!("voice.dc_block_state_{}", dc_idx)
    } else {
        format!("self.dc_block_state_{}", dc_idx)
    };

    format!(
        "process_dc_block(&mut {}, {})",
        state_target, input_code
    )
}

fn generate_pattern_expr(
    values: &[f64],
    rate: &Spanned<Expr>,
    ctx: &mut ProcessContext,
) -> String {
    let pat_idx = ctx.pattern_counter;
    ctx.pattern_counter += 1;
    ctx.pattern_values.push((pat_idx, values.to_vec()));

    let rate_code = generate_expr_as_param(&rate.0, ctx);
    let num_steps = values.len();

    let state_target = if ctx.is_polyphonic {
        format!("voice.pattern_state_{}", pat_idx)
    } else {
        format!("self.pattern_state_{}", pat_idx)
    };

    // Emit phase advance + step index update as pending lines
    // so they execute before the expression value is used.
    ctx.pending_lines.push(format!(
        "{state}.phase += {rate} / self.sample_rate;",
        state = state_target,
        rate = rate_code,
    ));
    ctx.pending_lines.push(format!(
        "if {state}.phase >= 1.0 {{ {state}.phase -= 1.0; {state}.step_index = ({state}.step_index + 1) % {n}; }}",
        state = state_target,
        n = num_steps,
    ));

    // The expression value is the current step value
    format!(
        "{state}.values[{state}.step_index]",
        state = state_target,
    )
}

fn generate_sample_hold_call_with_input(
    input_code: &str,
    args: &[Spanned<Expr>],
    ctx: &mut ProcessContext,
) -> String {
    ctx.used_primitives.insert(DspPrimitive::SampleAndHold);

    let sh_idx = ctx.sample_hold_counter;
    ctx.sample_hold_counter += 1;

    let trigger = if !args.is_empty() {
        generate_expr_as_param(&args[0].0, ctx)
    } else {
        "0.0_f32".to_string()
    };

    let state_target = if ctx.is_polyphonic {
        format!("voice.sample_hold_state_{}", sh_idx)
    } else {
        format!("self.sample_hold_state_{}", sh_idx)
    };

    format!(
        "process_sample_and_hold(&mut {}, {}, {})",
        state_target, input_code, trigger
    )
}
