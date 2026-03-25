//! Generates the `#[cfg(feature = "preview")]` module with C-ABI exports
//! for the hot-reload preview host.
//!
//! The preview host loads the plugin's cdylib via `libloading` and calls these
//! extern "C" functions to create, process, and parameterize the plugin without
//! going through the full nih-plug host lifecycle.
//!
//! The process function constructs a nih-plug Buffer from raw pointers and calls
//! the plugin's existing `process()` method, so DSP logic is never duplicated.

use crate::ast::{ParamDef, ParamType, PluginDef, PluginItem};
use crate::codegen::process::ProcessInfo;

/// Generate the `#[cfg(feature = "preview")] mod muse_preview { ... }` block
/// containing all C-ABI export functions.
pub fn generate_preview_exports(plugin: &PluginDef, process_info: &ProcessInfo) -> String {
    let struct_name = plugin_name_to_struct(&plugin.name);
    let params = collect_params(plugin);
    let num_channels = output_channel_count(plugin);
    let is_instrument = process_info.is_instrument;

    let mut out = String::new();

    out.push_str("\n#[cfg(feature = \"preview\")]\nmod muse_preview {\n");
    out.push_str("    use super::*;\n\n");

    // --- Minimal ProcessContext for preview ---
    out.push_str(&generate_preview_context(&struct_name, is_instrument));

    // --- PreviewInstance wrapper ---
    out.push_str("    struct PreviewInstance {\n");
    out.push_str(&format!("        plugin: {},\n", struct_name));
    out.push_str("        ctx: PreviewProcessContext,\n");
    out.push_str("    }\n\n");

    // --- muse_preview_create ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_create(sample_rate: f32) -> *mut u8 {\n");
    out.push_str(&format!(
        "        let mut plugin = {}::default();\n",
        struct_name
    ));
    // Initialize the plugin through nih-plug's initialize() path
    out.push_str("        let layout = AudioIOLayout {\n");
    out.push_str(&format!(
        "            main_input_channels: NonZeroU32::new({}),\n",
        if is_instrument { "None".to_string() } else { format!("Some(NonZeroU32::new({}).unwrap())", num_channels) }
    ));

    // Fix: for instruments, main_input_channels is None
    // Rewrite this section properly
    let mut create_body = String::new();
    create_body.push_str("        let layout = AudioIOLayout {\n");
    if is_instrument {
        create_body.push_str("            main_input_channels: None,\n");
    } else {
        create_body.push_str(&format!(
            "            main_input_channels: NonZeroU32::new({}),\n",
            num_channels
        ));
    }
    create_body.push_str(&format!(
        "            main_output_channels: NonZeroU32::new({}),\n",
        num_channels
    ));
    create_body.push_str("            ..AudioIOLayout::const_default()\n");
    create_body.push_str("        };\n");
    create_body.push_str("        let buffer_config = BufferConfig {\n");
    create_body.push_str("            sample_rate,\n");
    create_body.push_str("            min_buffer_size: None,\n");
    create_body.push_str("            max_buffer_size: 0,\n");
    create_body.push_str("            process_mode: ProcessMode::Realtime,\n");
    create_body.push_str("        };\n");
    create_body.push_str("        struct PreviewInitCtx;\n");
    create_body.push_str(&format!("        impl InitContext<{}> for PreviewInitCtx {{\n", struct_name));
    create_body.push_str("            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }\n");
    create_body.push_str("            fn execute(&self, _task: ()) {}\n");
    create_body.push_str("            fn set_latency_samples(&self, _samples: u32) {}\n");
    create_body.push_str("            fn set_current_voice_capacity(&self, _capacity: u32) {}\n");
    create_body.push_str("        }\n");
    create_body.push_str("        let mut init_ctx = PreviewInitCtx;\n");
    create_body.push_str("        plugin.initialize(&layout, &buffer_config, &mut init_ctx);\n");

    // Reset smoothers to their default values so audio works immediately
    for p in &params {
        let smoother_reset = generate_smoother_reset(p);
        if !smoother_reset.is_empty() {
            create_body.push_str(&format!("        {};\n", smoother_reset));
        }
    }

    create_body.push_str("        let ctx = PreviewProcessContext::new(sample_rate);\n");
    create_body.push_str("        let instance = Box::new(PreviewInstance { plugin, ctx });\n");
    create_body.push_str("        Box::into_raw(instance) as *mut u8\n");

    // Clear the partially-written create function and rewrite
    // (We wrote some duplicate lines above — clean slate)
    out.clear();
    out.push_str("\n#[cfg(feature = \"preview\")]\nmod muse_preview {\n");
    out.push_str("    use super::*;\n\n");
    out.push_str(&generate_preview_context(&struct_name, is_instrument));
    out.push_str("    struct PreviewInstance {\n");
    out.push_str(&format!("        plugin: {},\n", struct_name));
    out.push_str("        ctx: PreviewProcessContext,\n");
    out.push_str("    }\n\n");

    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_create(sample_rate: f32) -> *mut u8 {\n");
    out.push_str(&format!(
        "        let mut plugin = {}::default();\n",
        struct_name
    ));
    out.push_str(&create_body);
    out.push_str("    }\n\n");

    // --- muse_preview_destroy ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_destroy(ptr: *mut u8) {\n");
    out.push_str("        if !ptr.is_null() {\n");
    out.push_str("            drop(Box::from_raw(ptr as *mut PreviewInstance));\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // --- muse_preview_process ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_process(\n");
    out.push_str("        ptr: *mut u8,\n");
    out.push_str("        inputs: *const *const f32,\n");
    out.push_str("        outputs: *mut *mut f32,\n");
    out.push_str("        num_channels: u32,\n");
    out.push_str("        num_samples: u32,\n");
    out.push_str("    ) {\n");
    out.push_str("        if ptr.is_null() { return; }\n");
    out.push_str("        let instance = &mut *(ptr as *mut PreviewInstance);\n");
    out.push_str("        let nc = num_channels as usize;\n");
    out.push_str("        let ns = num_samples as usize;\n\n");

    // Copy input data into owned buffers
    out.push_str("        // Build owned channel buffers from raw pointers\n");
    out.push_str("        let mut channel_data: Vec<Vec<f32>> = Vec::with_capacity(nc);\n");
    out.push_str("        for ch in 0..nc {\n");
    out.push_str("            let mut samples = vec![0.0_f32; ns];\n");
    out.push_str("            if !inputs.is_null() {\n");
    out.push_str("                let in_ptr = *inputs.add(ch);\n");
    out.push_str("                if !in_ptr.is_null() {\n");
    out.push_str("                    std::ptr::copy_nonoverlapping(in_ptr, samples.as_mut_ptr(), ns);\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("            channel_data.push(samples);\n");
    out.push_str("        }\n\n");

    // Construct nih-plug Buffer and call process()
    out.push_str("        // Construct nih-plug Buffer from owned data\n");
    out.push_str("        let mut buffer = Buffer::default();\n");
    out.push_str("        buffer.set_slices(ns, |output_slices| {\n");
    out.push_str("            output_slices.clear();\n");
    out.push_str("            for ch in channel_data.iter_mut() {\n");
    out.push_str("                let slice: &mut [f32] = &mut ch[..];\n");
    out.push_str("                let slice: &'static mut [f32] = std::mem::transmute(slice);\n");
    out.push_str("                output_slices.push(slice);\n");
    out.push_str("            }\n");
    out.push_str("        });\n\n");

    out.push_str("        let mut aux = AuxiliaryBuffers {\n");
    out.push_str("            inputs: &mut [],\n");
    out.push_str("            outputs: &mut [],\n");
    out.push_str("        };\n\n");

    out.push_str("        instance.plugin.process(&mut buffer, &mut aux, &mut instance.ctx);\n\n");

    // Copy output data back to raw pointers
    out.push_str("        // Copy processed data to output pointers\n");
    out.push_str("        if !outputs.is_null() {\n");
    out.push_str("            for ch in 0..nc {\n");
    out.push_str("                let out_ptr = *outputs.add(ch);\n");
    out.push_str("                if !out_ptr.is_null() {\n");
    out.push_str("                    std::ptr::copy_nonoverlapping(\n");
    out.push_str("                        channel_data[ch].as_ptr(),\n");
    out.push_str("                        out_ptr,\n");
    out.push_str("                        ns,\n");
    out.push_str("                    );\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // --- muse_preview_get_param_count ---
    out.push_str("    #[no_mangle]\n");
    out.push_str(&format!(
        "    pub extern \"C\" fn muse_preview_get_param_count() -> u32 {{\n        {}\n    }}\n\n",
        params.len()
    ));

    // --- muse_preview_get_param_name ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_get_param_name(index: u32, buf: *mut u8, buf_len: u32) -> u32 {\n");
    out.push_str("        let name: &str = match index {\n");
    for (i, p) in params.iter().enumerate() {
        out.push_str(&format!("            {} => \"{}\",\n", i, p.name));
    }
    out.push_str("            _ => return 0,\n");
    out.push_str("        };\n");
    out.push_str("        let bytes = name.as_bytes();\n");
    out.push_str("        let copy_len = bytes.len().min(buf_len as usize);\n");
    out.push_str("        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, copy_len);\n");
    out.push_str("        copy_len as u32\n");
    out.push_str("    }\n\n");

    // --- muse_preview_get_param_default ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub extern \"C\" fn muse_preview_get_param_default(index: u32) -> f32 {\n");
    out.push_str("        let params = PluginParams::default();\n");
    out.push_str("        match index {\n");
    for (i, p) in params.iter().enumerate() {
        let expr = param_read_expr(p, "params.");
        out.push_str(&format!(
            "            {} => {},\n",
            i, expr
        ));
    }
    out.push_str("            _ => 0.0,\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // --- muse_preview_set_param ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_set_param(ptr: *mut u8, index: u32, value: f32) {\n");
    out.push_str("        if ptr.is_null() { return; }\n");
    out.push_str("        let instance = &mut *(ptr as *mut PreviewInstance);\n");
    out.push_str("        match index {\n");
    for (i, p) in params.iter().enumerate() {
        let setter = generate_smoother_set(p);
        out.push_str(&format!(
            "            {} => {{ {} }}\n",
            i, setter
        ));
    }
    out.push_str("            _ => {}\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // --- muse_preview_get_param ---
    out.push_str("    #[no_mangle]\n");
    out.push_str("    pub unsafe extern \"C\" fn muse_preview_get_param(ptr: *mut u8, index: u32) -> f32 {\n");
    out.push_str("        if ptr.is_null() { return 0.0; }\n");
    out.push_str("        let instance = &*(ptr as *mut PreviewInstance);\n");
    out.push_str("        match index {\n");
    for (i, p) in params.iter().enumerate() {
        let expr = param_read_expr(p, "instance.plugin.params.");
        out.push_str(&format!(
            "            {} => {},\n",
            i, expr
        ));
    }
    out.push_str("            _ => 0.0,\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // --- muse_preview_get_num_channels ---
    out.push_str("    #[no_mangle]\n");
    out.push_str(&format!(
        "    pub extern \"C\" fn muse_preview_get_num_channels() -> u32 {{\n        {}\n    }}\n",
        num_channels
    ));

    out.push_str("}\n");

    out
}

/// Generate a minimal `PreviewProcessContext` that implements `ProcessContext<T>`.
/// This is the lightest possible shim — no MIDI, no transport, no background tasks.
fn generate_preview_context(struct_name: &str, _is_instrument: bool) -> String {
    let mut out = String::new();

    out.push_str("    struct PreviewProcessContext {\n");
    out.push_str("        transport: Transport,\n");
    out.push_str(&format!(
        "        events: std::collections::VecDeque<PluginNoteEvent<{}>>,\n",
        struct_name
    ));
    out.push_str("    }\n\n");

    out.push_str("    impl PreviewProcessContext {\n");
    out.push_str("        fn new(sample_rate: f32) -> Self {\n");
    out.push_str("            let mut transport: Transport = unsafe { std::mem::zeroed() };\n");
    out.push_str("            transport.playing = true;\n");
    out.push_str("            transport.recording = false;\n");
    out.push_str("            transport.preroll_active = Some(false);\n");
    out.push_str("            transport.sample_rate = sample_rate;\n");
    out.push_str("            transport.tempo = Some(120.0);\n");
    out.push_str("            transport.time_sig_numerator = Some(4);\n");
    out.push_str("            transport.time_sig_denominator = Some(4);\n");
    out.push_str("            Self { transport, events: std::collections::VecDeque::new() }\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    impl ProcessContext<{}> for PreviewProcessContext {{\n",
        struct_name
    ));
    out.push_str("        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }\n");
    out.push_str("        fn execute_background(&self, _task: ()) {}\n");
    out.push_str("        fn execute_gui(&self, _task: ()) {}\n");
    out.push_str("        fn transport(&self) -> &Transport { &self.transport }\n");
    out.push_str(&format!(
        "        fn next_event(&mut self) -> Option<PluginNoteEvent<{}>> {{ self.events.pop_front() }}\n",
        struct_name
    ));
    out.push_str(&format!(
        "        fn send_event(&mut self, _event: PluginNoteEvent<{}>) {{}}\n",
        struct_name
    ));
    out.push_str("        fn set_latency_samples(&self, _samples: u32) {}\n");
    out.push_str("        fn set_current_voice_capacity(&self, _capacity: u32) {}\n");
    out.push_str("    }\n\n");

    out
}

// --- Param helpers ---

/// Generate the full expression for reading a param's current f32 value.
/// For dB params, converts from gain-linear back to dB domain.
fn param_read_expr(param: &ParamDef, prefix: &str) -> String {
    let field = format!("{}{}", prefix, param.name);
    match &param.param_type {
        ParamType::Float => {
            if is_db_param(param) {
                format!("util::gain_to_db({}.value())", field)
            } else {
                format!("{}.value()", field)
            }
        }
        ParamType::Int => format!("{}.value() as f32", field),
        ParamType::Bool => format!("if {}.value() {{ 1.0 }} else {{ 0.0 }}", field),
        ParamType::Enum(_) => format!("{}.value().to_index() as f32", field),
    }
}

/// Generate the line that resets a param's smoother to its default value.
fn generate_smoother_reset(param: &ParamDef) -> String {
    match &param.param_type {
        ParamType::Float => {
            let is_db = is_db_param(param);
            let default_val = default_value_f64(param);
            if is_db {
                format!(
                    "plugin.params.{}.smoothed.reset(util::db_to_gain({}_f32))",
                    param.name, default_val
                )
            } else {
                format!(
                    "plugin.params.{}.smoothed.reset({}_f32)",
                    param.name, default_val
                )
            }
        }
        ParamType::Int => {
            let default_val = default_value_f64(param) as i32;
            format!(
                "plugin.params.{}.smoothed.reset({})",
                param.name, default_val
            )
        }
        _ => String::new(), // Bool/Enum have no smoothers (K038)
    }
}

/// Generate the statement that sets a param's smoother via the C-ABI `value` argument.
fn generate_smoother_set(param: &ParamDef) -> String {
    match &param.param_type {
        ParamType::Float => {
            if is_db_param(param) {
                format!(
                    "instance.plugin.params.{}.smoothed.reset(util::db_to_gain(value));",
                    param.name
                )
            } else {
                format!(
                    "instance.plugin.params.{}.smoothed.reset(value);",
                    param.name
                )
            }
        }
        ParamType::Int => {
            format!(
                "instance.plugin.params.{}.smoothed.reset(value as i32);",
                param.name
            )
        }
        // Bool/Enum params have no public smoother API (K038) — silently ignore
        _ => String::new(),
    }
}

// --- AST helpers ---

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

fn output_channel_count(plugin: &PluginDef) -> u32 {
    use crate::ast::{ChannelSpec, IoDirection};
    for (item, _) in &plugin.items {
        if let PluginItem::IoDecl(io) = item {
            if io.direction == IoDirection::Output {
                return match io.channels {
                    ChannelSpec::Mono => 1,
                    ChannelSpec::Stereo => 2,
                    ChannelSpec::Count(n) => n,
                };
            }
        }
    }
    2
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

fn is_db_param(param: &ParamDef) -> bool {
    use crate::ast::ParamOption;
    param.options.iter().any(|(opt, _)| {
        matches!(opt, ParamOption::Unit(u) if u.eq_ignore_ascii_case("db"))
    })
}

fn default_value_f64(param: &ParamDef) -> f64 {
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
