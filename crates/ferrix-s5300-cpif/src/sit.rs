use crate::{ChannelId, Error};

/// Fixed Exynos SIT sync word from `include/exynos_ipc.h` in the pinned
/// Google S5300 source. Fields are little-endian on the target's AArch64
/// kernel.
pub const EXYNOS_START: u16 = 0xabcd;
pub const EXYNOS_HEADER_SIZE: usize = 12;
pub const EXYNOS_SINGLE_MASK: u8 = 0xc0;
pub const EXYNOS_MULTI_START_MASK: u8 = 0x80;
pub const EXYNOS_MULTI_LAST_MASK: u8 = 0x40;
pub const EXYNOS_MULTI_PACKET_MASK: u8 = 0x3f;
pub const EXYNOS_MAX_FRAME_SIZE: usize = u16::MAX as usize;
const EXYNOS_ALIGNMENT: usize = 8;

/// Exynos SIT fragment configuration as emitted by `exynos_build_header`.
///
/// `frame_index` is the raw low byte in the kernel's fragment config. It is a
/// remaining-frame value for non-final fragments and is zero on the final
/// fragment in the pinned implementation. The final fragment also clears the
/// start marker. The codec preserves these fields instead of trying to
/// reassemble modem payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SitFrameMode {
    Single,
    Fragment {
        packet_index: u8,
        frame_index: u8,
        last: bool,
    },
}

impl SitFrameMode {
    fn config(self) -> u16 {
        match self {
            Self::Single => u16::from(EXYNOS_SINGLE_MASK) << 8,
            Self::Fragment {
                packet_index,
                frame_index,
                last,
            } => {
                let flags = if last {
                    EXYNOS_MULTI_LAST_MASK
                } else {
                    EXYNOS_MULTI_START_MASK
                } | packet_index;
                u16::from(frame_index) | (u16::from(flags) << 8)
            }
        }
    }
}

/// An Exynos SIT link frame. This is a kernel-link representation only; the
/// S5300 IPC character device removes this header before userspace reads.
///
/// `padded` is an explicit serialization choice. SIT has no padding bit in
/// its header: the pinned kernel adds padding only when the link's separate
/// `aligned` property is enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SitFrame {
    channel: ChannelId,
    sequence: u16,
    channel_sequence: u8,
    mode: SitFrameMode,
    padded: bool,
    payload: Vec<u8>,
}

impl SitFrame {
    pub fn new(channel: ChannelId, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            channel,
            sequence: 0,
            channel_sequence: 0,
            mode: SitFrameMode::Single,
            padded: false,
            payload: payload.into(),
        }
    }

    pub fn with_mode(
        channel: ChannelId,
        payload: impl Into<Vec<u8>>,
        mode: SitFrameMode,
    ) -> Result<Self, Error> {
        validate_mode(mode)?;
        Ok(Self {
            channel,
            sequence: 0,
            channel_sequence: 0,
            mode,
            padded: false,
            payload: payload.into(),
        })
    }

    pub fn fragment(
        channel: ChannelId,
        payload: impl Into<Vec<u8>>,
        packet_index: u8,
        frame_index: u8,
        last: bool,
    ) -> Result<Self, Error> {
        Self::with_mode(
            channel,
            payload,
            SitFrameMode::Fragment {
                packet_index,
                frame_index,
                last,
            },
        )
    }

    pub const fn sequence(&self) -> u16 {
        self.sequence
    }

    pub const fn channel_sequence(&self) -> u8 {
        self.channel_sequence
    }

    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    pub const fn mode(&self) -> SitFrameMode {
        self.mode
    }

    pub const fn is_padded(&self) -> bool {
        self.padded
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn with_sequence(mut self, sequence: u16) -> Self {
        self.sequence = sequence;
        self
    }

    pub fn with_channel_sequence(mut self, sequence: u8) -> Self {
        self.channel_sequence = sequence;
        self
    }

    pub fn padded(mut self, padded: bool) -> Self {
        self.padded = padded;
        self
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        SitFrameCodec::default().encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        SitFrameCodec::default().decode(bytes)
    }

    /// Decodes a frame when the kernel link is known to use eight-byte
    /// alignment. SIT does not carry this fact in its header, so callers must
    /// provide it explicitly.
    pub fn decode_aligned(bytes: &[u8]) -> Result<Self, Error> {
        SitFrameCodec::default().with_alignment(true).decode(bytes)
    }
}

/// A bounded codec for the 12-byte Exynos SIT link header used by the pinned
/// tegu CPIF DTS (`mif,protocol = PROTOCOL_SIT`).
///
/// Decoding defaults to the unaligned link mode. Call [`Self::with_alignment`]
/// only when the kernel link configuration is known to set `ld->aligned`; the
/// SIT header itself cannot tell a parser whether the following bytes are
/// padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SitFrameCodec {
    max_frame_size: usize,
    aligned: bool,
}

impl Default for SitFrameCodec {
    fn default() -> Self {
        Self {
            max_frame_size: EXYNOS_MAX_FRAME_SIZE,
            aligned: false,
        }
    }
}

impl SitFrameCodec {
    pub fn new(max_frame_size: usize) -> Result<Self, Error> {
        if !(EXYNOS_HEADER_SIZE..=EXYNOS_MAX_FRAME_SIZE).contains(&max_frame_size) {
            return Err(Error::InvalidMaximumFrameSize {
                maximum: max_frame_size,
            });
        }
        Ok(Self {
            max_frame_size,
            aligned: false,
        })
    }

    pub const fn max_frame_size(self) -> usize {
        self.max_frame_size
    }

    /// Configures the link-alignment context used while decoding.
    ///
    /// The pinned kernel's `ld->aligned` flag is outside the SIT header. When
    /// it is true, the kernel's total-frame calculation rounds the declared
    /// length up to eight bytes; when it is false, the next byte is the next
    /// frame. Keeping this context explicit avoids consuming the first byte
    /// of a following frame as guessed padding.
    pub const fn with_alignment(mut self, aligned: bool) -> Self {
        self.aligned = aligned;
        self
    }

    pub const fn is_aligned(self) -> bool {
        self.aligned
    }

    pub fn encode(&self, frame: &SitFrame) -> Result<Vec<u8>, Error> {
        validate_mode(frame.mode)?;
        let declared_len = EXYNOS_HEADER_SIZE
            .checked_add(frame.payload.len())
            .ok_or(Error::LengthOverflow)?;
        self.check_length(declared_len)?;
        if declared_len > EXYNOS_MAX_FRAME_SIZE {
            return Err(Error::FrameTooLarge {
                length: declared_len,
                maximum: EXYNOS_MAX_FRAME_SIZE,
            });
        }
        let total_len = if frame.padded {
            padded_len(declared_len).ok_or(Error::LengthOverflow)?
        } else {
            declared_len
        };
        let mut output = Vec::with_capacity(total_len);
        output.extend_from_slice(&EXYNOS_START.to_le_bytes());
        output.extend_from_slice(&frame.sequence.to_le_bytes());
        output.extend_from_slice(&frame.mode.config().to_le_bytes());
        output.extend_from_slice(&(declared_len as u16).to_le_bytes());
        output.push(frame.channel.get());
        output.push(frame.channel_sequence);
        // The kernel uses a 12-byte header area although the named fields
        // occupy offsets 0 through 9. Keep the remaining bytes deterministic.
        output.extend_from_slice(&[0, 0]);
        output.extend_from_slice(&frame.payload);
        output.resize(total_len, 0);
        Ok(output)
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<SitFrame, Error> {
        let parsed = self.parse_prefix(bytes)?;
        if parsed.consumed != bytes.len() {
            return Err(Error::TrailingBytes {
                expected: parsed.consumed,
                actual: bytes.len(),
            });
        }
        Ok(parsed.frame)
    }

    pub fn parse_prefix(&self, bytes: &[u8]) -> Result<SitParsedFrame, Error> {
        if bytes.len() < EXYNOS_HEADER_SIZE {
            return Err(Error::Truncated {
                needed: EXYNOS_HEADER_SIZE,
                available: bytes.len(),
            });
        }
        let sync = u16::from_le_bytes([bytes[0], bytes[1]]);
        if sync != EXYNOS_START {
            return Err(Error::InvalidSitStart { sync });
        }
        let sequence = u16::from_le_bytes([bytes[2], bytes[3]]);
        let config = u16::from_le_bytes([bytes[4], bytes[5]]);
        let declared_len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        if declared_len < EXYNOS_HEADER_SIZE {
            return Err(Error::InvalidLength {
                declared: declared_len,
                header: EXYNOS_HEADER_SIZE,
            });
        }
        self.check_length(declared_len)?;
        let mode = parse_mode(config)?;
        let consumed = if self.aligned {
            padded_len(declared_len).ok_or(Error::LengthOverflow)?
        } else {
            declared_len
        };
        if bytes.len() < consumed {
            return Err(Error::Truncated {
                needed: consumed,
                available: bytes.len(),
            });
        }
        Ok(SitParsedFrame {
            frame: SitFrame {
                channel: ChannelId::new(bytes[8]),
                sequence,
                channel_sequence: bytes[9],
                mode,
                padded: self.aligned && consumed > declared_len,
                payload: bytes[EXYNOS_HEADER_SIZE..declared_len].to_vec(),
            },
            consumed,
        })
    }

    fn check_length(&self, length: usize) -> Result<(), Error> {
        if length > self.max_frame_size {
            return Err(Error::FrameTooLarge {
                length,
                maximum: self.max_frame_size,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SitParsedFrame {
    frame: SitFrame,
    consumed: usize,
}

impl SitParsedFrame {
    pub fn frame(&self) -> &SitFrame {
        &self.frame
    }

    pub fn into_frame(self) -> SitFrame {
        self.frame
    }

    pub const fn consumed(&self) -> usize {
        self.consumed
    }
}

fn validate_mode(mode: SitFrameMode) -> Result<(), Error> {
    if let SitFrameMode::Fragment {
        packet_index,
        frame_index,
        last,
    } = mode
    {
        if packet_index > EXYNOS_MULTI_PACKET_MASK {
            return Err(Error::UnsupportedSitConfig {
                config: (u16::from(packet_index) << 8) | u16::from(frame_index),
            });
        }
        if !last && frame_index == 0 {
            return Err(Error::UnsupportedSitConfig {
                config: (u16::from(EXYNOS_MULTI_START_MASK | packet_index) << 8)
                    | u16::from(frame_index),
            });
        }
        if last && frame_index != 0 {
            return Err(Error::UnsupportedSitConfig {
                config: (u16::from(EXYNOS_MULTI_LAST_MASK | packet_index) << 8)
                    | u16::from(frame_index),
            });
        }
    }
    Ok(())
}

fn parse_mode(config: u16) -> Result<SitFrameMode, Error> {
    let flags = (config >> 8) as u8;
    let frame_index = config as u8;
    // The kernel tests the single-frame mask on the high byte and does not
    // inspect the low byte when classifying a frame. Preserve that source
    // classification rather than turning a non-zero low byte into a
    // synthetic fragment.
    if flags & EXYNOS_SINGLE_MASK == EXYNOS_SINGLE_MASK {
        return Ok(SitFrameMode::Single);
    }
    if flags & (EXYNOS_MULTI_START_MASK | EXYNOS_MULTI_LAST_MASK) == 0 {
        return Err(Error::UnsupportedSitConfig { config });
    }
    let packet_index = flags & EXYNOS_MULTI_PACKET_MASK;
    let last = flags & EXYNOS_MULTI_LAST_MASK != 0;
    let mode = SitFrameMode::Fragment {
        packet_index,
        frame_index,
        last,
    };
    validate_mode(mode)?;
    Ok(mode)
}

fn padded_len(length: usize) -> Option<usize> {
    let remainder = length % EXYNOS_ALIGNMENT;
    if remainder == 0 {
        Some(length)
    } else {
        length.checked_add(EXYNOS_ALIGNMENT - remainder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_single_sit_fixture() {
        let frame = SitFrame::new(ChannelId::new(0), [1, 2, 3])
            .with_sequence(0x1234)
            .with_channel_sequence(9);
        let bytes = frame.encode().unwrap();
        assert_eq!(
            bytes,
            [
                0xcd, 0xab, 0x34, 0x12, 0x00, 0xc0, 0x0f, 0x00, 0x00, 0x09, 0x00, 0x00, 1, 2, 3
            ]
        );
        assert_eq!(SitFrame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn round_trips_fragment_and_preserves_raw_config() {
        let frame = SitFrame::fragment(ChannelId::new(1), [4, 5], 7, 2, false).unwrap();
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes[4..6], [0x02, 0x87]);
        assert_eq!(SitFrame::decode(&bytes).unwrap(), frame);

        let last = SitFrame::fragment(ChannelId::new(1), [6], 7, 0, true).unwrap();
        let last_bytes = last.encode().unwrap();
        assert_eq!(last_bytes[4..6], [0x00, 0x47]);
        assert_eq!(SitFrame::decode(&last_bytes).unwrap(), last);
    }

    #[test]
    fn parses_eight_byte_alignment_padding() {
        let frame = SitFrame::new(ChannelId::new(1), [1]).padded(true);
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes.len(), 16);
        let codec = SitFrameCodec::default().with_alignment(true);
        assert!(codec.is_aligned());
        assert_eq!(codec.decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn parses_a_prefix_without_accepting_the_following_frame() {
        let first = SitFrame::new(ChannelId::new(1), [1, 2, 3])
            .encode()
            .unwrap();
        let second = SitFrame::new(ChannelId::new(2), [5]).encode().unwrap();
        let mut stream = first.clone();
        stream.extend_from_slice(&second);
        let parsed = SitFrameCodec::default().parse_prefix(&stream).unwrap();
        assert_eq!(parsed.consumed(), first.len());
        assert_eq!(parsed.frame().channel(), ChannelId::new(1));
        assert_eq!(
            SitFrameCodec::default().decode(&stream),
            Err(Error::TrailingBytes {
                expected: first.len(),
                actual: stream.len(),
            })
        );
    }

    #[test]
    fn rejects_bad_sync_lengths_and_fragment_config() {
        assert_eq!(
            SitFrame::decode(&[0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0]),
            Err(Error::InvalidSitStart { sync: 0 })
        );
        assert_eq!(
            SitFrame::decode(&[0xcd, 0xab, 0, 0, 0xc0, 0, 11, 0, 0, 0, 0, 0]),
            Err(Error::InvalidLength {
                declared: 11,
                header: EXYNOS_HEADER_SIZE,
            })
        );
        let wrapped_packet = SitFrame::fragment(ChannelId::new(1), [], 0, 1, false).unwrap();
        assert_eq!(
            wrapped_packet.mode(),
            SitFrameMode::Fragment {
                packet_index: 0,
                frame_index: 1,
                last: false,
            }
        );
        assert_eq!(
            SitFrame::fragment(ChannelId::new(1), [], 7, 1, true),
            Err(Error::UnsupportedSitConfig { config: 0x4701 })
        );
    }

    #[test]
    fn does_not_infer_alignment_from_available_stream_bytes() {
        let first = SitFrame::new(ChannelId::new(1), [1]).encode().unwrap();
        let second = SitFrame::new(ChannelId::new(2), [2]).encode().unwrap();
        let mut stream = first.clone();
        stream.extend_from_slice(&second);

        let parsed = SitFrameCodec::default().parse_prefix(&stream).unwrap();
        assert_eq!(parsed.consumed(), first.len());
        assert!(!parsed.frame().is_padded());
    }
}
