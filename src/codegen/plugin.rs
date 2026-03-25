//! Generates the Plugin struct, Plugin/ClapPlugin/Vst3Plugin trait impls, and export macros.

use crate::ast::{
    ClapItem, ChannelSpec, FormatBlock, IoDirection, MetadataKey, MetadataValue,
    PluginDef, PluginItem, Vst3Item,
};
use crate::dsp::primitives::DspPrimitive;
use crate::codegen::process::ProcessInfo;

/// Info extracted from the plugin AST needed for code generation.
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

/// CLAP-specific metadata.
struct ClapInfo {
    id: String,
    description: String,
    features: Vec<String>,
}

/// VST3-specific metadata.
struct Vst3Info {
    id: String,
    subcategories: Vec<String>,
}

/// Generate the complete Plugin struct, trait impls, and export macros.
///
/// `process_info` provides which DSP primitives are used and per-branch filter state requirements.
pub fn generate_plugin_struct(plugin: &PluginDef, process_info: &ProcessInfo) -> String {
    let info = extract_plugin_info(plugin);
    let clap = extract_clap_info(plugin);
    let vst3 = extract_vst3_info(plugin);

    let used_primitives = &process_info.used_primitives;
    let is_instrument = process_info.is_instrument;

    // Check for non-branch (top-level) biquad usage
    let needs_top_level_biquad = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(_))
    }) && process_info.branch_filters.is_empty();

    // Collect unique (split_id, branch_idx) pairs that need biquad state
    let mut branch_biquad_fields: Vec<(usize, usize)> = Vec::new();
    for &(split_id, branch_idx, _) in &process_info.branch_filters {
        let key = (split_id, branch_idx);
        if !branch_biquad_fields.contains(&key) {
            branch_biquad_fields.push(key);
        }
    }

    let needs_any_biquad = needs_top_level_biquad || !branch_biquad_fields.is_empty();
    let needs_sample_rate = needs_any_biquad || is_instrument;
    let num_channels = info.output_channels.max(info.input_channels) as usize;

    let mut out = String::new();

    // Plugin struct
    out.push_str(&format!("struct {} {{\n", info.struct_name));
    out.push_str("    params: Arc<PluginParams>,\n");
    if needs_top_level_biquad {
        out.push_str(&format!(
            "    biquad_state: [BiquadState; {}],\n",
            num_channels
        ));
    }
    for &(split_id, branch_idx) in &branch_biquad_fields {
        out.push_str(&format!(
            "    split{}_branch{}_biquad: [BiquadState; {}],\n",
            split_id, branch_idx, num_channels
        ));
    }
    // Instrument-specific state fields
    if is_instrument {
        for i in 0..process_info.oscillator_count {
            out.push_str(&format!("    osc_state_{}: OscState,\n", i));
        }
        if process_info.has_adsr {
            out.push_str("    adsr_state: AdsrState,\n");
        }
        out.push_str("    active_note: Option<u8>,\n");
        out.push_str("    note_freq: f32,\n");
        out.push_str("    velocity: f32,\n");
    }
    if needs_sample_rate {
        out.push_str("    sample_rate: f32,\n");
    }
    out.push_str("}\n\n");

    // Default impl
    out.push_str(&format!(
        "impl Default for {} {{\n    fn default() -> Self {{\n        Self {{\n            params: Arc::new(PluginParams::default()),\n",
        info.struct_name
    ));
    if needs_top_level_biquad {
        out.push_str(&format!(
            "            biquad_state: [BiquadState::default(); {}],\n",
            num_channels
        ));
    }
    for &(split_id, branch_idx) in &branch_biquad_fields {
        out.push_str(&format!(
            "            split{}_branch{}_biquad: [BiquadState::default(); {}],\n",
            split_id, branch_idx, num_channels
        ));
    }
    if is_instrument {
        for i in 0..process_info.oscillator_count {
            out.push_str(&format!("            osc_state_{}: OscState::default(),\n", i));
        }
        if process_info.has_adsr {
            out.push_str("            adsr_state: AdsrState::default(),\n");
        }
        out.push_str("            active_note: None,\n");
        out.push_str("            note_freq: 440.0,\n");
        out.push_str("            velocity: 0.0,\n");
    }
    if needs_sample_rate {
        out.push_str("            sample_rate: 44100.0,\n");
    }
    out.push_str("        }\n    }\n}\n\n");

    // Plugin trait impl
    out.push_str(&generate_plugin_trait(&info, needs_sample_rate, is_instrument));

    // ClapPlugin trait impl + export macro
    if let Some(ref clap) = clap {
        out.push_str(&generate_clap_trait(&info, clap));
    }

    // Vst3Plugin trait impl + export macro
    if let Some(ref vst3) = vst3 {
        out.push_str(&generate_vst3_trait(&info, vst3));
    }

    // Export macros
    if clap.is_some() {
        out.push_str(&format!("nih_export_clap!({});\n", info.struct_name));
    }
    if vst3.is_some() {
        out.push_str(&format!("nih_export_vst3!({});\n", info.struct_name));
    }

    out
}

/// Extract core plugin info from the AST.
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
                    MetadataKey::Category => {} // not used in Plugin trait
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

fn generate_plugin_trait(info: &PluginInfo, needs_sample_rate: bool, is_instrument: bool) -> String {
    let s = &info.struct_name;
    let in_ch = info.input_channels;
    let out_ch = info.output_channels;

    let initialize_fn = if needs_sample_rate {
        r#"
    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }
"#
        .to_string()
    } else {
        String::new()
    };

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

    // In instrument mode, context is used (for MIDI events); in effect mode it's unused
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
    }}
{initialize_fn}
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
    )
}

fn generate_clap_trait(info: &PluginInfo, clap: &ClapInfo) -> String {
    let features: Vec<String> = clap.features.iter().map(|f| map_clap_feature(f)).collect();
    let features_str = features.join(",\n        ");

    format!(
        r#"impl ClapPlugin for {s} {{
    const CLAP_ID: &'static str = "{id}";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("{desc}");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        {features_str},
    ];
}}

"#,
        s = info.struct_name,
        id = clap.id,
        desc = clap.description,
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

/// Map a CLAP feature string to its nih-plug ClapFeature variant.
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

/// Map a VST3 subcategory string to its nih-plug Vst3SubCategory variant.
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
            // nih-plug doesn't support custom VST3 subcategories directly,
            // so we map unknown ones to Tools as a safe fallback
            eprintln!("codegen: unknown VST3 subcategory '{}', mapping to Tools", other);
            "Vst3SubCategory::Tools".to_string()
        }
    }
}

/// Generate a 16-byte VST3 class ID literal. Pads with spaces or truncates.
fn vst3_class_id_literal(id: &str) -> String {
    let bytes = id.as_bytes();
    let mut result = [b' '; 16];
    let len = bytes.len().min(16);
    result[..len].copy_from_slice(&bytes[..len]);
    // Ensure all bytes are valid ASCII for a byte string literal
    String::from_utf8(result.to_vec()).unwrap_or_else(|_| "MusePlugin______".to_string())
}

/// Convert a plugin display name to a Rust struct name (PascalCase, no spaces).
///
/// "Warm Gain" → "WarmGain"
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

/// Get the channel count from a ChannelSpec.
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
        assert_eq!(map_clap_feature("custom_thing"), "ClapFeature::Custom(\"custom_thing\")");
    }
}
