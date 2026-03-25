//! Audio input sources for effect plugin preview.
//!
//! Provides microphone capture (via CPAL input stream) and WAV file playback
//! (via hound + looping feeder thread), both routed through an rtrb lock-free
//! ring buffer into the plugin's input buffers. The Consumer end is passed to
//! `AudioHost::start()` for the output callback to read from.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

/// Ring buffer capacity in interleaved stereo samples.
/// ~4096 frames × 2 channels = 8192 individual f32 values.
const RING_BUFFER_CAPACITY: usize = 8192;

/// Chunk size for WAV feeder thread writes.
/// Writes this many stereo samples per iteration before checking stop flag.
const FILE_FEED_CHUNK: usize = 512;

/// A running audio input that feeds samples into the ring buffer.
/// Holds either a CPAL stream handle or a feeder thread handle.
/// Dropping it stops the input source.
pub struct AudioInput {
    _stream: Option<Stream>,
    _stop_flag: Option<Arc<AtomicBool>>,
    _thread: Option<thread::JoinHandle<()>>,
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

    Ok(Some((AudioInput { _stream: Some(stream), _stop_flag: None, _thread: None }, consumer)))
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

// ── WAV file input ──────────────────────────────────────────────────────────

/// Decode a WAV file to interleaved stereo f32 samples in [-1.0, 1.0].
///
/// Handles i16, i24 (i32 with 24 bits), and f32 sample formats.
/// Channel mapping: mono → duplicate to stereo, >2ch → take first 2.
///
/// Returns the decoded samples and the WAV file's sample rate.
fn decode_wav_to_stereo_f32(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let reader = hound::WavReader::open(path).map_err(|e| {
        if e.to_string().contains("No such file") || e.to_string().contains("not found") {
            format!("file not found: {}", path.display())
        } else {
            format!("cannot read WAV file '{}': {e}", path.display())
        }
    })?;

    let spec = reader.spec();
    let wav_channels = spec.channels as usize;
    let wav_rate = spec.sample_rate;

    if wav_channels == 0 {
        return Err(format!(
            "invalid WAV file '{}': 0 channels",
            path.display()
        ));
    }

    // Decode all samples to f32.
    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("error decoding WAV '{}': {e}", path.display()))?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            reader
                .into_samples::<i32>()
                .map(|s| {
                    s.map(|v| match bits {
                        16 => v as f32 / 32768.0,
                        24 => v as f32 / 8388608.0,
                        32 => v as f32 / 2147483648.0,
                        _ => v as f32 / ((1i64 << (bits - 1)) as f32),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("error decoding WAV '{}': {e}", path.display()))?
        }
    };

    if raw_samples.is_empty() {
        return Err(format!("WAV file '{}' contains no audio data", path.display()));
    }

    let num_frames = raw_samples.len() / wav_channels;

    // Convert to interleaved stereo.
    let mut stereo = Vec::with_capacity(num_frames * 2);
    for frame in 0..num_frames {
        let base = frame * wav_channels;
        let left = raw_samples[base];
        let right = if wav_channels >= 2 {
            raw_samples[base + 1]
        } else {
            left // mono → duplicate
        };
        stereo.push(left);
        stereo.push(right);
    }

    Ok((stereo, wav_rate))
}

/// Open a WAV file for looping playback into the ring buffer.
///
/// Decodes the entire file upfront (validates format, catches errors early),
/// then spawns a feeder thread that loops through the decoded samples.
///
/// `target_sample_rate` is the output device's rate. If the WAV rate differs,
/// a warning is printed but playback continues at the output rate (no resampling).
///
/// # Errors
/// Returns `Err(String)` on file-not-found or invalid WAV format.
/// These are fatal — the caller should exit(2).
pub fn start_file_input(
    path: &Path,
    target_sample_rate: u32,
) -> Result<(AudioInput, Consumer<f32>), String> {
    let (samples, wav_rate) = decode_wav_to_stereo_f32(path)?;

    if wav_rate != target_sample_rate {
        eprintln!(
            "[muse preview] warning: WAV sample rate ({} Hz) differs from output ({} Hz) — \
             playing without resampling",
            wav_rate, target_sample_rate
        );
    }

    let num_frames = samples.len() / 2;
    eprintln!(
        "[muse preview] input: file '{}' ({} Hz, {} frames, looping)",
        path.display(),
        wav_rate,
        num_frames
    );

    let (producer, consumer) = RingBuffer::<f32>::new(RING_BUFFER_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();

    let handle = thread::Builder::new()
        .name("muse-file-input".into())
        .spawn(move || {
            file_feeder_loop(samples, producer, stop_flag);
        })
        .map_err(|e| format!("failed to spawn file input thread: {e}"))?;

    Ok((
        AudioInput {
            _stream: None,
            _stop_flag: Some(stop),
            _thread: Some(handle),
        },
        consumer,
    ))
}

/// Feeder thread: loops through decoded stereo samples, pushing chunks to
/// the ring buffer. Sleeps briefly when the buffer is full to avoid busy-looping.
fn file_feeder_loop(
    samples: Vec<f32>,
    mut producer: Producer<f32>,
    stop: Arc<AtomicBool>,
) {
    let total = samples.len();
    let mut pos = 0;

    while !stop.load(Ordering::Relaxed) {
        let mut written = 0;

        // Try to write a chunk of stereo samples.
        while written < FILE_FEED_CHUNK * 2 && !stop.load(Ordering::Relaxed) {
            match producer.push(samples[pos]) {
                Ok(()) => {
                    pos += 1;
                    if pos >= total {
                        pos = 0; // loop seamlessly
                    }
                    written += 1;
                }
                Err(_) => {
                    // Buffer full — yield and retry.
                    break;
                }
            }
        }

        if written == 0 {
            // Buffer was completely full, sleep to avoid busy-loop.
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        // Signal the file feeder thread to stop and wait for it.
        if let Some(stop) = &self._stop_flag {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self._thread.take() {
            let _ = handle.join();
        }
        // CPAL stream (_stream) stops on drop automatically.
    }
}
