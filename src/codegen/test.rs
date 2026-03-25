//! Generates `#[cfg(test)] mod tests { ... }` for in-language test blocks.
//!
//! Each `TestBlock` in the AST becomes a `#[test]` function that:
//! 1. Creates the plugin with default params
//! 2. Generates an input buffer (silence/sine/impulse)
//! 3. Sets parameter values
//! 4. Runs `process()` through nih-plug's Plugin trait
//! 5. Asserts on output properties (RMS, peak)
//!
//! Failed assertions panic with a `MUSE_TEST_FAIL:{json}` prefix
//! that the `muse test` CLI can parse into structured results.

use crate::ast::{
    Expr, ParamOption, ParamType, PluginDef, PluginItem, TestBlock, TestOp, TestProperty,
    TestSignal, TestStatement,
};
use crate::codegen::process::ProcessInfo;

/// Info about a parameter needed for test smoother initialization.
struct ParamInfo {
    name: String,
    default_value: f64,
    is_db: bool,
}

/// Generate a `#[cfg(test)] mod tests { ... }` block from all TestBlock items.
///
/// Returns an empty string if there are no test blocks.
pub fn generate_test_module(plugin: &PluginDef, process_info: &ProcessInfo) -> String {
    let test_blocks: Vec<&TestBlock> = plugin
        .items
        .iter()
        .filter_map(|(item, _)| {
            if let PluginItem::TestBlock(tb) = item {
                Some(tb)
            } else {
                None
            }
        })
        .collect();

    if test_blocks.is_empty() {
        return String::new();
    }

    let struct_name = plugin_name_to_struct(&plugin.name);
    let is_instrument = process_info.is_instrument;
    let db_params = collect_db_param_names(plugin);
    let param_defaults = collect_param_defaults(plugin);

    let mut out = String::new();
    out.push_str("\n#[cfg(test)]\n#[allow(unused_variables, dead_code)]\nmod tests {\n    use super::*;\n\n");

    // Check if any test block has frequency assertions (needs rustfft)
    let needs_fft = test_blocks.iter().any(|tb| {
        tb.statements.iter().any(|(stmt, _)| {
            matches!(
                stmt,
                TestStatement::Assert(a) if matches!(a.property, TestProperty::Frequency(_))
            )
        })
    });

    if needs_fft {
        out.push_str("    use rustfft::{FftPlanner, num_complex::Complex};\n\n");
    }

    // Generate the mock ProcessContext (parameterized by struct name)
    out.push_str(&generate_mock_process_context(&struct_name));

    // Generate helper functions (parameterized by struct name)
    out.push_str(&generate_helpers(&struct_name, needs_fft));

    // Generate each test function
    for tb in &test_blocks {
        out.push_str(&generate_test_fn(tb, &struct_name, is_instrument, &db_params, &param_defaults));
    }

    out.push_str("}\n");
    out
}

/// Generate a minimal mock ProcessContext for testing.
///
/// Uses a VecDeque-based event queue so instrument tests can inject
/// MIDI NoteOn/NoteOff events via `note on` / `note off` statements.
fn generate_mock_process_context(struct_name: &str) -> String {
    format!(
        r#"    struct TestProcessContext {{
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<{s}>>,
    }}

    impl TestProcessContext {{
        fn new(sample_rate: f32) -> Self {{
            // Transport has pub(crate) fields we can't set, so zero-init and
            // overwrite the public ones. All private fields are Option<T> which
            // are valid as zeroed (None).
            let mut transport: Transport = unsafe {{ std::mem::zeroed() }};
            transport.playing = true;
            transport.recording = false;
            transport.preroll_active = Some(false);
            transport.sample_rate = sample_rate;
            transport.tempo = Some(120.0);
            transport.time_sig_numerator = Some(4);
            transport.time_sig_denominator = Some(4);
            Self {{ transport, events: std::collections::VecDeque::new() }}
        }}
    }}

    impl ProcessContext<{s}> for TestProcessContext {{
        fn plugin_api(&self) -> PluginApi {{ PluginApi::Clap }}
        fn execute_background(&self, _task: ()) {{}}
        fn execute_gui(&self, _task: ()) {{}}
        fn transport(&self) -> &Transport {{ &self.transport }}
        fn next_event(&mut self) -> Option<PluginNoteEvent<{s}>> {{ self.events.pop_front() }}
        fn send_event(&mut self, _event: PluginNoteEvent<{s}>) {{}}
        fn set_latency_samples(&self, _samples: u32) {{}}
        fn set_current_voice_capacity(&self, _capacity: u32) {{}}
    }}

"#,
        s = struct_name,
    )
}

/// Generate helper functions for buffer creation and measurement.
fn generate_helpers(struct_name: &str, needs_fft: bool) -> String {
    let mut out = format!(
        r#"    const TEST_SAMPLE_RATE: f32 = 44100.0;
    const TEST_CHANNELS: usize = 2;

    fn make_silence(samples: usize) -> Vec<Vec<f32>> {{
        vec![vec![0.0_f32; samples]; TEST_CHANNELS]
    }}

    fn make_sine(freq: f64, samples: usize) -> Vec<Vec<f32>> {{
        let sr = TEST_SAMPLE_RATE as f64;
        let channel: Vec<f32> = (0..samples)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin() as f32)
            .collect();
        vec![channel.clone(), channel]
    }}

    fn make_impulse(samples: usize) -> Vec<Vec<f32>> {{
        let mut channel = vec![0.0_f32; samples];
        if !channel.is_empty() {{
            channel[0] = 1.0;
        }}
        vec![channel.clone(), channel]
    }}

    fn compute_rms(data: &[f32]) -> f32 {{
        if data.is_empty() {{ return 0.0; }}
        let sum_sq: f32 = data.iter().map(|s| s * s).sum();
        (sum_sq / data.len() as f32).sqrt()
    }}

    fn compute_peak(data: &[f32]) -> f32 {{
        data.iter().map(|s| s.abs()).fold(0.0_f32, f32::max)
    }}

    fn rms_to_db(rms: f32) -> f32 {{
        if rms <= 0.0 {{ -f32::INFINITY }} else {{ 20.0 * rms.log10() }}
    }}

    fn peak_to_db(peak: f32) -> f32 {{
        if peak <= 0.0 {{ -f32::INFINITY }} else {{ 20.0 * peak.log10() }}
    }}

    /// Run a plugin's process() on owned audio data, returning the output buffer contents.
    fn run_process(
        plugin: &mut {s},
        channel_data: &mut Vec<Vec<f32>>,
        ctx: &mut TestProcessContext,
    ) -> Vec<Vec<f32>> {{
        let num_samples = channel_data[0].len();
        let mut buffer = Buffer::default();
        unsafe {{
            buffer.set_slices(num_samples, |output_slices| {{
                output_slices.clear();
                for ch in channel_data.iter_mut() {{
                    // Safety: the slice lives as long as channel_data which outlives buffer usage
                    let slice: &mut [f32] = &mut ch[..];
                    let slice: &'static mut [f32] = std::mem::transmute(slice);
                    output_slices.push(slice);
                }}
            }});
        }}
        let mut aux = AuxiliaryBuffers {{
            inputs: &mut [],
            outputs: &mut [],
        }};
        plugin.process(&mut buffer, &mut aux, ctx);
        // Copy output data before dropping the buffer
        channel_data.clone()
    }}

    fn muse_test_fail(test_name: &str, assertion: &str, expected: &str, actual: &str) -> String {{
        format!(
            "MUSE_TEST_FAIL:{{{{\"test\":\"{{}}\",\"assertion\":\"{{}}\",\"expected\":\"{{}}\",\"actual\":\"{{}}\"}}}}",
            test_name, assertion, expected, actual
        )
    }}

"#,
        s = struct_name,
    );

    if needs_fft {
        out.push_str(&format!(
            r#"    /// Compute the magnitude (in dB) of a specific frequency bin via FFT.
    #[cfg(test)]
    fn compute_magnitude_at_freq(data: &[f32], target_freq: f64, sample_rate: f64) -> f32 {{
        use rustfft::{{FftPlanner, num_complex::Complex}};
        let n = data.len();
        if n == 0 {{ return -f32::INFINITY; }}
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let mut buffer: Vec<Complex<f32>> = data.iter().map(|&s| Complex {{ re: s, im: 0.0 }}).collect();
        fft.process(&mut buffer);
        let bin_index = ((target_freq * n as f64) / sample_rate).round() as usize;
        if bin_index >= n / 2 {{ return -f32::INFINITY; }}
        let magnitude = buffer[bin_index].norm() / (n as f32 / 2.0);
        if magnitude <= 0.0 {{ -f32::INFINITY }} else {{ 20.0 * magnitude.log10() }}
    }}

"#
        ));
    }

    out
}

/// Generate one `#[test]` function from a TestBlock.
fn generate_test_fn(tb: &TestBlock, struct_name: &str, _is_instrument: bool, db_params: &[String], param_defaults: &[ParamInfo]) -> String {
    let fn_name = sanitize_test_name(&tb.name);
    let test_name = &tb.name;

    let mut out = String::new();
    out.push_str(&format!("    #[test]\n    fn test_{}() {{\n", fn_name));

    // Initialize plugin
    out.push_str(&format!(
        "        let mut plugin = {}::default();\n",
        struct_name
    ));

    // Initialize the plugin (sets sample_rate etc.)
    out.push_str("        let layout = AudioIOLayout {\n");
    out.push_str("            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),\n");
    out.push_str("            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),\n");
    out.push_str("            ..AudioIOLayout::const_default()\n");
    out.push_str("        };\n");
    out.push_str("        let buffer_config = BufferConfig {\n");
    out.push_str("            sample_rate: TEST_SAMPLE_RATE,\n");
    out.push_str("            min_buffer_size: None,\n");
    out.push_str("            max_buffer_size: 0,\n");
    out.push_str("            process_mode: ProcessMode::Realtime,\n");
    out.push_str("        };\n");
    out.push_str("        struct InitCtx;\n");
    out.push_str(&format!(
        "        impl InitContext<{}> for InitCtx {{\n",
        struct_name
    ));
    out.push_str("            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }\n");
    out.push_str("            fn execute(&self, _task: ()) {}\n");
    out.push_str("            fn set_latency_samples(&self, _samples: u32) {}\n");
    out.push_str("            fn set_current_voice_capacity(&self, _capacity: u32) {}\n");
    out.push_str("        }\n");
    out.push_str("        let mut init_ctx = InitCtx;\n");
    out.push_str(
        "        plugin.initialize(&layout, &buffer_config, &mut init_ctx);\n",
    );

    // Reset all parameter smoothers to their default values.
    // Without this, nih-plug smoothers start at 0.0 in tests (the host
    // normally handles this during its own initialization sequence).
    for p in param_defaults {
        if p.is_db {
            out.push_str(&format!(
                "        plugin.params.{}.smoothed.reset(util::db_to_gain({:.6}_f32));\n",
                p.name, p.default_value
            ));
        } else {
            out.push_str(&format!(
                "        plugin.params.{}.smoothed.reset({:.6}_f32);\n",
                p.name, p.default_value
            ));
        }
    }
    out.push('\n');

    // Track input signal info for potential input property assertions
    let input_var = "channel_data".to_string();
    let mut sample_count: u64 = 512;

    // Process test statements
    for (stmt, _span) in &tb.statements {
        match stmt {
            TestStatement::Input(input) => {
                sample_count = input.sample_count;
                let buf_expr = match &input.signal {
                    TestSignal::Silence => {
                        format!("make_silence({})", input.sample_count)
                    }
                    TestSignal::Sine { frequency } => {
                        format!("make_sine({:.1}, {})", frequency, input.sample_count)
                    }
                    TestSignal::Impulse => {
                        format!("make_impulse({})", input.sample_count)
                    }
                };
                out.push_str(&format!(
                    "        let mut {} = {};\n",
                    input_var, buf_expr
                ));
                // Compute input properties before processing (for input.rms/input.peak assertions)
                out.push_str(&format!(
                    "        let input_rms = compute_rms(&{}[0]);\n",
                    input_var
                ));
                out.push_str(&format!(
                    "        let input_peak = compute_peak(&{}[0]);\n",
                    input_var
                ));
            }
            TestStatement::Set(set) => {
                // Use smoother.reset() to set param value immediately.
                // For dB params, convert to gain-linear since the smoother
                // operates in the internal representation.
                if db_params.contains(&set.param_path) {
                    out.push_str(&format!(
                        "        plugin.params.{}.smoothed.reset(util::db_to_gain({:.6}_f32));\n",
                        set.param_path, set.value
                    ));
                } else {
                    out.push_str(&format!(
                        "        plugin.params.{}.smoothed.reset({:.6}_f32);\n",
                        set.param_path, set.value
                    ));
                }
            }
            TestStatement::Assert(_) | TestStatement::SafetyAssert(_) => {
                // Assertions are generated after processing — skip for now
            }
            TestStatement::SetPreset { name } => {
                // Apply a named preset — resets parameter smoothers to preset values.
                // The generated apply_preset() function lives in the same crate.
                out.push_str(&format!(
                    "        apply_preset(&plugin.params, \"{}\");\n",
                    name
                ));
            }
            TestStatement::NoteOn { .. } | TestStatement::NoteOff { .. } => {
                // MIDI events are pushed to context below — skip for now
            }
        }
    }

    // If no input statement was found, default to silence 512 samples
    let has_input = tb
        .statements
        .iter()
        .any(|(s, _)| matches!(s, TestStatement::Input(_)));
    if !has_input {
        out.push_str(&format!(
            "        let mut {} = make_silence({});\n",
            input_var, sample_count
        ));
        out.push_str(&format!(
            "        let input_rms = compute_rms(&{}[0]);\n",
            input_var
        ));
        out.push_str(&format!(
            "        let input_peak = compute_peak(&{}[0]);\n",
            input_var
        ));
    }

    // Create the test process context and push MIDI events
    out.push_str("        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);\n");
    for (stmt, _span) in &tb.statements {
        match stmt {
            TestStatement::NoteOn { note, velocity, timing } => {
                out.push_str(&format!(
                    "        ctx.events.push_back(NoteEvent::NoteOn {{ timing: {}, voice_id: None, channel: 0, note: {}, velocity: {:.6} }});\n",
                    timing, note, velocity
                ));
            }
            TestStatement::NoteOff { note, timing } => {
                out.push_str(&format!(
                    "        ctx.events.push_back(NoteEvent::NoteOff {{ timing: {}, voice_id: None, channel: 0, note: {}, velocity: 0.0 }});\n",
                    timing, note
                ));
            }
            _ => {}
        }
    }

    // Run process
    out.push_str(&format!(
        "        let output = run_process(&mut plugin, &mut {}, &mut ctx);\n\n",
        input_var
    ));

    // Compute output properties
    out.push_str("        let output_rms = compute_rms(&output[0]);\n");
    out.push_str("        let output_peak = compute_peak(&output[0]);\n");
    out.push_str("        let output_rms_db = rms_to_db(output_rms);\n");
    out.push_str("        let output_peak_db = peak_to_db(output_peak);\n");
    out.push_str("        let input_rms_db = rms_to_db(input_rms);\n");
    out.push_str("        let input_peak_db = peak_to_db(input_peak);\n\n");

    // Generate assertions
    for (stmt, _span) in &tb.statements {
        if let TestStatement::Assert(assert) = stmt {
            out.push_str(&generate_assertion(assert, test_name));
        }
        if let TestStatement::SafetyAssert(check) = stmt {
            out.push_str(&generate_safety_assertion(check, test_name));
        }
    }

    out.push_str("    }\n\n");
    out
}

/// Generate the assertion check code for a single TestAssert.
fn generate_assertion(
    assert: &crate::ast::TestAssert,
    test_name: &str,
) -> String {
    let mut preamble = String::new();

    // Determine which computed value to use
    let (actual_var, actual_db_var, prop_name) = match assert.property {
        TestProperty::OutputRms => ("output_rms".to_string(), "output_rms_db".to_string(), "output.rms".to_string()),
        TestProperty::OutputPeak => ("output_peak".to_string(), "output_peak_db".to_string(), "output.peak".to_string()),
        TestProperty::InputRms => ("input_rms".to_string(), "input_rms_db".to_string(), "input.rms".to_string()),
        TestProperty::InputPeak => ("input_peak".to_string(), "input_peak_db".to_string(), "input.peak".to_string()),
        TestProperty::OutputRmsIn(start, end) => {
            let var = format!("output_rms_in_{}_{}", start, end);
            let db_var = format!("{}_db", var);
            preamble.push_str(&format!(
                "        let {var} = compute_rms(&output[0][{start}..{end}]);\n\
                 let {db_var} = rms_to_db({var});\n",
            ));
            (var, db_var, format!("output.rms_in {}..{}", start, end))
        }
        TestProperty::OutputPeakIn(start, end) => {
            let var = format!("output_peak_in_{}_{}", start, end);
            let db_var = format!("{}_db", var);
            preamble.push_str(&format!(
                "        let {var} = compute_peak(&output[0][{start}..{end}]);\n\
                 let {db_var} = peak_to_db({var});\n",
            ));
            (var, db_var, format!("output.peak_in {}..{}", start, end))
        }
        TestProperty::Frequency(freq) => {
            let var = format!("freq_mag_{}", freq as u64);
            let db_var = var.clone(); // compute_magnitude_at_freq already returns dB
            preamble.push_str(&format!(
                "        let {var} = compute_magnitude_at_freq(&output[0], {freq:.1}, TEST_SAMPLE_RATE as f64);\n",
            ));
            (var, db_var, format!("frequency {}Hz", freq))
        }
    };

    let op_str = match assert.op {
        TestOp::LessThan => "<",
        TestOp::GreaterThan => ">",
        TestOp::Equal => "==",
        TestOp::ApproxEqual => "~=",
    };

    // Determine if the threshold is in dB (negative values are typically dB)
    // Convention: if value is negative or has dB-range magnitude, treat as dB
    let value = assert.value;
    let is_db_value = value < 0.0 || value.abs() > 10.0;

    let (compare_var, display_val) = if is_db_value {
        (&actual_db_var, format!("{:.1} dB", value))
    } else {
        (&actual_var, format!("{:.6}", value))
    };

    let assertion_text = format!("{} {} {}", prop_name, op_str, display_val);

    let check = match assert.op {
        TestOp::LessThan => {
            format!(
                "        if !({compare_var} < {value:.6}_f32) {{\n            \
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"{assertion_text}\", \"< {display_val}\", &format!(\"{{:.2}}\", {compare_var})));\n        \
                 }}\n",
            )
        }
        TestOp::GreaterThan => {
            format!(
                "        if !({compare_var} > {value:.6}_f32) {{\n            \
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"{assertion_text}\", \"> {display_val}\", &format!(\"{{:.2}}\", {compare_var})));\n        \
                 }}\n",
            )
        }
        TestOp::Equal => {
            format!(
                "        if !(({compare_var} - {value:.6}_f32).abs() < 1e-6) {{\n            \
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"{assertion_text}\", \"{display_val}\", &format!(\"{{:.6}}\", {compare_var})));\n        \
                 }}\n",
            )
        }
        TestOp::ApproxEqual => {
            let tolerance = assert.tolerance.unwrap_or(if is_db_value { 1.0 } else { 0.01 });
            format!(
                "        if !(({compare_var} - {value:.6}_f32).abs() < {tolerance:.6}) {{\n            \
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"{assertion_text}\", \"~= {display_val}\", &format!(\"{{:.2}}\", {compare_var})));\n        \
                 }}\n",
            )
        }
    };

    format!("{}{}", preamble, check)
}

/// Generate safety assertion code (no_nan, no_denormal, no_inf).
fn generate_safety_assertion(check: &crate::ast::SafetyCheck, test_name: &str) -> String {
    use crate::ast::SafetyCheck;
    match check {
        SafetyCheck::NoNan => {
            format!(
                "        for (ch_idx, ch) in output.iter().enumerate() {{\n\
                 for (s_idx, sample) in ch.iter().enumerate() {{\n\
                 if sample.is_nan() {{\n\
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"no_nan\", \"no NaN values\", &format!(\"NaN at channel {{}} sample {{}}\", ch_idx, s_idx)));\n\
                 }}\n\
                 }}\n\
                 }}\n"
            )
        }
        SafetyCheck::NoDenormal => {
            format!(
                "        for (ch_idx, ch) in output.iter().enumerate() {{\n\
                 for (s_idx, sample) in ch.iter().enumerate() {{\n\
                 if *sample != 0.0 && sample.abs() < f32::MIN_POSITIVE {{\n\
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"no_denormal\", \"no denormal values\", &format!(\"denormal {{:.2e}} at channel {{}} sample {{}}\", sample, ch_idx, s_idx)));\n\
                 }}\n\
                 }}\n\
                 }}\n"
            )
        }
        SafetyCheck::NoInf => {
            format!(
                "        for (ch_idx, ch) in output.iter().enumerate() {{\n\
                 for (s_idx, sample) in ch.iter().enumerate() {{\n\
                 if sample.is_infinite() {{\n\
                 panic!(\"{{}}\", muse_test_fail(\"{test_name}\", \"no_inf\", \"no infinite values\", &format!(\"inf at channel {{}} sample {{}}\", ch_idx, s_idx)));\n\
                 }}\n\
                 }}\n\
                 }}\n"
            )
        }
    }
}

/// Collect names of parameters that use dB units (internal representation is gain-linear).
fn collect_db_param_names(plugin: &PluginDef) -> Vec<String> {
    let mut db_params = Vec::new();
    for (item, _) in &plugin.items {
        if let PluginItem::ParamDecl(param) = item {
            if param.param_type == ParamType::Float {
                let is_db = param.options.iter().any(|(opt, _)| {
                    matches!(opt, ParamOption::Unit(u) if u.eq_ignore_ascii_case("db"))
                });
                if is_db {
                    db_params.push(param.name.clone());
                }
            }
        }
    }
    db_params
}

/// Collect all parameter names with their default values and dB status.
/// Used to emit smoother resets after plugin initialization in test code.
fn collect_param_defaults(plugin: &PluginDef) -> Vec<ParamInfo> {
    let mut params = Vec::new();
    for (item, _) in &plugin.items {
        if let PluginItem::ParamDecl(param) = item {
            if param.param_type == ParamType::Float {
                let default_value = param.default.as_ref().and_then(|(expr, _)| {
                    if let Expr::Number(v, _) = expr { Some(*v) } else { None }
                }).unwrap_or(0.0);
                let is_db = param.options.iter().any(|(opt, _)| {
                    matches!(opt, ParamOption::Unit(u) if u.eq_ignore_ascii_case("db"))
                });
                params.push(ParamInfo {
                    name: param.name.clone(),
                    default_value,
                    is_db,
                });
            }
        }
    }
    params
}

/// Sanitize a test name into a valid Rust function identifier.
fn sanitize_test_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert a plugin display name to a Rust struct name (PascalCase, no spaces).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_test_name() {
        assert_eq!(
            sanitize_test_name("silence in produces silence out"),
            "silence_in_produces_silence_out"
        );
        assert_eq!(
            sanitize_test_name("positive gain increases level"),
            "positive_gain_increases_level"
        );
        assert_eq!(
            sanitize_test_name("test with 123 numbers"),
            "test_with_123_numbers"
        );
    }

    #[test]
    fn test_plugin_name_to_struct() {
        assert_eq!(plugin_name_to_struct("Warm Gain"), "WarmGain");
        assert_eq!(plugin_name_to_struct("My Plugin"), "MyPlugin");
    }
}
