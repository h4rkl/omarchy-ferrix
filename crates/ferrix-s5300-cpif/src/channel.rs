use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::{Error, Frame, UnsupportedFeature};

/// Default bound for one payload returned by a target-supplied CPIF
/// character device. The kernel removes link framing (SIPC5 or SIT) before
/// `read(2)` and adds it around `write(2)`; this bound protects the userspace
/// allocation.
pub const DEFAULT_MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

/// The byte-sized CPIF channel identifier.
///
/// The generic S5300 driver reserves 0, 5, 6, 27, and 255.  It obtains the
/// semantic channel ranges and concrete I/O-device names from device tree
/// data, so this type intentionally does not invent FMT/RFS/boot/PDP ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(u8);

impl ChannelId {
    /// Creates an identifier without applying the ordinary-device reserved
    /// channel rule.  Raw frames can still carry reserved values.
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Whether the identifier is one of the five values excluded by
    /// `sipc5_is_not_reserved_channel()` in the pinned kernel source.
    pub const fn is_reserved(self) -> bool {
        matches!(self.0, 0 | 5 | 6 | 27 | 255)
    }

    /// Validates an identifier for an ordinary kernel I/O-device endpoint.
    pub fn for_device(value: u8) -> Result<Self, Error> {
        let channel = Self::new(value);
        if channel.is_reserved() {
            return Err(Error::ReservedChannel { id: value });
        }
        Ok(channel)
    }
}

impl From<u8> for ChannelId {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl From<ChannelId> for u8 {
    fn from(channel: ChannelId) -> Self {
        channel.get()
    }
}

/// A target-supplied association between a Linux device node and a CPIF
/// channel.
///
/// This is metadata only.  The pinned driver creates character devices from
/// DT-provided `iod,name` values and strips/adds the selected link headers
/// kernel.  It is therefore unsafe for this crate to open a guessed path and
/// exchange framed bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelDevice {
    path: PathBuf,
    channel: ChannelId,
}

impl ChannelDevice {
    pub fn new(path: impl Into<PathBuf>, channel: ChannelId) -> Result<Self, Error> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(Error::InvalidDevicePath);
        }
        Ok(Self { path, channel })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn channel(&self) -> ChannelId {
        self.channel
    }

    /// Returns the explicit unsupported transport boundary for this slice.
    pub fn transport(&self) -> UnsupportedDeviceTransport {
        UnsupportedDeviceTransport {
            device: self.clone(),
        }
    }
}

/// Operations a future target-specific CPIF binding must provide.
pub trait ChannelTransport {
    fn send(&mut self, frame: &Frame) -> Result<(), Error>;
    fn receive(&mut self) -> Result<Frame, Error>;
}

/// The userspace contract of an S5300 IPC character device.
///
/// Unlike [`ChannelTransport`], this trait deliberately carries modem payload
/// bytes rather than SIPC5/SIT link frames. The pinned kernel driver constructs
/// and removes the selected link header inside the kernel. A future target backend can
/// layer its proven modem protocol over this trait without coupling that
/// protocol to the link framing implementation. A read is an ordinary
/// character-device read: it may be short or partial, and is not a modem
/// message boundary.
pub trait PayloadTransport {
    fn send_payload(&mut self, payload: &[u8]) -> Result<(), Error>;
    fn receive_payload(&mut self) -> Result<Vec<u8>, Error>;
}

/// A payload transport for an exact, caller-supplied CPIF IPC character node.
///
/// Construction never discovers or guesses a path. The caller must provide a
/// path obtained from the exact target's device-tree/kernel evidence. The
/// character device itself must be the IPC endpoint created by the S5300
/// driver; this type does not open the CPIF control node or any raw link. It
/// does not poll, reassemble messages, or assign meaning to payload bytes.
pub struct DevicePayloadTransport {
    device: ChannelDevice,
    file: File,
    max_payload_size: usize,
}

impl std::fmt::Debug for DevicePayloadTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DevicePayloadTransport")
            .field("device", &self.device)
            .field("max_payload_size", &self.max_payload_size)
            .finish_non_exhaustive()
    }
}

impl DevicePayloadTransport {
    pub fn open(device: ChannelDevice) -> Result<Self, Error> {
        Self::open_with_limit(device, DEFAULT_MAX_PAYLOAD_SIZE)
    }

    pub fn open_with_limit(device: ChannelDevice, max_payload_size: usize) -> Result<Self, Error> {
        if max_payload_size == 0 {
            return Err(Error::InvalidMaximumFrameSize {
                maximum: max_payload_size,
            });
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(device.path())
            .map_err(|error| Error::Io {
                operation: "open CPIF payload device",
                reason: error.to_string(),
            })?;
        Ok(Self {
            device,
            file,
            max_payload_size,
        })
    }

    pub const fn device(&self) -> &ChannelDevice {
        &self.device
    }

    pub const fn max_payload_size(&self) -> usize {
        self.max_payload_size
    }
}

impl PayloadTransport for DevicePayloadTransport {
    fn send_payload(&mut self, payload: &[u8]) -> Result<(), Error> {
        if payload.len() > self.max_payload_size {
            return Err(Error::FrameTooLarge {
                length: payload.len(),
                maximum: self.max_payload_size,
            });
        }
        self.file.write_all(payload).map_err(|error| Error::Io {
            operation: "write CPIF payload",
            reason: error.to_string(),
        })
    }

    fn receive_payload(&mut self) -> Result<Vec<u8>, Error> {
        let mut payload = vec![0; self.max_payload_size];
        let length = self.file.read(&mut payload).map_err(|error| Error::Io {
            operation: "read CPIF payload",
            reason: error.to_string(),
        })?;
        if length == 0 {
            return Err(Error::NoData);
        }
        payload.truncate(length);
        Ok(payload)
    }
}

/// A deliberate placeholder instead of a guessed `/dev/cpif` implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedDeviceTransport {
    device: ChannelDevice,
}

impl UnsupportedDeviceTransport {
    pub fn device(&self) -> &ChannelDevice {
        &self.device
    }
}

impl ChannelTransport for UnsupportedDeviceTransport {
    fn send(&mut self, _frame: &Frame) -> Result<(), Error> {
        Err(Error::Unsupported {
            feature: UnsupportedFeature::KernelCharDeviceFraming,
        })
    }

    fn receive(&mut self) -> Result<Frame, Error> {
        Err(Error::Unsupported {
            feature: UnsupportedFeature::KernelCharDeviceFraming,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reserved_channel_rule_matches_pinned_kernel() {
        for value in [0, 5, 6, 27, 255] {
            assert!(ChannelId::for_device(value).is_err());
            assert!(ChannelId::new(value).is_reserved());
        }
        assert!(!ChannelId::new(7).is_reserved());
    }

    #[test]
    fn endpoint_keeps_path_and_channel_without_guessing_names() {
        let endpoint = ChannelDevice::new("/dev/iod-format0", ChannelId::new(7)).unwrap();
        assert_eq!(endpoint.path(), Path::new("/dev/iod-format0"));
        assert_eq!(endpoint.channel().get(), 7);
    }

    #[test]
    fn endpoint_keeps_sit_channel_zero_without_applying_the_sipc_rule() {
        let endpoint = ChannelDevice::new("/dev/umts_ipc0", ChannelId::new(0)).unwrap();
        assert_eq!(endpoint.channel(), ChannelId::new(0));
    }

    #[test]
    fn guessed_kernel_binding_is_typed_unsupported() {
        let endpoint = ChannelDevice::new("/dev/iod-format0", ChannelId::new(7)).unwrap();
        let mut transport = endpoint.transport();
        let frame = Frame::new(ChannelId::new(7), [0x01]);
        assert_eq!(
            transport.send(&frame),
            Err(Error::Unsupported {
                feature: UnsupportedFeature::KernelCharDeviceFraming,
            })
        );
        assert_eq!(
            transport.receive(),
            Err(Error::Unsupported {
                feature: UnsupportedFeature::KernelCharDeviceFraming,
            })
        );
    }

    #[test]
    fn payload_transport_requires_the_exact_supplied_node() {
        let path = std::env::temp_dir().join(format!(
            "ferrix-s5300-cpif-{}-{}",
            std::process::id(),
            ChannelId::new(7).get()
        ));
        fs::write(&path, []).unwrap();
        let endpoint = ChannelDevice::new(&path, ChannelId::new(7)).unwrap();
        let mut transport = DevicePayloadTransport::open_with_limit(endpoint, 8).unwrap();
        transport.send_payload(&[1, 2, 3]).unwrap();
        assert_eq!(transport.max_payload_size(), 8);
        drop(transport);
        assert_eq!(fs::read(&path).unwrap(), [1, 2, 3]);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn payload_transport_bounds_writes_before_the_syscall() {
        let path =
            std::env::temp_dir().join(format!("ferrix-s5300-cpif-limit-{}", std::process::id()));
        fs::write(&path, []).unwrap();
        let endpoint = ChannelDevice::new(&path, ChannelId::new(7)).unwrap();
        let mut transport = DevicePayloadTransport::open_with_limit(endpoint, 2).unwrap();
        assert_eq!(
            transport.send_payload(&[1, 2, 3]),
            Err(Error::FrameTooLarge {
                length: 3,
                maximum: 2,
            })
        );
        drop(transport);
        assert!(fs::read(&path).unwrap().is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn payload_transport_returns_character_device_chunks_without_framing() {
        let path =
            std::env::temp_dir().join(format!("ferrix-s5300-cpif-read-{}", std::process::id()));
        fs::write(&path, [1, 2, 3, 4]).unwrap();
        let endpoint = ChannelDevice::new(&path, ChannelId::new(7)).unwrap();
        let mut transport = DevicePayloadTransport::open_with_limit(endpoint, 2).unwrap();

        assert_eq!(transport.receive_payload().unwrap(), [1, 2]);
        assert_eq!(transport.receive_payload().unwrap(), [3, 4]);
        drop(transport);
        fs::remove_file(path).unwrap();
    }
}
