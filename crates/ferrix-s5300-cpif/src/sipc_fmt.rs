//! The seven-byte `struct sipc_fmt_hdr` envelope, for `PROTOCOL_SIPC` links
//! only.
//!
//! In the pinned Google S5300 driver (`a8976934`), `modem_prj.h` declares
//! `sipc_fmt_hdr`. Its only consumer is `modem_io_device.c:
//! gather_multi_frame`, which the `PROTOCOL_SIPC` branch of
//! `rx_frame_with_link_header` calls. **It does not describe tegu.** Tegu
//! selects `PROTOCOL_SIT` (see [`crate::TEGU_LINK_PROTOCOL`]). There the
//! kernel strips a 12-byte SIT header and passes an opaque payload to
//! userspace. Every constructor here therefore takes a [`SipcLink`] proof,
//! which a SIT link cannot produce (issue #76).

use crate::{Error, PayloadTransport, SipcLink};

/// Size of the packed `struct sipc_fmt_hdr` (`modem_prj.h`): `u16 len`,
/// `msg_seq`, `ack_seq`, `main_cmd`, `sub_cmd`, `cmd_type`. SIPC only.
pub const SIPC_FMT_HEADER_SIZE: usize = 7;
/// The `sipc_fmt_hdr.len` field is a little-endian `u16` holding the whole
/// packet length, including [`SIPC_FMT_HEADER_SIZE`].
pub const SIPC_FMT_MAX_WIRE_SIZE: usize = u16::MAX as usize;
/// Default parser/encoder bound. The `u16` wire field caps it, so an
/// untrusted length cannot request an unbounded allocation.
pub const DEFAULT_MAX_SIPC_FMT_SIZE: usize = SIPC_FMT_MAX_WIRE_SIZE;

/// A bounded SIPC FMT packet.
///
/// `main_cmd`, `sub_cmd`, and `cmd_type` stay raw bytes. The open S5300
/// kernel source defines this envelope but no firmware command vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SipcFmtPacket {
    msg_seq: u8,
    ack_seq: u8,
    main_cmd: u8,
    sub_cmd: u8,
    cmd_type: u8,
    payload: Vec<u8>,
}

impl SipcFmtPacket {
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
}

/// A bounded codec for the SIPC seven-byte FMT envelope.
///
/// There is deliberately no `Default` impl and no constructor without a
/// [`SipcLink`]:
///
/// ```compile_fail,E0599
/// let _codec = ferrix_s5300_cpif::SipcFmtCodec::default();
/// ```
///
/// A tegu caller fails at the witness step:
///
/// ```
/// use ferrix_s5300_cpif::{Error, TEGU_LINK_PROTOCOL, UnsupportedFeature};
///
/// assert_eq!(
///     TEGU_LINK_PROTOCOL.require_sipc(),
///     Err(Error::Unsupported { feature: UnsupportedFeature::SipcFmtOnSitLink }),
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SipcFmtCodec {
    max_fmt_size: usize,
}

impl SipcFmtCodec {
    /// Creates a codec with [`DEFAULT_MAX_SIPC_FMT_SIZE`].
    pub const fn for_link(_link: SipcLink) -> Self {
        Self {
            max_fmt_size: DEFAULT_MAX_SIPC_FMT_SIZE,
        }
    }

    /// Creates a codec with a caller-selected allocation bound.
    pub fn new(_link: SipcLink, max_fmt_size: usize) -> Result<Self, Error> {
        if !(SIPC_FMT_HEADER_SIZE..=SIPC_FMT_MAX_WIRE_SIZE).contains(&max_fmt_size) {
            return Err(Error::InvalidMaximumFrameSize {
                maximum: max_fmt_size,
            });
        }
        Ok(Self { max_fmt_size })
    }

    pub const fn max_fmt_size(self) -> usize {
        self.max_fmt_size
    }

    pub fn encode(&self, packet: &SipcFmtPacket) -> Result<Vec<u8>, Error> {
        let declared_len = SIPC_FMT_HEADER_SIZE
            .checked_add(packet.payload.len())
            .ok_or(Error::LengthOverflow)?;
        self.check_length(declared_len)?;
        if declared_len > SIPC_FMT_MAX_WIRE_SIZE {
            return Err(Error::FrameTooLarge {
                length: declared_len,
                maximum: SIPC_FMT_MAX_WIRE_SIZE,
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

    /// Decodes exactly one SIPC FMT packet and rejects trailing bytes.
    pub fn decode(&self, bytes: &[u8]) -> Result<SipcFmtPacket, Error> {
        if bytes.len() < SIPC_FMT_HEADER_SIZE {
            return Err(Error::Truncated {
                needed: SIPC_FMT_HEADER_SIZE,
                available: bytes.len(),
            });
        }

        let declared_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        if declared_len < SIPC_FMT_HEADER_SIZE {
            return Err(Error::InvalidLength {
                declared: declared_len,
                header: SIPC_FMT_HEADER_SIZE,
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

        Ok(SipcFmtPacket {
            msg_seq: bytes[2],
            ack_seq: bytes[3],
            main_cmd: bytes[4],
            sub_cmd: bytes[5],
            cmd_type: bytes[6],
            payload: bytes[SIPC_FMT_HEADER_SIZE..].to_vec(),
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

/// Applies the SIPC FMT envelope to an explicitly supplied payload transport
/// on a `PROTOCOL_SIPC` link. The inner transport still owns device access;
/// the kernel still owns link framing. This wrapper does not discover
/// devices, boot the CP, or interpret commands. It is not usable on tegu.
pub struct SipcFmtTransport<T> {
    inner: T,
    codec: SipcFmtCodec,
    /// Bytes retained when the payload transport returns only part of the
    /// next packet. The buffer is bounded by `codec.max_fmt_size()` before
    /// any bytes are appended.
    receive_buffer: Vec<u8>,
}

impl<T> SipcFmtTransport<T> {
    pub fn new(link: SipcLink, inner: T) -> Self {
        Self::with_codec(inner, SipcFmtCodec::for_link(link))
    }

    /// Uses an existing codec. The codec itself already carries the
    /// [`SipcLink`] proof.
    pub fn with_codec(inner: T, codec: SipcFmtCodec) -> Self {
        Self {
            inner,
            codec,
            receive_buffer: Vec::new(),
        }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub const fn codec(&self) -> SipcFmtCodec {
        self.codec
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub const fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: PayloadTransport> SipcFmtTransport<T> {
    pub fn send_packet(&mut self, packet: &SipcFmtPacket) -> Result<(), Error> {
        let encoded = self.codec.encode(packet)?;
        self.inner.send_payload(&encoded)
    }

    pub fn receive_packet(&mut self) -> Result<SipcFmtPacket, Error> {
        loop {
            if self.receive_buffer.len() >= SIPC_FMT_HEADER_SIZE {
                let declared_len =
                    u16::from_le_bytes([self.receive_buffer[0], self.receive_buffer[1]]) as usize;
                if declared_len < SIPC_FMT_HEADER_SIZE {
                    return Err(Error::InvalidLength {
                        declared: declared_len,
                        header: SIPC_FMT_HEADER_SIZE,
                    });
                }
                self.codec.check_length(declared_len)?;

                if self.receive_buffer.len() >= declared_len {
                    // Decode exactly the declared packet. A payload
                    // transport may return a short chunk, but a concatenated
                    // second packet is ambiguous at this boundary, so the
                    // strict codec rejects it.
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
    use crate::{
        ChannelId, EXYNOS_HEADER_SIZE, Frame, LinkProtocol, SitFrame, TEGU_LINK_PROTOCOL,
        UnsupportedFeature,
    };
    use std::collections::VecDeque;

    fn sipc() -> SipcLink {
        LinkProtocol::Sipc.require_sipc().unwrap()
    }

    fn codec() -> SipcFmtCodec {
        SipcFmtCodec::for_link(sipc())
    }

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
        let packet = SipcFmtPacket::new(1, 2, 3, 4, 5, [0xaa, 0xbb]);
        let wire = codec().encode(&packet).unwrap();
        assert_eq!(wire, [9, 0, 1, 2, 3, 4, 5, 0xaa, 0xbb]);
        assert_eq!(codec().decode(&wire).unwrap(), packet);
    }

    #[test]
    fn empty_body_is_a_valid_seven_byte_packet() {
        let packet = SipcFmtPacket::new(0, 0, 0x10, 0x20, 0x30, []);
        let wire = codec().encode(&packet).unwrap();
        assert_eq!(wire.len(), SIPC_FMT_HEADER_SIZE);
        assert_eq!(codec().decode(&wire).unwrap(), packet);
    }

    #[test]
    fn malformed_lengths_are_rejected() {
        assert_eq!(
            codec().decode(&[0, 0, 0, 0, 0, 0, 0]),
            Err(Error::InvalidLength {
                declared: 0,
                header: SIPC_FMT_HEADER_SIZE,
            })
        );
        assert_eq!(
            codec().decode(&[8, 0, 0, 0, 0, 0, 0]),
            Err(Error::Truncated {
                needed: 8,
                available: SIPC_FMT_HEADER_SIZE,
            })
        );
        let mut trailing = vec![7, 0, 0, 0, 0, 0, 0];
        trailing.push(0xff);
        assert_eq!(
            codec().decode(&trailing),
            Err(Error::TrailingBytes {
                expected: SIPC_FMT_HEADER_SIZE,
                actual: SIPC_FMT_HEADER_SIZE + 1,
            })
        );
    }

    #[test]
    fn codec_bound_is_enforced_before_encoding() {
        let bounded = SipcFmtCodec::new(sipc(), SIPC_FMT_HEADER_SIZE).unwrap();
        let packet = SipcFmtPacket::new(0, 0, 0, 0, 0, [1]);
        assert_eq!(
            bounded.encode(&packet),
            Err(Error::FrameTooLarge {
                length: SIPC_FMT_HEADER_SIZE + 1,
                maximum: SIPC_FMT_HEADER_SIZE,
            })
        );
        assert_eq!(
            SipcFmtCodec::new(sipc(), SIPC_FMT_HEADER_SIZE - 1),
            Err(Error::InvalidMaximumFrameSize {
                maximum: SIPC_FMT_HEADER_SIZE - 1,
            })
        );
    }

    #[test]
    fn transport_wraps_only_the_payload_boundary() {
        let packet = SipcFmtPacket::new(7, 8, 9, 10, 11, [0x55]);
        let wire = codec().encode(&packet).unwrap();
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(wire.clone());
        let mut transport = SipcFmtTransport::new(sipc(), fake);

        transport.send_packet(&packet).unwrap();
        assert_eq!(transport.receive_packet().unwrap(), packet);
        let fake = transport.into_inner();
        assert_eq!(fake.sent, vec![wire]);
    }

    #[test]
    fn transport_reassembles_a_packet_split_across_payload_reads() {
        let packet = SipcFmtPacket::new(7, 8, 9, 10, 11, [0x55, 0x66]);
        let wire = codec().encode(&packet).unwrap();
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(wire[..2].to_vec());
        fake.received.push_back(wire[2..6].to_vec());
        fake.received.push_back(wire[6..].to_vec());
        let mut transport = SipcFmtTransport::new(sipc(), fake);

        assert_eq!(transport.receive_packet().unwrap(), packet);
    }

    #[test]
    fn transport_rejects_an_empty_payload_chunk() {
        let mut fake = FakePayloadTransport::default();
        fake.received.push_back(Vec::new());
        let mut transport = SipcFmtTransport::new(sipc(), fake);

        assert_eq!(transport.receive_packet(), Err(Error::NoData));
    }

    // ---- Fixtures for the supported boundary (issue #76) ----

    /// SIPC fixture: a SIPC5 multi-frame link frame (config `0b011`, five-byte
    /// header with the `id:7 more:1` control byte) whose payload starts with
    /// `sipc_fmt_hdr`. This is the layout that `gather_multi_frame` logs
    /// `fh->len` from after `rx_frame_with_link_header` pulls the SIPC5 header.
    #[test]
    fn sipc_fixture_fmt_header_follows_the_stripped_sipc5_header() {
        const LINK: [u8; 14] = [
            0xfb, 0x01, 0x0e, 0x00, 0x05, // SIPC5: cfg, ch 1, len 14, ctl id 5 more 0
            0x09, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // sipc_fmt_hdr, len 9
            0xaa, 0xbb, // FMT body
        ];
        let frame = Frame::decode(&LINK).unwrap();
        assert_eq!(frame.channel(), ChannelId::new(1));
        // What the kernel queues for read(2) once the SIPC5 header is pulled.
        let payload = frame.payload();
        assert_eq!(payload.len(), LINK.len() - 5);

        let packet = codec().decode(payload).unwrap();
        assert_eq!(
            packet,
            SipcFmtPacket::new(0x11, 0x22, 0x33, 0x44, 0x55, [0xaa, 0xbb])
        );
        assert_eq!(codec().encode(&packet).unwrap(), payload);
    }

    /// SIT fixture: a single 12-byte SIT link frame on FMT0 (channel 245).
    /// The kernel pulls `EXYNOS_HEADER_SIZE` and queues the rest unparsed, so
    /// userspace sees these exact payload bytes. The tegu route cannot get a
    /// [`SipcLink`], so it cannot run the SIPC codec over them.
    #[test]
    fn sit_fixture_payload_is_opaque_and_tegu_cannot_select_the_sipc_codec() {
        const LINK: [u8; 20] = [
            0xcd, 0xab, // EXYNOS_START_MASK 0xABCD, little-endian
            0x01, 0x00, // frame sequence
            0x00, 0xc0, // fragment config: EXYNOS_SINGLE_MASK << 8
            0x14, 0x00, // length = 12 + 8
            0xf5, // channel 245 (EXYNOS_CH_ID_FMT_0)
            0x01, // channel sequence
            0x00, 0x00, // unnamed header bytes 10..11
            0x08, 0x00, 0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, // opaque payload
        ];
        let frame = SitFrame::decode(&LINK).unwrap();
        assert_eq!(frame.channel(), ChannelId::new(245));
        assert_eq!(frame.payload(), &LINK[EXYNOS_HEADER_SIZE..]);

        assert_eq!(
            TEGU_LINK_PROTOCOL.require_sipc(),
            Err(Error::Unsupported {
                feature: UnsupportedFeature::SipcFmtOnSitLink,
            })
        );
    }

    /// Why the guard exists: the SIT payload above happens to start with a
    /// little-endian `8`, which equals its own length. If the SIPC codec ran
    /// on it, decoding would "succeed" and invent a command tuple from bytes
    /// the kernel never defined. The codec cannot detect the misuse, so only
    /// the type-level protocol witness prevents it.
    #[test]
    fn sipc_codec_would_silently_misdecode_a_sit_payload() {
        let sit_payload = [0x08, 0x00, 0xde, 0xad, 0xbe, 0xef, 0x01, 0x02];
        let misread = codec().decode(&sit_payload).unwrap();
        assert_eq!(misread.main_cmd(), 0xbe);
    }
}
