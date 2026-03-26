use nih_plug::prelude::*;
use nih_plug::params::FloatParam;
use nih_plug::params::IntParam;
use nih_plug::params::BoolParam;
use nih_plug::params::EnumParam;
use nih_plug::params::Params;
use nih_plug::params::range::{FloatRange, IntRange};
use nih_plug::params::smoothing::SmoothingStyle;
use nih_plug::formatters;
use nih_plug::util;
use nih_plug::{nih_export_clap, nih_export_vst3};
use std::sync::Arc;

#[derive(Params)]
struct PluginParams {
    #[id = "threshold"]
    pub threshold: FloatParam,
    #[id = "amount"]
    pub amount: FloatParam,
}

impl Default for PluginParams {
    fn default() -> Self {
        Self {
            threshold: FloatParam::new(
                "Threshold",
                0.1,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Logarithmic(10.0)),
            amount: FloatParam::new(
                "Amount",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(5.0)),
        }
    }
}

const MAX_BLOCK_SIZE: usize = 64;

/// Per-call-site RMS level detector state (sliding window).
#[derive(Clone, Copy)]
struct RmsState {
    buffer: [f32; 4096],
    write_pos: usize,
    sum: f32,
    count: usize,
}

impl Default for RmsState {
    fn default() -> Self {
        Self {
            buffer: [0.0; 4096],
            write_pos: 0,
            sum: 0.0,
            count: 0,
        }
    }
}

/// Process one sample through a sliding-window RMS level detector.
///
/// `window_ms` is the analysis window in milliseconds. Returns the RMS level.
fn process_rms(state: &mut RmsState, input: f32, window_ms: f32, sample_rate: f32) -> f32 {
    let window_samples = ((window_ms / 1000.0 * sample_rate) as usize).clamp(1, 4096);

    // Remove oldest sample's contribution
    let oldest_idx = if state.write_pos >= window_samples {
        state.write_pos - window_samples
    } else {
        4096 + state.write_pos - window_samples
    };
    let oldest = state.buffer[oldest_idx];
    state.sum -= oldest * oldest;

    // Add new sample's contribution
    let sq = input * input;
    state.sum += sq;
    state.buffer[state.write_pos] = input;
    state.write_pos = (state.write_pos + 1) % 4096;

    if state.count < window_samples {
        state.count += 1;
    }

    (state.sum / state.count as f32).max(0.0).sqrt()
}


struct SidechainComp {
    params: Arc<PluginParams>,
    rms_state_0: RmsState,
    sample_rate: f32,
}

impl Default for SidechainComp {
    fn default() -> Self {
        Self {
            params: Arc::new(PluginParams::default()),
            rms_state_0: RmsState::default(),
            sample_rate: 44100.0,
        }
    }
}

impl Plugin for SidechainComp {
    const NAME: &'static str = "Sidechain Comp";
    const VENDOR: &'static str = "Muse Audio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: PortNames {
                aux_inputs: &["Sidechain"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }
    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
                let sidechain_slices = aux.inputs[0].as_slice_immutable();
        for (sample_idx, channel_samples) in buffer.iter_samples().enumerate() {
            let sidechain_sample = sidechain_slices[0][sample_idx];
            let threshold = self.params.threshold.smoothed.next();
            let amount = self.params.amount.smoothed.next();
            for sample in channel_samples {
                let sc_level = process_rms(&mut self.rms_state_0, sidechain_sample, 10.0_f32, self.sample_rate);
                let duck = if sc_level > threshold {
                    1.0_f32 - amount * (1.0_f32 - threshold / sc_level)
                } else {
                    1.0_f32
                };
                *sample = *sample * duck;
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for SidechainComp {
    const CLAP_ID: &'static str = "dev.museaudio.sidechain-comp";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A sidechain compressor — ducks the main signal when the sidechain is loud");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for SidechainComp {
    const VST3_CLASS_ID: [u8; 16] = *b"MuseSidechainCmp";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(SidechainComp);
nih_export_vst3!(SidechainComp);

#[cfg(test)]
#[allow(unused_variables, dead_code)]
mod tests {
    use super::*;

    struct TestProcessContext {
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<SidechainComp>>,
    }

    impl TestProcessContext {
        fn new(sample_rate: f32) -> Self {
            // Transport has pub(crate) fields we can't set, so zero-init and
            // overwrite the public ones. All private fields are Option<T> which
            // are valid as zeroed (None).
            let mut transport: Transport = unsafe { std::mem::zeroed() };
            transport.playing = true;
            transport.recording = false;
            transport.preroll_active = Some(false);
            transport.sample_rate = sample_rate;
            transport.tempo = Some(120.0);
            transport.time_sig_numerator = Some(4);
            transport.time_sig_denominator = Some(4);
            Self { transport, events: std::collections::VecDeque::new() }
        }
    }

    impl ProcessContext<SidechainComp> for TestProcessContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<SidechainComp>> { self.events.pop_front() }
        fn send_event(&mut self, _event: PluginNoteEvent<SidechainComp>) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    const TEST_SAMPLE_RATE: f32 = 44100.0;
    const TEST_CHANNELS: usize = 2;

    fn make_silence(samples: usize) -> Vec<Vec<f32>> {
        vec![vec![0.0_f32; samples]; TEST_CHANNELS]
    }

    fn make_sine(freq: f64, samples: usize) -> Vec<Vec<f32>> {
        let sr = TEST_SAMPLE_RATE as f64;
        let channel: Vec<f32> = (0..samples)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin() as f32)
            .collect();
        vec![channel.clone(), channel]
    }

    fn make_impulse(samples: usize) -> Vec<Vec<f32>> {
        let mut channel = vec![0.0_f32; samples];
        if !channel.is_empty() {
            channel[0] = 1.0;
        }
        vec![channel.clone(), channel]
    }

    fn compute_rms(data: &[f32]) -> f32 {
        if data.is_empty() { return 0.0; }
        let sum_sq: f32 = data.iter().map(|s| s * s).sum();
        (sum_sq / data.len() as f32).sqrt()
    }

    fn compute_peak(data: &[f32]) -> f32 {
        data.iter().map(|s| s.abs()).fold(0.0_f32, f32::max)
    }

    fn rms_to_db(rms: f32) -> f32 {
        if rms <= 0.0 { -f32::INFINITY } else { 20.0 * rms.log10() }
    }

    fn peak_to_db(peak: f32) -> f32 {
        if peak <= 0.0 { -f32::INFINITY } else { 20.0 * peak.log10() }
    }

    /// Run a plugin's process() on owned audio data, returning the output buffer contents.
    fn run_process(
        plugin: &mut SidechainComp,
        channel_data: &mut Vec<Vec<f32>>,
        aux_input_data: &mut Vec<Vec<Vec<f32>>>,
        ctx: &mut TestProcessContext,
    ) -> Vec<Vec<f32>> {
        let num_samples = channel_data[0].len();
        let mut buffer = Buffer::default();
        unsafe {
            buffer.set_slices(num_samples, |output_slices| {
                output_slices.clear();
                for ch in channel_data.iter_mut() {
                    // Safety: the slice lives as long as channel_data which outlives buffer usage
                    let slice: &mut [f32] = &mut ch[..];
                    let slice: &'static mut [f32] = std::mem::transmute(slice);
                    output_slices.push(slice);
                }
            });
        }
        // Construct aux input buffers from aux_input_data[aux_idx][channel][sample]
        let mut aux_bufs: Vec<Buffer> = Vec::new();
        for aux_channels in aux_input_data.iter_mut() {
            let mut aux_buf = Buffer::default();
            if !aux_channels.is_empty() {
                let aux_samples = aux_channels[0].len();
                unsafe {
                    aux_buf.set_slices(aux_samples, |slices| {
                        slices.clear();
                        for ch in aux_channels.iter_mut() {
                            let slice: &mut [f32] = &mut ch[..];
                            let slice: &'static mut [f32] = std::mem::transmute(slice);
                            slices.push(slice);
                        }
                    });
                }
            }
            aux_bufs.push(aux_buf);
        }
        let mut aux = AuxiliaryBuffers {
            inputs: &mut aux_bufs,
            outputs: &mut [],
        };
        plugin.process(&mut buffer, &mut aux, ctx);
        // Copy output data before dropping the buffer
        channel_data.clone()
    }

    fn muse_test_fail(test_name: &str, assertion: &str, expected: &str, actual: &str) -> String {
        format!(
            "MUSE_TEST_FAIL:{{\"test\":\"{}\",\"assertion\":\"{}\",\"expected\":\"{}\",\"actual\":\"{}\"}}",
            test_name, assertion, expected, actual
        )
    }

    #[test]
    fn test_silence_sidechain_no_ducking() {
        let mut plugin = SidechainComp::default();
        const AUX_PORTS: [NonZeroU32; 1] = [new_nonzero_u32(2)];
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            aux_input_ports: &AUX_PORTS,
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate: TEST_SAMPLE_RATE,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct InitCtx;
        impl InitContext<SidechainComp> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.threshold.smoothed.reset(0.100000_f32);
        plugin.params.amount.smoothed.reset(1.000000_f32);

        let mut channel_data = make_sine(440.0, 4096);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        plugin.params.threshold.smoothed.reset(0.100000_f32);
        plugin.params.amount.smoothed.reset(1.000000_f32);
        let mut aux_input_data: Vec<Vec<Vec<f32>>> = vec![make_silence(4096); 1];
        aux_input_data[0] = make_silence(4096);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut aux_input_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db > -10.000000_f32) {
            panic!("{}", muse_test_fail("silence sidechain no ducking", "output.rms > -10.0 dB", "> -10.0 dB", &format!("{:.2}", output_rms_db)));
        }
    }

    #[test]
    fn test_loud_sidechain_causes_ducking() {
        let mut plugin = SidechainComp::default();
        const AUX_PORTS: [NonZeroU32; 1] = [new_nonzero_u32(2)];
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            aux_input_ports: &AUX_PORTS,
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate: TEST_SAMPLE_RATE,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct InitCtx;
        impl InitContext<SidechainComp> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.threshold.smoothed.reset(0.100000_f32);
        plugin.params.amount.smoothed.reset(1.000000_f32);

        let mut channel_data = make_sine(440.0, 4096);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        plugin.params.threshold.smoothed.reset(0.100000_f32);
        plugin.params.amount.smoothed.reset(1.000000_f32);
        let mut aux_input_data: Vec<Vec<Vec<f32>>> = vec![make_silence(4096); 1];
        aux_input_data[0] = make_sine(100.0, 4096);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut aux_input_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db < -6.000000_f32) {
            panic!("{}", muse_test_fail("loud sidechain causes ducking", "output.rms < -6.0 dB", "< -6.0 dB", &format!("{:.2}", output_rms_db)));
        }
    }

    #[test]
    fn test_safety_checks() {
        let mut plugin = SidechainComp::default();
        const AUX_PORTS: [NonZeroU32; 1] = [new_nonzero_u32(2)];
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            aux_input_ports: &AUX_PORTS,
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate: TEST_SAMPLE_RATE,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct InitCtx;
        impl InitContext<SidechainComp> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.threshold.smoothed.reset(0.100000_f32);
        plugin.params.amount.smoothed.reset(1.000000_f32);

        let mut channel_data = make_sine(440.0, 1024);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        let mut aux_input_data: Vec<Vec<Vec<f32>>> = vec![make_silence(1024); 1];
        aux_input_data[0] = make_sine(100.0, 1024);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut aux_input_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        for (ch_idx, ch) in output.iter().enumerate() {
for (s_idx, sample) in ch.iter().enumerate() {
if sample.is_nan() {
panic!("{}", muse_test_fail("safety checks", "no_nan", "no NaN values", &format!("NaN at channel {} sample {}", ch_idx, s_idx)));
}
}
}
        for (ch_idx, ch) in output.iter().enumerate() {
for (s_idx, sample) in ch.iter().enumerate() {
if sample.is_infinite() {
panic!("{}", muse_test_fail("safety checks", "no_inf", "no infinite values", &format!("inf at channel {} sample {}", ch_idx, s_idx)));
}
}
}
        for (ch_idx, ch) in output.iter().enumerate() {
for (s_idx, sample) in ch.iter().enumerate() {
if *sample != 0.0 && sample.abs() < f32::MIN_POSITIVE {
panic!("{}", muse_test_fail("safety checks", "no_denormal", "no denormal values", &format!("denormal {:.2e} at channel {} sample {}", sample, ch_idx, s_idx)));
}
}
}
    }

}

#[cfg(feature = "preview")]
mod muse_preview {
    use super::*;

    struct PreviewProcessContext {
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<SidechainComp>>,
    }

    impl PreviewProcessContext {
        fn new(sample_rate: f32) -> Self {
            let mut transport: Transport = unsafe { std::mem::zeroed() };
            transport.playing = true;
            transport.recording = false;
            transport.preroll_active = Some(false);
            transport.sample_rate = sample_rate;
            transport.tempo = Some(120.0);
            transport.time_sig_numerator = Some(4);
            transport.time_sig_denominator = Some(4);
            Self { transport, events: std::collections::VecDeque::new() }
        }
    }

    impl ProcessContext<SidechainComp> for PreviewProcessContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<SidechainComp>> { self.events.pop_front() }
        fn send_event(&mut self, _event: PluginNoteEvent<SidechainComp>) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    struct PreviewInstance {
        plugin: SidechainComp,
        ctx: PreviewProcessContext,
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_create(sample_rate: f32) -> *mut u8 {
        let mut plugin = SidechainComp::default();
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct PreviewInitCtx;
        impl InitContext<SidechainComp> for PreviewInitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = PreviewInitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.threshold.smoothed.reset(0.1_f32);
        plugin.params.amount.smoothed.reset(1_f32);
        let ctx = PreviewProcessContext::new(sample_rate);
        let instance = Box::new(PreviewInstance { plugin, ctx });
        Box::into_raw(instance) as *mut u8
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_destroy(ptr: *mut u8) {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr as *mut PreviewInstance));
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_process(
        ptr: *mut u8,
        inputs: *const *const f32,
        outputs: *mut *mut f32,
        num_channels: u32,
        num_samples: u32,
    ) {
        if ptr.is_null() { return; }
        let instance = &mut *(ptr as *mut PreviewInstance);
        let nc = num_channels as usize;
        let ns = num_samples as usize;

        // Build owned channel buffers from raw pointers
        let mut channel_data: Vec<Vec<f32>> = Vec::with_capacity(nc);
        for ch in 0..nc {
            let mut samples = vec![0.0_f32; ns];
            if !inputs.is_null() {
                let in_ptr = *inputs.add(ch);
                if !in_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(in_ptr, samples.as_mut_ptr(), ns);
                }
            }
            channel_data.push(samples);
        }

        // Construct nih-plug Buffer from owned data
        let mut buffer = Buffer::default();
        buffer.set_slices(ns, |output_slices| {
            output_slices.clear();
            for ch in channel_data.iter_mut() {
                let slice: &mut [f32] = &mut ch[..];
                let slice: &'static mut [f32] = std::mem::transmute(slice);
                output_slices.push(slice);
            }
        });

        let mut aux = AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        };

        instance.plugin.process(&mut buffer, &mut aux, &mut instance.ctx);

        // Copy processed data to output pointers
        if !outputs.is_null() {
            for ch in 0..nc {
                let out_ptr = *outputs.add(ch);
                if !out_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        channel_data[ch].as_ptr(),
                        out_ptr,
                        ns,
                    );
                }
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_get_param_count() -> u32 {
        2
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_get_param_name(index: u32, buf: *mut u8, buf_len: u32) -> u32 {
        let name: &str = match index {
            0 => "threshold",
            1 => "amount",
            _ => return 0,
        };
        let bytes = name.as_bytes();
        let copy_len = bytes.len().min(buf_len as usize);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, copy_len);
        copy_len as u32
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_get_param_default(index: u32) -> f32 {
        let params = PluginParams::default();
        match index {
            0 => params.threshold.value(),
            1 => params.amount.value(),
            _ => 0.0,
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_set_param(ptr: *mut u8, index: u32, value: f32) {
        if ptr.is_null() { return; }
        let instance = &mut *(ptr as *mut PreviewInstance);
        match index {
            0 => { instance.plugin.params.threshold.smoothed.reset(value); }
            1 => { instance.plugin.params.amount.smoothed.reset(value); }
            _ => {}
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_get_param(ptr: *mut u8, index: u32) -> f32 {
        if ptr.is_null() { return 0.0; }
        let instance = &*(ptr as *mut PreviewInstance);
        match index {
            0 => instance.plugin.params.threshold.smoothed.previous_value(),
            1 => instance.plugin.params.amount.smoothed.previous_value(),
            _ => 0.0,
        }
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_get_num_channels() -> u32 {
        2
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_note_on(ptr: *mut u8, note: u8, velocity: f32) {
        let _ = (ptr, note, velocity);
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_note_off(ptr: *mut u8, note: u8) {
        let _ = (ptr, note);
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_is_instrument() -> bool {
        false
    }
}
