use std::fmt;

/// Features intentionally not implemented by this bounded transport slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedFeature {
    /// The kernel's CPIF character devices expose payloads to userspace; they
    /// do not expose the SIPC5/SIT link headers for userspace to read or
    /// write.
    KernelCharDeviceFraming,
    /// The S5300 driver gets the concrete I/O-device names and channel
    /// assignments from board device tree data, which is outside this pinned
    /// generic driver repository.
    DeviceTreeChannelBinding,
    /// The shared-memory/PCIe CPIF rings and their doorbell protocol are
    /// kernel-internal in the current slice.
    CpifLinkTransport,
}

impl fmt::Display for UnsupportedFeature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KernelCharDeviceFraming => {
                formatter.write_str("kernel character-device link framing")
            }
            Self::DeviceTreeChannelBinding => formatter.write_str("device-tree channel binding"),
            Self::CpifLinkTransport => {
                formatter.write_str("CPIF shared-memory/PCIe link transport")
            }
        }
    }
}

/// Errors returned by frame parsing, encoding, channel construction, and the
/// read-only sysfs status probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The caller supplied a zero or otherwise unusable parser limit.
    InvalidMaximumFrameSize { maximum: usize },
    /// A frame begins with a byte that does not carry the SIPC5 start marker.
    InvalidStart { config: u8 },
    /// An Exynos SIT frame does not begin with its fixed little-endian sync.
    InvalidSitStart { sync: u16 },
    /// The low configuration bits are not one of the layouts implemented by
    /// the pinned driver (`0`, `2`, or `3`).
    UnsupportedConfig { config: u8 },
    /// An Exynos SIT fragment configuration cannot be interpreted safely.
    UnsupportedSitConfig { config: u16 },
    /// The input ended before the requested frame bytes were available.
    Truncated { needed: usize, available: usize },
    /// The declared length does not include a complete SIPC5 header.
    InvalidLength { declared: usize, header: usize },
    /// A frame exceeds the codec's configured bound or a format's wire limit.
    FrameTooLarge { length: usize, maximum: usize },
    /// A decoded frame was followed by bytes; use `parse_prefix` for streams.
    TrailingBytes { expected: usize, actual: usize },
    /// The seven-bit multi-frame identifier cannot contain the supplied value.
    InvalidMultiFrameId { id: u8 },
    /// Arithmetic needed to calculate a wire length overflowed.
    LengthOverflow,
    /// An endpoint path was empty.
    InvalidDevicePath,
    /// The generic driver reserves these channel identifiers instead of
    /// registering them as ordinary I/O devices.
    ReservedChannel { id: u8 },
    /// The target-supplied character device returned an I/O error.
    Io {
        operation: &'static str,
        reason: String,
    },
    /// A payload read returned no bytes. The kernel character-device API uses
    /// a short timed read and may legitimately return zero when its queue is
    /// empty.
    NoData,
    /// The source-backed protocol boundary has not been implemented here.
    Unsupported { feature: UnsupportedFeature },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMaximumFrameSize { maximum } => {
                write!(formatter, "maximum frame size {maximum} is invalid")
            }
            Self::InvalidStart { config } => {
                write!(
                    formatter,
                    "invalid SIPC5 start marker in config 0x{config:02x}"
                )
            }
            Self::InvalidSitStart { sync } => {
                write!(formatter, "invalid Exynos SIT sync 0x{sync:04x}")
            }
            Self::UnsupportedConfig { config } => {
                write!(formatter, "unsupported SIPC5 config bits 0b{config:03b}")
            }
            Self::UnsupportedSitConfig { config } => {
                write!(
                    formatter,
                    "unsupported Exynos SIT fragment config 0x{config:04x}"
                )
            }
            Self::Truncated { needed, available } => {
                write!(
                    formatter,
                    "truncated frame: need {needed} bytes, have {available}"
                )
            }
            Self::InvalidLength { declared, header } => {
                write!(
                    formatter,
                    "declared frame length {declared} is shorter than header {header}"
                )
            }
            Self::FrameTooLarge { length, maximum } => {
                write!(formatter, "frame length {length} exceeds maximum {maximum}")
            }
            Self::TrailingBytes { expected, actual } => {
                write!(
                    formatter,
                    "frame consumes {expected} bytes but input has {actual}"
                )
            }
            Self::InvalidMultiFrameId { id } => {
                write!(
                    formatter,
                    "multi-frame identifier {id} does not fit in seven bits"
                )
            }
            Self::LengthOverflow => formatter.write_str("frame length arithmetic overflowed"),
            Self::InvalidDevicePath => formatter.write_str("device path must not be empty"),
            Self::ReservedChannel { id } => {
                write!(formatter, "channel {id} is reserved by the SIPC5 driver")
            }
            Self::Io { operation, reason } => write!(formatter, "{operation} failed: {reason}"),
            Self::NoData => formatter.write_str("CPIF payload queue is empty"),
            Self::Unsupported { feature } => write!(formatter, "{feature} is unsupported"),
        }
    }
}

impl std::error::Error for Error {}
