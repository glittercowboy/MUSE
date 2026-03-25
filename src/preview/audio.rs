//! CPAL-based audio output host for the live preview.
//!
//! `AudioHost` opens the default output device, creates a non-blocking output
//! stream, and drives audio processing through a shared `HostPlugin` reference.
//! During hot-swap (plugin is `None`), the callback outputs silence.

use super::host_plugin::HostPlugin;
use super::midi::MidiEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Shared plugin slot. `None` during hot-swap (outputs silence).
pub type PluginSlot = Arc<Mutex<Option<HostPlugin>>>;

/// Manages the CPAL audio stream and the shared plugin reference.
pub struct AudioHost {
    plugin_slot: PluginSlot,
    _stream: Stream,
    sample_rate: f32,
    num_channels: u16,
}

impl AudioHost {
    /// Open the default output device and start the audio stream.
    ///
    /// The stream immediately begins calling the audio callback. If no plugin
    /// is loaded yet, the callback outputs silence until `swap_plugin` is called.
    ///
    /// If `midi_rx` is provided, MIDI events are drained each callback and
    /// forwarded to the plugin via `note_on`/`note_off` before `process()`.
    pub fn start(
        initial_plugin: Option<HostPlugin>,
        midi_rx: Option<mpsc::Receiver<MidiEvent>>,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device found")?;

        let default_config = device
            .default_output_config()
            .map_err(|e| format!("failed to get default output config: {e}"))?;

        // Use the device's default sample rate and channel count.
        // Clamp channels to what the plugin expects (typically 2).
        let device_channels = default_config.channels();
        let sample_rate = default_config.sample_rate().0;

        let config = StreamConfig {
            channels: device_channels,
            sample_rate: SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let plugin_slot: PluginSlot = Arc::new(Mutex::new(initial_plugin));
        let slot_clone = Arc::clone(&plugin_slot);
        let num_channels = device_channels;
        let midi_rx = midi_rx.map(|rx| Arc::new(Mutex::new(rx)));
        let midi_rx_clone = midi_rx.clone();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    audio_callback(data, num_channels, &slot_clone, &midi_rx_clone);
                },
                |err| {
                    eprintln!("[muse preview] audio stream error: {err}");
                },
                None, // no timeout
            )
            .map_err(|e| format!("failed to build output stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("failed to start audio stream: {e}"))?;

        eprintln!(
            "[muse preview] audio: {} Hz, {} channels",
            sample_rate, device_channels
        );

        Ok(Self {
            plugin_slot,
            _stream: stream,
            sample_rate: sample_rate as f32,
            num_channels: device_channels,
        })
    }

    /// The sample rate the audio stream was opened at.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The number of output channels.
    pub fn num_channels(&self) -> u16 {
        self.num_channels
    }

    /// Hot-swap the loaded plugin.
    ///
    /// Briefly locks the mutex to swap the old plugin out and the new one in.
    /// The old plugin is dropped outside the lock (the Drop impl calls
    /// `muse_preview_destroy` and `dlclose`).
    ///
    /// Returns the old plugin (if any) so the caller can snapshot params
    /// before dropping it.
    pub fn swap_plugin(&self, new_plugin: HostPlugin) -> Option<HostPlugin> {
        let mut slot = self.plugin_slot.lock().unwrap();
        slot.replace(new_plugin)
    }

    /// Remove the current plugin (outputs silence). Returns the removed plugin.
    pub fn take_plugin(&self) -> Option<HostPlugin> {
        let mut slot = self.plugin_slot.lock().unwrap();
        slot.take()
    }

    /// Access the shared plugin slot directly (e.g. for param reads from main thread).
    pub fn plugin_slot(&self) -> &PluginSlot {
        &self.plugin_slot
    }
}

/// CPAL audio callback. Called on the audio thread for each buffer.
///
/// CPAL delivers interleaved f32 samples: [L0, R0, L1, R1, ...].
/// The plugin expects de-interleaved channel buffers, so we convert both ways.
///
/// If a MIDI receiver is provided, pending events are drained and forwarded to
/// the plugin via `note_on`/`note_off` BEFORE `process()` is called.
fn audio_callback(
    data: &mut [f32],
    device_channels: u16,
    slot: &PluginSlot,
    midi_rx: &Option<Arc<Mutex<mpsc::Receiver<MidiEvent>>>>,
) {
    let num_device_ch = device_channels as usize;
    if num_device_ch == 0 {
        return;
    }

    let num_frames = data.len() / num_device_ch;
    if num_frames == 0 {
        return;
    }

    // Try to lock without blocking. If we can't get the lock (swap in progress),
    // output silence for this buffer — better than blocking the audio thread.
    let mut guard = match slot.try_lock() {
        Ok(g) => g,
        Err(_) => {
            // Mutex is poisoned or contended — output silence
            data.fill(0.0);
            return;
        }
    };

    let plugin = match guard.as_mut() {
        Some(p) => p,
        None => {
            data.fill(0.0);
            return;
        }
    };

    // Drain pending MIDI events and forward to the plugin BEFORE process().
    // try_lock + try_recv are both non-blocking and allocation-free.
    if let Some(rx_arc) = midi_rx {
        if let Ok(rx) = rx_arc.try_lock() {
            while let Ok(event) = rx.try_recv() {
                match event {
                    MidiEvent::NoteOn { note, velocity } => {
                        plugin.note_on(note, velocity);
                    }
                    MidiEvent::NoteOff { note } => {
                        plugin.note_off(note);
                    }
                }
            }
        }
    }

    let plugin_channels = plugin.num_channels() as usize;

    // Allocate de-interleaved buffers for the plugin.
    // For effect plugins, we'd fill inputs from a source — but for preview
    // we generate silence as input and let the plugin's process block produce output.
    // (Instruments ignore input; effects pass through.)
    let input_bufs: Vec<Vec<f32>> = (0..plugin_channels)
        .map(|_| vec![0.0; num_frames])
        .collect();
    let mut output_bufs: Vec<Vec<f32>> = (0..plugin_channels)
        .map(|_| vec![0.0; num_frames])
        .collect();

    // For effect plugins, copy interleaved device input into de-interleaved plugin input.
    // (In preview mode we currently feed silence — a future version could route device input.)
    // For now: output_bufs are pre-zeroed, plugin writes into them.

    // Build slice references the plugin expects.
    let input_refs: Vec<&[f32]> = input_bufs.iter().map(|b| b.as_slice()).collect();
    let mut output_refs: Vec<&mut [f32]> = output_bufs.iter_mut().map(|b| b.as_mut_slice()).collect();

    plugin.process(&input_refs, &mut output_refs);

    // Interleave plugin output back into CPAL's buffer.
    // If plugin has fewer channels than device, duplicate channel 0 to fill.
    // If plugin has more, we only use the first `device_channels` worth.
    for frame in 0..num_frames {
        for dev_ch in 0..num_device_ch {
            let plugin_ch = if dev_ch < plugin_channels {
                dev_ch
            } else {
                // Upmix: repeat last available channel
                plugin_channels.saturating_sub(1)
            };
            data[frame * num_device_ch + dev_ch] = output_bufs[plugin_ch][frame];
        }
    }
}
