//! Generates inline DSP helper functions for the generated plugin crate.
//!
//! Generates state structs and processing functions for DSP primitives:
//! - **Filter (Lowpass)**: BiquadState struct, coefficient calculation, process_biquad()
//! - **Tanh**: uses f32's built-in .tanh(), no helper needed
//! - **Mix**: inline (a + b) * 0.5, no helper needed
//! - **Gain**: inline multiply, no helper needed

use std::collections::HashSet;

use crate::dsp::primitives::{DspPrimitive, EnvKind, EqKind, FilterKind, OscKind};

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

    // EQ/shelving filters also use BiquadState
    let needs_peak_eq = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::EqFilter(EqKind::PeakEq))
    });
    let needs_low_shelf = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::EqFilter(EqKind::LowShelf))
    });
    let needs_high_shelf = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::EqFilter(EqKind::HighShelf))
    });
    let needs_any_eq_biquad = needs_peak_eq || needs_low_shelf || needs_high_shelf;

    if needs_any_biquad || needs_any_eq_biquad {
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

    // ── EQ / Shelving biquad filters ─────────────────────────
    if needs_peak_eq {
        out.push_str(&generate_process_biquad_peak_eq());
        out.push('\n');
    }

    if needs_low_shelf {
        out.push_str(&generate_process_biquad_low_shelf());
        out.push('\n');
    }

    if needs_high_shelf {
        out.push_str(&generate_process_biquad_high_shelf());
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

    // ── RMS ──────────────────────────────────────────────────
    let needs_rms = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Rms)
    });

    if needs_rms {
        out.push_str(&generate_rms_state());
        out.push('\n');
        out.push_str(&generate_process_rms());
        out.push('\n');
    }

    // ── Peak Follow ──────────────────────────────────────────
    let needs_peak_follow = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::PeakFollow)
    });

    if needs_peak_follow {
        out.push_str(&generate_peak_follow_state());
        out.push('\n');
        out.push_str(&generate_process_peak_follow());
        out.push('\n');
    }

    // ── Gate ─────────────────────────────────────────────────
    let needs_gate = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Gate)
    });

    if needs_gate {
        out.push_str(&generate_gate_state());
        out.push('\n');
        out.push_str(&generate_process_gate());
        out.push('\n');
    }

    // ── DC Block ─────────────────────────────────────────────
    let needs_dc_block = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::DcBlock)
    });

    if needs_dc_block {
        out.push_str(&generate_dc_block_state());
        out.push('\n');
        out.push_str(&generate_process_dc_block());
        out.push('\n');
    }

    // ── Sample and Hold ──────────────────────────────────────
    let needs_sample_hold = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::SampleAndHold)
    });

    if needs_sample_hold {
        out.push_str(&generate_sample_hold_state());
        out.push('\n');
        out.push_str(&generate_process_sample_hold());
        out.push('\n');
    }

    // ── Wavetable oscillator ─────────────────────────────────
    let needs_wavetable_osc = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::WavetableOsc)
    });

    if needs_wavetable_osc {
        out.push_str(&generate_wt_osc_state());
        out.push('\n');
        out.push_str(&generate_process_wavetable_osc());
        out.push('\n');
    }

    // ── Delay ────────────────────────────────────────────────
    let needs_delay = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Delay | DspPrimitive::ModDelay | DspPrimitive::Allpass | DspPrimitive::Comb)
    });

    if needs_delay {
        out.push_str(&generate_delay_state());
        out.push('\n');
    }

    if used_primitives.contains(&DspPrimitive::Delay) {
        out.push_str(&generate_process_delay());
        out.push('\n');
    }

    if used_primitives.contains(&DspPrimitive::ModDelay) {
        out.push_str(&generate_process_mod_delay());
        out.push('\n');
    }

    if used_primitives.contains(&DspPrimitive::Allpass) {
        out.push_str(&generate_process_allpass());
        out.push('\n');
    }

    if used_primitives.contains(&DspPrimitive::Comb) {
        out.push_str(&generate_process_comb());
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
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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

// ══════════════════════════════════════════════════════════════
// Delay helper
// ══════════════════════════════════════════════════════════════

/// Maximum delay time in seconds. Buffer is allocated at this size × sample_rate.
pub const MAX_DELAY_SECONDS: f32 = 5.0;

/// Generate the DelayLine state struct.
pub fn generate_delay_state() -> String {
    r#"/// Per-call-site delay line with heap-allocated ring buffer.
///
/// Buffer is allocated in initialize() at MAX_DELAY_SECONDS × sample_rate.
/// Not Copy/Clone — contains a Vec.
struct DelayLine {
    buffer: Vec<f32>,
    write_pos: usize,
    lfo_phase: f32,
}

impl Default for DelayLine {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            write_pos: 0,
            lfo_phase: 0.0,
        }
    }
}

impl DelayLine {
    fn allocate(&mut self, sample_rate: f32) {
        let max_samples = (5.0_f32 * sample_rate) as usize;
        self.buffer.resize(max_samples, 0.0);
        self.write_pos = 0;
        self.lfo_phase = 0.0;
    }
}
"#
    .to_string()
}

/// Generate the process_delay function (simple delay with linear interpolation).
pub fn generate_process_delay() -> String {
    r#"/// Process one sample through a delay line.
///
/// `delay_time` is in seconds. Reads from the ring buffer at the appropriate
/// offset behind the write head, using linear interpolation for sub-sample accuracy.
/// Returns only the delayed (wet) signal — caller mixes dry/wet as needed.
fn process_delay(state: &mut DelayLine, input: f32, delay_time: f32, sample_rate: f32) -> f32 {
    if state.buffer.is_empty() {
        return input;
    }
    let buf_len = state.buffer.len();

    // Write input
    state.buffer[state.write_pos] = input;
    state.write_pos = (state.write_pos + 1) % buf_len;

    // Compute read position
    let delay_samples = (delay_time * sample_rate).clamp(0.0, (buf_len - 1) as f32);
    let read_pos = state.write_pos as f32 - delay_samples - 1.0;
    let read_pos = if read_pos < 0.0 { read_pos + buf_len as f32 } else { read_pos };

    // Linear interpolation
    let idx0 = read_pos.floor() as usize % buf_len;
    let idx1 = (idx0 + 1) % buf_len;
    let frac = read_pos.fract();
    state.buffer[idx0] * (1.0 - frac) + state.buffer[idx1] * frac
}
"#
    .to_string()
}

/// Generate the process_mod_delay function (delay with LFO-modulated read position).
pub fn generate_process_mod_delay() -> String {
    r#"/// Process one sample through a modulated delay line.
///
/// Like delay() but with an internal LFO modulating the read position.
/// `delay_time` is center delay in seconds, `depth` controls modulation amount (0..1),
/// `rate` is LFO frequency in Hz. Uses linear interpolation for fractional reads.
fn process_mod_delay(state: &mut DelayLine, input: f32, delay_time: f32, depth: f32, rate: f32, sample_rate: f32) -> f32 {
    if state.buffer.is_empty() {
        return input;
    }
    let buf_len = state.buffer.len();

    // Write input
    state.buffer[state.write_pos] = input;
    state.write_pos = (state.write_pos + 1) % buf_len;

    // Internal LFO for read-position modulation
    let lfo = (state.lfo_phase * std::f32::consts::TAU).sin();
    state.lfo_phase += rate / sample_rate;
    state.lfo_phase -= state.lfo_phase.floor();

    // Modulate delay time
    let center_samples = (delay_time * sample_rate).clamp(1.0, (buf_len - 1) as f32);
    let mod_amount = center_samples * depth.clamp(0.0, 1.0);
    let delay_samples = (center_samples + lfo * mod_amount).clamp(1.0, (buf_len - 1) as f32);

    let read_pos = state.write_pos as f32 - delay_samples - 1.0;
    let read_pos = if read_pos < 0.0 { read_pos + buf_len as f32 } else { read_pos };

    // Linear interpolation
    let idx0 = read_pos.floor() as usize % buf_len;
    let idx1 = (idx0 + 1) % buf_len;
    let frac = read_pos.fract();
    state.buffer[idx0] * (1.0 - frac) + state.buffer[idx1] * frac
}
"#
    .to_string()
}

/// Generate the process_allpass function (Schroeder allpass filter).
pub fn generate_process_allpass() -> String {
    r#"/// Process one sample through a Schroeder allpass filter.
///
/// output = -input*g + delayed + g*feedback. Building block for phasers and reverbs.
/// `delay_time` is in seconds, `feedback` is the allpass coefficient (typically 0.0..0.9).
fn process_allpass(state: &mut DelayLine, input: f32, delay_time: f32, feedback: f32, sample_rate: f32) -> f32 {
    if state.buffer.is_empty() {
        return input;
    }
    let buf_len = state.buffer.len();

    // Read delayed sample
    let delay_samples = (delay_time * sample_rate).clamp(0.0, (buf_len - 1) as f32);
    let read_pos = state.write_pos as f32 - delay_samples;
    let read_pos = if read_pos < 0.0 { read_pos + buf_len as f32 } else { read_pos };
    let idx0 = read_pos.floor() as usize % buf_len;
    let idx1 = (idx0 + 1) % buf_len;
    let frac = read_pos.fract();
    let delayed = state.buffer[idx0] * (1.0 - frac) + state.buffer[idx1] * frac;

    // Schroeder allpass: output = -g*input + delayed + g*(delayed fed back)
    let g = feedback.clamp(-0.99, 0.99);
    let output = -g * input + delayed;
    let write_val = input + g * delayed;

    // Write to buffer
    state.buffer[state.write_pos] = write_val;
    state.write_pos = (state.write_pos + 1) % buf_len;

    output
}
"#
    .to_string()
}

/// Generate the process_comb function (feedback comb filter).
pub fn generate_process_comb() -> String {
    r#"/// Process one sample through a feedback comb filter.
///
/// output = input + delayed * feedback. The output feeds back into the buffer.
/// Building block for Karplus-Strong and reverb.
/// `delay_time` is in seconds, `feedback` controls decay (typically 0.0..0.99).
fn process_comb(state: &mut DelayLine, input: f32, delay_time: f32, feedback: f32, sample_rate: f32) -> f32 {
    if state.buffer.is_empty() {
        return input;
    }
    let buf_len = state.buffer.len();

    // Read delayed sample
    let delay_samples = (delay_time * sample_rate).clamp(0.0, (buf_len - 1) as f32);
    let read_pos = state.write_pos as f32 - delay_samples;
    let read_pos = if read_pos < 0.0 { read_pos + buf_len as f32 } else { read_pos };
    let idx0 = read_pos.floor() as usize % buf_len;
    let idx1 = (idx0 + 1) % buf_len;
    let frac = read_pos.fract();
    let delayed = state.buffer[idx0] * (1.0 - frac) + state.buffer[idx1] * frac;

    // Comb filter: output = input + delayed * feedback
    let output = input + delayed * feedback.clamp(-0.99, 0.99);

    // Write output back (feedback path)
    state.buffer[state.write_pos] = output;
    state.write_pos = (state.write_pos + 1) % buf_len;

    output
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// EQ / Shelving biquad helpers (Audio EQ Cookbook)
// ══════════════════════════════════════════════════════════════

/// Generate the process_biquad_peak_eq function using the Audio EQ Cookbook peakingEQ formula.
fn generate_process_biquad_peak_eq() -> String {
    r#"/// Process a single sample through a peaking EQ biquad filter.
///
/// `freq` is center frequency in Hz, `gain_db` is boost/cut in dB, `q` controls bandwidth.
/// Audio EQ Cookbook: peakingEQ (H(s) = (s^2 + s*(A/Q) + 1) / (s^2 + s/(A*Q) + 1))
fn process_biquad_peak_eq(state: &mut BiquadState, input: f32, freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> f32 {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * q.max(0.001));

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_omega;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_omega;
    let a2 = 1.0 - alpha / a;

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

/// Generate the process_biquad_low_shelf function using the Audio EQ Cookbook lowShelf formula.
fn generate_process_biquad_low_shelf() -> String {
    r#"/// Process a single sample through a low shelf biquad filter.
///
/// `freq` is shelf frequency in Hz, `gain_db` is boost/cut in dB, `q` controls slope.
/// Audio EQ Cookbook: lowShelf
fn process_biquad_low_shelf(state: &mut BiquadState, input: f32, freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> f32 {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * q.max(0.001));
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_omega + two_sqrt_a_alpha);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_omega);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_omega - two_sqrt_a_alpha);
    let a0 = (a + 1.0) + (a - 1.0) * cos_omega + two_sqrt_a_alpha;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_omega);
    let a2 = (a + 1.0) + (a - 1.0) * cos_omega - two_sqrt_a_alpha;

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

/// Generate the process_biquad_high_shelf function using the Audio EQ Cookbook highShelf formula.
fn generate_process_biquad_high_shelf() -> String {
    r#"/// Process a single sample through a high shelf biquad filter.
///
/// `freq` is shelf frequency in Hz, `gain_db` is boost/cut in dB, `q` controls slope.
/// Audio EQ Cookbook: highShelf
fn process_biquad_high_shelf(state: &mut BiquadState, input: f32, freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> f32 {
    let a = 10.0_f32.powf(gain_db / 40.0);
    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_omega = omega.sin();
    let cos_omega = omega.cos();
    let alpha = sin_omega / (2.0 * q.max(0.001));
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_omega + two_sqrt_a_alpha);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_omega - two_sqrt_a_alpha);
    let a0 = (a + 1.0) - (a - 1.0) * cos_omega + two_sqrt_a_alpha;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_omega);
    let a2 = (a + 1.0) - (a - 1.0) * cos_omega - two_sqrt_a_alpha;

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
// RMS level detector helper
// ══════════════════════════════════════════════════════════════

/// Generate the RmsState struct.
fn generate_rms_state() -> String {
    r#"/// Per-call-site RMS level detector state (sliding window).
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
"#
    .to_string()
}

/// Generate the process_rms function.
fn generate_process_rms() -> String {
    r#"/// Process one sample through a sliding-window RMS level detector.
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
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Peak follow (envelope follower) helper
// ══════════════════════════════════════════════════════════════

/// Generate the PeakFollowState struct.
fn generate_peak_follow_state() -> String {
    r#"/// Per-call-site envelope follower state.
#[derive(Clone, Copy)]
struct PeakFollowState {
    envelope: f32,
}

impl Default for PeakFollowState {
    fn default() -> Self {
        Self { envelope: 0.0 }
    }
}
"#
    .to_string()
}

/// Generate the process_peak_follow function.
fn generate_process_peak_follow() -> String {
    r#"/// Process one sample through an envelope follower.
///
/// `attack_ms` and `release_ms` control the envelope timing.
/// Returns the current envelope level.
fn process_peak_follow(state: &mut PeakFollowState, input: f32, attack_ms: f32, release_ms: f32, sample_rate: f32) -> f32 {
    let attack_coeff = (-1.0_f32 / (attack_ms * 0.001 * sample_rate)).exp();
    let release_coeff = (-1.0_f32 / (release_ms * 0.001 * sample_rate)).exp();

    let abs_input = input.abs();
    if abs_input > state.envelope {
        state.envelope = attack_coeff * state.envelope + (1.0 - attack_coeff) * abs_input;
    } else {
        state.envelope = release_coeff * state.envelope + (1.0 - release_coeff) * abs_input;
    }

    state.envelope
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Gate (noise gate) helper
// ══════════════════════════════════════════════════════════════

/// Generate the GateState struct.
fn generate_gate_state() -> String {
    r#"/// Per-call-site noise gate state (envelope + gain + hold counter).
#[derive(Clone, Copy)]
struct GateState {
    envelope: f32,
    gain: f32,
    hold_counter: u32,
}

impl Default for GateState {
    fn default() -> Self {
        Self {
            envelope: 0.0,
            gain: 1.0,
            hold_counter: 0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_gate function.
fn generate_process_gate() -> String {
    r#"/// Process one sample through a noise gate.
///
/// `threshold_db` is the gate threshold in dB. `attack_ms` and `release_ms` control
/// the envelope follower. `hold_ms` keeps the gate open after signal drops below threshold.
fn process_gate(state: &mut GateState, input: f32, threshold_db: f32, attack_ms: f32, release_ms: f32, hold_ms: f32, sample_rate: f32) -> f32 {
    let attack_coeff = (-1.0_f32 / (attack_ms * 0.001 * sample_rate)).exp();
    let release_coeff = (-1.0_f32 / (release_ms * 0.001 * sample_rate)).exp();
    let threshold = 10.0_f32.powf(threshold_db / 20.0);
    let hold_samples = (hold_ms * 0.001 * sample_rate) as u32;

    // Envelope follower
    let abs_input = input.abs();
    if abs_input > state.envelope {
        state.envelope = attack_coeff * state.envelope + (1.0 - attack_coeff) * abs_input;
    } else {
        state.envelope = release_coeff * state.envelope + (1.0 - release_coeff) * abs_input;
    }

    // Gate logic
    if state.envelope > threshold {
        state.hold_counter = hold_samples;
        state.gain = 1.0;
    } else if state.hold_counter > 0 {
        state.hold_counter -= 1;
    } else {
        state.gain *= release_coeff;
    }

    input * state.gain
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// DC Block helper
// ══════════════════════════════════════════════════════════════

/// Generate the DcBlockState struct.
fn generate_dc_block_state() -> String {
    r#"/// Per-call-site DC blocking filter state.
#[derive(Clone, Copy)]
struct DcBlockState {
    prev_x: f32,
    prev_y: f32,
}

impl Default for DcBlockState {
    fn default() -> Self {
        Self {
            prev_x: 0.0,
            prev_y: 0.0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_dc_block function.
fn generate_process_dc_block() -> String {
    r#"/// Process one sample through a DC blocking filter.
///
/// Removes DC offset using a first-order highpass: y[n] = x[n] - x[n-1] + 0.995 * y[n-1]
fn process_dc_block(state: &mut DcBlockState, input: f32) -> f32 {
    let y = input - state.prev_x + 0.995 * state.prev_y;
    state.prev_x = input;
    state.prev_y = y;
    y
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Sample and Hold helper
// ══════════════════════════════════════════════════════════════

/// Generate the SampleAndHoldState struct.
fn generate_sample_hold_state() -> String {
    r#"/// Per-call-site sample-and-hold state.
#[derive(Clone, Copy)]
struct SampleAndHoldState {
    held_value: f32,
    prev_trigger: f32,
}

impl Default for SampleAndHoldState {
    fn default() -> Self {
        Self {
            held_value: 0.0,
            prev_trigger: 0.0,
        }
    }
}
"#
    .to_string()
}

/// Generate the process_sample_and_hold function.
fn generate_process_sample_hold() -> String {
    r#"/// Process one sample through a sample-and-hold.
///
/// On a rising edge (trigger crosses above 0.5), captures the input value.
/// Always outputs the most recently held value.
fn process_sample_and_hold(state: &mut SampleAndHoldState, input: f32, trigger: f32) -> f32 {
    if trigger > 0.5 && state.prev_trigger <= 0.5 {
        state.held_value = input;
    }
    state.prev_trigger = trigger;
    state.held_value
}
"#
    .to_string()
}

// ══════════════════════════════════════════════════════════════
// Wavetable oscillator helper
// ══════════════════════════════════════════════════════════════

/// Generate the WtOscState struct.
fn generate_wt_osc_state() -> String {
    r#"/// Per-call-site wavetable oscillator state (phase accumulator).
#[derive(Clone, Copy)]
struct WtOscState {
    phase: f32,
}

impl Default for WtOscState {
    fn default() -> Self {
        Self { phase: 0.0 }
    }
}
"#
    .to_string()
}

/// Generate the process_wavetable_osc function with dual-axis linear interpolation.
fn generate_process_wavetable_osc() -> String {
    r#"/// Process one sample of a wavetable oscillator with frame morphing.
///
/// `data` is the full wavetable (all frames concatenated), `frame_size` is samples per frame,
/// `frame_count` is the number of frames. `position` (0.0..1.0) morphs between frames.
/// Uses dual-axis linear interpolation: within-frame (phase) and between-frame (position).
fn process_wavetable_osc(
    state: &mut WtOscState,
    data: &[f32],
    frame_size: usize,
    frame_count: usize,
    frequency: f32,
    position: f32,
    sample_rate: f32,
) -> f32 {
    if data.is_empty() || frame_size == 0 || frame_count == 0 {
        return 0.0_f32;
    }

    // Clamp position to valid range
    let position = position.clamp(0.0, 1.0);

    // Compute frame indices for interpolation
    let frame_pos = position * (frame_count as f32 - 1.0);
    let frame_idx0 = (frame_pos.floor() as usize).min(frame_count - 1);
    let frame_idx1 = (frame_idx0 + 1).min(frame_count - 1);
    let frame_frac = frame_pos.fract();

    // Compute sample position within frame using phase
    let sample_pos = state.phase * frame_size as f32;
    let idx0 = sample_pos.floor() as usize % frame_size;
    let idx1 = (idx0 + 1) % frame_size;
    let sample_frac = sample_pos.fract();

    // Read from frame 0
    let offset0 = frame_idx0 * frame_size;
    let s0a = data[offset0 + idx0];
    let s0b = data[offset0 + idx1];
    let val0 = s0a + (s0b - s0a) * sample_frac;

    // Read from frame 1
    let offset1 = frame_idx1 * frame_size;
    let s1a = data[offset1 + idx0];
    let s1b = data[offset1 + idx1];
    let val1 = s1a + (s1b - s1a) * sample_frac;

    // Interpolate between frames
    let output = val0 + (val1 - val0) * frame_frac;

    // Advance phase
    state.phase += frequency / sample_rate;
    state.phase -= state.phase.floor();

    output
}
"#
    .to_string()
}