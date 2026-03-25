//! Audio input sources for effect plugin preview.
//!
//! Provides microphone capture (via CPAL input stream) routed through an rtrb
//! lock-free ring buffer into the plugin's input buffers. The Consumer end is
//! passed to `AudioHost::start()` for the output callback to read from.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

/// Ring buffer capacity in interleaved stereo samples.
/// ~4096 frames × 2 channels = 8192 individual f32 values.
const RING_BUFFER_CAPACITY: usize = 8192;

/// A running audio input that feeds samples into the ring buffer.
/// Holds the CPAL stream handle — dropping it stops the input.
pub struct AudioInput {
    _stream: Stream,
}

/// Open a CPAL input stream for microphone capture.
///
/// Returns the AudioInput handle (owns the stream) and the Consumer end
/// of the ring buffer. The input callback writes interleaved stereo f32
/// samples to the Producer; the output callback reads from the Consumer.
///
/// `target_sample_rate` is the output device's sample rate. The mic input
/// stream requests this rate; if unavailable, falls back to None with a warning.
///
/// Returns `Ok(None)` when the mic can't be opened (no device, permission
/// denied, unsupported sample rate). Callers should fall back to silence.
pub fn start_mic_input(
    target_sample_rate: u32,
) -> Result<Option<(AudioInput, Consumer<f32>)>, String> {
    let host = cpal::default_host();

    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            eprintln!("[muse preview] warning: no input device found, using silence");
            return Ok(None);
        }
    };

    let device_name = device.name().unwrap_or_else(|_| "unknown".into());

    // Find a supported config matching target sample rate.
    let supported_configs = match device.supported_input_configs() {
        Ok(cfgs) => cfgs.collect::<Vec<_>>(),
        Err(e) => {
            eprintln!(
                "[muse preview] warning: cannot query input configs: {e}, using silence"
            );
            return Ok(None);
        }
    };

    // Try to find a config that supports the target sample rate.
    let target_rate = SampleRate(target_sample_rate);
    let chosen_config = supported_configs
        .iter()
        .find(|cfg| cfg.min_sample_rate() <= target_rate && cfg.max_sample_rate() >= target_rate)
        .map(|cfg| cfg.with_sample_rate(target_rate));

    let supported_config = match chosen_config {
        Some(cfg) => cfg,
        None => {
            eprintln!(
                "[muse preview] warning: input device '{}' does not support {} Hz, using silence",
                device_name, target_sample_rate
            );
            return Ok(None);
        }
    };

    let input_channels = supported_config.channels() as usize;
    let config = StreamConfig {
        channels: supported_config.channels(),
        sample_rate: target_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let (mut producer, consumer) = RingBuffer::<f32>::new(RING_BUFFER_CAPACITY);

    // Build the input stream. The callback converts to interleaved stereo f32
    // and pushes to the ring buffer.
    let stream = match device.build_input_stream(
        &config,
        move |data: &[f32], _info: &cpal::InputCallbackInfo| {
            mic_input_callback(data, input_channels, &mut producer);
        },
        |err| {
            eprintln!("[muse preview] input stream error: {err}");
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            // macOS microphone permission denial shows up here.
            eprintln!(
                "[muse preview] warning: cannot open microphone ({}), using silence",
                e
            );
            return Ok(None);
        }
    };

    stream
        .play()
        .map_err(|e| format!("failed to start input stream: {e}"))?;

    eprintln!(
        "[muse preview] input: mic '{}' ({} Hz, {} ch)",
        device_name, target_sample_rate, input_channels
    );

    Ok(Some((AudioInput { _stream: stream }, consumer)))
}

/// CPAL input callback — converts captured audio to interleaved stereo f32
/// and pushes to the ring buffer.
///
/// Channel mapping:
/// - mono → duplicate to stereo
/// - stereo → pass through
/// - >2 channels → take first 2
///
/// On buffer full, samples are silently dropped (no blocking).
fn mic_input_callback(data: &[f32], input_channels: usize, producer: &mut Producer<f32>) {
    if input_channels == 0 {
        return;
    }

    let num_frames = data.len() / input_channels;

    for frame in 0..num_frames {
        let base = frame * input_channels;

        // Extract left and right from however many input channels we have.
        let left = data[base];
        let right = if input_channels >= 2 {
            data[base + 1]
        } else {
            // Mono: duplicate to stereo.
            left
        };

        // Push stereo pair. On full buffer, drop samples (no blocking).
        let _ = producer.push(left);
        let _ = producer.push(right);
    }
}
