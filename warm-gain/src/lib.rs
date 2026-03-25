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
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for PluginParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db())
            .with_smoother(SmoothingStyle::Logarithmic(50.0)),
        }
    }
}

const MAX_BLOCK_SIZE: usize = 64;

struct WarmGain {
    params: Arc<PluginParams>,
}

impl Default for WarmGain {
    fn default() -> Self {
        Self {
            params: Arc::new(PluginParams::default()),
        }
    }
}

impl Plugin for WarmGain {
    const NAME: &'static str = "Warm Gain";
    const VENDOR: &'static str = "Muse Audio";
    const URL: &'static str = "https://museaudio.dev";
    const EMAIL: &'static str = "hello@museaudio.dev";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
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
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::WebViewEditor::new(self.params.clone())))
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
                for channel_samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample = *sample * gain;
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for WarmGain {
    const CLAP_ID: &'static str = "dev.museaudio.warm-gain";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A warm, musical gain stage");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for WarmGain {
    const VST3_CLASS_ID: [u8; 16] = *b"MuseWarmGain1   ";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(WarmGain);
nih_export_vst3!(WarmGain);

#[cfg(test)]
#[allow(unused_variables, dead_code)]
mod tests {
    use super::*;

    struct TestProcessContext {
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<WarmGain>>,
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

    impl ProcessContext<WarmGain> for TestProcessContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<WarmGain>> { self.events.pop_front() }
        fn send_event(&mut self, _event: PluginNoteEvent<WarmGain>) {}
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
        plugin: &mut WarmGain,
        channel_data: &mut Vec<Vec<f32>>,
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
        let mut aux = AuxiliaryBuffers {
            inputs: &mut [],
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
    fn test_silence_in_produces_silence_out() {
        let mut plugin = WarmGain::default();
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate: TEST_SAMPLE_RATE,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct InitCtx;
        impl InitContext<WarmGain> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.gain.smoothed.reset(util::db_to_gain(0.000000_f32));

        let mut channel_data = make_silence(512);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        plugin.params.gain.smoothed.reset(util::db_to_gain(0.000000_f32));
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db < -120.000000_f32) {
            panic!("{}", muse_test_fail("silence in produces silence out", "output.rms < -120.0 dB", "< -120.0 dB", &format!("{:.2}", output_rms_db)));
        }
    }

    #[test]
    fn test_positive_gain_increases_level() {
        let mut plugin = WarmGain::default();
        let layout = AudioIOLayout {
            main_input_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            main_output_channels: NonZeroU32::new(TEST_CHANNELS as u32),
            ..AudioIOLayout::const_default()
        };
        let buffer_config = BufferConfig {
            sample_rate: TEST_SAMPLE_RATE,
            min_buffer_size: None,
            max_buffer_size: 0,
            process_mode: ProcessMode::Realtime,
        };
        struct InitCtx;
        impl InitContext<WarmGain> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
        plugin.params.gain.smoothed.reset(util::db_to_gain(0.000000_f32));

        let mut channel_data = make_sine(440.0, 1024);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        plugin.params.gain.smoothed.reset(util::db_to_gain(6.000000_f32));
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_peak > 1.000000_f32) {
            panic!("{}", muse_test_fail("positive gain increases level", "output.peak > 1.000000", "> 1.000000", &format!("{:.2}", output_peak)));
        }
    }

}
mod editor {
    use std::any::Any;
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
    use nih_plug::prelude::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, ProtocolObject, Sel};
    use objc2::{msg_send, msg_send_id, class, sel, AllocAnyThread, ClassType, DefinedClass, define_class, MainThreadOnly, MainThreadMarker};
    use objc2_foundation::{NSString, NSObjectProtocol};
    use objc2_app_kit::NSView;
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration, WKUserContentController, WKScriptMessageHandler};
    use super::PluginParams;

    pub struct WebViewEditor {
        params: Arc<PluginParams>,
        width: AtomicU32,
        height: AtomicU32,
        webview_ptr: Arc<AtomicPtr<c_void>>,
    }

    impl WebViewEditor {
        pub fn new(params: Arc<PluginParams>) -> Self {
            Self {
                params,
                width: AtomicU32::new(600),
                height: AtomicU32::new(400),
                webview_ptr: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            }
        }
    }

    struct WebViewHandle {
        webview: Retained<WKWebView>,
        webview_ptr: Arc<AtomicPtr<c_void>>,
    }

    // SAFETY: WKWebView is created and dropped on the main thread (GUI thread).
    // nih-plug guarantees Editor::spawn() is called from the main thread.
    unsafe impl Send for WebViewHandle {}

    impl Drop for WebViewHandle {
        fn drop(&mut self) {
            // Null the shared pointer BEFORE removing from superview
            // so param_value_changed() sees null and skips JS calls.
            self.webview_ptr.store(std::ptr::null_mut(), Ordering::Release);
            unsafe {
                let _: () = msg_send![&self.webview, removeFromSuperview];
            }
        }
    }

    struct ParamBridgeHandlerIvars {
        params: Arc<PluginParams>,
        context: Arc<dyn GuiContext>,
    }

    define_class! {
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MuseParamBridgeHandler"]
        #[ivars = ParamBridgeHandlerIvars]
        struct ParamBridgeHandler;

        unsafe impl NSObjectProtocol for ParamBridgeHandler {}

        unsafe impl WKScriptMessageHandler for ParamBridgeHandler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:))]
            unsafe fn userContentController_didReceiveScriptMessage(
                &self,
                _controller: &WKUserContentController,
                message: &objc2_web_kit::WKScriptMessage,
            ) {
                unsafe {
                    let body: Retained<AnyObject> = msg_send_id![message, body];
                    let body_str: Retained<NSString> = msg_send_id![&body, description];
                    let json_str = body_str.to_string();

                    // Parse JSON: {"id": "param_name", "value": 0.5}
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let (Some(id), Some(value)) = (
                            parsed.get("id").and_then(|v| v.as_str()),
                            parsed.get("value").and_then(|v| v.as_f64()),
                        ) {
                            let ivars = self.ivars();
                            let setter = ParamSetter::new(ivars.context.as_ref());
                            match id {
                                "gain" => {
                                    setter.begin_set_parameter(&ivars.params.gain);
                                    setter.set_parameter_normalized(&ivars.params.gain, value as f32);
                                    setter.end_set_parameter(&ivars.params.gain);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    impl ParamBridgeHandler {
        fn new(params: Arc<PluginParams>, context: Arc<dyn GuiContext>, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ParamBridgeHandlerIvars { params, context });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    impl Editor for WebViewEditor {
        fn spawn(
            &self,
            parent: ParentWindowHandle,
            context: Arc<dyn GuiContext>,
        ) -> Box<dyn Any + Send> {
            let ns_view_ptr = match parent {
                ParentWindowHandle::AppKitNsView(ptr) => ptr,
                _ => panic!("WebViewEditor only supports macOS (AppKitNsView)"),
            };

            unsafe {
                // SAFETY: nih-plug guarantees Editor::spawn() is called on the main thread.
                let mtm = MainThreadMarker::new_unchecked();
                let parent_view: &NSView = &*(ns_view_ptr as *const NSView);

                let config = WKWebViewConfiguration::new(mtm);
                let content_controller = config.userContentController();

                let handler = ParamBridgeHandler::new(self.params.clone(), context, mtm);
                let handler_proto = ProtocolObject::from_retained(handler);
                let handler_name = NSString::from_str("paramBridge");
                content_controller.addScriptMessageHandler_name(&handler_proto, &handler_name);

                let frame = parent_view.frame();
                let webview = WKWebView::initWithFrame_configuration(
                    WKWebView::alloc(mtm),
                    frame,
                    &config,
                );

                // Disable opaque background for seamless embedding
                let _: () = msg_send![&webview, setValue: Bool::NO forKey: &*NSString::from_str("drawsBackground")];

                let html_source = include_str!("../assets/editor.html");
                let html_string = NSString::from_str(html_source);
                let base_url: Option<&objc2_foundation::NSURL> = None;
                let _: () = msg_send![&webview, loadHTMLString: &*html_string baseURL: base_url];

                parent_view.addSubview(&webview);

                let raw_ptr = Retained::as_ptr(&webview) as *mut c_void;
                self.webview_ptr.store(raw_ptr, Ordering::Release);

                Box::new(WebViewHandle { webview, webview_ptr: self.webview_ptr.clone() })
            }
        }

        fn size(&self) -> (u32, u32) {
            (self.width.load(Ordering::Relaxed), self.height.load(Ordering::Relaxed))
        }

        fn set_scale_factor(&self, _factor: f32) -> bool {
            // macOS handles DPI natively, no scaling needed
            true
        }

        fn param_value_changed(&self, id: &str, normalized_value: f32) {
            let ptr = self.webview_ptr.load(Ordering::Acquire);
            if ptr.is_null() {
                return;
            }
            unsafe {
                let webview = ptr as *const objc2::runtime::AnyObject;
                let js = format!("window.updateParam('{}', {})", id, normalized_value);
                let ns_js = NSString::from_str(&js);
                let null_handler: *const c_void = std::ptr::null();
                let _: () = msg_send![webview, evaluateJavaScript: &*ns_js completionHandler: null_handler];
            }
        }

        fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
            // Modulation visualization deferred to future tier
        }

        fn param_values_changed(&self) {
            // Bulk sync: re-push all param values to JS.
            // Called on preset load, automation batch updates, etc.
            let ptr = self.webview_ptr.load(Ordering::Acquire);
            if ptr.is_null() {
                return;
            }
            self.param_value_changed("gain", self.params.gain.modulated_normalized_value());
        }
    }
}
