//! Bounded SIPC5 and Exynos-SIT framing primitives for the pinned Google
//! S5300 CPIF driver. The exact tegu DT selects SIT ([`TEGU_LINK_PROTOCOL`]);
//! SIPC5 remains a generic source-backed slice for related S5300
//! configurations.
//!
//! The seven-byte `sipc_fmt_hdr` codec ([`SipcFmtCodec`]) applies only to
//! `PROTOCOL_SIPC` links. It requires a [`SipcLink`] proof that
//! [`LinkProtocol::Sit`], and therefore tegu, cannot produce. On tegu the
//! kernel strips the 12-byte SIT link header and the payload is opaque here
//! (issue #76; [`LinkProtocol`] cites the pinned source functions).
//!
//! This crate deliberately stops at the link-frame and explicit payload-I/O
//! boundary. It does not implement modem commands, SIM, SMS, packet data,
//! voice, IMS, or target device discovery. See `README.md` for the source and
//! the exact boundary.

#![forbid(unsafe_code)]

mod channel;
mod error;
mod frame;
mod protocol;
mod sipc_fmt;
mod sit;
mod status;

pub use channel::{
    ChannelDevice, ChannelId, ChannelTransport, DEFAULT_MAX_PAYLOAD_SIZE, DevicePayloadTransport,
    PayloadTransport, UnsupportedDeviceTransport,
};
pub use error::{Error, UnsupportedFeature};
pub use frame::{
    DEFAULT_MAX_FRAME_SIZE, Frame, FrameCodec, FrameMode, ParsedFrame, SIPC5_CONFIG_MASK,
    SIPC5_EXT_FIELD_EXIST, SIPC5_EXT_FIELD_MASK, SIPC5_EXT_LENGTH_CFG,
    SIPC5_HEADER_SIZE_WITH_CTL_FLD, SIPC5_HEADER_SIZE_WITH_EXT_LEN, SIPC5_MIN_HEADER_SIZE,
    SIPC5_MULTI_FRAME_CFG, SIPC5_PADDING_EXIST, SIPC5_START_MASK,
};
pub use protocol::{LinkProtocol, SipcLink, TEGU_LINK_PROTOCOL};
pub use sipc_fmt::{
    DEFAULT_MAX_SIPC_FMT_SIZE, SIPC_FMT_HEADER_SIZE, SIPC_FMT_MAX_WIRE_SIZE, SipcFmtCodec,
    SipcFmtPacket, SipcFmtTransport,
};
pub use sit::{
    EXYNOS_HEADER_SIZE, EXYNOS_MAX_FRAME_SIZE, EXYNOS_SINGLE_MASK, EXYNOS_START, SitFrame,
    SitFrameCodec, SitFrameMode, SitParsedFrame,
};
pub use status::{
    CPIF_COMPATIBLE, CpifStatus, DEFAULT_PLATFORM_SYSFS_ROOT, ModemState, discover, discover_at,
};
