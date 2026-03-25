//! DSP function definitions, primitive enum, and the built-in registry.

use std::collections::HashMap;

use crate::types::DspType;

// ── Sub-enums ────────────────────────────────────────────────

/// Oscillator variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OscKind {
    Sine,
    Saw,
    Square,
    Triangle,
}

/// Filter variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterKind {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

/// Envelope variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvKind {
    Adsr,
    Ar,
}

// ── Primitive enum ───────────────────────────────────────────

/// Identifies a specific DSP operation. Used by codegen to select the
/// concrete implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DspPrimitive {
    Oscillator(OscKind),
    Filter(FilterKind),
    Envelope(EnvKind),
    Gain,
    Pan,
    Delay,
    Mix,
    Clip,
    Tanh,
    Noise,
    Fold,
    Bitcrush,
    Lfo,
    Pulse,
    Chorus,
    Compressor,
}

// ── Function signature types ─────────────────────────────────

/// A single parameter in a DSP function signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DspParam {
    pub name: String,
    pub dsp_type: DspType,
    pub optional: bool,
}

/// A complete DSP function signature with its primitive identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DspFunction {
    pub name: String,
    pub params: Vec<DspParam>,
    pub return_type: DspType,
    pub primitive: DspPrimitive,
}

// ── Registry ─────────────────────────────────────────────────

/// Holds the mapping from function name to its DSP function definition.
#[derive(Debug)]
pub struct DspRegistry {
    pub functions: HashMap<String, DspFunction>,
}

impl DspRegistry {
    /// Look up a function by name.
    pub fn lookup(&self, name: &str) -> Option<&DspFunction> {
        self.functions.get(name)
    }
}

/// Build the standard library registry with all 16 built-in DSP functions.
pub fn builtin_registry() -> DspRegistry {
    let mut functions = HashMap::new();

    let entries: Vec<DspFunction> = vec![
        // ── Oscillators (all return Signal) ──
        osc("sine", OscKind::Sine),
        osc("saw", OscKind::Saw),
        osc("square", OscKind::Square),
        osc("triangle", OscKind::Triangle),
        // noise() → Signal (no params)
        DspFunction {
            name: "noise".into(),
            params: vec![],
            return_type: DspType::Signal,
            primitive: DspPrimitive::Noise,
        },
        // ── Filters (all return Processor) ──
        filter("lowpass", FilterKind::Lowpass),
        filter("highpass", FilterKind::Highpass),
        filter("bandpass", FilterKind::Bandpass),
        filter("notch", FilterKind::Notch),
        // ── Envelopes ──
        // adsr(attack: Time, decay: Time, sustain: Number, release: Time) → Envelope
        DspFunction {
            name: "adsr".into(),
            params: vec![
                param("attack", DspType::Time),
                param("decay", DspType::Time),
                param("sustain", DspType::Number),
                param("release", DspType::Time),
            ],
            return_type: DspType::Envelope,
            primitive: DspPrimitive::Envelope(EnvKind::Adsr),
        },
        // ar(attack: Time, release: Time) → Envelope
        DspFunction {
            name: "ar".into(),
            params: vec![
                param("attack", DspType::Time),
                param("release", DspType::Time),
            ],
            return_type: DspType::Envelope,
            primitive: DspPrimitive::Envelope(EnvKind::Ar),
        },
        // ── Utilities ──
        // gain(amount: Gain) → Processor
        DspFunction {
            name: "gain".into(),
            params: vec![param("amount", DspType::Gain)],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Gain,
        },
        // pan(position: Number) → Processor
        DspFunction {
            name: "pan".into(),
            params: vec![param("position", DspType::Number)],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Pan,
        },
        // delay(time: Time) → Processor
        DspFunction {
            name: "delay".into(),
            params: vec![param("time", DspType::Time)],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Delay,
        },
        // mix(dry: Signal, wet: Signal) → Signal
        DspFunction {
            name: "mix".into(),
            params: vec![
                param("dry", DspType::Signal),
                param("wet", DspType::Signal),
            ],
            return_type: DspType::Signal,
            primitive: DspPrimitive::Mix,
        },
        // clip(min: Number, max: Number) → Processor
        DspFunction {
            name: "clip".into(),
            params: vec![
                param("min", DspType::Number),
                param("max", DspType::Number),
            ],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Clip,
        },
        // tanh() → Processor
        DspFunction {
            name: "tanh".into(),
            params: vec![],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Tanh,
        },
        // ── New DSP primitives ──
        // fold(amount: Number) → Processor — wavefolder distortion
        DspFunction {
            name: "fold".into(),
            params: vec![param("amount", DspType::Number)],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Fold,
        },
        // bitcrush(bits: Number) → Processor — bit depth reducer
        DspFunction {
            name: "bitcrush".into(),
            params: vec![param("bits", DspType::Number)],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Bitcrush,
        },
        // lfo(rate: Rate) → Signal — low-frequency oscillator (sine)
        DspFunction {
            name: "lfo".into(),
            params: vec![param("rate", DspType::Rate)],
            return_type: DspType::Signal,
            primitive: DspPrimitive::Lfo,
        },
        // pulse(freq: Frequency, width: Number) → Signal — pulse wave oscillator
        DspFunction {
            name: "pulse".into(),
            params: vec![
                param("freq", DspType::Frequency),
                param("width", DspType::Number),
            ],
            return_type: DspType::Signal,
            primitive: DspPrimitive::Pulse,
        },
        // chorus(rate: Rate, depth: Number) → Processor — chorus effect
        DspFunction {
            name: "chorus".into(),
            params: vec![
                param("rate", DspType::Rate),
                param("depth", DspType::Number),
            ],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Chorus,
        },
        // compressor(threshold: Gain, ratio: Number) → Processor — dynamics compressor
        DspFunction {
            name: "compressor".into(),
            params: vec![
                param("threshold", DspType::Gain),
                param("ratio", DspType::Number),
            ],
            return_type: DspType::Processor,
            primitive: DspPrimitive::Compressor,
        },
    ];

    for func in entries {
        functions.insert(func.name.clone(), func);
    }

    DspRegistry { functions }
}

// ── Helpers ──────────────────────────────────────────────────

fn param(name: &str, dsp_type: DspType) -> DspParam {
    DspParam {
        name: name.into(),
        dsp_type,
        optional: false,
    }
}

fn optional_param(name: &str, dsp_type: DspType) -> DspParam {
    DspParam {
        name: name.into(),
        dsp_type,
        optional: true,
    }
}

/// Standard oscillator: one required freq param, returns Signal.
fn osc(name: &str, kind: OscKind) -> DspFunction {
    DspFunction {
        name: name.into(),
        params: vec![param("freq", DspType::Frequency)],
        return_type: DspType::Signal,
        primitive: DspPrimitive::Oscillator(kind),
    }
}

/// Standard filter: cutoff + optional resonance, returns Processor.
fn filter(name: &str, kind: FilterKind) -> DspFunction {
    DspFunction {
        name: name.into(),
        params: vec![
            param("cutoff", DspType::Frequency),
            optional_param("resonance", DspType::Number),
        ],
        return_type: DspType::Processor,
        primitive: DspPrimitive::Filter(kind),
    }
}
