use crate::{ChannelId, Error};

/// SIPC5 config start marker from `include/sipc5.h` in the pinned driver.
pub const SIPC5_START_MASK: u8 = 0xf8;
/// Mask for the three low SIPC5 config bits.
pub const SIPC5_CONFIG_MASK: u8 = 0x07;
/// Mask for the mode selector; the padding bit is outside this selector.
pub const SIPC5_EXT_FIELD_MASK: u8 = 0x03;
/// Indicates that an optional SIPC5 field follows the length.
pub const SIPC5_EXT_FIELD_EXIST: u8 = 0x02;
/// Indicates eight-byte alignment padding after the declared frame.
pub const SIPC5_PADDING_EXIST: u8 = 0x04;
/// Low config bits for a multi-frame control byte.
pub const SIPC5_MULTI_FRAME_CFG: u8 = 0x03;
/// Low config bits for an extended four-byte length.
pub const SIPC5_EXT_LENGTH_CFG: u8 = 0x02;
pub const SIPC5_MIN_HEADER_SIZE: usize = 4;
pub const SIPC5_HEADER_SIZE_WITH_CTL_FLD: usize = 5;
pub const SIPC5_HEADER_SIZE_WITH_EXT_LEN: usize = 6;
const SIPC5_ALIGNMENT: usize = 8;

/// Default bound for one decoded or encoded frame.
///
/// The kernel wire format permits a 32-bit extended length, but this crate is
/// intentionally bounded so an untrusted length field cannot request an
/// unreviewed allocation.  Callers with a justified larger frame can choose a
/// smaller or larger explicit bound with [`FrameCodec::new`].
pub const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// SIPC5 link-frame layout selected by the config low bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMode {
    /// Four-byte header and a 16-bit total frame length.
    Single,
    /// Five-byte header and a 16-bit total frame length.  The control byte is
    /// `(more << 7) | id`.
    Multi { id: u8, more: bool },
    /// Six-byte header and a 32-bit total frame length.
    Extended,
}

impl FrameMode {
    pub const fn header_len(self) -> usize {
        match self {
            Self::Single => SIPC5_MIN_HEADER_SIZE,
            Self::Multi { .. } => SIPC5_HEADER_SIZE_WITH_CTL_FLD,
            Self::Extended => SIPC5_HEADER_SIZE_WITH_EXT_LEN,
        }
    }

    const fn config_bits(self) -> u8 {
        match self {
            Self::Single => 0,
            Self::Multi { .. } => SIPC5_MULTI_FRAME_CFG,
            Self::Extended => SIPC5_EXT_LENGTH_CFG,
        }
    }
}

/// An owned SIPC5 frame without any modem-level interpretation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    channel: ChannelId,
    mode: FrameMode,
    padded: bool,
    payload: Vec<u8>,
}

impl Frame {
    /// Creates an unpadded single frame.
    pub fn new(channel: ChannelId, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            channel,
            mode: FrameMode::Single,
            padded: false,
            payload: payload.into(),
        }
    }

    pub fn with_mode(
        channel: ChannelId,
        payload: impl Into<Vec<u8>>,
        mode: FrameMode,
    ) -> Result<Self, Error> {
        validate_mode(mode)?;
        Ok(Self {
            channel,
            mode,
            padded: false,
            payload: payload.into(),
        })
    }

    pub fn multi(
        channel: ChannelId,
        payload: impl Into<Vec<u8>>,
        id: u8,
        more: bool,
    ) -> Result<Self, Error> {
        Self::with_mode(channel, payload, FrameMode::Multi { id, more })
    }

    pub fn extended(channel: ChannelId, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            channel,
            mode: FrameMode::Extended,
            padded: false,
            payload: payload.into(),
        }
    }

    pub fn padded(mut self, padded: bool) -> Self {
        self.padded = padded;
        self
    }

    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    pub const fn mode(&self) -> FrameMode {
        self.mode
    }

    pub const fn is_padded(&self) -> bool {
        self.padded
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        FrameCodec::default().encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        FrameCodec::default().decode(bytes)
    }
}

/// A bounded SIPC5 codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCodec {
    max_frame_size: usize,
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self {
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
        }
    }
}

impl FrameCodec {
    pub fn new(max_frame_size: usize) -> Result<Self, Error> {
        if !(SIPC5_MIN_HEADER_SIZE..=u32::MAX as usize).contains(&max_frame_size) {
            return Err(Error::InvalidMaximumFrameSize {
                maximum: max_frame_size,
            });
        }
        Ok(Self { max_frame_size })
    }

    pub const fn max_frame_size(self) -> usize {
        self.max_frame_size
    }

    pub fn encode(&self, frame: &Frame) -> Result<Vec<u8>, Error> {
        validate_mode(frame.mode)?;
        let header_len = frame.mode.header_len();
        let declared_len = header_len
            .checked_add(frame.payload.len())
            .ok_or(Error::LengthOverflow)?;
        self.check_length(declared_len)?;

        match frame.mode {
            FrameMode::Single | FrameMode::Multi { .. } => {
                if declared_len > u16::MAX as usize {
                    return Err(Error::FrameTooLarge {
                        length: declared_len,
                        maximum: u16::MAX as usize,
                    });
                }
            }
            FrameMode::Extended => {
                if declared_len > u32::MAX as usize {
                    return Err(Error::FrameTooLarge {
                        length: declared_len,
                        maximum: u32::MAX as usize,
                    });
                }
            }
        }

        let total_len = if frame.padded {
            padded_len(declared_len).ok_or(Error::LengthOverflow)?
        } else {
            declared_len
        };
        let config = SIPC5_START_MASK
            | frame.mode.config_bits()
            | if frame.padded { SIPC5_PADDING_EXIST } else { 0 };
        let mut output = Vec::with_capacity(total_len);
        output.push(config);
        output.push(frame.channel.get());
        match frame.mode {
            FrameMode::Single | FrameMode::Multi { .. } => {
                output.extend_from_slice(&(declared_len as u16).to_le_bytes());
            }
            FrameMode::Extended => {
                output.extend_from_slice(&(declared_len as u32).to_le_bytes());
            }
        }
        if let FrameMode::Multi { id, more } = frame.mode {
            output.push(id | if more { 0x80 } else { 0 });
        }
        output.extend_from_slice(&frame.payload);
        output.resize(total_len, 0);
        Ok(output)
    }

    /// Parses one frame and rejects any trailing bytes.
    pub fn decode(&self, bytes: &[u8]) -> Result<Frame, Error> {
        let parsed = self.parse_prefix(bytes)?;
        if parsed.consumed != bytes.len() {
            return Err(Error::TrailingBytes {
                expected: parsed.consumed,
                actual: bytes.len(),
            });
        }
        Ok(parsed.frame)
    }

    /// Parses the first frame in a byte stream and reports the number of
    /// bytes consumed, including optional alignment padding.
    pub fn parse_prefix(&self, bytes: &[u8]) -> Result<ParsedFrame, Error> {
        if bytes.len() < SIPC5_MIN_HEADER_SIZE {
            return Err(Error::Truncated {
                needed: SIPC5_MIN_HEADER_SIZE,
                available: bytes.len(),
            });
        }

        let config = bytes[0];
        if config & SIPC5_START_MASK != SIPC5_START_MASK {
            return Err(Error::InvalidStart { config });
        }

        let mode_bits = config & SIPC5_EXT_FIELD_MASK;
        let (header_len, declared_len, mode) = match mode_bits {
            0 => (
                SIPC5_MIN_HEADER_SIZE,
                u16::from_le_bytes([bytes[2], bytes[3]]) as usize,
                FrameMode::Single,
            ),
            SIPC5_MULTI_FRAME_CFG => {
                if bytes.len() < SIPC5_HEADER_SIZE_WITH_CTL_FLD {
                    return Err(Error::Truncated {
                        needed: SIPC5_HEADER_SIZE_WITH_CTL_FLD,
                        available: bytes.len(),
                    });
                }
                let control = bytes[4];
                (
                    SIPC5_HEADER_SIZE_WITH_CTL_FLD,
                    u16::from_le_bytes([bytes[2], bytes[3]]) as usize,
                    FrameMode::Multi {
                        id: control & 0x7f,
                        more: control & 0x80 != 0,
                    },
                )
            }
            SIPC5_EXT_LENGTH_CFG => {
                if bytes.len() < SIPC5_HEADER_SIZE_WITH_EXT_LEN {
                    return Err(Error::Truncated {
                        needed: SIPC5_HEADER_SIZE_WITH_EXT_LEN,
                        available: bytes.len(),
                    });
                }
                (
                    SIPC5_HEADER_SIZE_WITH_EXT_LEN,
                    u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize,
                    FrameMode::Extended,
                )
            }
            config => return Err(Error::UnsupportedConfig { config }),
        };

        if declared_len < header_len {
            return Err(Error::InvalidLength {
                declared: declared_len,
                header: header_len,
            });
        }
        self.check_length(declared_len)?;
        if bytes.len() < header_len {
            return Err(Error::Truncated {
                needed: header_len,
                available: bytes.len(),
            });
        }
        let consumed = if config & SIPC5_PADDING_EXIST != 0 {
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

        Ok(ParsedFrame {
            frame: Frame {
                channel: ChannelId::new(bytes[1]),
                mode,
                padded: config & SIPC5_PADDING_EXIST != 0,
                payload: bytes[header_len..declared_len].to_vec(),
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

/// A parsed frame plus its exact wire footprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFrame {
    frame: Frame,
    consumed: usize,
}

impl ParsedFrame {
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub fn into_frame(self) -> Frame {
        self.frame
    }

    pub const fn consumed(&self) -> usize {
        self.consumed
    }
}

fn validate_mode(mode: FrameMode) -> Result<(), Error> {
    if let FrameMode::Multi { id, .. } = mode
        && id & 0x80 != 0
    {
        return Err(Error::InvalidMultiFrameId { id });
    }
    Ok(())
}

fn padded_len(length: usize) -> Option<usize> {
    let remainder = length % SIPC5_ALIGNMENT;
    if remainder == 0 {
        Some(length)
    } else {
        length.checked_add(SIPC5_ALIGNMENT - remainder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codec() -> FrameCodec {
        FrameCodec::new(1024).unwrap()
    }

    #[test]
    fn encodes_and_decodes_single_fixture() {
        let frame = Frame::new(ChannelId::new(7), [0x10, 0x20, 0x30]);
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes, [0xf8, 0x07, 0x07, 0x00, 0x10, 0x20, 0x30]);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn encodes_eight_byte_padding_from_kernel_alignment_rule() {
        let frame = Frame::new(ChannelId::new(7), [0xaa, 0xbb, 0xcc]).padded(true);
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes, [0xfc, 0x07, 0x07, 0x00, 0xaa, 0xbb, 0xcc, 0x00]);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn encodes_multi_fixture_with_more_bit_and_identifier() {
        let frame = Frame::multi(ChannelId::new(0x12), [0xaa, 0xbb], 9, true).unwrap();
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes, [0xfb, 0x12, 0x07, 0x00, 0x89, 0xaa, 0xbb]);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn encodes_extended_length_fixture() {
        let frame = Frame::extended(ChannelId::new(0x34), [1, 2, 3, 4]);
        let bytes = frame.encode().unwrap();
        assert_eq!(bytes, [0xfa, 0x34, 0x0a, 0x00, 0x00, 0x00, 1, 2, 3, 4]);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn parses_one_frame_from_a_stream_without_accepting_stream_as_one_frame() {
        let first = Frame::new(ChannelId::new(7), [1, 2]).encode().unwrap();
        let second = Frame::new(ChannelId::new(8), [3]).encode().unwrap();
        let mut stream = first.clone();
        stream.extend_from_slice(&second);

        let parsed = codec().parse_prefix(&stream).unwrap();
        assert_eq!(parsed.consumed(), first.len());
        assert_eq!(parsed.frame().payload(), [1, 2]);
        assert_eq!(
            codec().decode(&stream),
            Err(Error::TrailingBytes {
                expected: first.len(),
                actual: stream.len(),
            })
        );
    }

    #[test]
    fn rejects_malformed_and_ambiguous_fixtures() {
        assert_eq!(
            codec().decode(&[]),
            Err(Error::Truncated {
                needed: 4,
                available: 0
            })
        );
        assert_eq!(
            codec().decode(&[0xfb, 7, 5, 0]),
            Err(Error::Truncated {
                needed: SIPC5_HEADER_SIZE_WITH_CTL_FLD,
                available: 4
            })
        );
        assert_eq!(
            codec().decode(&[0xfa, 7, 6, 0]),
            Err(Error::Truncated {
                needed: SIPC5_HEADER_SIZE_WITH_EXT_LEN,
                available: 4
            })
        );
        assert_eq!(
            codec().decode(&[0, 7, 4, 0]),
            Err(Error::InvalidStart { config: 0 })
        );
        assert_eq!(
            codec().decode(&[0xf9, 7, 4, 0]),
            Err(Error::UnsupportedConfig { config: 1 })
        );
        assert_eq!(
            codec().decode(&[0xf8, 7, 3, 0]),
            Err(Error::InvalidLength {
                declared: 3,
                header: 4,
            })
        );
        assert_eq!(
            Frame::multi(ChannelId::new(7), [], 0x80, false),
            Err(Error::InvalidMultiFrameId { id: 0x80 })
        );
    }

    #[test]
    fn enforces_configured_length_bound_before_allocating_payload() {
        let codec = FrameCodec::new(8).unwrap();
        assert_eq!(
            codec.decode(&[0xfa, 7, 0xff, 0xff, 0xff, 0xff]),
            Err(Error::FrameTooLarge {
                length: u32::MAX as usize,
                maximum: 8,
            })
        );
    }

    #[test]
    fn round_trips_deterministic_mode_table() {
        let cases = [
            (FrameMode::Single, false),
            (FrameMode::Single, true),
            (FrameMode::Multi { id: 0, more: false }, false),
            (
                FrameMode::Multi {
                    id: 127,
                    more: true,
                },
                true,
            ),
            (FrameMode::Extended, false),
            (FrameMode::Extended, true),
        ];
        for (mode, padded) in cases {
            let frame = Frame::with_mode(ChannelId::new(42), [0, 1, 2, 3], mode)
                .unwrap()
                .padded(padded);
            assert_eq!(
                codec().decode(&codec().encode(&frame).unwrap()).unwrap(),
                frame
            );
        }
    }
}
