//! Generates the sample-accurate MIDI event processing loop for instrument plugins.
//!
//! The generated code iterates MIDI events at the correct sample offset,
//! handling NoteOn (with velocity) and NoteOff events for monophonic or
//! polyphonic note tracking.

/// Generate the Rust code for a sample-accurate MIDI event loop.
///
/// Returns a String containing the event-processing loop body that should be
/// placed inside the per-sample loop. The loop advances through MIDI events
/// whose timing matches the current sample index, dispatching NoteOn/NoteOff
/// to update voice state.
///
/// The generated code expects these variables to be in scope:
/// - `next_event`: `Option<NoteEvent<()>>` — the next buffered MIDI event
/// - `sample_idx`: `usize` — current sample index within the buffer
/// - `context`: the nih-plug `ProcessContext` — used to fetch subsequent events
///
/// Voice state variables (`note_pitch`, `note_velocity`, `note_gate`) are
/// updated by the generated match arms.
pub fn generate_midi_event_loop() -> String {
    r#"// Sample-accurate MIDI event processing
while let Some(event) = next_event {
    if event.timing() > sample_idx as u32 {
        break;
    }
    match event {
        NoteEvent::NoteOn { note, velocity, .. } => {
            note_pitch = util::midi_note_to_freq(note);
            note_velocity = velocity;
            note_gate = 1.0;
        }
        NoteEvent::NoteOff { note, .. } => {
            // Only release if this is the currently playing note
            if (util::midi_note_to_freq(note) - note_pitch).abs() < 0.01 {
                note_gate = 0.0;
            }
        }
        _ => {}
    }
    next_event = context.next_event();
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_event_loop_contains_note_on() {
        let code = generate_midi_event_loop();
        assert!(
            code.contains("NoteEvent::NoteOn"),
            "MIDI loop should handle NoteOn events"
        );
    }

    #[test]
    fn midi_event_loop_contains_note_off() {
        let code = generate_midi_event_loop();
        assert!(
            code.contains("NoteEvent::NoteOff"),
            "MIDI loop should handle NoteOff events"
        );
    }

    #[test]
    fn midi_event_loop_contains_timing_check() {
        let code = generate_midi_event_loop();
        assert!(
            code.contains("event.timing()"),
            "MIDI loop should check event timing for sample accuracy"
        );
    }

    #[test]
    fn midi_event_loop_contains_next_event() {
        let code = generate_midi_event_loop();
        assert!(
            code.contains("context.next_event()"),
            "MIDI loop should advance to next event"
        );
    }
}
