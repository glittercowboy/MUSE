//! DSP primitives and function registry for the Muse standard library.

pub mod primitives;

pub use primitives::{
    builtin_registry, DspFunction, DspParam, DspPrimitive, DspRegistry, EnvKind, FilterKind,
    OscKind,
};
