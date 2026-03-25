//! Generates inline DSP helper functions for the generated plugin crate.
//!
//! Generates state structs and processing functions for DSP primitives:
//! - **Filter (Lowpass)**: BiquadState struct, coefficient calculation, process_biquad()
//! - **Tanh**: uses f32's built-in .tanh(), no helper needed
//! - **Mix**: inline (a + b) * 0.5, no helper needed
//! - **Gain**: inline multiply, no helper needed

use std::collections::HashSet;

use crate::dsp::primitives::{DspPrimitive, FilterKind};

/// Generate DSP helper code for the set of primitives used in the plugin.
///
/// Returns Rust code for any state structs and processing functions needed.
pub fn generate_dsp_helpers(used_primitives: &HashSet<DspPrimitive>) -> String {
    let mut out = String::new();

    let needs_biquad = used_primitives.iter().any(|p| {
        matches!(p, DspPrimitive::Filter(FilterKind::Lowpass))
    });

    if needs_biquad {
        out.push_str(&generate_biquad_state());
        out.push('\n');
        out.push_str(&generate_process_biquad());
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
