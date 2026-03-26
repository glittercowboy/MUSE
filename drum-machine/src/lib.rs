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
struct PluginParams {}

impl Default for PluginParams {
    fn default() -> Self {
        Self {}
    }
}

const MAX_BLOCK_SIZE: usize = 64;

const SAMPLE_KICK_DATA: &[u8] = include_bytes!("/Users/lexchristopherson/Developer/music-lang/examples/samples/kick.wav");
const SAMPLE_SNARE_DATA: &[u8] = include_bytes!("/Users/lexchristopherson/Developer/music-lang/examples/samples/snare.wav");
const SAMPLE_HIHAT_DATA: &[u8] = include_bytes!("/Users/lexchristopherson/Developer/music-lang/examples/samples/hihat.wav");

fn voice_is_silent(voice: &Voice) -> bool {
    if let Some(level) = voice_adsr_level(voice) {
        level <= 0.0001
    } else {
        false
    }
}

fn voice_adsr_level(_voice: &Voice) -> Option<f32> {
    None
}

#[derive(Clone, Copy)]
struct Voice {
    voice_id: i32,
    channel: u8,
    note: u8,
    internal_voice_id: u64,
    note_freq: f32,
    velocity: f32,
    pressure: f32,
    tuning: f32,
    slide: f32,
    releasing: bool,
    play_pos_0: f32,
    play_active_0: bool,
    play_pos_1: f32,
    play_active_1: bool,
    play_pos_2: f32,
    play_active_2: bool,
}

struct DrumMachine {
    params: Arc<PluginParams>,
    voices: [Option<Voice>; 8],
    next_internal_voice_id: u64,
    sample_rate: f32,
    sample_kick: Vec<f32>,
    sample_kick_rate: u32,
    sample_snare: Vec<f32>,
    sample_snare_rate: u32,
    sample_hihat: Vec<f32>,
    sample_hihat_rate: u32,
}

impl Default for DrumMachine {
    fn default() -> Self {
        Self {
            params: Arc::new(PluginParams::default()),
            voices: [(); 8].map(|_| None),
            next_internal_voice_id: 0,
            sample_rate: 44100.0,
            sample_kick: Vec::new(),
            sample_kick_rate: 0,
            sample_snare: Vec::new(),
            sample_snare_rate: 0,
            sample_hihat: Vec::new(),
            sample_hihat_rate: 0,
        }
    }
}

impl Plugin for DrumMachine {
    const NAME: &'static str = "Drum Machine";
    const VENDOR: &'static str = "Muse Audio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
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
        {
            let cursor = std::io::Cursor::new(SAMPLE_KICK_DATA);
            let reader = hound::WavReader::new(cursor).expect("invalid WAV: kick");
            let spec = reader.spec();
            self.sample_kick_rate = spec.sample_rate;
            self.sample_kick = match spec.sample_format {
                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
                hound::SampleFormat::Int => {
                    let bits = spec.bits_per_sample;
                    let max_val = (1u64 << (bits - 1)) as f32;
                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()
                }
            };
        }
        {
            let cursor = std::io::Cursor::new(SAMPLE_SNARE_DATA);
            let reader = hound::WavReader::new(cursor).expect("invalid WAV: snare");
            let spec = reader.spec();
            self.sample_snare_rate = spec.sample_rate;
            self.sample_snare = match spec.sample_format {
                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
                hound::SampleFormat::Int => {
                    let bits = spec.bits_per_sample;
                    let max_val = (1u64 << (bits - 1)) as f32;
                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()
                }
            };
        }
        {
            let cursor = std::io::Cursor::new(SAMPLE_HIHAT_DATA);
            let reader = hound::WavReader::new(cursor).expect("invalid WAV: hihat");
            let spec = reader.spec();
            self.sample_hihat_rate = spec.sample_rate;
            self.sample_hihat = match spec.sample_format {
                hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
                hound::SampleFormat::Int => {
                    let bits = spec.bits_per_sample;
                    let max_val = (1u64 << (bits - 1)) as f32;
                    reader.into_samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max_val).collect()
                }
            };
        }
        true
    }

    fn reset(&mut self) {
        self.voices.fill(None);
        self.next_internal_voice_id = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
                let num_samples = buffer.samples();
        let output = buffer.as_slice();
        let mut next_event = context.next_event();
        let mut block_start: usize = 0;
        let mut block_end: usize = MAX_BLOCK_SIZE.min(num_samples);
        while block_start < num_samples {
            let this_block_internal_voice_id_start = self.next_internal_voice_id;
            'events: loop {
                match next_event {
                    Some(event) if (event.timing() as usize) <= block_start => {
                        match event {
                            NoteEvent::NoteOn {
                                timing,
                                voice_id,
                                channel,
                                note,
                                velocity,
                            } => {
                                let voice = self.start_voice(context, timing, voice_id, channel, note);
                                voice.note_freq = util::midi_note_to_freq(note);
                                voice.velocity = velocity;
                                voice.releasing = false;
                            }
                            NoteEvent::NoteOff {
                                voice_id,
                                channel,
                                note,
                                ..
                            } => {
                                self.start_release_for_voices(voice_id, channel, note);
                            }
                            NoteEvent::Choke {
                                timing,
                                voice_id,
                                channel,
                                note,
                            } => {
                                self.choke_voices(context, timing, voice_id, channel, note);
                            }
                            NoteEvent::PolyPressure {
                                voice_id,
                                note,
                                channel,
                                pressure,
                                ..
                            } => {
                                let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                                if let Some(idx) = self.get_voice_idx(search_id) {
                                    if let Some(ref mut voice) = self.voices[idx] {
                                        voice.pressure = pressure;
                                    }
                                }
                            }
                            NoteEvent::PolyTuning {
                                voice_id,
                                note,
                                channel,
                                tuning,
                                ..
                            } => {
                                let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                                if let Some(idx) = self.get_voice_idx(search_id) {
                                    if let Some(ref mut voice) = self.voices[idx] {
                                        voice.tuning = tuning;
                                    }
                                }
                            }
                            NoteEvent::PolyBrightness {
                                voice_id,
                                note,
                                channel,
                                brightness,
                                ..
                            } => {
                                let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                                if let Some(idx) = self.get_voice_idx(search_id) {
                                    if let Some(ref mut voice) = self.voices[idx] {
                                        voice.slide = brightness;
                                    }
                                }
                            }
                            _ => {}
                        }
                        next_event = context.next_event();
                    }
                    Some(event) if (event.timing() as usize) < block_end => {
                        block_end = event.timing() as usize;
                        break 'events;
                    }
                    _ => break 'events,
                }
            }
            let block_len = block_end - block_start;
            for channel in output.iter_mut() {
                channel[block_start..block_end].fill(0.0);
            }
            let mut terminated_voices = Vec::new();
            for voice in self.voices.iter_mut().filter_map(|voice| voice.as_mut()) {
                for value_idx in 0..block_len {
                    let sample_idx = block_start + value_idx;
                    let snd = if voice.note as f32 == 36.0_f32 {
                    if voice.play_active_0 { let __pos = voice.play_pos_0 as usize; if __pos < self.sample_kick.len() { let __s = self.sample_kick[__pos]; voice.play_pos_0 += 1.0; __s } else { voice.play_active_0 = false; 0.0_f32 } } else { 0.0_f32 }
                } else {
                    if voice.note as f32 == 38.0_f32 {
                    if voice.play_active_1 { let __pos = voice.play_pos_1 as usize; if __pos < self.sample_snare.len() { let __s = self.sample_snare[__pos]; voice.play_pos_1 += 1.0; __s } else { voice.play_active_1 = false; 0.0_f32 } } else { 0.0_f32 }
                } else {
                    if voice.note as f32 == 42.0_f32 {
                    if voice.play_active_2 { let __pos = voice.play_pos_2 as usize; if __pos < self.sample_hihat.len() { let __s = self.sample_hihat[__pos]; voice.play_pos_2 += 1.0; __s } else { voice.play_active_2 = false; 0.0_f32 } } else { 0.0_f32 }
                } else {
                    0.0_f32
                }
                }
                };
                    let output_sample = snd * voice.velocity;
                    for channel in output.iter_mut() { channel[sample_idx] += output_sample; }
                }
                if voice.releasing && voice_is_silent(voice) {
                    terminated_voices.push((voice.voice_id, voice.channel, voice.note));
                }
            }
            for (voice_id, channel, note) in terminated_voices {
                context.send_event(NoteEvent::VoiceTerminated {
                    timing: block_end as u32,
                    voice_id: Some(voice_id),
                    channel,
                    note,
                });
                if let Some(idx) = self.get_voice_idx(voice_id) {
                    self.voices[idx] = None;
                }
            }
            block_start = block_end;
            block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);
        }
        ProcessStatus::Normal
    }
}

impl DrumMachine {
    fn get_voice_idx(&mut self, voice_id: i32) -> Option<usize> {
        self.voices
            .iter_mut()
            .position(|voice| matches!(voice, Some(voice) if voice.voice_id == voice_id))
    }

    fn start_voice(
        &mut self,
        context: &mut impl ProcessContext<Self>,
        sample_offset: u32,
        voice_id: Option<i32>,
        channel: u8,
        note: u8,
    ) -> &mut Voice {
        let new_voice = Voice {
            voice_id: voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel)),
            internal_voice_id: self.next_internal_voice_id,
            channel,
            note,
            note_freq: util::midi_note_to_freq(note),
            velocity: 0.0,
            pressure: 0.0,
            tuning: 0.0,
            slide: 0.0,
            releasing: false,
            play_pos_0: 0.0, play_active_0: true, play_pos_1: 0.0, play_active_1: true, play_pos_2: 0.0, play_active_2: true,
        };
        self.next_internal_voice_id = self.next_internal_voice_id.wrapping_add(1);

        match self.voices.iter().position(|voice| voice.is_none()) {
            Some(free_voice_idx) => {
                self.voices[free_voice_idx] = Some(new_voice);
                self.voices[free_voice_idx].as_mut().unwrap()
            }
            None => {
                let oldest_voice = unsafe {
                    self.voices
                        .iter_mut()
                        .min_by_key(|voice| voice.as_ref().unwrap_unchecked().internal_voice_id)
                        .unwrap_unchecked()
                };

                {
                    let oldest_voice = oldest_voice.as_ref().unwrap();
                    context.send_event(NoteEvent::VoiceTerminated {
                        timing: sample_offset,
                        voice_id: Some(oldest_voice.voice_id),
                        channel: oldest_voice.channel,
                        note: oldest_voice.note,
                    });
                }

                *oldest_voice = Some(new_voice);
                oldest_voice.as_mut().unwrap()
            }
        }
    }

    fn start_release_for_voices(&mut self, voice_id: Option<i32>, channel: u8, note: u8) {
        for voice in self.voices.iter_mut() {
            match voice {
                Some(Voice {
                    voice_id: candidate_voice_id,
                    channel: candidate_channel,
                    note: candidate_note,
                    releasing,
                    ..
                }) if voice_id == Some(*candidate_voice_id)
                    || (channel == *candidate_channel && note == *candidate_note) =>
                {
                    *releasing = true;
                    if voice_id.is_some() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn choke_voices(
        &mut self,
        context: &mut impl ProcessContext<Self>,
        sample_offset: u32,
        voice_id: Option<i32>,
        channel: u8,
        note: u8,
    ) {
        for voice in self.voices.iter_mut() {
            match voice {
                Some(Voice {
                    voice_id: candidate_voice_id,
                    channel: candidate_channel,
                    note: candidate_note,
                    ..
                }) if voice_id == Some(*candidate_voice_id)
                    || (channel == *candidate_channel && note == *candidate_note) =>
                {
                    context.send_event(NoteEvent::VoiceTerminated {
                        timing: sample_offset,
                        voice_id: Some(*candidate_voice_id),
                        channel: *candidate_channel,
                        note: *candidate_note,
                    });
                    *voice = None;

                    if voice_id.is_some() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    const fn compute_fallback_voice_id(note: u8, channel: u8) -> i32 {
        note as i32 | ((channel as i32) << 16)
    }
}

impl ClapPlugin for DrumMachine {
    const CLAP_ID: &'static str = "dev.museaudio.drum-machine";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A sample-based drum machine");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Stereo,
    ];
    const CLAP_POLY_MODULATION_CONFIG: Option<PolyModulationConfig> = Some(PolyModulationConfig {
        max_voice_capacity: 8,
        supports_overlapping_voices: true,
    });
}

impl Vst3Plugin for DrumMachine {
    const VST3_CLASS_ID: [u8; 16] = *b"MuseDrumMach01  ";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
}

nih_export_clap!(DrumMachine);
nih_export_vst3!(DrumMachine);

#[cfg(test)]
#[allow(unused_variables, dead_code)]
mod tests {
    use super::*;

    struct TestProcessContext {
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<DrumMachine>>,
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

    impl ProcessContext<DrumMachine> for TestProcessContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<DrumMachine>> { self.events.pop_front() }
        fn send_event(&mut self, _event: PluginNoteEvent<DrumMachine>) {}
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
        plugin: &mut DrumMachine,
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
    fn test_silence_before_any_notes() {
        let mut plugin = DrumMachine::default();
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
        impl InitContext<DrumMachine> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);

        let mut channel_data = make_silence(512);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        let output = run_process(&mut plugin, &mut channel_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db < -120.000000_f32) {
            panic!("{}", muse_test_fail("silence before any notes", "output.rms < -120.0 dB", "< -120.0 dB", &format!("{:.2}", output_rms_db)));
        }
    }

    #[test]
    fn test_kick_on_note_36() {
        let mut plugin = DrumMachine::default();
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
        impl InitContext<DrumMachine> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);

        let mut channel_data = make_silence(8192);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        ctx.events.push_back(NoteEvent::NoteOn { timing: 0, voice_id: None, channel: 0, note: 36, velocity: 0.800000 });
        ctx.events.push_back(NoteEvent::NoteOff { timing: 4096, voice_id: None, channel: 0, note: 36, velocity: 0.0 });
        let output = run_process(&mut plugin, &mut channel_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db > -40.000000_f32) {
            panic!("{}", muse_test_fail("kick on note 36", "output.rms > -40.0 dB", "> -40.0 dB", &format!("{:.2}", output_rms_db)));
        }
        for (ch_idx, ch) in output.iter().enumerate() {
for (s_idx, sample) in ch.iter().enumerate() {
if sample.is_nan() {
panic!("{}", muse_test_fail("kick on note 36", "no_nan", "no NaN values", &format!("NaN at channel {} sample {}", ch_idx, s_idx)));
}
}
}
    }

    #[test]
    fn test_snare_on_note_38() {
        let mut plugin = DrumMachine::default();
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
        impl InitContext<DrumMachine> for InitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = InitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);

        let mut channel_data = make_silence(8192);
        let input_rms = compute_rms(&channel_data[0]);
        let input_peak = compute_peak(&channel_data[0]);
        let mut ctx = TestProcessContext::new(TEST_SAMPLE_RATE);
        ctx.events.push_back(NoteEvent::NoteOn { timing: 0, voice_id: None, channel: 0, note: 38, velocity: 0.800000 });
        ctx.events.push_back(NoteEvent::NoteOff { timing: 4096, voice_id: None, channel: 0, note: 38, velocity: 0.0 });
        let output = run_process(&mut plugin, &mut channel_data, &mut ctx);

        let output_rms = compute_rms(&output[0]);
        let output_peak = compute_peak(&output[0]);
        let output_rms_db = rms_to_db(output_rms);
        let output_peak_db = peak_to_db(output_peak);
        let input_rms_db = rms_to_db(input_rms);
        let input_peak_db = peak_to_db(input_peak);

        if !(output_rms_db > -40.000000_f32) {
            panic!("{}", muse_test_fail("snare on note 38", "output.rms > -40.0 dB", "> -40.0 dB", &format!("{:.2}", output_rms_db)));
        }
    }

}

#[cfg(feature = "preview")]
mod muse_preview {
    use super::*;

    struct PreviewProcessContext {
        transport: Transport,
        events: std::collections::VecDeque<PluginNoteEvent<DrumMachine>>,
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

    impl ProcessContext<DrumMachine> for PreviewProcessContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<DrumMachine>> { self.events.pop_front() }
        fn send_event(&mut self, _event: PluginNoteEvent<DrumMachine>) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    struct PreviewInstance {
        plugin: DrumMachine,
        ctx: PreviewProcessContext,
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_create(sample_rate: f32) -> *mut u8 {
        let mut plugin = DrumMachine::default();
        let layout = AudioIOLayout {
            main_input_channels: None,
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
        impl InitContext<DrumMachine> for PreviewInitCtx {
            fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
            fn execute(&self, _task: ()) {}
            fn set_latency_samples(&self, _samples: u32) {}
            fn set_current_voice_capacity(&self, _capacity: u32) {}
        }
        let mut init_ctx = PreviewInitCtx;
        plugin.initialize(&layout, &buffer_config, &mut init_ctx);
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
        0
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_get_param_name(index: u32, buf: *mut u8, buf_len: u32) -> u32 {
        let name: &str = match index {
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
            _ => 0.0,
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_set_param(ptr: *mut u8, index: u32, value: f32) {
        if ptr.is_null() { return; }
        let instance = &mut *(ptr as *mut PreviewInstance);
        match index {
            _ => {}
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_get_param(ptr: *mut u8, index: u32) -> f32 {
        if ptr.is_null() { return 0.0; }
        let instance = &*(ptr as *mut PreviewInstance);
        match index {
            _ => 0.0,
        }
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_get_num_channels() -> u32 {
        2
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_note_on(ptr: *mut u8, note: u8, velocity: f32) {
        if ptr.is_null() { return; }
        let instance = &mut *(ptr as *mut PreviewInstance);
        instance.ctx.events.push_back(NoteEvent::NoteOn {
            timing: 0,
            voice_id: None,
            channel: 0,
            note,
            velocity,
        });
    }

    #[no_mangle]
    pub unsafe extern "C" fn muse_preview_note_off(ptr: *mut u8, note: u8) {
        if ptr.is_null() { return; }
        let instance = &mut *(ptr as *mut PreviewInstance);
        instance.ctx.events.push_back(NoteEvent::NoteOff {
            timing: 0,
            voice_id: None,
            channel: 0,
            note,
            velocity: 0.0,
        });
    }

    #[no_mangle]
    pub extern "C" fn muse_preview_is_instrument() -> bool {
        true
    }
}
