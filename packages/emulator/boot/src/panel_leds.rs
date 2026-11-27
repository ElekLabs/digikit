//! Front-panel LED state decoded from the UART8 TX stream (the firmware's own panel records).
//!
//! Framing is the one in `tools/emulator/panel_tx.py`: `0x10..=0x17 col d0..d7` (OLED page row, 9 data bytes),
//! `b8` (frame present), `b9 idx val` (LED set), `b5 idx r g b` (RGB LED), `b7 x`, `bb a b c`, `bc x`,
//! `0x40..=0x4f 0`; any other byte is skipped alone. A record may be split across drains, so the decoder
//! keeps the partly received record between calls. Decoding has no effect on emulation.
//!
//! Two protocols (`device_profile::LedProtocol`, `[emulator] led_protocol`). `Indexed` is the one above. `PackedStates` (finding 70
//! "Panel sweep") has no `b9`: `b5 n r g b` sets palette entry n (5 bits per channel), `b0..b3 led n` sets the palette entry that the
//! LED shows in state 0..3, and `0x20 + k, v` sets the states of LEDs 4k..4k+3 at two bits each (3 is the idle state). The decoder then
//! reports per LED the colour of its current state in `rgb`, and in `leds` the web faceplate's value (0x01 off, else the colour mask in
//! bits 4-7 and 1 for a bright colour, 0x0a for a dim one), so the faceplate needs no protocol of its own.

use device_profile::LedProtocol;

pub const LED_COUNT: usize = 64;
/// A channel value (5 bits) from which the web value counts the channel as on / the colour as bright.
const ON_LEVEL: u8 = 0x08;
const BRIGHT_LEVEL: u8 = 0x10;
const NO_PALETTE: u8 = 0xff;

#[derive(Clone, Debug)]
pub struct PanelLeds {
    pub leds: [u8; LED_COUNT],
    pub rgb: [[u8; 3]; LED_COUNT],
    /// Bumped on each b9/b5 record that changes a value.
    pub revision: u32,
    protocol: LedProtocol,
    /// `PackedStates`: the palette (`b5`), the palette entry of each LED state (`b0..b3`) and each LED's state (`0x20 + k`).
    palette: [[u8; 3]; LED_COUNT],
    state_palette: [[u8; 4]; LED_COUNT],
    states: [u8; LED_COUNT],
    opcode: u8,
    need: u8,
    len: u8,
    data: [u8; 9],
}

impl Default for PanelLeds {
    fn default() -> Self {
        Self::new(LedProtocol::Indexed)
    }
}

fn data_len(protocol: LedProtocol, opcode: u8) -> Option<u8> {
    match (protocol, opcode) {
        (_, 0x10..=0x17) => Some(9),
        (_, 0xb8) => Some(0),
        (LedProtocol::Indexed, 0xb9) => Some(2),
        (_, 0xb5) => Some(4),
        (_, 0xb7 | 0xbc) => Some(1),
        (_, 0xbb) => Some(3),
        (_, 0x40..=0x4f) => Some(1),
        (LedProtocol::PackedStates, 0x20..=0x2f) => Some(1),
        (LedProtocol::PackedStates, 0xb0..=0xb3) => Some(2),
        _ => None,
    }
}

/// The web faceplate's value for a colour (5 bits per channel): 0x01 when no channel is on.
fn web_value(rgb: [u8; 3]) -> u8 {
    let mask = rgb
        .iter()
        .enumerate()
        .fold(0u8, |m, (i, &c)| if c >= ON_LEVEL { m | 1 << i } else { m });
    if mask == 0 {
        return 0x01;
    }
    let bright = rgb.iter().any(|&c| c >= BRIGHT_LEVEL);
    mask << 4 | if bright { 0x01 } else { 0x0a }
}

impl PanelLeds {
    pub fn new(protocol: LedProtocol) -> Self {
        Self {
            leds: [0; LED_COUNT],
            rgb: [[0; 3]; LED_COUNT],
            revision: 0,
            protocol,
            palette: [[0; 3]; LED_COUNT],
            state_palette: [[NO_PALETTE; 4]; LED_COUNT],
            states: [3; LED_COUNT],
            opcode: 0,
            need: 0,
            len: 0,
            data: [0; 9],
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.need == 0 {
                match data_len(self.protocol, byte) {
                    None => {}
                    Some(0) => {}
                    Some(n) => {
                        self.opcode = byte;
                        self.need = n;
                        self.len = 0;
                    }
                }
                continue;
            }
            self.data[self.len as usize] = byte;
            self.len += 1;
            self.need -= 1;
            if self.need == 0 {
                self.apply();
            }
        }
    }

    /// `PackedStates`: the LED's colour in its current state. Bumps the revision when the reported value changes.
    fn show(&mut self, led: usize) {
        let entry = self.state_palette[led][usize::from(self.states[led] & 3)];
        let rgb = self
            .palette
            .get(usize::from(entry))
            .copied()
            .unwrap_or([0; 3]);
        let value = web_value(rgb);
        if self.leds[led] != value || self.rgb[led] != rgb {
            self.leds[led] = value;
            self.rgb[led] = rgb;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn apply_packed(&mut self) {
        let d = self.data;
        match self.opcode {
            0xb5 if usize::from(d[0]) < LED_COUNT => {
                self.palette[usize::from(d[0])] = [d[1], d[2], d[3]];
                (0..LED_COUNT).for_each(|led| self.show(led));
            }
            0xb0..=0xb3 if usize::from(d[0]) < LED_COUNT => {
                self.state_palette[usize::from(d[0])][usize::from(self.opcode - 0xb0)] = d[1];
                self.show(usize::from(d[0]));
            }
            0x20..=0x2f => {
                let first = usize::from(self.opcode - 0x20) * 4;
                for j in 0..4 {
                    if first + j < LED_COUNT {
                        self.states[first + j] = d[0] >> (2 * j) & 3;
                        self.show(first + j);
                    }
                }
            }
            _ => {}
        }
    }

    fn apply(&mut self) {
        if self.protocol == LedProtocol::PackedStates {
            return self.apply_packed();
        }
        let d = self.data;
        let idx = d[0] as usize;
        match self.opcode {
            0xb9 if idx < LED_COUNT => {
                if self.leds[idx] != d[1] {
                    self.leds[idx] = d[1];
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            0xb5 if idx < LED_COUNT => {
                let rgb = [d[1], d[2], d[3]];
                if self.rgb[idx] != rgb {
                    self.rgb[idx] = rgb;
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            _ => {}
        }
    }

    /// Whether the protocol keeps state between records (`PackedStates`), so a snapshot must carry it.
    pub fn is_stateful(&self) -> bool {
        self.protocol == LedProtocol::PackedStates
    }

    /// The state of a `PackedStates` decoder for a snapshot: palette, state palettes, states, and the reported values.
    pub fn snap_save(&self, w: &mut periph::snap::Writer) {
        w.u32(self.revision);
        for led in 0..LED_COUNT {
            w.raw(&self.palette[led]);
            w.raw(&self.state_palette[led]);
            w.u8(self.states[led]);
            w.u8(self.leds[led]);
            w.raw(&self.rgb[led]);
        }
    }

    pub fn snap_load(&mut self, r: &mut periph::snap::Reader) -> Result<(), String> {
        self.revision = r.u32()?;
        for led in 0..LED_COUNT {
            self.palette[led].copy_from_slice(r.raw(3)?);
            self.state_palette[led].copy_from_slice(r.raw(4)?);
            self.states[led] = r.u8()?;
            self.leds[led] = r.u8()?;
            self.rgb[led].copy_from_slice(r.raw(3)?);
        }
        Ok(())
    }

    /// `leds` as 128 lowercase hex digits.
    pub fn leds_hex(&self) -> String {
        hex(self.leds.iter().copied())
    }

    /// `rgb` as 384 hex digits, or `None` while no RGB LED has been set.
    pub fn rgb_hex(&self) -> Option<String> {
        self.rgb
            .iter()
            .any(|c| *c != [0; 3])
            .then(|| hex(self.rgb.iter().flatten().copied()))
    }
}

fn hex(bytes: impl Iterator<Item = u8>) -> String {
    bytes.map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_b9_and_b5() {
        let mut p = PanelLeds::default();
        p.feed(&[0xb9, 0x19, 0x71, 0xb5, 0x05, 1, 2, 3]);
        assert_eq!(p.leds[0x19], 0x71);
        assert_eq!(p.rgb[5], [1, 2, 3]);
        assert_eq!(p.revision, 2);
        p.feed(&[0xb9, 0x19, 0x71]);
        assert_eq!(p.revision, 2, "unchanged value does not bump");
    }

    #[test]
    fn records_split_across_drains() {
        let mut p = PanelLeds::default();
        p.feed(&[0xb9]);
        p.feed(&[0x1a]);
        assert_eq!(p.revision, 0);
        p.feed(&[0x11, 0xb5, 0x02]);
        p.feed(&[9, 8]);
        p.feed(&[7]);
        assert_eq!(p.leds[0x1a], 0x11);
        assert_eq!(p.rgb[2], [9, 8, 7]);
    }

    #[test]
    fn oled_rows_and_other_records_are_skipped() {
        let mut p = PanelLeds::default();
        p.feed(&[0x13, 0x00, 0xb9, 0x03, 0xff, 0xb9, 0x04, 0xb9, 0x05, 0x00]);
        p.feed(&[
            0xb8, 0xb7, 0xb9, 0xbb, 0xb9, 0x01, 0xb9, 0xbc, 0xb9, 0x43, 0x00,
        ]);
        p.feed(&[0x00, 0xb9, 0xff, 0x05, 0xb9, 0x07, 0x21]);
        assert_eq!(p.leds[0x07], 0x21);
        assert_eq!(p.leds[3], 0);
        assert_eq!(p.leds[4], 0);
        assert_eq!(p.revision, 1);
    }

    // The records of a cold boot of the device with packed states and a track-1 selection (finding 70): palette 3 is bright red, 4 half red, 2 dim white; LED 0
    // is the trig key 1 and shows palette 4 idle and 3 in state 2.
    fn packed() -> PanelLeds {
        let mut p = PanelLeds::new(LedProtocol::PackedStates);
        p.feed(&[0xb5, 2, 1, 1, 1, 0xb5, 3, 0x1f, 0, 0, 0xb5, 4, 0x0f, 0, 0]);
        p.feed(&[0xb3, 0, 4, 0xb2, 0, 3, 0xb3, 1, 2]);
        p
    }

    #[test]
    fn packed_states_report_the_colour_of_the_current_state() {
        let mut p = packed();
        p.feed(&[0x20, 0xff]);
        assert_eq!(p.rgb[0], [0x0f, 0, 0]);
        assert_eq!(p.leds[0], 0x1a, "dim red");
        assert_eq!(p.rgb[1], [1, 1, 1]);
        assert_eq!(p.leds[1], 0x01, "near black is off");
        p.feed(&[0x20, 0xfe]);
        assert_eq!(p.rgb[0], [0x1f, 0, 0]);
        assert_eq!(p.leds[0], 0x11, "bright red");
        assert_eq!(p.rgb[1], [1, 1, 1], "other LEDs keep their state");
    }

    #[test]
    fn packed_records_split_across_drains_and_palette_changes_follow() {
        let mut p = packed();
        p.feed(&[0x20]);
        p.feed(&[0xfe, 0xb5, 3]);
        p.feed(&[0, 0x1f]);
        assert_eq!(p.rgb[0], [0x1f, 0, 0], "the b5 record is not complete yet");
        p.feed(&[0]);
        assert_eq!((p.rgb[0], p.leds[0]), ([0, 0x1f, 0], 0x21));
        let revision = p.revision;
        p.feed(&[0x20, 0xfe]);
        assert_eq!(p.revision, revision, "unchanged state does not bump");
    }

    #[test]
    fn packed_state_round_trips_through_a_snapshot() {
        let mut p = packed();
        p.feed(&[0x20, 0xfe]);
        let mut w = periph::snap::Writer::new();
        p.snap_save(&mut w);
        let mut q = PanelLeds::new(LedProtocol::PackedStates);
        q.snap_load(&mut periph::snap::Reader::new(&w.buf)).unwrap();
        assert_eq!(
            (q.leds[0], q.rgb[0], q.revision),
            (0x11, [0x1f, 0, 0], p.revision)
        );
        q.feed(&[0x20, 0xff]);
        assert_eq!(q.rgb[0], [0x0f, 0, 0], "palette survived");
    }

    #[test]
    fn packed_protocol_has_no_b9_record() {
        let mut p = PanelLeds::new(LedProtocol::PackedStates);
        p.feed(&[0xb9, 0x03, 0x71]);
        assert_eq!(p.revision, 0);
        assert!(p.leds.iter().all(|&v| v == 0));
    }

    #[test]
    fn hex_forms() {
        let mut p = PanelLeds::default();
        assert_eq!(p.leds_hex().len(), 128);
        assert!(p.rgb_hex().is_none());
        p.feed(&[0xb5, 0, 1, 0, 0]);
        assert_eq!(p.rgb_hex().unwrap().len(), 384);
    }
}
