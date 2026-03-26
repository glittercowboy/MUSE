//! Generates MIDI event handling code for instrument plugins.
//!
//! Mono instruments use a per-sample event loop. Polyphonic instruments use
//! block-based event handling with explicit voice allocation and termination.

/// Generate the Rust code for a sample-accurate MIDI event loop for mono instruments.
pub fn generate_midi_event_loop(play_call_count: usize, wt_osc_call_count: usize, loop_call_count: usize) -> String {
    let mut play_reset = String::new();
    for i in 0..play_call_count {
        play_reset.push_str(&format!("            self.play_pos_{} = 0.0;\n", i));
        play_reset.push_str(&format!("            self.play_active_{} = true;\n", i));
    }
    for i in 0..loop_call_count {
        play_reset.push_str(&format!("            self.loop_pos_{} = 0.0;\n", i));
        play_reset.push_str(&format!("            self.loop_active_{} = true;\n", i));
    }
    for i in 0..wt_osc_call_count {
        play_reset.push_str(&format!("            self.wt_osc_state_{}.phase = 0.0;\n", i));
    }
    format!(
        r#"// Sample-accurate MIDI event processing
while let Some(event) = next_event {{
    if event.timing() > sample_idx as u32 {{
        break;
    }}
    match event {{
        NoteEvent::NoteOn {{ note, velocity, .. }} => {{
            self.active_note = Some(note);
            self.note_freq = util::midi_note_to_freq(note);
            self.velocity = velocity;
{play_reset}        }}
        NoteEvent::NoteOff {{ note, .. }} => {{
            // Only release if this is the currently playing note
            if self.active_note == Some(note) {{
                self.active_note = None;
            }}
        }}
        _ => {{}}
    }}
    next_event = context.next_event();
}}
"#
    )
}

/// Generate the Rust code for a block-oriented MIDI event loop for polyphonic instruments.
pub fn generate_polyphonic_event_handler(unison_config: Option<&crate::codegen::CodegenUnisonConfig>) -> String {
    let note_on_body = if let Some(unison) = unison_config {
        let count = unison.count;
        let detune = unison.detune_cents;
        format!(
            r#"                    let base_freq = util::midi_note_to_freq(note);
                    let base_vid = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                    for u_idx in 0..{count}u32 {{
                        let detune_spread = if {count} > 1 {{
                            let t = u_idx as f32 / ({count} as f32 - 1.0);
                            (t - 0.5) * 2.0 * {detune}_f32
                        }} else {{
                            0.0_f32
                        }};
                        let detuned_freq = base_freq * 2.0_f32.powf(detune_spread / 1200.0);
                        let unison_vid = Some(base_vid.wrapping_mul(UNISON_MAX).wrapping_add(u_idx as i32));
                        let voice = self.start_voice(context, timing, unison_vid, channel, note);
                        voice.note_freq = detuned_freq;
                        voice.velocity = velocity;
                        voice.releasing = false;
                    }}"#
        )
    } else {
        r#"                    let voice = self.start_voice(context, timing, voice_id, channel, note);
                    voice.note_freq = util::midi_note_to_freq(note);
                    voice.velocity = velocity;
                    voice.releasing = false;"#.to_string()
    };

    format!(r#"let this_block_internal_voice_id_start = self.next_internal_voice_id;
'events: loop {{
    match next_event {{
        Some(event) if (event.timing() as usize) <= block_start => {{
            match event {{
                NoteEvent::NoteOn {{
                    timing,
                    voice_id,
                    channel,
                    note,
                    velocity,
                }} => {{
{note_on_body}
                }}
                NoteEvent::NoteOff {{
                    voice_id,
                    channel,
                    note,
                    ..
                }} => {{
                    self.start_release_for_voices(voice_id, channel, note);
                }}
                NoteEvent::Choke {{
                    timing,
                    voice_id,
                    channel,
                    note,
                }} => {{
                    self.choke_voices(context, timing, voice_id, channel, note);
                }}
                NoteEvent::PolyPressure {{
                    voice_id,
                    note,
                    channel,
                    pressure,
                    ..
                }} => {{
                    let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                    if let Some(idx) = self.get_voice_idx(search_id) {{
                        if let Some(ref mut voice) = self.voices[idx] {{
                            voice.pressure = pressure;
                        }}
                    }}
                }}
                NoteEvent::PolyTuning {{
                    voice_id,
                    note,
                    channel,
                    tuning,
                    ..
                }} => {{
                    let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                    if let Some(idx) = self.get_voice_idx(search_id) {{
                        if let Some(ref mut voice) = self.voices[idx] {{
                            voice.tuning = tuning;
                        }}
                    }}
                }}
                NoteEvent::PolyBrightness {{
                    voice_id,
                    note,
                    channel,
                    brightness,
                    ..
                }} => {{
                    let search_id = voice_id.unwrap_or_else(|| Self::compute_fallback_voice_id(note, channel));
                    if let Some(idx) = self.get_voice_idx(search_id) {{
                        if let Some(ref mut voice) = self.voices[idx] {{
                            voice.slide = brightness;
                        }}
                    }}
                }}
                _ => {{}}
            }}
            next_event = context.next_event();
        }}
        Some(event) if (event.timing() as usize) < block_end => {{
            block_end = event.timing() as usize;
            break 'events;
        }}
        _ => break 'events,
    }}
}}
"#)
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
            pressure: 0.0,
            tuning: 0.0,
            slide: 0.0,
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
        let code = generate_midi_event_loop(0, 0, 0);
        assert!(
            code.contains("NoteEvent::NoteOn"),
            "MIDI loop should handle NoteOn events"
        );
    }

    #[test]
    fn midi_event_loop_contains_note_off() {
        let code = generate_midi_event_loop(0, 0, 0);
        assert!(
            code.contains("NoteEvent::NoteOff"),
            "MIDI loop should handle NoteOff events"
        );
    }

    #[test]
    fn midi_event_loop_contains_timing_check() {
        let code = generate_midi_event_loop(0, 0, 0);
        assert!(
            code.contains("event.timing()"),
            "MIDI loop should check event timing for sample accuracy"
        );
    }

    #[test]
    fn midi_event_loop_contains_next_event() {
        let code = generate_midi_event_loop(0, 0, 0);
        assert!(
            code.contains("context.next_event()"),
            "MIDI loop should advance to next event"
        );
    }

    #[test]
    fn midi_event_loop_uses_struct_fields() {
        let code = generate_midi_event_loop(0, 0, 0);
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
        let code = generate_polyphonic_event_handler(None);
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
