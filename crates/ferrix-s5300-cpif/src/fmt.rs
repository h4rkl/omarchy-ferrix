use crate::{Error, PayloadTransport};

/// The packed userspace FMT header exposed by the S5300 IPC character device.
///
/// The kernel removes the SIT link header before userspace reads a packet and
/// adds it again around writes.  FMT therefore starts with its own seven-byte
/// little-endian header at the [`PayloadTransport`] boundary.
pub const FMT_HEADER_SIZE: usize = 7;
/// The FMT length field is a little-endian `u16` containing the complete FMT
/// packet length, including [`FMT_HEADER_SIZE`].
pub const FMT_MAX_WIRE_SIZE: usize = u16::MAX as usize;
/// Default parser/encoder bound.  This is both protocol-safe (the wire field
/// is `u16`) and small enough to prevent an untrusted length from allocating
/// an unreviewed userspace buffer.
pub const DEFAULT_MAX_FMT_SIZE: usize = FMT_MAX_WIRE_SIZE;

/// A bounded userspace FMT packet.
///
/// `main_cmd`, `sub_cmd`, and `cmd_type` are intentionally preserved as raw
/// bytes.  The open S5300 kernel source defines this envelope but does not
/// publish the firmware-specific command vocabulary, so this type does not
/// guess meanings or implement a RIL/Android command map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FmtPacket {
    msg_seq: u8,
    ack_seq: u8,
    main_cmd: u8,
    sub_cmd: u8,
    cmd_type: u8,
    payload: Vec<u8>,
}

impl FmtPacket {
    /// Creates a packet with the raw command tuple and an optional body.
    pub fn new(
        msg_seq: u8,
        ack_seq: u8,
        main_cmd: u8,
        sub_cmd: u8,
        cmd_type: u8,
        payload: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            msg_seq,
            ack_seq,
            main_cmd,
            sub_cmd,
            cmd_type,
            payload: payload.into(),
        }
    }

    pub const fn msg_seq(&self) -> u8 {
        self.msg_seq
    }

    pub const fn ack_seq(&self) -> u8 {
        self.ack_seq
    }

    pub const fn main_cmd(&self) -> u8 {
        self.main_cmd
    }

    pub const fn sub_cmd(&self) -> u8 {
        self.sub_cmd
    }

    pub const fn cmd_type(&self) -> u8 {
        self.cmd_type
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Encodes using [`DEFAULT_MAX_FMT_SIZE`].
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        FmtCodec::default().encode(self)
    }

    /// Decodes one complete packet using [`DEFAULT_MAX_FMT_SIZE`].
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        FmtCodec::default().decode(bytes)
    }
}

/// A bounded codec for the seven-byte userspace FMT envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FmtCodec {
    max_fmt_size: usize,
}

impl Default for FmtCodec {
    fn default() -> Self {
        Self {
            max_fmt_size: DEFAULT_MAX_FMT_SIZE,
        }
    }
}

impl FmtCodec {
    pub fn new(max_fmt_size: usize) -> Result<Self, Error> {
        if !(FMT_HEADER_SIZE..=FMT_MAX_WIRE_SIZE).contains(&max_fmt_size) {
            return Err(Error::InvalidMaximumFrameSize {
                maximum: max_fmt_size,
            });
        }
        Ok(Self { max_fmt_size })
    }

    pub const fn max_fmt_size(self) -> usize {
        self.max_fmt_size
    }

    pub fn encode(&self, packet: &FmtPacket) -> Result<Vec<u8>, Error> {
        let declared_len = FMT_HEADER_SIZE
            .checked_add(packet.payload.len())
            .ok_or(Error::LengthOverflow)?;
        self.check_length(declared_len)?;
        if declared_len > FMT_MAX_WIRE_SIZE {
            return Err(Error::FrameTooLarge {
                length: declared_len,
                maximum: FMT_MAX_WIRE_SIZE,
            });
        }

        let mut output = Vec::with_capacity(declared_len);
        output.extend_from_slice(&(declared_len as u16).to_le_bytes());
        output.extend_from_slice(&[
            packet.msg_seq,
            packet.ack_seq,
            packet.main_cmd,
            packet.sub_cmd,
            packet.cmd_type,
        ]);
        output.extend_from_slice(&packet.payload);
        Ok(output)
    }

    /// Decodes exactly one FMT packet and rejects trailing bytes.
    pub fn decode(&self, bytes: &[u8]) -> Result<FmtPacket, Error> {
        if bytes.len() < FMT_HEADER_SIZE {
            return Err(Error::Truncated {
                needed: FMT_HEADER_SIZE,
                available: bytes.len(),
            });
        }

        let declared_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        if declared_len < FMT_HEADER_SIZE {
            return Err(Error::InvalidLength {
                declared: declared_len,
                header: FMT_HEADER_SIZE,
            });
        }
        self.check_length(declared_len)?;
        if bytes.len() < declared_len {
            return Err(Error::Truncated {
                needed: declared_len,
                available: bytes.len(),
            });
        }
        if bytes.len() != declared_len {
            return Err(Error::TrailingBytes {
                expected: declared_len,
                actual: bytes.len(),
            });
        }

        Ok(FmtPacket {
            msg_seq: bytes[2],
            ack_seq: bytes[3],
            main_cmd: bytes[4],
            sub_cmd: bytes[5],
            cmd_type: bytes[6],
            payload: bytes[FMT_HEADER_SIZE..].to_vec(),
        })
    }

    fn check_length(&self, length: usize) -> Result<(), Error> {
        if length > self.max_fmt_size {
            return Err(Error::FrameTooLarge {
                length,
                maximum: self.max_fmt_size,
            });
        }
        Ok(())
    }
}

/// Applies the FMT envelope to an explicitly supplied CPIF payload
/// transport.  The inner transport still owns device access and CPIF link
/// framing; this wrapper only encodes/decodes the userspace packet boundary.
/// It does not discover device nodes, boot the CP, or interpret commands.
pub struct FmtTransport<T> {
    inner: T,
    codec: FmtCodec,
    /// Bytes retained when the payload transport returns only part of the
    /// next FMT packet.  The buffer is bounded by `codec.max_fmt_size()`
    /// before any bytes are appended.
    receive_buffer: Vec<u8>,
}

impl<T> FmtTransport<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            codec: FmtCodec::default(),
            receive_buffer: Vec::new(),
        }
    }

    pub fn with_codec(inner: T, codec: FmtCodec) -> Self {
        Self {
            inner,
            codec,
            receive_buffer: Vec::new(),
        }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub const fn codec(&self) -> FmtCodec {
        self.codec
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub const fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: PayloadTransport> FmtTransport<T> {
    pub fn send_packet(&mut self, packet: &FmtPacket) -> Result<(), Error> {
        let encoded = self.codec.encode(packet)?;
        self.inner.send_payload(&encoded)
    }

    pub fn receive_packet(&mut self) -> Result<FmtPacket, Error> {
        loop {
            if self.receive_buffer.len() >= FMT_HEADER_SIZE {
                let declared_len =
                    u16::from_le_bytes([self.receive_buffer[0], self.receive_buffer[1]]) as usize;
                if declared_len < FMT_HEADER_SIZE {
                    return Err(Error::InvalidLength {
                        declared: declared_len,
                        header: FMT_HEADER_SIZE,
                    });
                }
                self.codec.check_length(declared_len)?;

                if self.receive_buffer.len() >= declared_len {
                    // Decode exactly the declared packet.  A payload
                    // transport is allowed to return a short chunk, but a
                    // concatenated second packet remains an ambiguity at
                    // this boundary and is rejected by the strict codec.
                    let packet = self.codec.decode(&self.receive_buffer)?;
                    self.receive_buffer.clear();
                    return Ok(packet);
                }
            }

            let payload = self.inner.receive_payload()?;
            if payload.is_empty() {
                return Err(Error::NoData);
            }
            let new_len = self
                .receive_buffer
                .len()
                .checked_add(payload.len())
                .ok_or(Error::LengthOverflow)?;
            if new_len > self.codec.max_fmt_size() {
                return Err(Error::FrameTooLarge {
                    length: new_len,
                    maximum: self.codec.max_fmt_size(),
                });
            }
            self.receive_buffer.extend_from_slice(&payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct FakePayloadTransport {
        sent: Vec<Vec<u8>>,
        received: VecDeque<Vec<u8>>,
    }

    impl PayloadTransport for FakePayloadTransport {
        fn send_payload(&mut self, payload: &[u8]) -> Result<(), Error> {
            self.sent.push(payload.to_vec());
            Ok(())
        }

        fn receive_payload(&mut self) -> Result<Vec<u8>, Error> {
            self.received.pop_front().ok_or(Error::NoData)
        }
    }

    #[test]
    fn packet_round_trip_preserves_raw_header_and_body() {
        let packet = FmtPacket::new(1, 2, 3, 4, 5, [0xaa, 0xbb]);
        let wire = packet.encode().unwrap();
        assert_eq!(wire, [9, 0, 1, 2, 3, 4, 5, 0xaa, 0xbb]);
        assert_eq!(FmtPacket::decode(&wire).unwrap(), packet);
    }

    #[test]
    fn empty_body_is_a_valid_seven_byte_packet() {
        let packet = FmtPacket::new(0, 0, 0x10, 0x20, 0x30, []);
        assert_eq!(packet.encode().unwrap().len(), FMT_HEADER_SIZE);
        assert_eq!(
            FmtPacket::decode(&packet.encode().unwrap()).unwrap(),
            packet
        );
    }

    #[test]
    fn malformed_lengths_are_rejected() {
        assert_eq!(
            FmtPacket::decode(&[0, 0, 0, 0, 0, 0, 0]),
            Err(Error::InvalidLength {
                declared: 0,
                header: FMT_HEADER_SIZE,
            })
        );
        assert_eq!(
            FmtPacket::decode(&[8, 0, 0, 0, 0, 0, 0]),
            Err(Error::Truncated {
                needed: 8,
                available: FMT_HEADER_SIZE,
            })
        );
        let mut trailing = vec![7, 0, 0, 0, 0, 0, 0];
        trailing.push(0xff);
        assert_eq!(
            FmtPacket::decode(&trailing),
            Err(Error::TrailingBytes {
                expected: FMT_HEADER_SIZE,
                actual: FMT_HEADER_SIZE + 1,
            })
        );
    }

    #[test]
    fn codec_bound_is_enforced_before_encoding() {
        let codec = FmtCodec::new(FMT_HEADER_SIZE).unwrap();
        let packet = FmtPacket::new(0, 0, 0, 0, 0, [1]);
        assert_eq!(
            codec.encode(&packet),
            Err(Error::FrameTooLarge {
                length: FMT_HEADER_SIZE + 1,
                maximum: FMT_HEADER_SIZE,
            })
        );
        assert_eq!(
            FmtCodec::new(FMT_HEADER_SIZE - 1),
            Err(Error::InvalidMaximumFrameSize {
                maximum: FMT_HEADER_SIZE - 1,
            })
        );
    }

    #[test]
    fn transport_wraps_only_the_payload_boundary() {
        let packet = FmtPacket::new(7, 8, 9, 10, 11, [0x55]);
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(packet.encode().unwrap());
        let mut transport = FmtTransport::new(fake);

        transport.send_packet(&packet).unwrap();
        assert_eq!(transport.receive_packet().unwrap(), packet);
        let fake = transport.into_inner();
        assert_eq!(fake.sent, vec![packet.encode().unwrap()]);
    }

    #[test]
    fn transport_reassembles_a_packet_split_across_payload_reads() {
        let packet = FmtPacket::new(7, 8, 9, 10, 11, [0x55, 0x66]);
        let wire = packet.encode().unwrap();
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(wire[..2].to_vec());
        fake.received.push_back(wire[2..6].to_vec());
        fake.received.push_back(wire[6..].to_vec());
        let mut transport = FmtTransport::new(fake);

        assert_eq!(transport.receive_packet().unwrap(), packet);
    }

    #[test]
    fn transport_rejects_an_empty_payload_chunk() {
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(Vec::new());
        let mut transport = FmtTransport::new(fake);

        assert_eq!(transport.receive_packet(), Err(Error::NoData));
    }
}
