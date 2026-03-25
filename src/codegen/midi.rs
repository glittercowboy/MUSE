//! Generates MIDI event handling code for instrument plugins.
//!
//! Mono instruments use a per-sample event loop. Polyphonic instruments use
//! block-based event handling with explicit voice allocation and termination.

/// Generate the Rust code for a sample-accurate MIDI event loop for mono instruments.
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

/// Generate the Rust code for a block-oriented MIDI event loop for polyphonic instruments.
pub fn generate_polyphonic_event_handler() -> String {
    r#"let this_block_internal_voice_id_start = self.next_internal_voice_id;
'events: loop {
    match next_event {
        Some(event) if (event.timing() as usize) <= block_start => {
            match event {
                NoteEvent::NoteOn {
                    timing,
                    voice_id,
                    channel,
                    note,
                    velocity,
                } => {
                    let voice = self.start_voice(context, timing, voice_id, channel, note);
                    voice.note_freq = util::midi_note_to_freq(note);
                    voice.velocity = velocity;
                    voice.releasing = false;
                }
                NoteEvent::NoteOff {
                    voice_id,
                    channel,
                    note,
                    ..
                } => {
                    self.start_release_for_voices(voice_id, channel, note);
                }
                NoteEvent::Choke {
                    timing,
                    voice_id,
                    channel,
                    note,
                } => {
                    self.choke_voices(context, timing, voice_id, channel, note);
                }
                _ => {}
            }
            next_event = context.next_event();
        }
        Some(event) if (event.timing() as usize) < block_end => {
            block_end = event.timing() as usize;
            break 'events;
        }
        _ => break 'events,
    }
}
"#
        .to_string()
}

/// Generate helper methods for polyphonic voice allocation and release.
pub fn generate_voice_helper_methods() -> String {
    r#"impl {STRUCT_NAME} {
    fn get_voice_idx(&mut self, voice_id: i32) -> Option<usize> {
        self.voices
            .iter_mut()
            .position(|voice| matches!(voice, Some(voice) if voice.voice_id == voice_id))
    }

    fn start_voice(
        &mut self,
        context: &mut impl ProcessContext<Self>,
        sample_offset: u32,
        voice_id: Option<i32>,
        channel: u8,
        note: u8,
    ) -> &mut Voice {
        let new_voice = Voice {
            voice_id: voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel)),
            internal_voice_id: self.next_internal_voice_id,
            channel,
            note,
            note_freq: util::midi_note_to_freq(note),
            velocity: 0.0,
            releasing: false,
            {VOICE_FIELD_DEFAULTS}
        };
        self.next_internal_voice_id = self.next_internal_voice_id.wrapping_add(1);

        match self.voices.iter().position(|voice| voice.is_none()) {
            Some(free_voice_idx) => {
                self.voices[free_voice_idx] = Some(new_voice);
                self.voices[free_voice_idx].as_mut().unwrap()
            }
            None => {
                let oldest_voice = unsafe {
                    self.voices
                        .iter_mut()
                        .min_by_key(|voice| voice.as_ref().unwrap_unchecked().internal_voice_id)
                        .unwrap_unchecked()
                };

                {
                    let oldest_voice = oldest_voice.as_ref().unwrap();
                    context.send_event(NoteEvent::VoiceTerminated {
                        timing: sample_offset,
                        voice_id: Some(oldest_voice.voice_id),
                        channel: oldest_voice.channel,
                        note: oldest_voice.note,
                    });
                }

                *oldest_voice = Some(new_voice);
                oldest_voice.as_mut().unwrap()
            }
        }
    }

    fn start_release_for_voices(&mut self, voice_id: Option<i32>, channel: u8, note: u8) {
        for voice in self.voices.iter_mut() {
            match voice {
                Some(Voice {
                    voice_id: candidate_voice_id,
                    channel: candidate_channel,
                    note: candidate_note,
                    releasing,
                    ..
                }) if voice_id == Some(*candidate_voice_id)
                    || (channel == *candidate_channel && note == *candidate_note) =>
                {
                    *releasing = true;
                    if voice_id.is_some() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn choke_voices(
        &mut self,
        context: &mut impl ProcessContext<Self>,
        sample_offset: u32,
        voice_id: Option<i32>,
        channel: u8,
        note: u8,
    ) {
        for voice in self.voices.iter_mut() {
            match voice {
                Some(Voice {
                    voice_id: candidate_voice_id,
                    channel: candidate_channel,
                    note: candidate_note,
                    ..
                }) if voice_id == Some(*candidate_voice_id)
                    || (channel == *candidate_channel && note == *candidate_note) =>
                {
                    context.send_event(NoteEvent::VoiceTerminated {
                        timing: sample_offset,
                        voice_id: Some(*candidate_voice_id),
                        channel: *candidate_channel,
                        note: *candidate_note,
                    });
                    *voice = None;

                    if voice_id.is_some() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    const fn compute_fallback_voice_id(note: u8, channel: u8) -> i32 {
        note as i32 | ((channel as i32) << 16)
    }
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

    #[test]
    fn polyphonic_event_handler_contains_block_splitting() {
        let code = generate_polyphonic_event_handler();
        assert!(code.contains("block_start"));
        assert!(code.contains("block_end"));
        assert!(code.contains("this_block_internal_voice_id_start"));
    }

    #[test]
    fn voice_helpers_include_voice_termination() {
        let code = generate_voice_helper_methods();
        assert!(code.contains("VoiceTerminated"));
        assert!(code.contains("compute_fallback_voice_id"));
        assert!(code.contains("start_voice"));
    }
}
