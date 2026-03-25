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
/// to update voice state on `self`.
///
/// The generated code expects these variables to be in scope:
/// - `next_event`: `Option<NoteEvent<()>>` — the next buffered MIDI event
/// - `sample_idx`: `usize` — current sample index within the buffer
/// - `context`: the nih-plug `ProcessContext` — used to fetch subsequent events
///
/// Voice state fields on `self`:
/// - `active_note: Option<u8>` — currently held MIDI note number
/// - `note_freq: f32` — frequency of the active note
/// - `velocity: f32` — velocity of the active note
pub fn generate_midi_event_loop() -> String {
    r#"// Sample-accurate MIDI event processing
while let Some(event) = next_event {
    if event.timing() > sample_idx as u32 {
        break;
    }
    match event {
        NoteEvent::NoteOn { note, velocity, .. } => {
            self.active_note = Some(note);
            self.note_freq = util::midi_note_to_freq(note);
            self.velocity = velocity;
        }
        NoteEvent::NoteOff { note, .. } => {
            // Only release if this is the currently playing note
            if self.active_note == Some(note) {
                self.active_note = None;
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

    #[test]
    fn midi_event_loop_uses_struct_fields() {
        let code = generate_midi_event_loop();
        assert!(
            code.contains("self.active_note"),
            "MIDI loop should use self.active_note for note tracking"
        );
        assert!(
            code.contains("self.note_freq"),
            "MIDI loop should set self.note_freq from MIDI note"
        );
        assert!(
            code.contains("self.velocity"),
            "MIDI loop should capture velocity into self.velocity"
        );
    }
}
