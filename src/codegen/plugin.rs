//! Generates the Plugin struct, Plugin/ClapPlugin/Vst3Plugin trait impls, and export macros.

use crate::ast::{
    ClapItem, ChannelSpec, FormatBlock, IoDirection, MetadataKey, MetadataValue, PluginDef,
    PluginItem, Vst3Item,
};
use crate::codegen::process::ProcessInfo;
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

pub fn generate_plugin_struct(plugin: &PluginDef, process_info: &ProcessInfo) -> String {
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
    let has_oscillators = process_info.oscillator_count > 0;
    let has_adsr = process_info.has_adsr;
    let has_chorus = process_info.chorus_count > 0;
    let has_compressor = process_info.compressor_count > 0;
    let has_delay = process_info.delay_count > 0;
    let has_eq_biquad = process_info.eq_biquad_count > 0;
    let has_rms = process_info.rms_count > 0;
    let has_peak_follow = process_info.peak_follow_count > 0;
    let has_gate = process_info.gate_count > 0;
    let _has_dc_block = process_info.dc_block_count > 0;
    let _has_sample_hold = process_info.sample_hold_count > 0;
    let needs_sample_rate = needs_any_biquad || is_instrument || has_oscillators || has_chorus || has_compressor || has_delay || has_eq_biquad || has_rms || has_peak_follow || has_gate;
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
        for i in 0..process_info.oscillator_count {
            out.push_str(&format!("    osc_state_{}: OscState,\n", i));
        }
        if has_adsr {
            out.push_str("    adsr_state: AdsrState,\n");
        }
        for i in 0..process_info.chorus_count {
            out.push_str(&format!("    chorus_state_{}: ChorusState,\n", i));
        }
        for i in 0..process_info.compressor_count {
            out.push_str(&format!("    compressor_state_{}: CompressorState,\n", i));
        }
        for i in 0..process_info.rms_count {
            out.push_str(&format!("    rms_state_{}: RmsState,\n", i));
        }
        for i in 0..process_info.peak_follow_count {
            out.push_str(&format!("    peak_follow_state_{}: PeakFollowState,\n", i));
        }
        for i in 0..process_info.gate_count {
            out.push_str(&format!("    gate_state_{}: GateState,\n", i));
        }
        for i in 0..process_info.dc_block_count {
            out.push_str(&format!("    dc_block_state_{}: DcBlockState,\n", i));
        }
        for i in 0..process_info.sample_hold_count {
            out.push_str(&format!("    sample_hold_state_{}: SampleAndHoldState,\n", i));
        }
    }

    // Delay state fields are outside the poly/mono guard — delays work in both effects and instruments
    for i in 0..process_info.delay_count {
        out.push_str(&format!("    delay_state_{}: DelayLine,\n", i));
    }

    // EQ biquad state fields — per-call-site, per-channel (outside poly/mono guard)
    if !is_polyphonic {
        for i in 0..process_info.eq_biquad_count {
            out.push_str(&format!("    eq_biquad_state_{}: [BiquadState; {}],\n", i, num_channels));
        }
    }

    if is_instrument && !is_polyphonic {
        out.push_str("    active_note: Option<u8>,\n");
        out.push_str("    note_freq: f32,\n");
        out.push_str("    velocity: f32,\n");
    }
    if needs_sample_rate {
        out.push_str("    sample_rate: f32,\n");
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
        out.push_str(&format!(
            "            voices: [(); {}].map(|_| None),\n",
            voice_count
        ));
        out.push_str("            next_internal_voice_id: 0,\n");
    } else {
        for i in 0..process_info.oscillator_count {
            out.push_str(&format!("            osc_state_{}: OscState::default(),\n", i));
        }
        if has_adsr {
            out.push_str("            adsr_state: AdsrState::default(),\n");
        }
        for i in 0..process_info.chorus_count {
            out.push_str(&format!("            chorus_state_{}: ChorusState::default(),\n", i));
        }
        for i in 0..process_info.compressor_count {
            out.push_str(&format!("            compressor_state_{}: CompressorState::default(),\n", i));
        }
        for i in 0..process_info.rms_count {
            out.push_str(&format!("            rms_state_{}: RmsState::default(),\n", i));
        }
        for i in 0..process_info.peak_follow_count {
            out.push_str(&format!("            peak_follow_state_{}: PeakFollowState::default(),\n", i));
        }
        for i in 0..process_info.gate_count {
            out.push_str(&format!("            gate_state_{}: GateState::default(),\n", i));
        }
        for i in 0..process_info.dc_block_count {
            out.push_str(&format!("            dc_block_state_{}: DcBlockState::default(),\n", i));
        }
        for i in 0..process_info.sample_hold_count {
            out.push_str(&format!("            sample_hold_state_{}: SampleAndHoldState::default(),\n", i));
        }
    }

    for i in 0..process_info.delay_count {
        out.push_str(&format!("            delay_state_{}: DelayLine::default(),\n", i));
    }

    if !is_polyphonic {
        for i in 0..process_info.eq_biquad_count {
            out.push_str(&format!("            eq_biquad_state_{}: [BiquadState::default(); {}],\n", i, num_channels));
        }
    }

    if is_instrument && !is_polyphonic {
        out.push_str("            active_note: None,\n");
        out.push_str("            note_freq: 440.0,\n");
        out.push_str("            velocity: 0.0,\n");
    }
    if needs_sample_rate {
        out.push_str("            sample_rate: 44100.0,\n");
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str(&generate_plugin_trait(&info, needs_sample_rate, is_instrument, is_polyphonic, has_gui, process_info.delay_count));

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
    out.push_str("    voice_id: i32,\n");
    out.push_str("    channel: u8,\n");
    out.push_str("    note: u8,\n");
    out.push_str("    internal_voice_id: u64,\n");
    out.push_str("    note_freq: f32,\n");
    out.push_str("    velocity: f32,\n");
    out.push_str("    pressure: f32,\n");
    out.push_str("    tuning: f32,\n");
    out.push_str("    slide: f32,\n");
    out.push_str("    releasing: bool,\n");
    // Per-voice filter state (if any filters are used)
    let has_filters = process_info.used_primitives.iter().any(|p| matches!(p, DspPrimitive::Filter(_)));
    if has_filters {
        out.push_str("    biquad_state: BiquadState,\n");
    }
    for i in 0..process_info.oscillator_count {
        out.push_str(&format!("    osc_state_{}: OscState,\n", i));
    }
    if process_info.has_adsr {
        out.push_str("    adsr_state: AdsrState,\n");
    }
    for i in 0..process_info.chorus_count {
        out.push_str(&format!("    chorus_state_{}: ChorusState,\n", i));
    }
    for i in 0..process_info.compressor_count {
        out.push_str(&format!("    compressor_state_{}: CompressorState,\n", i));
    }
    for i in 0..process_info.rms_count {
        out.push_str(&format!("    rms_state_{}: RmsState,\n", i));
    }
    for i in 0..process_info.peak_follow_count {
        out.push_str(&format!("    peak_follow_state_{}: PeakFollowState,\n", i));
    }
    for i in 0..process_info.gate_count {
        out.push_str(&format!("    gate_state_{}: GateState,\n", i));
    }
    for i in 0..process_info.dc_block_count {
        out.push_str(&format!("    dc_block_state_{}: DcBlockState,\n", i));
    }
    for i in 0..process_info.sample_hold_count {
        out.push_str(&format!("    sample_hold_state_{}: SampleAndHoldState,\n", i));
    }
    for i in 0..process_info.eq_biquad_count {
        out.push_str(&format!("    eq_biquad_state_{}: BiquadState,\n", i));
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
    for i in 0..process_info.oscillator_count {
        fields.push(format!("osc_state_{}: OscState::default()", i));
    }
    if process_info.has_adsr {
        fields.push("adsr_state: AdsrState::default()".to_string());
    }
    for i in 0..process_info.chorus_count {
        fields.push(format!("chorus_state_{}: ChorusState::default()", i));
    }
    for i in 0..process_info.compressor_count {
        fields.push(format!("compressor_state_{}: CompressorState::default()", i));
    }
    for i in 0..process_info.rms_count {
        fields.push(format!("rms_state_{}: RmsState::default()", i));
    }
    for i in 0..process_info.peak_follow_count {
        fields.push(format!("peak_follow_state_{}: PeakFollowState::default()", i));
    }
    for i in 0..process_info.gate_count {
        fields.push(format!("gate_state_{}: GateState::default()", i));
    }
    for i in 0..process_info.dc_block_count {
        fields.push(format!("dc_block_state_{}: DcBlockState::default()", i));
    }
    for i in 0..process_info.sample_hold_count {
        fields.push(format!("sample_hold_state_{}: SampleAndHoldState::default()", i));
    }
    for i in 0..process_info.eq_biquad_count {
        fields.push(format!("eq_biquad_state_{}: BiquadState::default()", i));
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
            PluginItem::IoDecl(io) => match io.direction {
                IoDirection::Input => input_channels = channel_count(&io.channels),
                IoDirection::Output => output_channels = channel_count(&io.channels),
            },
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
) -> String {
    let s = &info.struct_name;
    let in_ch = info.input_channels;
    let out_ch = info.output_channels;

    let mut lifecycle_fns = String::new();
    if needs_sample_rate {
        let mut init_body = String::from("self.sample_rate = buffer_config.sample_rate;\n");
        for i in 0..delay_count {
            init_body.push_str(&format!(
                "        self.delay_state_{}.allocate(buffer_config.sample_rate);\n",
                i
            ));
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

    let context_param = if is_instrument { "context" } else { "_context" };

    format!(
        r#"impl Plugin for {s} {{
    const NAME: &'static str = "{name}";
    const VENDOR: &'static str = "{vendor}";
    const URL: &'static str = "{url}";
    const EMAIL: &'static str = "{email}";
    const VERSION: &'static str = "{version}";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {{
{main_input}
            main_output_channels: NonZeroU32::new({out_ch}),
            ..AudioIOLayout::const_default()
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
        _aux: &mut AuxiliaryBuffers,
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
