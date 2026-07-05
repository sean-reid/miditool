//! Encoding and decoding of channel voice messages on the MIDI 1.0 wire.
//!
//! System messages (0xF0..=0xFF) are not modeled as [`EventKind`]; the I/O
//! layer forwards them verbatim. The decoder handles running status so raw
//! DIN streams work, and normalizes note-on velocity 0 to note-off.

use crate::event::EventKind;

/// Streaming decoder for one MIDI input. Feed it bytes, get events.
///
/// Zero-allocation and constant-space. Backends that deliver complete
/// messages (CoreMIDI, ALSA seq) can call [`Decoder::feed`] per packet;
/// raw byte streams work too, including running status and interleaved
/// realtime bytes.
#[derive(Debug, Default)]
pub struct Decoder {
    status: u8,
    data: [u8; 2],
    have: u8,
    in_sysex: bool,
}

/// One step of decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded {
    /// A complete channel voice message.
    Event(EventKind),
    /// A system realtime byte (0xF8..=0xFF except 0xF7): forward verbatim.
    Realtime(u8),
    /// Byte consumed, message not complete yet (or inside SysEx).
    Pending,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a whole packet, invoking `emit` for each decoded item.
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(Decoded)) {
        for &b in bytes {
            match self.step(b) {
                Decoded::Pending => {}
                d => emit(d),
            }
        }
    }

    /// Consume one byte.
    pub fn step(&mut self, byte: u8) -> Decoded {
        // Realtime bytes may appear anywhere, even inside other messages,
        // and must not disturb decoder state.
        if byte >= 0xF8 {
            return Decoded::Realtime(byte);
        }
        if byte >= 0x80 {
            return self.on_status(byte);
        }
        self.on_data(byte)
    }

    fn on_status(&mut self, byte: u8) -> Decoded {
        match byte {
            0xF0 => {
                self.in_sysex = true;
                self.status = 0;
                self.have = 0;
            }
            0xF7 => {
                self.in_sysex = false;
            }
            0xF1..=0xF6 => {
                // System common cancels running status; contents are
                // forwarded by the I/O layer, not modeled here.
                self.in_sysex = false;
                self.status = 0;
                self.have = 0;
            }
            _ => {
                self.in_sysex = false;
                self.status = byte;
                self.have = 0;
            }
        }
        Decoded::Pending
    }

    fn on_data(&mut self, byte: u8) -> Decoded {
        if self.in_sysex || self.status == 0 {
            return Decoded::Pending;
        }
        let ch = self.status & 0x0F;
        let needed = message_len(self.status);
        self.data[self.have as usize] = byte;
        self.have += 1;
        if self.have < needed {
            return Decoded::Pending;
        }
        // Message complete; keep status for running status, reset data.
        self.have = 0;
        let d0 = self.data[0];
        let d1 = self.data[1];
        let kind = match self.status & 0xF0 {
            0x80 => EventKind::NoteOff {
                ch,
                key: d0,
                vel: d1,
            },
            0x90 => {
                if d1 == 0 {
                    EventKind::NoteOff {
                        ch,
                        key: d0,
                        vel: 0,
                    }
                } else {
                    EventKind::NoteOn {
                        ch,
                        key: d0,
                        vel: d1,
                    }
                }
            }
            0xA0 => EventKind::PolyPressure {
                ch,
                key: d0,
                value: d1,
            },
            0xB0 => EventKind::ControlChange {
                ch,
                cc: d0,
                value: d1,
            },
            0xC0 => EventKind::ProgramChange { ch, program: d0 },
            0xD0 => EventKind::ChannelPressure { ch, value: d0 },
            0xE0 => EventKind::PitchBend {
                ch,
                value: bend_from_wire(d0, d1),
            },
            _ => unreachable!("status verified as channel voice"),
        };
        Decoded::Event(kind)
    }
}

fn message_len(status: u8) -> u8 {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1,
        _ => 2,
    }
}

fn bend_from_wire(lsb: u8, msb: u8) -> i16 {
    (((msb as i16) << 7) | lsb as i16) - 8192
}

/// Encode a channel voice message. Returns the valid prefix of `buf`.
pub fn encode<'a>(kind: &EventKind, buf: &'a mut [u8; 3]) -> &'a [u8] {
    let len = match *kind {
        EventKind::NoteOff { ch, key, vel } => {
            *buf = [0x80 | ch, key, vel];
            3
        }
        EventKind::NoteOn { ch, key, vel } => {
            *buf = [0x90 | ch, key, vel];
            3
        }
        EventKind::PolyPressure { ch, key, value } => {
            *buf = [0xA0 | ch, key, value];
            3
        }
        EventKind::ControlChange { ch, cc, value } => {
            *buf = [0xB0 | ch, cc, value];
            3
        }
        EventKind::ProgramChange { ch, program } => {
            *buf = [0xC0 | ch, program, 0];
            2
        }
        EventKind::ChannelPressure { ch, value } => {
            *buf = [0xD0 | ch, value, 0];
            2
        }
        EventKind::PitchBend { ch, value } => {
            let raw = (value + 8192) as u16;
            *buf = [0xE0 | ch, (raw & 0x7F) as u8, (raw >> 7) as u8];
            3
        }
    };
    &buf[..len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn decode_all(bytes: &[u8]) -> Vec<Decoded> {
        let mut d = Decoder::new();
        let mut out = Vec::new();
        d.feed(bytes, |x| out.push(x));
        out
    }

    #[test]
    fn note_on_off() {
        let out = decode_all(&[0x90, 60, 100, 0x80, 60, 40]);
        assert_eq!(
            out,
            vec![
                Decoded::Event(EventKind::NoteOn {
                    ch: 0,
                    key: 60,
                    vel: 100
                }),
                Decoded::Event(EventKind::NoteOff {
                    ch: 0,
                    key: 60,
                    vel: 40
                }),
            ]
        );
    }

    #[test]
    fn velocity_zero_note_on_is_note_off() {
        let out = decode_all(&[0x93, 60, 0]);
        assert_eq!(
            out,
            vec![Decoded::Event(EventKind::NoteOff {
                ch: 3,
                key: 60,
                vel: 0
            })]
        );
    }

    #[test]
    fn running_status() {
        let out = decode_all(&[0x90, 60, 100, 62, 90, 64, 80]);
        assert_eq!(out.len(), 3);
        assert_eq!(
            out[2],
            Decoded::Event(EventKind::NoteOn {
                ch: 0,
                key: 64,
                vel: 80
            })
        );
    }

    #[test]
    fn realtime_interleaved_mid_message() {
        // Clock byte between the data bytes of a note-on.
        let out = decode_all(&[0x90, 60, 0xF8, 100]);
        assert_eq!(
            out,
            vec![
                Decoded::Realtime(0xF8),
                Decoded::Event(EventKind::NoteOn {
                    ch: 0,
                    key: 60,
                    vel: 100
                }),
            ]
        );
    }

    #[test]
    fn sysex_is_skipped_and_cancels_running_status() {
        let out = decode_all(&[0x90, 60, 100, 0xF0, 1, 2, 3, 0xF7, 61, 100]);
        // The trailing data bytes have no status to attach to.
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn pitch_bend_center_and_extremes() {
        let out = decode_all(&[0xE0, 0x00, 0x40, 0xE0, 0x00, 0x00, 0xE0, 0x7F, 0x7F]);
        assert_eq!(
            out,
            vec![
                Decoded::Event(EventKind::PitchBend { ch: 0, value: 0 }),
                Decoded::Event(EventKind::PitchBend {
                    ch: 0,
                    value: -8192
                }),
                Decoded::Event(EventKind::PitchBend { ch: 0, value: 8191 }),
            ]
        );
    }

    proptest! {
        #[test]
        fn encode_decode_round_trip(kind in arb_kind()) {
            let mut buf = [0u8; 3];
            let bytes = encode(&kind, &mut buf);
            let out = decode_all(bytes);
            prop_assert_eq!(out, vec![Decoded::Event(kind)]);
        }
    }

    fn arb_kind() -> impl Strategy<Value = EventKind> {
        let ch = 0u8..16;
        let d = 0u8..128;
        prop_oneof![
            (ch.clone(), d.clone(), 1u8..128).prop_map(|(ch, key, vel)| EventKind::NoteOn {
                ch,
                key,
                vel
            }),
            (ch.clone(), d.clone(), d.clone()).prop_map(|(ch, key, vel)| EventKind::NoteOff {
                ch,
                key,
                vel
            }),
            (ch.clone(), d.clone(), d.clone())
                .prop_map(|(ch, key, value)| EventKind::PolyPressure { ch, key, value }),
            (ch.clone(), d.clone(), d.clone())
                .prop_map(|(ch, cc, value)| EventKind::ControlChange { ch, cc, value }),
            (ch.clone(), d.clone())
                .prop_map(|(ch, program)| EventKind::ProgramChange { ch, program }),
            (ch.clone(), d).prop_map(|(ch, value)| EventKind::ChannelPressure { ch, value }),
            (ch, -8192i16..=8191).prop_map(|(ch, value)| EventKind::PitchBend { ch, value }),
        ]
    }
}
