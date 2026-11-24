//! CPIF link-protocol selection and the SIPC-only codec guard (issue #76).

use crate::{Error, UnsupportedFeature};

/// The CPIF link protocol a board selects with the DT `mif,protocol`
/// property.
///
/// The pinned Google S5300 driver (`a8976934fe7f396627f9900958eb1457a74ebcca`)
/// supports two link protocols. The board device tree picks one through
/// `mif,protocol` (`modem_main.c: parse_dt_common_pdata`), and
/// `link_device.c: set_ld_attr` copies it into `ld->protocol` before
/// `set_protocol_attr` installs the per-protocol header helpers.
///
/// The receive path then splits on that value in
/// `modem_io_device.c: rx_frame_with_link_header`:
///
/// - `PROTOCOL_SIPC` pulls the SIPC5 header and, for multi-frames, calls
///   `gather_multi_frame`. That is the only reader of the seven-byte
///   `struct sipc_fmt_hdr` (`modem_prj.h`) in the whole pinned tree.
/// - `PROTOCOL_SIT` pulls the fixed 12-byte `EXYNOS_HEADER_SIZE` link header
///   and, for fragments, calls `gather_multi_frame_sit`, which concatenates
///   payload bytes without parsing them.
///
/// On transmit, `ipc_io_device.c: ipc_write` reserves `EXYNOS_HEADER_SIZE`
/// headroom for SIT and copies the user buffer as an opaque payload.
/// `modem_io_device.c: exynos_build_header` then writes the 12-byte header.
///
/// The exact tegu DTS (`zuma-cp-s5300-sit.dtsi` at
/// `42395e8adcb880aa336f0da6bb2a19ed53929c40`) sets
/// `mif,protocol = <PROTOCOL_SIT>`. Bytes that tegu userspace reads from
/// `umts_ipc0`/`umts_ipc1` are therefore SIT payloads that the kernel never
/// interprets. They are not `sipc_fmt_hdr` packets. In the route-A plan, the
/// external, process-isolated `sit-proxy`/`samsung-sit` defines their framing.
/// Ferrix does not reimplement it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkProtocol {
    /// `PROTOCOL_SIPC`: SIPC5 link headers, seven-byte `sipc_fmt_hdr` on the
    /// FMT multi-frame path.
    Sipc,
    /// `PROTOCOL_SIT`: 12-byte Exynos SIT link header, stripped by the
    /// kernel; the payload is opaque to the kernel.
    Sit,
}

impl LinkProtocol {
    /// Size of the link header the kernel adds on `write(2)` and removes
    /// before `read(2)`. For SIPC this is the minimum, because SIPC5 headers
    /// can grow with a control byte or an extended length.
    pub const fn min_link_header_size(self) -> usize {
        match self {
            Self::Sipc => crate::SIPC5_MIN_HEADER_SIZE,
            Self::Sit => crate::EXYNOS_HEADER_SIZE,
        }
    }

    /// Returns the proof needed to build a SIPC FMT codec, or a typed refusal
    /// for any other protocol.
    ///
    /// This is the only constructor of [`SipcLink`]. A tegu caller holds
    /// [`TEGU_LINK_PROTOCOL`] (= [`LinkProtocol::Sit`]) and gets
    /// [`UnsupportedFeature::SipcFmtOnSitLink`]. It cannot build
    /// [`crate::SipcFmtCodec`] or [`crate::SipcFmtTransport`].
    pub const fn require_sipc(self) -> Result<SipcLink, Error> {
        match self {
            Self::Sipc => Ok(SipcLink { _private: () }),
            Self::Sit => Err(Error::Unsupported {
                feature: UnsupportedFeature::SipcFmtOnSitLink,
            }),
        }
    }
}

/// Link protocol for the exact `google-pixel-9a-tegu-gxq96-mp1-0` target.
///
/// Source: `mif,protocol = <PROTOCOL_SIT>` in
/// [`zuma-cp-s5300-sit.dtsi` @ `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5300-sit.dtsi),
/// included by tegu through `zumapro-tegu-common.dtsi` →
/// `zumapro-board-cp-s5300-sit.dtsi`. The factory MP1.0 `dtbo.img` `cpif`
/// node matches that include chain (E0 U4 in the board's
/// `s5300-source-evidence.md`). This is source evidence, not a runtime probe.
pub const TEGU_LINK_PROTOCOL: LinkProtocol = LinkProtocol::Sit;

/// Zero-sized proof that the caller's CPIF link uses `PROTOCOL_SIPC`.
///
/// Callers obtain it only from [`LinkProtocol::require_sipc`]. Its private
/// field stops target code from building one directly:
///
/// ```compile_fail,E0451
/// let _forged = ferrix_s5300_cpif::SipcLink { _private: () };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SipcLink {
    _private: (),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tegu_is_sit_and_cannot_obtain_a_sipc_witness() {
        assert_eq!(TEGU_LINK_PROTOCOL, LinkProtocol::Sit);
        assert_eq!(
            TEGU_LINK_PROTOCOL.require_sipc(),
            Err(Error::Unsupported {
                feature: UnsupportedFeature::SipcFmtOnSitLink,
            })
        );
    }

    #[test]
    fn only_sipc_yields_a_witness() {
        assert!(LinkProtocol::Sipc.require_sipc().is_ok());
        assert!(LinkProtocol::Sit.require_sipc().is_err());
    }

    #[test]
    fn link_header_sizes_match_the_pinned_headers() {
        // `EXYNOS_HEADER_SIZE` (include/exynos_ipc.h) and
        // `SIPC5_MIN_HEADER_SIZE` (include/sipc5.h) at a8976934.
        assert_eq!(LinkProtocol::Sit.min_link_header_size(), 12);
        assert_eq!(LinkProtocol::Sipc.min_link_header_size(), 4);
    }
}
