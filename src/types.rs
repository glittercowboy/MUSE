//! Domain type system for the Muse language.
//!
//! These are the language's own types used during semantic analysis to validate
//! DSP function calls before codegen. They are NOT Rust types — they represent
//! the Muse type vocabulary.

use std::fmt;

use crate::ast::UnitSuffix;

/// The Muse language's internal type vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DspType {
    /// Audio buffer — produced by oscillators and mix().
    Signal,
    /// Signal→signal transform — chain target (filters, gain, pan, etc.).
    Processor,
    /// Control signal in 0.0–1.0 range — produced by envelope generators.
    Envelope,
    /// Frequency value (Hz, kHz).
    Frequency,
    /// Gain/amplitude value (dB or linear).
    Gain,
    /// Time duration (ms, s).
    Time,
    /// Rate value (e.g. LFO rate).
    Rate,
    /// Named parameter reference.
    Param,
    /// Boolean value.
    Bool,
    /// Generic numeric value — compatible with all numeric-domain types.
    Number,
}

impl fmt::Display for DspType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DspType::Signal => write!(f, "Signal"),
            DspType::Processor => write!(f, "Processor"),
            DspType::Envelope => write!(f, "Envelope"),
            DspType::Frequency => write!(f, "Frequency"),
            DspType::Gain => write!(f, "Gain"),
            DspType::Time => write!(f, "Time"),
            DspType::Rate => write!(f, "Rate"),
            DspType::Param => write!(f, "Param"),
            DspType::Bool => write!(f, "Bool"),
            DspType::Number => write!(f, "Number"),
        }
    }
}

impl DspType {
    /// Returns true if a value of type `self` can be used where `expected` is required.
    ///
    /// Rules:
    /// - Exact match always works.
    /// - `Number` is compatible with any numeric-domain type (Frequency, Gain, Time, Rate).
    /// - Numeric-domain types are NOT compatible with each other (e.g. Frequency ≠ Time).
    pub fn is_compatible_with(self, expected: DspType) -> bool {
        if self == expected {
            return true;
        }
        // Number can substitute for any numeric-domain type
        if self == DspType::Number && expected.is_numeric_domain() {
            return true;
        }
        // Envelope is a 0.0–1.0 control signal — usable as a numeric value
        if self == DspType::Envelope && expected.is_numeric_domain() {
            return true;
        }
        false
    }

    /// Returns true if this type is in the numeric domain
    /// (Frequency, Gain, Time, Rate, Number).
    pub fn is_numeric_domain(self) -> bool {
        matches!(
            self,
            DspType::Frequency
                | DspType::Gain
                | DspType::Time
                | DspType::Rate
                | DspType::Number
        )
    }
}

/// Map a parsed unit suffix to its corresponding domain type.
pub fn type_from_unit_suffix(suffix: UnitSuffix) -> DspType {
    match suffix {
        UnitSuffix::Hz | UnitSuffix::KHz => DspType::Frequency,
        UnitSuffix::Ms | UnitSuffix::S => DspType::Time,
        UnitSuffix::DB => DspType::Gain,
        UnitSuffix::Percent | UnitSuffix::St => DspType::Number,
    }
}
