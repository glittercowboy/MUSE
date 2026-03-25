//! Generates inline DSP helper functions for the generated plugin crate.
//!
//! Generates state structs and processing functions for DSP primitives:
//! - **Filter (Lowpass)**: BiquadState struct, coefficient calculation, process_biquad()
//! - **Tanh**: uses f32's built-in .tanh(), no helper needed
//! - **Mix**: inline (a + b) * 0.5, no helper needed
//! - **Gain**: inline multiply, no helper needed

use std::collections::HashSet;

use crate::dsp::primitives::{DspPrimitive, EnvKind, FilterKind, OscKind};

/// Generate DSP helper code for the set of primitives used in the plugin.
///
/// Returns Rust code for any state structs and processing functions needed.
pub fn generate_dsp_helpers(used_primitives: &HashSet<DspPrimitive>) -> String {
    let mut out = String::new();

    let needs_lowpass = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(FilterKind::Lowpass))
    });
    let needs_bandpass = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(FilterKind::Bandpass))
    });
    let needs_highpass = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(FilterKind::Highpass))
    });
    let needs_notch = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(FilterKind::Notch))
    });
    let needs_any_biquad = needs_lowpass || needs_bandpass || needs_highpass || needs_notch;

    if needs_any_biquad {
        out.push_str(&generate_biquad_state());
        out.push('\n');
    }

    if needs_lowpass {
        out.push_str(&generate_process_biquad());
        out.push('\n');
    }

    if needs_bandpass {
        out.push_str(&generate_process_biquad_bandpass());
        out.push('\n');
    }

    if needs_highpass {
        out.push_str(&generate_process_biquad_highpass());
        out.push('\n');
    }

    if needs_notch {
        out.push_str(&generate_process_biquad_notch());
        out.push('\n');
    }

    // ── Oscillators ──────────────────────────────────────────
    let needs_any_osc = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(_) | DspPrimitive::Lfo | DspPrimitive::Pulse)
    });

    if needs_any_osc {
        out.push_str(&generate_osc_state());
        out.push('\n');
    }

    let needs_saw = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(OscKind::Saw))
    });
    let needs_square = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(OscKind::Square))
    });
    let needs_sine = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(OscKind::Sine) | DspPrimitive::Lfo)
    });
    let needs_triangle = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(OscKind::Triangle))
    });

    if needs_saw {
        out.push_str(&generate_process_osc_saw());
        out.push('\n');
    }
    if needs_square {
        out.push_str(&generate_process_osc_square());
        out.push('\n');
    }
    if needs_sine {
        out.push_str(&generate_process_osc_sine());
        out.push('\n');
    }
    if needs_triangle {
        out.push_str(&generate_process_osc_triangle());
        out.push('\n');
    }

    let needs_pulse = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Pulse)
    });
    if needs_pulse {
        out.push_str(&generate_process_osc_pulse());
        out.push('\n');
    }

    // ── Envelopes ────────────────────────────────────────────
    let needs_adsr = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Envelope(EnvKind::Adsr))
    });

    if needs_adsr {
        out.push_str(&generate_adsr_state());
        out.push('\n');
        out.push_str(&generate_process_adsr());
        out.push('\n');
    }

    // ── Chorus ───────────────────────────────────────────────
    let needs_chorus = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Chorus)
    });

    if needs_chorus {
        out.push_str(&generate_chorus_state());
        out.push('\n');
        out.push_str(&generate_process_chorus());
        out.push('\n');
    }

    // ── Compressor ───────────────────────────────────────────
    let needs_compressor = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Compressor)
    });

    if needs_compressor {
        out.push_str(&generate_compressor_state());
        out.push('\n');
        out.push_str(&generate_process_compressor());
        out.push('\n');
    }

    out
}

/// Generate the BiquadState struct with Default impl.
fn generate_biquad_state() -> String {
    r#"/// Per-channel biquad filter state for IIR filtering.
#[derive(Clone, Copy)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for BiquadState {
    fn default() -> Self {
        Self {
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_biquad function that computes lowpass biquad coefficients
/// per-sample and applies the filter.
fn generate_process_biquad() -> String {
    r#"/// Process a single sample through a lowpass biquad filter.
///
/// Recalculates coefficients each sample from cutoff and resonance.
/// This is simple but correct — suitable for proof-of-concept.
fn process_biquad(state: &mut BiquadState, input: f32, cutoff: f32, resonance: f32, sample_rate: f32) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * (resonance + 0.001));

    let b0 = (1.0 - cos_omega) / 2.0;
    let b1 = 1.0 - cos_omega;
    let b2 = (1.0 - cos_omega) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    // Normalize coefficients
    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    // Direct Form I
    let output = b0 * input + b1 * state.x1 + b2 * state.x2 - a1 * state.y1 - a2 * state.y2;

    state.x2 = state.x1;
    state.x1 = input;
    state.y2 = state.y1;
    state.y1 = output;

    output
}
"#
    .to_string()
}

/// Generate the process_biquad_bandpass function using the Audio EQ Cookbook BPF formula.
fn generate_process_biquad_bandpass() -> String {
    r#"/// Process a single sample through a bandpass biquad filter (constant skirt gain).
fn process_biquad_bandpass(state: &mut BiquadState, input: f32, cutoff: f32, resonance: f32, sample_rate: f32) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * (resonance + 0.001));

    let b0 = alpha;
    let b1 = 0.0_f32;
    let b2 = -alpha;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let output = b0 * input + b1 * state.x1 + b2 * state.x2 - a1 * state.y1 - a2 * state.y2;

    state.x2 = state.x1;
    state.x1 = input;
    state.y2 = state.y1;
    state.y1 = output;

    output
}
"#
    .to_string()
}

/// Generate the process_biquad_highpass function using the Audio EQ Cookbook HPF formula.
fn generate_process_biquad_highpass() -> String {
    r#"/// Process a single sample through a highpass biquad filter.
fn process_biquad_highpass(state: &mut BiquadState, input: f32, cutoff: f32, resonance: f32, sample_rate: f32) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * (resonance + 0.001));

    let b0 = (1.0 + cos_omega) / 2.0;
    let b1 = -(1.0 + cos_omega);
    let b2 = (1.0 + cos_omega) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let output = b0 * input + b1 * state.x1 + b2 * state.x2 - a1 * state.y1 - a2 * state.y2;

    state.x2 = state.x1;
    state.x1 = input;
    state.y2 = state.y1;
    state.y1 = output;

    output
}
"#
    .to_string()
}

/// Generate the process_biquad_notch function using the Audio EQ Cookbook notch (band-reject) formula.
fn generate_process_biquad_notch() -> String {
    r#"/// Process a single sample through a notch (band-reject) biquad filter.
fn process_biquad_notch(state: &mut BiquadState, input: f32, cutoff: f32, resonance: f32, sample_rate: f32) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * (resonance + 0.001));

    let b0 = 1.0_f32;
    let b1 = -2.0 * cos_omega;
    let b2 = 1.0_f32;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha;

    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let output = b0 * input + b1 * state.x1 + b2 * state.x2 - a1 * state.y1 - a2 * state.y2;

    state.x2 = state.x1;
    state.x1 = input;
    state.y2 = state.y1;
    state.y1 = output;

    output
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Oscillator helpers
// ══════════════════════════════════════════════════════════════

/// Generate the OscState struct shared by all oscillator types.
fn generate_osc_state() -> String {
    r#"/// Per-voice oscillator state (phase accumulator).
#[derive(Clone, Copy)]
struct OscState {
    phase: f32,
}

impl Default for OscState {
    fn default() -> Self {
        Self { phase: 0.0 }
    }
}
"#
    .to_string()
}

/// Generate the saw oscillator processing function.
fn generate_process_osc_saw() -> String {
    r#"/// Process one sample of a naive saw oscillator.
fn process_osc_saw(state: &mut OscState, frequency: f32, sample_rate: f32) -> f32 {
    let output = state.phase * 2.0 - 1.0;
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();
    output
}
"#
    .to_string()
}

/// Generate the square oscillator processing function.
fn generate_process_osc_square() -> String {
    r#"/// Process one sample of a naive square oscillator.
fn process_osc_square(state: &mut OscState, frequency: f32, sample_rate: f32) -> f32 {
    let output = if state.phase < 0.5 { 1.0_f32 } else { -1.0_f32 };
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();
    output
}
"#
    .to_string()
}

/// Generate the sine oscillator processing function.
fn generate_process_osc_sine() -> String {
    r#"/// Process one sample of a sine oscillator.
fn process_osc_sine(state: &mut OscState, frequency: f32, sample_rate: f32) -> f32 {
    let output = (state.phase * std::f32::consts::TAU).sin();
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();
    output
}
"#
    .to_string()
}

/// Generate the triangle oscillator processing function.
fn generate_process_osc_triangle() -> String {
    r#"/// Process one sample of a naive triangle oscillator.
fn process_osc_triangle(state: &mut OscState, frequency: f32, sample_rate: f32) -> f32 {
    let output = (2.0 * (state.phase - (state.phase + 0.5).floor()).abs()) * 2.0 - 1.0;
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();
    output
}
"#
    .to_string()
}

/// Generate the pulse oscillator processing function (variable duty cycle).
fn generate_process_osc_pulse() -> String {
    r#"/// Process one sample of a pulse oscillator with variable width.
///
/// `width` controls the duty cycle: 0.5 = square wave, 0.1 = narrow pulse.
fn process_osc_pulse(state: &mut OscState, frequency: f32, width: f32, sample_rate: f32) -> f32 {
    let output = if state.phase < width { 1.0_f32 } else { -1.0_f32 };
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();
    output
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// ADSR envelope helper
// ══════════════════════════════════════════════════════════════

/// Generate the AdsrState struct and AdsrStage enum.
fn generate_adsr_state() -> String {
    r#"/// ADSR envelope stage.
#[derive(Clone, Copy, PartialEq)]
enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-voice ADSR envelope state.
#[derive(Clone, Copy)]
struct AdsrState {
    stage: AdsrStage,
    level: f32,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            stage: AdsrStage::Idle,
            level: 0.0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_adsr function.
fn generate_process_adsr() -> String {
    r#"/// Process one sample of an ADSR envelope.
///
/// `gate` should be > 0.0 while a note is held and 0.0 when released.
/// Returns the envelope level (0.0..1.0).
fn process_adsr(
    state: &mut AdsrState,
    gate: f32,
    attack_ms: f32,
    decay_ms: f32,
    sustain_level: f32,
    release_ms: f32,
    sample_rate: f32,
) -> f32 {
    let attack_samples = (attack_ms * 0.001 * sample_rate).max(1.0);
    let decay_samples = (decay_ms * 0.001 * sample_rate).max(1.0);
    let release_samples = (release_ms * 0.001 * sample_rate).max(1.0);

    if gate > 0.0 {
        // Gate is on
        if state.stage == AdsrStage::Idle || state.stage == AdsrStage::Release {
            state.stage = AdsrStage::Attack;
        }
        match state.stage {
            AdsrStage::Attack => {
                state.level += 1.0 / attack_samples;
                if state.level >= 1.0 {
                    state.level = 1.0;
                    state.stage = AdsrStage::Decay;
                }
            }
            AdsrStage::Decay => {
                state.level -= (1.0 - sustain_level) / decay_samples;
                if state.level <= sustain_level {
                    state.level = sustain_level;
                    state.stage = AdsrStage::Sustain;
                }
            }
            AdsrStage::Sustain => {
                state.level = sustain_level;
            }
            _ => {}
        }
    } else {
        // Gate is off
        if state.stage != AdsrStage::Idle {
            state.stage = AdsrStage::Release;
            state.level -= state.level / release_samples;
            if state.level < 0.0001 {
                state.level = 0.0;
                state.stage = AdsrStage::Idle;
            }
        }
    }

    state.level
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Chorus helper
// ══════════════════════════════════════════════════════════════

/// Generate the ChorusState struct.
fn generate_chorus_state() -> String {
    r#"/// Per-call-site chorus effect state (delay line + internal LFO).
struct ChorusState {
    buffer: [f32; 1323], // ~30ms at 44100 Hz
    write_pos: usize,
    lfo_phase: f32,
}

impl Default for ChorusState {
    fn default() -> Self {
        Self {
            buffer: [0.0; 1323],
            write_pos: 0,
            lfo_phase: 0.0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_chorus function.
fn generate_process_chorus() -> String {
    r#"/// Process one sample through a chorus effect.
///
/// Uses a short delay line modulated by an internal LFO.
/// `rate` is LFO frequency in Hz, `depth` is modulation depth (0.0..1.0).
fn process_chorus(state: &mut ChorusState, input: f32, rate: f32, depth: f32, sample_rate: f32) -> f32 {
    let buf_len = state.buffer.len();

    // Write input to delay line
    state.buffer[state.write_pos] = input;
    state.write_pos = (state.write_pos + 1) % buf_len;

    // LFO modulates the read position
    let lfo = (state.lfo_phase * std::f32::consts::TAU).sin();
    state.lfo_phase += rate / sample_rate;
    state.lfo_phase -= state.lfo_phase.floor();

    // Delay time: center ~15ms, modulated by depth
    let center_delay = buf_len as f32 * 0.5;
    let mod_amount = center_delay * depth.clamp(0.0, 1.0);
    let delay_samples = center_delay + lfo * mod_amount;

    // Read from delay line with linear interpolation
    let read_pos = state.write_pos as f32 - delay_samples;
    let read_pos = if read_pos < 0.0 { read_pos + buf_len as f32 } else { read_pos };
    let idx0 = read_pos.floor() as usize % buf_len;
    let idx1 = (idx0 + 1) % buf_len;
    let frac = read_pos.fract();
    let delayed = state.buffer[idx0] * (1.0 - frac) + state.buffer[idx1] * frac;

    // Mix dry + wet (50/50)
    (input + delayed) * 0.5
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Compressor helper
// ══════════════════════════════════════════════════════════════

/// Generate the CompressorState struct.
fn generate_compressor_state() -> String {
    r#"/// Per-call-site compressor state (envelope follower).
struct CompressorState {
    envelope: f32,
}

impl Default for CompressorState {
    fn default() -> Self {
        Self { envelope: 0.0 }
    }
}
"#
    .to_string()
}

/// Generate the process_compressor function.
fn generate_process_compressor() -> String {
    r#"/// Process one sample through a dynamics compressor.
///
/// `threshold` is in linear gain (not dB). `ratio` is compression ratio (e.g. 4.0 = 4:1).
/// Uses a simple envelope follower with fixed attack/release.
fn process_compressor(state: &mut CompressorState, input: f32, threshold: f32, ratio: f32, sample_rate: f32) -> f32 {
    let attack_coeff = (-1.0 / (0.01 * sample_rate)).exp();  // ~10ms attack
    let release_coeff = (-1.0 / (0.1 * sample_rate)).exp();  // ~100ms release

    let abs_input = input.abs();
    if abs_input > state.envelope {
        state.envelope = attack_coeff * state.envelope + (1.0 - attack_coeff) * abs_input;
    } else {
        state.envelope = release_coeff * state.envelope + (1.0 - release_coeff) * abs_input;
    }

    // Compute gain reduction
    let threshold = threshold.max(0.0001); // prevent division by zero
    let ratio = ratio.max(1.0);
    if state.envelope > threshold {
        let over = state.envelope / threshold;
        let gain = (over.powf(1.0 / ratio - 1.0)).min(1.0);
        input * gain
    } else {
        input
    }
}
"#
    .to_string()
}