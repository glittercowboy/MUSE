//! Generates the Plugin struct, Plugin/ClapPlugin/Vst3Plugin trait impls, and export macros.

use crate::ast::{
    ClapItem, ChannelSpec, FormatBlock, IoDirection, MetadataKey, MetadataValue, PluginDef,
    PluginItem, Vst3Item,
};
use crate::codegen::process::{ProcessInfo, emit_state_fields, emit_state_defaults, emit_playback_fields, emit_playback_defaults};
use crate::codegen::SampleInfo;
use crate::codegen::WavetableInfo;
use crate::dsp::primitives::DspPrimitive;

struct PluginInfo {
    name: String,
    vendor: String,
    url: String,
    email: String,
    version: String,
    input_channels: u32,
    output_channels: u32,
    struct_name: String,
    /// Auxiliary input buses: (name, channel_count)
    aux_inputs: Vec<(String, u32)>,
    /// Auxiliary output buses: (name, channel_count)
    aux_outputs: Vec<(String, u32)>,
}

struct ClapInfo {
    id: String,
    description: String,
    features: Vec<String>,
}

struct Vst3Info {
    id: String,
    subcategories: Vec<String>,
}

pub fn generate_plugin_struct(plugin: &PluginDef, process_info: &ProcessInfo, sample_infos: &[SampleInfo], wavetable_infos: &[WavetableInfo]) -> String {
    let info = extract_plugin_info(plugin);
    let clap = extract_clap_info(plugin);
    let vst3 = extract_vst3_info(plugin);

    let used_primitives = &process_info.used_primitives;
    let is_instrument = process_info.is_instrument;
    let is_polyphonic = is_instrument && process_info.voice_count.is_some();
    let voice_count = process_info.voice_count.unwrap_or(0) as usize;

    let needs_top_level_biquad = used_primitives.iter().any(|p| matches!(p, DspPrimitive::Filter(_)))
        && process_info.branch_filters.is_empty();

    let mut branch_biquad_fields: Vec<(usize, usize)> = Vec::new();
    for &(split_id, branch_idx, _) in &process_info.branch_filters {
        let key = (split_id, branch_idx);
        if !branch_biquad_fields.contains(&key) {
            branch_biquad_fields.push(key);
        }
    }

    let needs_any_biquad = needs_top_level_biquad || !branch_biquad_fields.is_empty();
    let has_adsr = process_info.has_adsr;
    let needs_sample_rate = process_info.needs_sample_rate(needs_any_biquad);
    let simple_slots = process_info.simple_state_slots();
    let num_channels = info.output_channels.max(info.input_channels) as usize;
    let has_gui = crate::codegen::gui::find_gui_block(plugin).is_some();

    let mut out = String::new();

    if is_polyphonic {
        out.push_str(&generate_voice_struct(process_info));
        out.push('\n');
    }

    out.push_str(&format!("struct {} {{\n", info.struct_name));
    out.push_str("    params: Arc<PluginParams>,\n");
    if needs_top_level_biquad && !is_polyphonic {
        out.push_str(&format!("    biquad_state: [BiquadState; {}],\n", num_channels));
    }
    for &(split_id, branch_idx) in &branch_biquad_fields {
        out.push_str(&format!(
            "    split{}_branch{}_biquad: [BiquadState; {}],\n",
            split_id, branch_idx, num_channels
        ));
    }

    if is_polyphonic {
        out.push_str(&format!("    voices: [Option<Voice>; {}],\n", voice_count));
        out.push_str("    next_internal_voice_id: u64,\n");
    } else {
        if has_adsr {
            out.push_str("    adsr_state: AdsrState,\n");
        }
        out.push_str(&emit_state_fields(&simple_slots, "    "));
        // User-declared state variable fields (non-polyphonic: on plugin struct)
        for (name, state_type, _) in &process_info.state_decls {
            let rust_type = match state_type {
                crate::ast::StateType::Float => "f32",
                crate::ast::StateType::Int => "i32",
                crate::ast::StateType::Bool => "bool",
            };
            out.push_str(&format!("    state_{}: {},\n", name, rust_type));
        }
        for (idx, values) in &process_info.pattern_values {
            out.push_str(&format!("    pattern_state_{}: PatternState{},\n", idx, values.len()));
        }
    }

    // Delay state fields — outside the poly/mono guard (delays work in both effects and instruments)
    for i in 0..process_info.delay_count {
        out.push_str(&format!("    delay_state_{}: DelayLine,\n", i));
    }

    // Oversample state fields
    for &(idx, factor) in &process_info.oversample_factors {
        out.push_str(&format!("    oversample_state_{}: OversampleState, // {}x\n", idx, factor));
    }

    // EQ biquad state fields — per-call-site, per-channel (outside poly/mono guard)
    if !is_polyphonic {
        for i in 0..process_info.eq_biquad_count {
            out.push_str(&format!("    eq_biquad_state_{}: [BiquadState; {}],\n", i, num_channels));
        }
    }

    if is_instrument && !is_polyphonic {
        out.push_str("    active_note: Option<u8>,\n    note_freq: f32,\n    velocity: f32,\n");
    }
    if needs_sample_rate {
        out.push_str("    sample_rate: f32,\n");
    }
    for sample in sample_infos {
        out.push_str(&format!("    sample_{}: Vec<f32>,\n    sample_{}_rate: u32,\n", sample.name, sample.name));
    }
    if !is_polyphonic {
        out.push_str(&emit_playback_fields("play", process_info.play_call_count, "    "));
        out.push_str(&emit_playback_fields("loop", process_info.loop_call_count, "    "));
    }
    for wt in wavetable_infos {
        out.push_str(&format!("    wavetable_{}: Vec<f32>,\n    wavetable_{}_frame_size: usize,\n    wavetable_{}_frame_count: usize,\n", wt.name, wt.name, wt.name));
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "impl Default for {} {{\n    fn default() -> Self {{\n        Self {{\n            params: Arc::new(PluginParams::default()),\n",
        info.struct_name
    ));
    if needs_top_level_biquad && !is_polyphonic {
        out.push_str(&format!("            biquad_state: [BiquadState::default(); {}],\n", num_channels));
    }
    for &(split_id, branch_idx) in &branch_biquad_fields {
        out.push_str(&format!(
            "            split{}_branch{}_biquad: [BiquadState::default(); {}],\n",
            split_id, branch_idx, num_channels
        ));
    }

    if is_polyphonic {
        out.push_str(&format!("            voices: [(); {}].map(|_| None),\n", voice_count));
        out.push_str("            next_internal_voice_id: 0,\n");
    } else {
        if has_adsr {
            out.push_str("            adsr_state: AdsrState::default(),\n");
        }
        out.push_str(&emit_state_defaults(&simple_slots, "            "));
        // User-declared state variable defaults (non-polyphonic)
        for (name, _state_type, default_code) in &process_info.state_decls {
            out.push_str(&format!("            state_{}: {},\n", name, default_code));
        }
        for (idx, values) in &process_info.pattern_values {
            let values_str: Vec<String> = values.iter().map(|v| format!("{:.1}", v)).collect();
            out.push_str(&format!(
                "            pattern_state_{}: PatternState{} {{ phase: 0.0, step_index: 0, values: [{}] }},\n",
                idx,
                values.len(),
                values_str.join(", ")
            ));
        }
    }

    for i in 0..process_info.delay_count {
        out.push_str(&format!("            delay_state_{}: DelayLine::default(),\n", i));
    }

    for &(idx, factor) in &process_info.oversample_factors {
        out.push_str(&format!("            oversample_state_{}: OversampleState::new({}),\n", idx, factor));
    }

    if !is_polyphonic {
        for i in 0..process_info.eq_biquad_count {
            out.push_str(&format!("            eq_biquad_state_{}: [BiquadState::default(); {}],\n", i, num_channels));
        }
    }

    if is_instrument && !is_polyphonic {
        out.push_str("            active_note: None,\n            note_freq: 440.0,\n            velocity: 0.0,\n");
    }
    if needs_sample_rate {
        out.push_str("            sample_rate: 44100.0,\n");
    }
    for sample in sample_infos {
        out.push_str(&format!("            sample_{}: Vec::new(),\n            sample_{}_rate: 0,\n", sample.name, sample.name));
    }
    if !is_polyphonic {
        out.push_str(&emit_playback_defaults("play", process_info.play_call_count, "            ", "false"));
        out.push_str(&emit_playback_defaults("loop", process_info.loop_call_count, "            ", "false"));
    }
    for wt in wavetable_infos {
        out.push_str(&format!("            wavetable_{}: Vec::new(),\n            wavetable_{}_frame_size: 0,\n            wavetable_{}_frame_count: 0,\n", wt.name, wt.name, wt.name));
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str(&generate_plugin_trait(&info, needs_sample_rate, is_instrument, is_polyphonic, has_gui, process_info.delay_count, process_info.reverb_count, sample_infos, wavetable_infos, process_info.needs_transport));

    if is_polyphonic {
        let helper_defaults = generate_voice_field_defaults(process_info);
        let helpers = crate::codegen::midi::generate_voice_helper_methods()
            .replace("{STRUCT_NAME}", &info.struct_name)
            .replace("{VOICE_FIELD_DEFAULTS}", &helper_defaults);
        out.push_str(&helpers);
        out.push('\n');
    }

    if let Some(ref clap) = clap {
        out.push_str(&generate_clap_trait(&info, clap, process_info));
    }
    if let Some(ref vst3) = vst3 {
        out.push_str(&generate_vst3_trait(&info, vst3));
    }

    if clap.is_some() {
        out.push_str(&format!("nih_export_clap!({});\n", info.struct_name));
    }
    if vst3.is_some() {
        out.push_str(&format!("nih_export_vst3!({});\n", info.struct_name));
    }

    out
}

fn generate_voice_struct(process_info: &ProcessInfo) -> String {
    let mut out = String::new();
    out.push_str("#[derive(Clone, Copy)]\nstruct Voice {\n");
    out.push_str("    voice_id: i32,\n    channel: u8,\n    note: u8,\n    internal_voice_id: u64,\n");
    out.push_str("    note_freq: f32,\n    velocity: f32,\n    pressure: f32,\n    tuning: f32,\n    slide: f32,\n    releasing: bool,\n");

    let has_filters = process_info.used_primitives.iter().any(|p| matches!(p, DspPrimitive::Filter(_)));
    if has_filters {
        out.push_str("    biquad_state: BiquadState,\n");
    }
    if process_info.has_adsr {
        out.push_str("    adsr_state: AdsrState,\n");
    }
    // Voice uses simple BiquadState (not per-channel array) for eq_biquad
    let voice_slots = process_info.simple_state_slots();
    out.push_str(&emit_state_fields(&voice_slots, "    "));
    for (idx, values) in &process_info.pattern_values {
        out.push_str(&format!("    pattern_state_{}: PatternState{},\n", idx, values.len()));
    }
    for i in 0..process_info.eq_biquad_count {
        out.push_str(&format!("    eq_biquad_state_{}: BiquadState,\n", i));
    }
    out.push_str(&emit_playback_fields("play", process_info.play_call_count, "    "));
    out.push_str(&emit_playback_fields("loop", process_info.loop_call_count, "    "));
    // Per-voice user-declared state variables
    for (name, state_type, _) in &process_info.state_decls {
        let rust_type = match state_type {
            crate::ast::StateType::Float => "f32",
            crate::ast::StateType::Int => "i32",
            crate::ast::StateType::Bool => "bool",
        };
        out.push_str(&format!("    state_{}: {},\n", name, rust_type));
    }
    out.push_str("}\n");
    out
}

fn generate_voice_field_defaults(process_info: &ProcessInfo) -> String {
    let mut fields = Vec::new();
    let has_filters = process_info.used_primitives.iter().any(|p| matches!(p, DspPrimitive::Filter(_)));
    if has_filters {
        fields.push("biquad_state: BiquadState::default()".to_string());
    }
    if process_info.has_adsr {
        fields.push("adsr_state: AdsrState::default()".to_string());
    }
    let voice_slots = process_info.simple_state_slots();
    for (idx, values) in &process_info.pattern_values {
        let values_str: Vec<String> = values.iter().map(|v| format!("{:.1}", v)).collect();
        fields.push(format!(
            "pattern_state_{}: PatternState{} {{ phase: 0.0, step_index: 0, values: [{}] }}",
            idx,
            values.len(),
            values_str.join(", ")
        ));
    }
    for i in 0..process_info.eq_biquad_count {
        fields.push(format!("eq_biquad_state_{}: BiquadState::default()", i));
    }
    for slot in &voice_slots {
        for i in 0..slot.count {
            fields.push(format!("{}_{}: {}::default()", slot.prefix, i, slot.type_name));
        }
    }
    for i in 0..process_info.play_call_count {
        fields.push(format!("play_pos_{}: 0.0", i));
        fields.push(format!("play_active_{}: true", i));
    }
    for i in 0..process_info.loop_call_count {
        fields.push(format!("loop_pos_{}: 0.0", i));
        fields.push(format!("loop_active_{}: true", i));
    }
    // User-declared state variable defaults
    for (name, _state_type, default_code) in &process_info.state_decls {
        fields.push(format!("state_{}: {}", name, default_code));
    }
    if fields.is_empty() {
        String::new()
    } else {
        format!("{},", fields.join(", "))
    }
}

fn extract_plugin_info(plugin: &PluginDef) -> PluginInfo {
    let name = plugin.name.clone();
    let struct_name = plugin_name_to_struct(&name);

    let mut vendor = String::new();
    let mut url = String::new();
    let mut email = String::new();
    let mut version = "0.1.0".to_string();
    let mut input_channels = 2u32;
    let mut output_channels = 2u32;
    let mut aux_inputs: Vec<(String, u32)> = Vec::new();
    let mut aux_outputs: Vec<(String, u32)> = Vec::new();

    for (item, _) in &plugin.items {
        match item {
            PluginItem::Metadata(meta) => {
                let val = match &meta.value {
                    MetadataValue::StringVal(s) => s.clone(),
                    MetadataValue::Identifier(s) => s.clone(),
                };
                match meta.key {
                    MetadataKey::Vendor => vendor = val,
                    MetadataKey::Version => version = val,
                    MetadataKey::Url => url = val,
                    MetadataKey::Email => email = val,
                    MetadataKey::Category => {}
                }
            }
            PluginItem::IoDecl(io) => {
                let ch = channel_count(&io.channels);
                let effective_name = io.name.as_deref().unwrap_or("main");
                if effective_name == "main" {
                    // Main bus — assign to main channel counts (last one wins, matching existing behavior)
                    match io.direction {
                        IoDirection::Input => input_channels = ch,
                        IoDirection::Output => output_channels = ch,
                    }
                } else {
                    // Auxiliary bus
                    match io.direction {
                        IoDirection::Input => aux_inputs.push((effective_name.to_string(), ch)),
                        IoDirection::Output => aux_outputs.push((effective_name.to_string(), ch)),
                    }
                }
            }
            _ => {}
        }
    }

    PluginInfo {
        name,
        vendor,
        url,
        email,
        version,
        input_channels,
        output_channels,
        struct_name,
        aux_inputs,
        aux_outputs,
    }
}

fn extract_clap_info(plugin: &PluginDef) -> Option<ClapInfo> {
    for (item, _) in &plugin.items {
        if let PluginItem::FormatBlock(FormatBlock::Clap(clap)) = item {
            let mut id = String::new();
            let mut description = String::new();
            let mut features = Vec::new();

            for (ci, _) in &clap.items {
                match ci {
                    ClapItem::Id(s) => id = s.clone(),
                    ClapItem::Description(s) => description = s.clone(),
                    ClapItem::Features(f) => features = f.clone(),
                }
            }
            return Some(ClapInfo {
                id,
                description,
                features,
            });
        }
    }
    None
}

fn extract_vst3_info(plugin: &PluginDef) -> Option<Vst3Info> {
    for (item, _) in &plugin.items {
        if let PluginItem::FormatBlock(FormatBlock::Vst3(vst3)) = item {
            let mut id = String::new();
            let mut subcategories = Vec::new();

            for (vi, _) in &vst3.items {
                match vi {
                    Vst3Item::Id(s) => id = s.clone(),
                    Vst3Item::Subcategories(s) => subcategories = s.clone(),
                }
            }
            return Some(Vst3Info { id, subcategories });
        }
    }
    None
}

fn generate_plugin_trait(
    info: &PluginInfo,
    needs_sample_rate: bool,
    is_instrument: bool,
    is_polyphonic: bool,
    has_gui: bool,
    delay_count: usize,
    reverb_count: usize,
    sample_infos: &[SampleInfo],
    wavetable_infos: &[WavetableInfo],
    needs_transport: bool,
) -> String {
    let s = &info.struct_name;
    let in_ch = info.input_channels;
    let out_ch = info.output_channels;

    let mut lifecycle_fns = String::new();
    let needs_initialize = needs_sample_rate || reverb_count > 0 || !sample_infos.is_empty() || !wavetable_infos.is_empty();
    if needs_initialize {
        let mut init_body = String::new();
        if needs_sample_rate {
            init_body.push_str("self.sample_rate = buffer_config.sample_rate;\n");
        }
        for i in 0..delay_count {
            init_body.push_str(&format!(
                "        self.delay_state_{}.allocate(buffer_config.sample_rate);\n",
                i
            ));
        }
        for i in 0..reverb_count {
            init_body.push_str(&format!(
                "        init_reverb_state(&mut self.reverb_state_{}, buffer_config.sample_rate as f32);\n",
                i
            ));
        }
        // Sample decode: hound-based WAV decode from embedded bytes or runtime file
        for sample in sample_infos {
            let upper_name = sample.name.to_uppercase();
            let field_name = &sample.name;
            if sample.embed {
                init_body.push_str(&format!(
                    "        {{\n            let cursor = std::io::Cursor::new(SAMPLE_{}_DATA);\n            let reader = hound::WavReader::new(cursor).expect(\"invalid WAV: {}\");\n            let spec = reader.spec();\n            self.sample_{}_rate = spec.sample_rate;\n            self.sample_{} = match spec.sample_format {{\n                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),\n                hound::SampleFormat::Int => {{\n                    let bits = spec.bits_per_sample;\n                    let max_val = (1u64 << (bits - 1)) as f32;\n                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()\n                }}\n            }};\n        }}\n",
                    upper_name, field_name, field_name, field_name
                ));
            } else {
                let path_escaped = sample.path.replace('\\', "/");
                init_body.push_str(&format!(
                    "        {{\n            let data = std::fs::read(\"{}\").expect(\"failed to read external sample: {}\");\n            let cursor = std::io::Cursor::new(data);\n            let reader = hound::WavReader::new(cursor).expect(\"invalid WAV: {}\");\n            let spec = reader.spec();\n            self.sample_{}_rate = spec.sample_rate;\n            self.sample_{} = match spec.sample_format {{\n                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),\n                hound::SampleFormat::Int => {{\n                    let bits = spec.bits_per_sample;\n                    let max_val = (1u64 << (bits - 1)) as f32;\n                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()\n                }}\n            }};\n        }}\n",
                    path_escaped, field_name, field_name, field_name, field_name
                ));
            }
        }
        // Wavetable decode: hound-based WAV decode from embedded bytes or runtime file
        for wt in wavetable_infos {
            let upper_name = wt.name.to_uppercase();
            let field_name = &wt.name;
            let frame_size = wt.frame_size;
            if wt.embed {
                init_body.push_str(&format!(
                    "        {{\n            let cursor = std::io::Cursor::new(WAVETABLE_{}_DATA);\n            let reader = hound::WavReader::new(cursor).expect(\"invalid WAV: {}\");\n            let spec = reader.spec();\n            let data: Vec<f32> = match spec.sample_format {{\n                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),\n                hound::SampleFormat::Int => {{\n                    let bits = spec.bits_per_sample;\n                    let max_val = (1u64 << (bits - 1)) as f32;\n                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()\n                }}\n            }};\n            self.wavetable_{}_frame_size = {};\n            self.wavetable_{}_frame_count = data.len() / {};\n            self.wavetable_{} = data;\n        }}\n",
                    upper_name, field_name, field_name, frame_size, field_name, frame_size, field_name
                ));
            } else {
                let path_escaped = wt.path.replace('\\', "/");
                init_body.push_str(&format!(
                    "        {{\n            let data = std::fs::read(\"{}\").expect(\"failed to read external wavetable: {}\");\n            let cursor = std::io::Cursor::new(data);\n            let reader = hound::WavReader::new(cursor).expect(\"invalid WAV: {}\");\n            let spec = reader.spec();\n            let samples: Vec<f32> = match spec.sample_format {{\n                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),\n                hound::SampleFormat::Int => {{\n                    let bits = spec.bits_per_sample;\n                    let max_val = (1u64 << (bits - 1)) as f32;\n                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()\n                }}\n            }};\n            self.wavetable_{}_frame_size = {};\n            self.wavetable_{}_frame_count = samples.len() / {};\n            self.wavetable_{} = samples;\n        }}\n",
                    path_escaped, field_name, field_name, field_name, frame_size, field_name, frame_size, field_name
                ));
            }
        }
        init_body.push_str("        true");
        lifecycle_fns.push_str(&format!(
            "\n    fn initialize(\n        &mut self,\n        _audio_io_layout: &AudioIOLayout,\n        buffer_config: &BufferConfig,\n        _context: &mut impl InitContext<Self>,\n    ) -> bool {{\n        {}\n    }}\n",
            init_body
        ));
    }
    if is_polyphonic {
        lifecycle_fns.push_str(
            "\n    fn reset(&mut self) {\n        self.voices.fill(None);\n        self.next_internal_voice_id = 0;\n    }\n",
        );
    }

    let midi_config = if is_instrument {
        "MidiConfig::Basic"
    } else {
        "MidiConfig::None"
    };

    let main_input = if is_instrument {
        "            main_input_channels: None,".to_string()
    } else {
        format!("            main_input_channels: NonZeroU32::new({}),", in_ch)
    };

    let editor_fn = if has_gui {
        format!(
            "\n    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {{\n        Some(Box::new(editor::WebViewEditor::new(self.params.clone())))\n    }}\n"
        )
    } else {
        String::new()
    };

    let context_param = if is_instrument || needs_transport { "context" } else { "_context" };

    // Build AUDIO_IO_LAYOUTS block — include aux ports when declared
    let mut layout_fields = String::new();
    layout_fields.push_str(&main_input);
    layout_fields.push('\n');
    layout_fields.push_str(&format!("            main_output_channels: NonZeroU32::new({out_ch}),\n"));

    if !info.aux_inputs.is_empty() {
        layout_fields.push_str("            aux_input_ports: &[");
        for (i, (_name, ch)) in info.aux_inputs.iter().enumerate() {
            if i > 0 { layout_fields.push_str(", "); }
            layout_fields.push_str(&format!("new_nonzero_u32({ch})"));
        }
        layout_fields.push_str("],\n");
    }

    if !info.aux_outputs.is_empty() {
        layout_fields.push_str("            aux_output_ports: &[");
        for (i, (_name, ch)) in info.aux_outputs.iter().enumerate() {
            if i > 0 { layout_fields.push_str(", "); }
            layout_fields.push_str(&format!("new_nonzero_u32({ch})"));
        }
        layout_fields.push_str("],\n");
    }

    // If there are any named aux ports, emit PortNames to carry human-readable names
    if !info.aux_inputs.is_empty() || !info.aux_outputs.is_empty() {
        layout_fields.push_str("            names: PortNames {\n");
        if !info.aux_inputs.is_empty() {
            layout_fields.push_str("                aux_inputs: &[");
            for (i, (name, _)) in info.aux_inputs.iter().enumerate() {
                if i > 0 { layout_fields.push_str(", "); }
                // Capitalize first letter per nih-plug convention
                let capitalized = capitalize_first(name);
                layout_fields.push_str(&format!("\"{}\"", capitalized));
            }
            layout_fields.push_str("],\n");
        }
        if !info.aux_outputs.is_empty() {
            layout_fields.push_str("                aux_outputs: &[");
            for (i, (name, _)) in info.aux_outputs.iter().enumerate() {
                if i > 0 { layout_fields.push_str(", "); }
                let capitalized = capitalize_first(name);
                layout_fields.push_str(&format!("\"{}\"", capitalized));
            }
            layout_fields.push_str("],\n");
        }
        layout_fields.push_str("                ..PortNames::const_default()\n");
        layout_fields.push_str("            },\n");
    }

    layout_fields.push_str("            ..AudioIOLayout::const_default()");

    let aux_param_name = if !info.aux_inputs.is_empty() || !info.aux_outputs.is_empty() {
        "aux"
    } else {
        "_aux"
    };

    format!(
        r#"impl Plugin for {s} {{
    const NAME: &'static str = "{name}";
    const VENDOR: &'static str = "{vendor}";
    const URL: &'static str = "{url}";
    const EMAIL: &'static str = "{email}";
    const VERSION: &'static str = "{version}";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {{
{layout_fields}
        }},
    ];

    const MIDI_INPUT: MidiConfig = {midi_config};
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {{
        self.params.clone()
    }}{lifecycle_fns}{editor_fn}
    fn process(
        &mut self,
        buffer: &mut Buffer,
        {aux_param_name}: &mut AuxiliaryBuffers,
        {context_param}: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {{
        {{PROCESS_BODY}}
    }}
}}

"#,
        name = info.name,
        vendor = info.vendor,
        url = info.url,
        email = info.email,
        version = info.version,
        lifecycle_fns = lifecycle_fns,
        editor_fn = editor_fn,
        layout_fields = layout_fields,
    )
}

fn generate_clap_trait(info: &PluginInfo, clap: &ClapInfo, process_info: &ProcessInfo) -> String {
    let features: Vec<String> = clap.features.iter().map(|f| map_clap_feature(f)).collect();
    let features_str = features.join(",\n        ");
    let poly_mod_config = if let Some(voice_count) = process_info.voice_count {
        format!(
            "\n    const CLAP_POLY_MODULATION_CONFIG: Option<PolyModulationConfig> = Some(PolyModulationConfig {{\n        max_voice_capacity: {},\n        supports_overlapping_voices: true,\n    }});",
            voice_count
        )
    } else {
        String::new()
    };

    format!(
        r#"impl ClapPlugin for {s} {{
    const CLAP_ID: &'static str = "{id}";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("{desc}");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        {features_str},
    ];{poly_mod_config}
}}

"#,
        s = info.struct_name,
        id = clap.id,
        desc = clap.description,
        poly_mod_config = poly_mod_config,
    )
}

fn generate_vst3_trait(info: &PluginInfo, vst3: &Vst3Info) -> String {
    let class_id = vst3_class_id_literal(&vst3.id);
    let subcats: Vec<String> = vst3
        .subcategories
        .iter()
        .map(|s| map_vst3_subcategory(s))
        .collect();
    let subcats_str = subcats.join(", ");

    format!(
        r#"impl Vst3Plugin for {s} {{
    const VST3_CLASS_ID: [u8; 16] = *b"{class_id}";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[{subcats_str}];
}}

"#,
        s = info.struct_name,
    )
}

fn map_clap_feature(feature: &str) -> String {
    match feature {
        "audio_effect" => "ClapFeature::AudioEffect".to_string(),
        "stereo" => "ClapFeature::Stereo".to_string(),
        "utility" => "ClapFeature::Utility".to_string(),
        "instrument" => "ClapFeature::Instrument".to_string(),
        "synthesizer" => "ClapFeature::Synthesizer".to_string(),
        "mono" => "ClapFeature::Mono".to_string(),
        "surround" => "ClapFeature::Surround".to_string(),
        "ambisonic" => "ClapFeature::Ambisonic".to_string(),
        "filter" => "ClapFeature::Filter".to_string(),
        other => format!("ClapFeature::Custom(\"{}\")", other),
    }
}

fn map_vst3_subcategory(subcat: &str) -> String {
    match subcat {
        "Fx" => "Vst3SubCategory::Fx".to_string(),
        "Dynamics" => "Vst3SubCategory::Dynamics".to_string(),
        "EQ" => "Vst3SubCategory::Eq".to_string(),
        "Filter" => "Vst3SubCategory::Filter".to_string(),
        "Instrument" => "Vst3SubCategory::Instrument".to_string(),
        "Synth" => "Vst3SubCategory::Synth".to_string(),
        "Delay" => "Vst3SubCategory::Delay".to_string(),
        "Reverb" => "Vst3SubCategory::Reverb".to_string(),
        "Distortion" => "Vst3SubCategory::Distortion".to_string(),
        "Tools" => "Vst3SubCategory::Tools".to_string(),
        other => {
            eprintln!("codegen: unknown VST3 subcategory '{}', mapping to Tools", other);
            "Vst3SubCategory::Tools".to_string()
        }
    }
}

fn vst3_class_id_literal(id: &str) -> String {
    let bytes = id.as_bytes();
    let mut result = [b' '; 16];
    let len = bytes.len().min(16);
    result[..len].copy_from_slice(&bytes[..len]);
    String::from_utf8(result.to_vec()).unwrap_or_else(|_| "MusePlugin______".to_string())
}

fn plugin_name_to_struct(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

fn channel_count(spec: &ChannelSpec) -> u32 {
    match spec {
        ChannelSpec::Mono => 1,
        ChannelSpec::Stereo => 2,
        ChannelSpec::Count(n) => *n,
    }
}

/// Capitalize the first letter of a string (for nih-plug port name convention).
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + chars.as_str()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::process::MAX_BLOCK_SIZE;

    #[test]
    fn test_plugin_name_to_struct() {
        assert_eq!(plugin_name_to_struct("Warm Gain"), "WarmGain");
        assert_eq!(plugin_name_to_struct("simple"), "Simple");
    }

    #[test]
    fn test_vst3_class_id_literal() {
        let id = vst3_class_id_literal("MuseWarmGain1");
        assert_eq!(id.len(), 16);
        assert!(id.starts_with("MuseWarmGain1"));
    }

    #[test]
    fn test_map_clap_feature() {
        assert_eq!(map_clap_feature("audio_effect"), "ClapFeature::AudioEffect");
        assert_eq!(
            map_clap_feature("custom_thing"),
            "ClapFeature::Custom(\"custom_thing\")"
        );
    }

    #[test]
    fn max_block_size_constant_matches_contract() {
        assert_eq!(MAX_BLOCK_SIZE, 64);
    }
}
