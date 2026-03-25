//! Tests for DSP domain types, function registry, and type system.

use muse_lang::ast::UnitSuffix;
use muse_lang::dsp::*;
use muse_lang::types::{type_from_unit_suffix, DspType};

// ── Registry completeness ────────────────────────────────────

#[test]
fn registry_contains_all_24_functions() {
    let reg = builtin_registry();
    let expected = [
        "sine", "saw", "square", "triangle", "noise", "lowpass", "highpass", "bandpass", "notch",
        "adsr", "ar", "gain", "pan", "delay", "mix", "clip", "tanh",
        "fold", "bitcrush", "lfo", "pulse", "chorus", "compressor", "semitones_to_ratio",
    ];
    assert_eq!(reg.functions.len(), expected.len(), "registry size mismatch");
    for name in &expected {
        assert!(
            reg.lookup(name).is_some(),
            "missing function: {name}"
        );
    }
}

#[test]
fn unknown_function_returns_none() {
    let reg = builtin_registry();
    assert!(reg.lookup("reverb").is_none());
    assert!(reg.lookup("").is_none());
}

// ── Oscillators ──────────────────────────────────────────────

#[test]
fn sine_signature() {
    let f = lookup("sine");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "freq", DspType::Frequency, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Oscillator(OscKind::Sine));
}

#[test]
fn saw_signature() {
    let f = lookup("saw");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "freq", DspType::Frequency, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Oscillator(OscKind::Saw));
}

#[test]
fn square_signature() {
    let f = lookup("square");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "freq", DspType::Frequency, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Oscillator(OscKind::Square));
}

#[test]
fn triangle_signature() {
    let f = lookup("triangle");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "freq", DspType::Frequency, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Oscillator(OscKind::Triangle));
}

#[test]
fn noise_signature() {
    let f = lookup("noise");
    assert_eq!(f.params.len(), 0);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Noise);
}

// ── Filters ──────────────────────────────────────────────────

#[test]
fn lowpass_signature() {
    let f = lookup("lowpass");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "cutoff", DspType::Frequency, false);
    assert_param(&f.params[1], "resonance", DspType::Number, true);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Filter(FilterKind::Lowpass));
}

#[test]
fn highpass_signature() {
    let f = lookup("highpass");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "cutoff", DspType::Frequency, false);
    assert_param(&f.params[1], "resonance", DspType::Number, true);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Filter(FilterKind::Highpass));
}

#[test]
fn bandpass_signature() {
    let f = lookup("bandpass");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "cutoff", DspType::Frequency, false);
    assert_param(&f.params[1], "resonance", DspType::Number, true);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Filter(FilterKind::Bandpass));
}

#[test]
fn notch_signature() {
    let f = lookup("notch");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "cutoff", DspType::Frequency, false);
    assert_param(&f.params[1], "resonance", DspType::Number, true);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Filter(FilterKind::Notch));
}

// ── Envelopes ────────────────────────────────────────────────

#[test]
fn adsr_signature() {
    let f = lookup("adsr");
    assert_eq!(f.params.len(), 4);
    assert_param(&f.params[0], "attack", DspType::Time, false);
    assert_param(&f.params[1], "decay", DspType::Time, false);
    assert_param(&f.params[2], "sustain", DspType::Number, false);
    assert_param(&f.params[3], "release", DspType::Time, false);
    assert_eq!(f.return_type, DspType::Envelope);
    assert_eq!(f.primitive, DspPrimitive::Envelope(EnvKind::Adsr));
}

#[test]
fn ar_signature() {
    let f = lookup("ar");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "attack", DspType::Time, false);
    assert_param(&f.params[1], "release", DspType::Time, false);
    assert_eq!(f.return_type, DspType::Envelope);
    assert_eq!(f.primitive, DspPrimitive::Envelope(EnvKind::Ar));
}

// ── Utilities ────────────────────────────────────────────────

#[test]
fn gain_signature() {
    let f = lookup("gain");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "amount", DspType::Gain, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Gain);
}

#[test]
fn pan_signature() {
    let f = lookup("pan");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "position", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Pan);
}

#[test]
fn delay_signature() {
    let f = lookup("delay");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "time", DspType::Time, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Delay);
}

#[test]
fn mix_signature() {
    let f = lookup("mix");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "dry", DspType::Signal, false);
    assert_param(&f.params[1], "wet", DspType::Signal, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Mix);
}

#[test]
fn clip_signature() {
    let f = lookup("clip");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "min", DspType::Number, false);
    assert_param(&f.params[1], "max", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Clip);
}

#[test]
fn tanh_signature() {
    let f = lookup("tanh");
    assert_eq!(f.params.len(), 0);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Tanh);
}

// ── New primitives ───────────────────────────────────────────

#[test]
fn fold_signature() {
    let f = lookup("fold");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "amount", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Fold);
}

#[test]
fn bitcrush_signature() {
    let f = lookup("bitcrush");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "bits", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Bitcrush);
}

#[test]
fn lfo_signature() {
    let f = lookup("lfo");
    assert_eq!(f.params.len(), 1);
    assert_param(&f.params[0], "rate", DspType::Rate, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Lfo);
}

#[test]
fn pulse_signature() {
    let f = lookup("pulse");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "freq", DspType::Frequency, false);
    assert_param(&f.params[1], "width", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Signal);
    assert_eq!(f.primitive, DspPrimitive::Pulse);
}

#[test]
fn chorus_signature() {
    let f = lookup("chorus");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "rate", DspType::Rate, false);
    assert_param(&f.params[1], "depth", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Chorus);
}

#[test]
fn compressor_signature() {
    let f = lookup("compressor");
    assert_eq!(f.params.len(), 2);
    assert_param(&f.params[0], "threshold", DspType::Gain, false);
    assert_param(&f.params[1], "ratio", DspType::Number, false);
    assert_eq!(f.return_type, DspType::Processor);
    assert_eq!(f.primitive, DspPrimitive::Compressor);
}

// ── Type compatibility ───────────────────────────────────────

#[test]
fn exact_match_is_compatible() {
    assert!(DspType::Signal.is_compatible_with(DspType::Signal));
    assert!(DspType::Frequency.is_compatible_with(DspType::Frequency));
    assert!(DspType::Number.is_compatible_with(DspType::Number));
}

#[test]
fn number_compatible_with_numeric_domain() {
    assert!(DspType::Number.is_compatible_with(DspType::Frequency));
    assert!(DspType::Number.is_compatible_with(DspType::Gain));
    assert!(DspType::Number.is_compatible_with(DspType::Time));
    assert!(DspType::Number.is_compatible_with(DspType::Rate));
}

#[test]
fn numeric_domain_types_not_interchangeable() {
    assert!(!DspType::Frequency.is_compatible_with(DspType::Time));
    assert!(!DspType::Time.is_compatible_with(DspType::Gain));
    assert!(!DspType::Gain.is_compatible_with(DspType::Rate));
    assert!(!DspType::Rate.is_compatible_with(DspType::Frequency));
}

#[test]
fn non_numeric_types_not_compatible_across() {
    assert!(!DspType::Signal.is_compatible_with(DspType::Processor));
    assert!(!DspType::Processor.is_compatible_with(DspType::Signal));
    assert!(!DspType::Envelope.is_compatible_with(DspType::Signal));
    assert!(!DspType::Bool.is_compatible_with(DspType::Number));
    assert!(!DspType::Param.is_compatible_with(DspType::Number));
}

#[test]
fn is_numeric_domain() {
    assert!(DspType::Frequency.is_numeric_domain());
    assert!(DspType::Gain.is_numeric_domain());
    assert!(DspType::Time.is_numeric_domain());
    assert!(DspType::Rate.is_numeric_domain());
    assert!(DspType::Number.is_numeric_domain());

    assert!(!DspType::Signal.is_numeric_domain());
    assert!(!DspType::Processor.is_numeric_domain());
    assert!(!DspType::Envelope.is_numeric_domain());
    assert!(!DspType::Param.is_numeric_domain());
    assert!(!DspType::Bool.is_numeric_domain());
}

// ── Unit suffix → type mapping ───────────────────────────────

#[test]
fn unit_suffix_hz_maps_to_frequency() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::Hz), DspType::Frequency);
}

#[test]
fn unit_suffix_khz_maps_to_frequency() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::KHz), DspType::Frequency);
}

#[test]
fn unit_suffix_ms_maps_to_time() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::Ms), DspType::Time);
}

#[test]
fn unit_suffix_s_maps_to_time() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::S), DspType::Time);
}

#[test]
fn unit_suffix_db_maps_to_gain() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::DB), DspType::Gain);
}

#[test]
fn unit_suffix_percent_maps_to_number() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::Percent), DspType::Number);
}

#[test]
fn unit_suffix_st_maps_to_number() {
    assert_eq!(type_from_unit_suffix(UnitSuffix::St), DspType::Number);
}

// ── Display ──────────────────────────────────────────────────

#[test]
fn dsp_type_display() {
    assert_eq!(DspType::Signal.to_string(), "Signal");
    assert_eq!(DspType::Processor.to_string(), "Processor");
    assert_eq!(DspType::Envelope.to_string(), "Envelope");
    assert_eq!(DspType::Frequency.to_string(), "Frequency");
    assert_eq!(DspType::Number.to_string(), "Number");
}

// ── Helpers ──────────────────────────────────────────────────

fn lookup(name: &str) -> DspFunction {
    let reg = builtin_registry();
    reg.lookup(name)
        .unwrap_or_else(|| panic!("function '{name}' not found in registry"))
        .clone()
}

fn assert_param(p: &DspParam, name: &str, dsp_type: DspType, optional: bool) {
    assert_eq!(p.name, name, "param name mismatch");
    assert_eq!(p.dsp_type, dsp_type, "param type mismatch for '{name}'");
    assert_eq!(p.optional, optional, "optional flag mismatch for '{name}'");
}
