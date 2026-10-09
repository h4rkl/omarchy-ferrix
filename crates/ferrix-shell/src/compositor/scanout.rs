//! Framebuffer export for the pixman dumb-buffer path, honouring the target
//! profile's `display.scanout_byte_order`.
//!
//! Some display controllers read 32-bit DRM fourccs byte-reversed: the Pixel 9a
//! exynos DPU scans an `AB24` buffer as `RA24` and an `XB24` buffer as `RX24`
//! (verified with libdrm `modetest` colour bars on the exact target). The buffer
//! is still rendered in its nominal format; only the code registered with
//! `ADDFB2` is reversed so the controller's reading matches the rendered bytes.

use smithay::{
    backend::{
        allocator::{Buffer, Fourcc, Modifier, dumb::DumbBuffer, format::Format},
        drm::{
            DrmDeviceFd, Framebuffer,
            exporter::{ExportBuffer, ExportFramebuffer},
        },
    },
    reexports::drm::{
        buffer::{Handle as BufferHandle, PlanarBuffer},
        control::{Device as ControlDevice, FbCmd2Flags, framebuffer},
    },
};

use ferrix_target_profile::ScanoutByteOrder;

/// Exports dumb buffers as DRM framebuffers with the profile's byte order.
#[derive(Clone, Debug)]
pub(super) struct ScanoutExporter {
    fd: DrmDeviceFd,
    order: ScanoutByteOrder,
}

impl ScanoutExporter {
    pub(super) fn new(fd: DrmDeviceFd, order: ScanoutByteOrder) -> Self {
        Self { fd, order }
    }
}

/// A framebuffer registered by [`ScanoutExporter`]; removed again on drop.
#[derive(Debug)]
pub(super) struct ScanoutFramebuffer {
    fd: DrmDeviceFd,
    handle: framebuffer::Handle,
    format: Format,
}

impl AsRef<framebuffer::Handle> for ScanoutFramebuffer {
    fn as_ref(&self) -> &framebuffer::Handle {
        &self.handle
    }
}

impl Framebuffer for ScanoutFramebuffer {
    fn format(&self) -> Format {
        self.format
    }
}

impl Drop for ScanoutFramebuffer {
    fn drop(&mut self) {
        let _ = self.fd.destroy_framebuffer(self.handle);
    }
}

/// The dumb buffer's single plane, presented to `ADDFB2` under `code`.
struct ScanoutPlanes<'a> {
    buffer: &'a DumbBuffer,
    code: Fourcc,
}

impl PlanarBuffer for ScanoutPlanes<'_> {
    fn size(&self) -> (u32, u32) {
        self.buffer.handle().size()
    }

    fn format(&self) -> Fourcc {
        self.code
    }

    fn modifier(&self) -> Option<Modifier> {
        None
    }

    fn pitches(&self) -> [u32; 4] {
        [self.buffer.handle().pitch(), 0, 0, 0]
    }

    fn handles(&self) -> [Option<BufferHandle>; 4] {
        [Some(self.buffer.handle().handle()), None, None, None]
    }

    fn offsets(&self) -> [u32; 4] {
        [0; 4]
    }
}

/// The fourcc whose DRM byte layout is the reverse of `code`'s.
pub(super) fn byte_reversed(code: Fourcc) -> Fourcc {
    match code {
        Fourcc::Xrgb8888 => Fourcc::Bgrx8888,
        Fourcc::Argb8888 => Fourcc::Bgra8888,
        Fourcc::Xbgr8888 => Fourcc::Rgbx8888,
        Fourcc::Abgr8888 => Fourcc::Rgba8888,
        Fourcc::Bgrx8888 => Fourcc::Xrgb8888,
        Fourcc::Bgra8888 => Fourcc::Argb8888,
        Fourcc::Rgbx8888 => Fourcc::Xbgr8888,
        Fourcc::Rgba8888 => Fourcc::Abgr8888,
        other => other,
    }
}

/// The alpha-free variant DRM uses when a plane must be scanned out opaque.
fn opaque(code: Fourcc) -> Fourcc {
    match code {
        Fourcc::Argb8888 => Fourcc::Xrgb8888,
        Fourcc::Abgr8888 => Fourcc::Xbgr8888,
        Fourcc::Bgra8888 => Fourcc::Bgrx8888,
        Fourcc::Rgba8888 => Fourcc::Rgbx8888,
        other => other,
    }
}

impl ExportFramebuffer<DumbBuffer> for ScanoutExporter {
    type Framebuffer = ScanoutFramebuffer;
    type Error = std::io::Error;

    fn add_framebuffer(
        &self,
        _drm: &DrmDeviceFd,
        buffer: ExportBuffer<'_, DumbBuffer>,
        use_opaque: bool,
    ) -> Result<Option<Self::Framebuffer>, Self::Error> {
        let ExportBuffer::Allocator(buffer) = buffer else {
            return Ok(None);
        };
        let nominal = buffer.format();
        let code = if use_opaque {
            opaque(nominal.code)
        } else {
            nominal.code
        };
        let registered = match self.order {
            ScanoutByteOrder::Standard => code,
            ScanoutByteOrder::Reversed => byte_reversed(code),
        };
        let handle = self.fd.add_planar_framebuffer(
            &ScanoutPlanes {
                buffer,
                code: registered,
            },
            FbCmd2Flags::empty(),
        )?;
        Ok(Some(ScanoutFramebuffer {
            fd: self.fd.clone(),
            handle,
            format: Format {
                code,
                modifier: nominal.modifier,
            },
        }))
    }

    fn can_add_framebuffer(&self, buffer: &ExportBuffer<'_, DumbBuffer>) -> bool {
        matches!(buffer, ExportBuffer::Allocator(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_reversal_is_an_involution_on_32bit_rgb_codes() {
        for code in [
            Fourcc::Xrgb8888,
            Fourcc::Argb8888,
            Fourcc::Xbgr8888,
            Fourcc::Abgr8888,
            Fourcc::Bgrx8888,
            Fourcc::Bgra8888,
            Fourcc::Rgbx8888,
            Fourcc::Rgba8888,
        ] {
            assert_ne!(byte_reversed(code), code);
            assert_eq!(byte_reversed(byte_reversed(code)), code);
        }
        assert_eq!(byte_reversed(Fourcc::Rgb565), Fourcc::Rgb565);
    }

    #[test]
    fn opaque_drops_only_alpha() {
        assert_eq!(opaque(Fourcc::Argb8888), Fourcc::Xrgb8888);
        assert_eq!(opaque(Fourcc::Xrgb8888), Fourcc::Xrgb8888);
    }
}
