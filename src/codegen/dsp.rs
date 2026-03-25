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
    let needs_any_biquad = needs_lowpass || needs_bandpass || needs_highpass;

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

    // ── Oscillators ──────────────────────────────────────────
    let needs_any_osc = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Oscillator(_))
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
        matches!(p, DspPrimitive::Oscillator(OscKind::Sine))
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