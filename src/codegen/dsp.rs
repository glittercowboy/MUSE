//! Generates inline DSP helper functions for the generated plugin crate.
//!
//! For T01 (gain.muse), the gain primitive needs no extra helpers —
//! it uses `*sample *= gain` directly. This module exists for future
//! DSP primitives that need helper structs or functions (filters, oscillators, etc.).

use std::collections::HashSet;

use crate::dsp::primitives::DspPrimitive;

/// Generate DSP helper code for the set of primitives used in the plugin.
///
/// Returns an empty string when no helpers are needed (e.g. gain-only plugins).
pub fn generate_dsp_helpers(used_primitives: &HashSet<DspPrimitive>) -> String {
    let out = String::new();

    // For now, only Gain is supported — and it needs no helpers.
    // Future tasks (T02, T03) will add filter state structs, oscillator helpers, etc.
    for prim in used_primitives {
        match prim {
            DspPrimitive::Gain => {
                // No helper needed — gain is a direct multiply
            }
            _ => {
                // Not yet implemented — will be added in T02/T03
            }
        }
    }

    out
}
