//! Generates inline DSP helper functions for the generated plugin crate.
//!
//! Generates state structs and processing functions for DSP primitives:
//! - **Filter (Lowpass)**: BiquadState struct, coefficient calculation, process_biquad()
//! - **Tanh**: uses f32's built-in .tanh(), no helper needed
//! - **Mix**: inline (a + b) * 0.5, no helper needed
//! - **Gain**: inline multiply, no helper needed

use std::collections::HashSet;

use crate::dsp::primitives::{DspPrimitive, EnvKind, EqKind, FilterKind, OscKind};

/// Append generated code to `out` if `cond` is true.
fn emit_if(out: &mut String, cond: bool, gen: fn() -> String) {
    if cond { out.push_str(&gen()); out.push('\n'); }
}

/// Check if any primitive in the set matches a predicate.
fn has(prims: &HashSet<DspPrimitive>, f: impl Fn(&DspPrimitive) -> bool) -> bool {
    prims.iter().any(f)
}

/// Generate DSP helper code for the set of primitives used in the plugin.
///
/// Returns Rust code for any state structs and processing functions needed.
pub fn generate_dsp_helpers(used_primitives: &HashSet<DspPrimitive>) -> String {
    let mut out = String::new();

    // ── Biquad filters (shared state struct) ─────────────────
    let needs_any_biquad = has(used_primitives, |p| matches!(p, DspPrimitive::Filter(_)));
    let needs_any_eq = has(used_primitives, |p| matches!(p, DspPrimitive::EqFilter(_)));

    emit_if(&mut out, needs_any_biquad || needs_any_eq, generate_biquad_state);

    // Per-variant filter process functions
    let filter_fns: &[(FilterKind, fn() -> String)] = &[
        (FilterKind::Lowpass,  generate_process_biquad),
        (FilterKind::Bandpass, generate_process_biquad_bandpass),
        (FilterKind::Highpass, generate_process_biquad_highpass),
        (FilterKind::Notch,    generate_process_biquad_notch),
    ];
    for (kind, gen) in filter_fns {
        emit_if(&mut out, used_primitives.contains(&DspPrimitive::Filter(*kind)), *gen);
    }

    // Per-variant EQ process functions
    let eq_fns: &[(EqKind, fn() -> String)] = &[
        (EqKind::PeakEq,    generate_process_biquad_peak_eq),
        (EqKind::LowShelf,  generate_process_biquad_low_shelf),
        (EqKind::HighShelf, generate_process_biquad_high_shelf),
    ];
    for (kind, gen) in eq_fns {
        emit_if(&mut out, used_primitives.contains(&DspPrimitive::EqFilter(*kind)), *gen);
    }

    // ── Oscillators (shared state struct) ────────────────────
    let needs_any_osc = has(used_primitives, |p| matches!(p, DspPrimitive::Oscillator(_) | DspPrimitive::Lfo | DspPrimitive::Pulse));
    emit_if(&mut out, needs_any_osc, generate_osc_state);

    let osc_fns: &[(fn(&DspPrimitive) -> bool, fn() -> String)] = &[
        (|p| matches!(p, DspPrimitive::Oscillator(OscKind::Saw)),      generate_process_osc_saw),
        (|p| matches!(p, DspPrimitive::Oscillator(OscKind::Square)),   generate_process_osc_square),
        (|p| matches!(p, DspPrimitive::Oscillator(OscKind::Sine) | DspPrimitive::Lfo), generate_process_osc_sine),
        (|p| matches!(p, DspPrimitive::Oscillator(OscKind::Triangle)), generate_process_osc_triangle),
        (|p| matches!(p, DspPrimitive::Pulse),                         generate_process_osc_pulse),
    ];
    for (pred, gen) in osc_fns {
        emit_if(&mut out, has(used_primitives, pred), *gen);
    }

    // ── Stateful primitives (state struct + process fn) ──────
    type GenPair = (DspPrimitive, fn() -> String, fn() -> String);
    let stateful: &[GenPair] = &[
        (DspPrimitive::Envelope(EnvKind::Adsr), generate_adsr_state,        generate_process_adsr),
        (DspPrimitive::Chorus,                  generate_chorus_state,       generate_process_chorus),
        (DspPrimitive::Compressor,              generate_compressor_state,   generate_process_compressor),
        (DspPrimitive::Rms,                     generate_rms_state,          generate_process_rms),
        (DspPrimitive::PeakFollow,              generate_peak_follow_state,  generate_process_peak_follow),
        (DspPrimitive::Gate,                    generate_gate_state,         generate_process_gate),
        (DspPrimitive::DcBlock,                 generate_dc_block_state,     generate_process_dc_block),
        (DspPrimitive::SampleAndHold,           generate_sample_hold_state,  generate_process_sample_hold),
        (DspPrimitive::WavetableOsc,            generate_wt_osc_state,       generate_process_wavetable_osc),
        (DspPrimitive::Reverb,                  generate_reverb_state,       generate_process_reverb),
    ];
    for (prim, state_fn, process_fn) in stateful {
        if used_primitives.contains(prim) {
            out.push_str(&state_fn()); out.push('\n');
            out.push_str(&process_fn()); out.push('\n');
        }
    }

    // ── Delay variants (shared state struct) ─────────────────
    let needs_delay = has(used_primitives, |p| matches!(p, DspPrimitive::Delay | DspPrimitive::ModDelay | DspPrimitive::Allpass | DspPrimitive::Comb));
    emit_if(&mut out, needs_delay, generate_delay_state);

    let delay_fns: &[(DspPrimitive, fn() -> String)] = &[
        (DspPrimitive::Delay,    generate_process_delay),
        (DspPrimitive::ModDelay, generate_process_mod_delay),
        (DspPrimitive::Allpass,  generate_process_allpass),
        (DspPrimitive::Comb,     generate_process_comb),
    ];
    for (prim, gen) in delay_fns {
        emit_if(&mut out, used_primitives.contains(prim), *gen);
    }

    if used_primitives.contains(&DspPrimitive::Oversample) {
        out.push_str(&generate_oversample_state());
        out.push('\n');
    }

    out
}

/// Generate PatternState structs for each unique step count used.
///
/// Each pattern instance may have a different number of steps, so we generate
/// `PatternStateN` where N is the step count (e.g. PatternState8 for 8 steps).
pub fn generate_pattern_helpers(pattern_values: &[(usize, Vec<f64>)]) -> String {
    if pattern_values.is_empty() {
        return String::new();
    }

    let mut out = String::new();

    // Collect unique step counts to avoid duplicate struct definitions
    let mut seen_sizes: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (_idx, values) in pattern_values {
        let n = values.len();
        if seen_sizes.insert(n) {
            out.push_str(&format!(
                r#"/// Pattern step sequencer state with {} steps.
#[derive(Clone, Copy)]
struct PatternState{n} {{
    phase: f32,
    step_index: usize,
    values: [f32; {n}],
}}

"#,
                n,
                n = n,
            ));
        }
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
// Reverb helper
// ══════════════════════════════════════════════════════════════

/// Generate the ReverbState struct.
fn generate_reverb_state() -> String {
    r#"/// Maximum buffer size for reverb delay lines (~186ms at 192kHz).
const REVERB_MAX_BUF: usize = 8192;

/// Per-call-site Freeverb-style reverb state.
/// 8 parallel comb filters + 4 series allpass diffusers, all with fixed-size buffers.
#[derive(Clone, Copy)]
struct ReverbComb {
    buffer: [f32; REVERB_MAX_BUF],
    write_pos: usize,
    delay_len: usize,
    filter_state: f32,
}

impl Default for ReverbComb {
    fn default() -> Self {
        Self {
            buffer: [0.0; REVERB_MAX_BUF],
            write_pos: 0,
            delay_len: 1116,
            filter_state: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
struct ReverbAllpass {
    buffer: [f32; REVERB_MAX_BUF],
    write_pos: usize,
    delay_len: usize,
}

impl Default for ReverbAllpass {
    fn default() -> Self {
        Self {
            buffer: [0.0; REVERB_MAX_BUF],
            write_pos: 0,
            delay_len: 556,
        }
    }
}

#[derive(Clone, Copy)]
struct ReverbState {
    combs: [ReverbComb; 8],
    allpasses: [ReverbAllpass; 4],
}

impl Default for ReverbState {
    fn default() -> Self {
        Self {
            combs: [ReverbComb::default(); 8],
            allpasses: [ReverbAllpass::default(); 4],
        }
    }
}

/// Initialize reverb delay lengths scaled to the current sample rate.
fn init_reverb_state(state: &mut ReverbState, sample_rate: f32) {
    // Standard Freeverb tunings at 44100 Hz
    let comb_tunings: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    let allpass_tunings: [usize; 4] = [556, 441, 341, 225];
    let scale = sample_rate / 44100.0;

    for (i, &tuning) in comb_tunings.iter().enumerate() {
        let len = ((tuning as f32) * scale) as usize;
        state.combs[i].delay_len = len.min(REVERB_MAX_BUF).max(1);
        state.combs[i].write_pos = 0;
        state.combs[i].filter_state = 0.0;
        state.combs[i].buffer = [0.0; REVERB_MAX_BUF];
    }
    for (i, &tuning) in allpass_tunings.iter().enumerate() {
        let len = ((tuning as f32) * scale) as usize;
        state.allpasses[i].delay_len = len.min(REVERB_MAX_BUF).max(1);
        state.allpasses[i].write_pos = 0;
        state.allpasses[i].buffer = [0.0; REVERB_MAX_BUF];
    }
}
"#
    .to_string()
}

/// Generate the process_reverb function.
fn generate_process_reverb() -> String {
    r#"/// Process one sample through a Freeverb-style reverb.
///
/// `room_size` scales delay line lengths (0.0..1.0), `decay` is feedback amount (seconds mapped to 0..1),
/// `damping` applies per-comb lowpass (0.0..1.0), `mix` is dry/wet balance (0.0..1.0).
fn process_reverb(state: &mut ReverbState, input: f32, room_size: f32, decay: f32, damping: f32, mix: f32) -> f32 {
    let room_size = room_size.clamp(0.0, 1.0);
    let feedback = decay.clamp(0.0, 1.0) * 0.98 + 0.01; // map to safe feedback range
    let damping = damping.clamp(0.0, 1.0);
    let mix = mix.clamp(0.0, 1.0);

    // Scale effective delay lengths by room_size (0.5..1.0 range for stability)
    let size_scale = 0.5 + room_size * 0.5;

    // Sum output of 8 parallel comb filters
    let mut comb_out = 0.0_f32;
    for comb in state.combs.iter_mut() {
        let eff_len = ((comb.delay_len as f32) * size_scale) as usize;
        let eff_len = eff_len.min(REVERB_MAX_BUF).max(1);

        // Read from delay line
        let read_pos = if comb.write_pos >= eff_len {
            comb.write_pos - eff_len
        } else {
            REVERB_MAX_BUF + comb.write_pos - eff_len
        };
        let delayed = comb.buffer[read_pos % REVERB_MAX_BUF];

        // Apply damping (one-pole lowpass on feedback path)
        comb.filter_state = delayed * (1.0 - damping) + comb.filter_state * damping;

        // Write input + filtered feedback to delay line
        comb.buffer[comb.write_pos] = input + comb.filter_state * feedback;
        comb.write_pos = (comb.write_pos + 1) % REVERB_MAX_BUF;

        comb_out += delayed;
    }

    // Pass through 4 series allpass filters for diffusion
    let mut allpass_out = comb_out;
    let allpass_feedback = 0.5_f32;
    for ap in state.allpasses.iter_mut() {
        let eff_len = ((ap.delay_len as f32) * size_scale) as usize;
        let eff_len = eff_len.min(REVERB_MAX_BUF).max(1);

        let read_pos = if ap.write_pos >= eff_len {
            ap.write_pos - eff_len
        } else {
            REVERB_MAX_BUF + ap.write_pos - eff_len
        };
        let delayed = ap.buffer[read_pos % REVERB_MAX_BUF];

        let ap_input = allpass_out + delayed * allpass_feedback;
        ap.buffer[ap.write_pos] = ap_input;
        ap.write_pos = (ap.write_pos + 1) % REVERB_MAX_BUF;

        allpass_out = delayed - allpass_out * allpass_feedback;
    }

    // Dry/wet mix
    input * (1.0 - mix) + allpass_out * mix * 0.125 // 1/8 to normalize 8 comb outputs
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

/// Generate the OversampleState struct and its methods.
///
/// Uses a linear-phase FIR half-band filter for anti-aliasing.
/// The filter is applied at each 2x stage. For 4x oversampling,
/// two 2x stages are cascaded. The FIR coefficients are a 7-tap
/// half-band kernel: [0.03125, 0.0, 0.234375, 0.46875, 0.234375, 0.0, 0.03125].
pub fn generate_oversample_state() -> String {
    r#"/// Oversampling state for anti-aliased nonlinear processing.
///
/// Supports 2x, 4x, 8x, and 16x oversampling via cascaded half-band FIR stages.
/// Each stage uses a 7-tap linear-phase half-band filter for anti-aliasing.
/// All buffers are fixed-size (no heap allocation after init).
const OS_HALF_BAND_KERNEL: [f32; 7] = [0.03125, 0.0, 0.234375, 0.46875, 0.234375, 0.0, 0.03125];
const OS_KERNEL_LEN: usize = 7;
const OS_MAX_FACTOR: usize = 16;

struct OversampleState {
    factor: usize,
    /// Number of 2x stages (log2 of factor)
    num_stages: usize,
    /// Upsampling FIR delay lines — one per 2x stage
    up_buf: [[f32; OS_KERNEL_LEN]; 4],
    /// Downsampling FIR delay lines — one per 2x stage
    down_buf: [[f32; OS_KERNEL_LEN]; 4],
    /// Scratch buffer for upsampled samples
    scratch: [f32; OS_MAX_FACTOR],
}

impl OversampleState {
    fn new(factor: usize) -> Self {
        let num_stages = match factor {
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            _ => 1,
        };
        Self {
            factor,
            num_stages,
            up_buf: [[0.0; OS_KERNEL_LEN]; 4],
            down_buf: [[0.0; OS_KERNEL_LEN]; 4],
            scratch: [0.0; OS_MAX_FACTOR],
        }
    }

    /// Upsample a single input sample by the configured factor.
    /// Returns a fixed-size array; only the first `factor` elements are valid.
    fn upsample(&mut self, input: f32) -> [f32; OS_MAX_FACTOR] {
        let mut buf = [0.0f32; OS_MAX_FACTOR];
        // Start with the input for the first 2x stage
        let mut stage_len = 1usize;
        buf[0] = input;

        for stage in 0..self.num_stages {
            // Expand: insert zeros between existing samples (zero-stuffing)
            // Work backwards to avoid overwriting
            let new_len = stage_len * 2;
            for i in (0..stage_len).rev() {
                buf[i * 2] = buf[i] * 2.0; // compensate for zero-stuffing energy loss
                buf[i * 2 + 1] = 0.0;
            }
            // Apply half-band lowpass to remove imaging
            for i in 0..new_len {
                // Shift delay line
                let dl = &mut self.up_buf[stage];
                for j in (1..OS_KERNEL_LEN).rev() {
                    dl[j] = dl[j - 1];
                }
                dl[0] = buf[i];
                // Convolve
                let mut sum = 0.0f32;
                for j in 0..OS_KERNEL_LEN {
                    sum += dl[j] * OS_HALF_BAND_KERNEL[j];
                }
                buf[i] = sum;
            }
            stage_len = new_len;
        }

        buf
    }

    /// Accumulate a downsampled result. Call once per oversampled sample.
    /// Returns the final decimated output on the last call (os_idx == factor - 1).
    fn downsample_accumulate(&mut self, sample: f32, os_idx: usize) -> f32 {
        self.scratch[os_idx] = sample;

        if os_idx < self.factor - 1 {
            return 0.0;
        }

        // Decimate through each 2x stage in reverse
        let mut stage_len = self.factor;
        let mut buf = self.scratch;

        for stage in (0..self.num_stages).rev() {
            // Apply half-band lowpass before decimating
            for i in 0..stage_len {
                let dl = &mut self.down_buf[stage];
                for j in (1..OS_KERNEL_LEN).rev() {
                    dl[j] = dl[j - 1];
                }
                dl[0] = buf[i];
                let mut sum = 0.0f32;
                for j in 0..OS_KERNEL_LEN {
                    sum += dl[j] * OS_HALF_BAND_KERNEL[j];
                }
                buf[i] = sum;
            }
            // Decimate: keep every other sample
            let new_len = stage_len / 2;
            for i in 0..new_len {
                buf[i] = buf[i * 2];
            }
            stage_len = new_len;
        }

        buf[0]
    }
}
"#
    .to_string()
}