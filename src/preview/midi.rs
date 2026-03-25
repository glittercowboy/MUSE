//! MIDI input via `midir` for the live preview host.
//!
//! Enumerates available MIDI input ports, connects to one by name (or the
//! first available), parses raw MIDI bytes, and sends structured `MidiEvent`
//! values through an `mpsc` channel for the audio thread to drain.

use midir::{MidiInput, MidiInputConnection};
use std::sync::mpsc;

/// A parsed MIDI event ready for the audio callback.
#[derive(Debug, Clone, Copy)]
pub enum MidiEvent {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
}

/// List available MIDI input port names. Returns an empty Vec if none found.
pub fn list_midi_ports() -> Vec<String> {
    let midi_in = match MidiInput::new("muse-preview-list") {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    midi_in
        .ports()
        .iter()
        .filter_map(|p| midi_in.port_name(p).ok())
        .collect()
}

/// An active MIDI connection that feeds events into an `mpsc` channel.
///
/// Holds the `MidiInputConnection` — dropping this closes the port.
pub struct MidiConnection {
    _conn: MidiInputConnection<()>,
    pub port_name: String,
}

/// Connect to a MIDI input port and return a `(MidiConnection, Receiver)`.
///
/// If `port_name_filter` is `Some`, connect to the first port whose name
/// contains the given substring. Otherwise, connect to the first available port.
///
/// Returns `Err` if no matching port is found or the connection fails.
pub fn connect_midi(
    port_name_filter: Option<&str>,
) -> Result<(MidiConnection, mpsc::Receiver<MidiEvent>), String> {
    let midi_in = MidiInput::new("muse-preview")
        .map_err(|e| format!("failed to create MIDI input: {e}"))?;

    let ports = midi_in.ports();
    if ports.is_empty() {
        return Err("no MIDI input ports found".into());
    }

    // Find the target port.
    let (port, name) = if let Some(filter) = port_name_filter {
        let found = ports.iter().find_map(|p| {
            let name = midi_in.port_name(p).ok()?;
            if name.contains(filter) {
                Some((p.clone(), name))
            } else {
                None
            }
        });
        found.ok_or_else(|| format!("no MIDI port matching '{filter}'"))?
    } else {
        let p = &ports[0];
        let name = midi_in
            .port_name(p)
            .map_err(|e| format!("failed to get port name: {e}"))?;
        (p.clone(), name)
    };

    let (tx, rx) = mpsc::channel::<MidiEvent>();

    let conn = midi_in
        .connect(
            &port,
            "muse-preview-input",
            move |_timestamp_us, message, _data| {
                if let Some(event) = parse_midi_message(message) {
                    // Send is non-blocking when the channel has capacity.
                    // If the receiver is gone (audio stopped), silently drop.
                    let _ = tx.send(event);
                }
            },
            (),
        )
        .map_err(|e| format!("failed to connect to MIDI port '{name}': {e}"))?;

    Ok((
        MidiConnection {
            _conn: conn,
            port_name: name,
        },
        rx,
    ))
}

/// Parse a raw MIDI byte slice into a `MidiEvent`.
///
/// Handles:
/// - `0x90..=0x9F` (NoteOn) — velocity 0 treated as NoteOff per MIDI spec
/// - `0x80..=0x8F` (NoteOff)
///
/// Returns `None` for all other message types (CC, pitchbend, sysex, etc.).
fn parse_midi_message(msg: &[u8]) -> Option<MidiEvent> {
    if msg.len() < 2 {
        return None;
    }
    let status = msg[0] & 0xF0; // strip channel nibble
    match status {
        0x90 if msg.len() >= 3 => {
            let note = msg[1] & 0x7F;
            let vel = msg[2] & 0x7F;
            if vel == 0 {
                // Velocity 0 NoteOn = NoteOff (common in running status)
                Some(MidiEvent::NoteOff { note })
            } else {
                Some(MidiEvent::NoteOn {
                    note,
                    velocity: vel as f32 / 127.0,
                })
            }
        }
        0x80 if msg.len() >= 3 => {
            let note = msg[1] & 0x7F;
            Some(MidiEvent::NoteOff { note })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_note_on() {
        let msg = [0x90, 60, 100]; // NoteOn, C4, vel 100
        match parse_midi_message(&msg) {
            Some(MidiEvent::NoteOn { note, velocity }) => {
                assert_eq!(note, 60);
                assert!((velocity - 100.0 / 127.0).abs() < 0.01);
            }
            other => panic!("expected NoteOn, got {other:?}"),
        }
    }

    #[test]
    fn parse_note_on_vel_zero_is_note_off() {
        let msg = [0x92, 60, 0]; // NoteOn channel 2, C4, vel 0
        match parse_midi_message(&msg) {
            Some(MidiEvent::NoteOff { note }) => assert_eq!(note, 60),
            other => panic!("expected NoteOff, got {other:?}"),
        }
    }

    #[test]
    fn parse_note_off() {
        let msg = [0x81, 72, 64]; // NoteOff channel 1, C5, vel 64
        match parse_midi_message(&msg) {
            Some(MidiEvent::NoteOff { note }) => assert_eq!(note, 72),
            other => panic!("expected NoteOff, got {other:?}"),
        }
    }

    #[test]
    fn parse_cc_ignored() {
        let msg = [0xB0, 1, 64]; // CC, mod wheel
        assert!(parse_midi_message(&msg).is_none());
    }

    #[test]
    fn parse_short_message() {
        assert!(parse_midi_message(&[0x90]).is_none());
        assert!(parse_midi_message(&[]).is_none());
    }

    #[test]
    fn list_ports_does_not_panic() {
        // Just verify it doesn't crash — CI may have no MIDI hardware.
        let ports = list_midi_ports();
        eprintln!("available MIDI ports: {ports:?}");
    }
}
