//! ZMK Studio RPC framing (`zmk/app/src/studio/msg_framing.{h,c}`).
//!
//! A frame is `SOF payload EOF`. Inside the payload, any byte equal to SOF,
//! ESC or EOF is sent as `ESC byte`. The decoder is a small state machine
//! fed one read at a time, so frames may arrive split across reads (or
//! several frames in one read).

pub const SOF: u8 = 0xAB;
pub const ESC: u8 = 0xAC;
pub const EOF: u8 = 0xAD;

/// Wraps `payload` in a frame, escaping the three framing bytes.
pub fn encode(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 2);
    out.push(SOF);
    for &b in payload {
        if matches!(b, SOF | ESC | EOF) {
            out.push(ESC);
        }
        out.push(b);
    }
    out.push(EOF);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Between frames; bytes other than SOF are discarded.
    Idle,
    /// Inside a frame.
    Frame,
    /// Inside a frame, right after an ESC.
    Escaped,
}

/// Streaming frame decoder.
#[derive(Debug)]
pub struct Decoder {
    state: State,
    buf: Vec<u8>,
    /// Bytes thrown away outside a frame (noise, or a frame cut short).
    discarded: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            buf: Vec::new(),
            discarded: 0,
        }
    }

    /// Feeds bytes in and returns every frame they complete, in order.
    pub fn feed(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for &b in data {
            match self.state {
                State::Idle => {
                    if b == SOF {
                        self.state = State::Frame;
                        self.buf.clear();
                    } else {
                        self.discarded += 1;
                    }
                }
                State::Frame => match b {
                    ESC => self.state = State::Escaped,
                    EOF => {
                        frames.push(std::mem::take(&mut self.buf));
                        self.state = State::Idle;
                    }
                    // An unescaped SOF mid-frame: the previous frame was cut
                    // short. Drop it and treat this SOF as a new start. (The
                    // firmware instead enters an error state until the *next*
                    // SOF; on the host, restarting here loses less.)
                    SOF => {
                        self.discarded += self.buf.len();
                        self.buf.clear();
                    }
                    _ => self.buf.push(b),
                },
                State::Escaped => {
                    self.buf.push(b);
                    self.state = State::Frame;
                }
            }
        }
        frames
    }

    /// True while a frame has started but not ended.
    pub fn in_frame(&self) -> bool {
        self.state != State::Idle
    }

    /// Bytes discarded so far (outside frames or from truncated frames).
    pub fn discarded(&self) -> usize {
        self.discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_plain() {
        assert_eq!(encode(&[1, 2, 3]), vec![SOF, 1, 2, 3, EOF]);
    }

    #[test]
    fn encode_escapes_framing_bytes() {
        assert_eq!(
            encode(&[0xAB, 0x00, 0xAC, 0xAD]),
            vec![SOF, ESC, 0xAB, 0x00, ESC, 0xAC, ESC, 0xAD, EOF]
        );
    }

    #[test]
    fn encode_empty() {
        assert_eq!(encode(&[]), vec![SOF, EOF]);
    }

    #[test]
    fn round_trip_all_bytes() {
        let payload: Vec<u8> = (0..=255u8).chain([0xAB, 0xAC, 0xAD, 0xAC, 0xAC]).collect();
        let mut d = Decoder::new();
        assert_eq!(d.feed(&encode(&payload)), vec![payload]);
        assert!(!d.in_frame());
    }

    #[test]
    fn decode_split_byte_by_byte() {
        let payload = vec![0x08, 0xAB, 0xAC, 0xAD, 0x42];
        let wire = encode(&payload);
        let mut d = Decoder::new();
        let mut out = Vec::new();
        for b in &wire {
            out.extend(d.feed(std::slice::from_ref(b)));
        }
        assert_eq!(out, vec![payload]);
    }

    #[test]
    fn decode_split_on_escape_boundary() {
        // Split right after the ESC so the escaped byte arrives in the next read.
        let wire = encode(&[0x01, 0xAD, 0x02]);
        let esc_at = wire.iter().position(|&b| b == ESC).unwrap();
        let mut d = Decoder::new();
        assert!(d.feed(&wire[..=esc_at]).is_empty());
        assert!(d.in_frame());
        assert_eq!(d.feed(&wire[esc_at + 1..]), vec![vec![0x01, 0xAD, 0x02]]);
    }

    #[test]
    fn decode_several_frames_in_one_read_with_noise() {
        let mut wire = vec![0x00, 0xFF];
        wire.extend(encode(&[1]));
        wire.push(0x55);
        wire.extend(encode(&[2, 0xAB]));
        wire.extend(encode(&[]));
        let mut d = Decoder::new();
        assert_eq!(d.feed(&wire), vec![vec![1], vec![2, 0xAB], vec![]]);
        assert_eq!(d.discarded(), 3);
    }

    #[test]
    fn decode_restarts_on_unescaped_sof() {
        let mut wire = vec![SOF, 9, 9];
        wire.extend(encode(&[7]));
        let mut d = Decoder::new();
        assert_eq!(d.feed(&wire), vec![vec![7]]);
        assert_eq!(d.discarded(), 2);
    }
}
