# ferrix-s5300-cpif

This crate provides bounded, Rust-only SIPC5 and Exynos SIT link-frame codecs
for the Google S5300 CPIF work. The exact tegu DTS selects `PROTOCOL_SIT`, so
the SIT codec is the board-relevant framing slice; SIPC5 remains a generic
source-backed slice for related configurations. It also models the seven-byte
`sipc_fmt_hdr` envelope, which is **SIPC-only**. That codec requires a
`SipcLink` proof that tegu's `PROTOCOL_SIT` cannot produce; see
[tegu uses SIT, not `sipc_fmt_hdr`](#tegu-uses-sit-not-sipc_fmt_hdr-issue-76).
The crate intentionally
stops at a wire-frame and payload-I/O boundary. It does not claim a working
modem, phone, SMS, packet-data, voice, IMS, or call-audio implementation; it
neither discovers target device nodes nor binds board channels.

## Source basis

The board target pins the Google S5300 driver in
[`kernel-sources.toml`](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-sources.toml)
to commit `a8976934fe7f396627f9900958eb1457a74ebcca`. The codec follows these
source definitions from that exact revision:

- [`include/sipc5.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/include/sipc5.h): the SIPC5 link header, config bits, header sizes, little-endian native length fields, and eight-byte padding calculation;
- [`include/exynos_ipc.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/include/exynos_ipc.h): the Exynos SIT sync word, 12-byte allocation, fragment configuration, channel fields, and length semantics;
- [`modem_prj.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_prj.h): the packed seven-byte `sipc_fmt_hdr` (consumed only on the SIPC path) and CP-state ioctl declarations;
- [`modem_v1.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_v1.h): the packed multi-frame control byte (`id:7`, `more:1`);
- [`modem_io_device.c`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_io_device.c): transmit-side header construction, the declared length as header plus payload excluding padding, and device registration;
- [`ipc_io_device.c`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/ipc_io_device.c): the character-device payload boundary, where the kernel adds/removes link framing around userspace `read(2)`/`write(2)` payloads;
- [`modem_main.c`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_main.c): DT-derived I/O-device names, channels, types, and multi-channel expansion;
- [`link_device.c`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/link_device.c): protocol selection and the separate `ld->aligned` link property;
- [`net_io_device.c`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/net_io_device.c): the raw-IP network-device path for packet data.

The exact board protocol and IPC device names are source-derived from the pinned
Google zuma DTS
[`zuma-cp-s5300-sit.dtsi`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5300-sit.dtsi):
`mif,protocol = PROTOCOL_SIT`, `mif,link_type = LINKDEV_PCIE`, and the
`umts_ipc` multi-channel I/O device generate `umts_ipc0` and `umts_ipc1`.
The same DTS describes `rmnet` as 30 `IODEV_NET` instances, which are the
kernel raw-IP packet-data surface rather than character-device modem
commands. Those names are checked-in source metadata, not proof that the
devices exist on a booted phone.

The source uses native C pointer loads/stores for the length fields. This
crate makes the target assumption explicit as little-endian encoding, which
matches the Pixel 9a's AArch64 Linux target. It does not attempt to support a
different byte order.

The exact SIT channel numbers are pinned separately in the target-local
`s5300-channels.toml` from Google's
[`exynos-cpif.h`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/include/dt-bindings/soc/google/exynos-cpif.h).
Those values are source metadata, not permission to open guessed device nodes.

## Implemented boundary

`FrameCodec` supports only the three layouts the pinned driver actually
handles:

| Config low bits | Header | Length | Meaning |
| --- | ---: | ---: | --- |
| `0b000` | 4 bytes | little-endian `u16` | ordinary frame |
| `0b011` | 5 bytes | little-endian `u16` | multi-frame control byte |
| `0b010` | 6 bytes | little-endian `u32` | extended length |

The length includes the header and payload, but not optional alignment
padding. When the padding bit is set, the wire footprint is rounded to the
next eight-byte boundary. The codec rejects the unused `0b001` layout,
truncated headers, lengths shorter than their selected header, oversized
lengths, invalid multi-frame identifiers, and trailing bytes in strict
single-frame decoding. `parse_prefix` is available for a caller that already
owns a stream boundary and wants the consumed byte count.

`ChannelId` preserves raw byte values and reports the five values reserved by
the generic driver (`0`, `5`, `6`, `27`, and `255`). `ChannelDevice` carries a
caller-supplied device path and channel without guessing FMT/RFS/boot/PDP
ranges; those assignments are DT data, not established by this generic
driver repository.

`SitFrameCodec` implements the pinned tegu SIT 12-byte header, little-endian
sync/sequence/length fields, channel and channel-sequence bytes, fragment
configuration, bounded payloads, and explicit eight-byte alignment context. It
is a link-format test primitive only: the S5300 character device removes this
header before userspace sees a payload. SIT has no padding flag; the pinned
kernel's separate `ld->aligned` property decides whether the next eight-byte
boundary is part of the link footprint. The parser therefore never infers
padding merely because more bytes are available.

The exact DTS link attributes include dump/boot alignment but do not declare
the separate `LINK_ATTR_IPC_ALIGNED` property used by the kernel's normal IPC
path. The default codec is therefore unaligned; an aligned parser must be
selected only after the final kernel/link configuration proves that property.

The source declares a packed ten-byte field struct but uses a 12-byte header
area and writes named fields only at offsets 0 through 9. The codec emits
zeroes for the two remaining bytes so fixtures are deterministic; it does not
assign protocol fields to them.

### Tegu uses SIT, not `sipc_fmt_hdr` (issue #76)

All references below are at the pinned driver revision `a8976934` (G) and the
pinned zuma DTS revision `42395e8a` (D). The E0 report in the board's
[`s5300-source-evidence.md`](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-source-evidence.md#s5300-e0-device-free-cpif-abi-comparison-issue-1)
(rows F1 and F2) gives the matching line ranges.

| Step | Pinned source | What it shows |
| --- | --- | --- |
| Board selects the protocol | D `zuma-cp-s5300-sit.dtsi`: `mif,protocol = <PROTOCOL_SIT>`; G `modem_main.c: parse_dt_common_pdata` reads it; G `link_device.c: set_ld_attr` → `set_protocol_attr` installs the SIT helpers | tegu is `PROTOCOL_SIT` |
| RX strips the link header | G `modem_io_device.c: rx_demux` → `rx_fmt_ipc` → `rx_frame_with_link_header`, `case PROTOCOL_SIT`: `skb_pull(skb, EXYNOS_HEADER_SIZE)` | the 12-byte SIT header never reaches userspace |
| RX reassembles opaquely | G `modem_io_device.c: gather_multi_frame_sit` | fragments are concatenated with `memcpy`; the payload is not parsed |
| `sipc_fmt_hdr` is SIPC-only | G `modem_prj.h: struct sipc_fmt_hdr`; its sole use is G `modem_io_device.c: gather_multi_frame`, reached only from the `case PROTOCOL_SIPC` branch | the seven-byte header has no role on SIT |
| TX adds the link header | G `ipc_io_device.c: ipc_write` (`case PROTOCOL_SIT`: `headroom = EXYNOS_HEADER_SIZE`, then `copy_from_user`); G `modem_io_device.c: exynos_build_header` | the user buffer is an opaque SIT payload |
| Userspace read | G `ipc_io_device.c: ipc_read` | one queued skb per read, already stripped |

On tegu, the bytes on `umts_ipc0`/`umts_ipc1` are SIT payloads. In the
route-A plan the external, process-isolated `sit-proxy`/`samsung-sit` defines
their framing (E0 F1). This crate does not reimplement it and does not decode
those payloads.

The crate enforces this as follows:

- `LinkProtocol { Sipc, Sit }` names the DT choice, and `TEGU_LINK_PROTOCOL`
  is `LinkProtocol::Sit`, cited to D.
- `SipcFmtCodec`, `SipcFmtTransport`, and the `SIPC_FMT_*` constants are the
  only seven-byte codec API. There is no `Default` and no constructor without
  a `SipcLink`.
- `SipcLink` is a zero-sized proof with a private field. Only
  `LinkProtocol::Sipc.require_sipc()` produces one. On a SIT link,
  `require_sipc()` returns `Error::Unsupported` with
  `UnsupportedFeature::SipcFmtOnSitLink`.
- Tests cover the boundary with a SIPC5 multi-frame fixture whose stripped
  payload decodes as `sipc_fmt_hdr`, and a 12-byte SIT FMT0 fixture whose
  payload stays opaque and cannot reach the SIPC codec from tegu. A further
  test shows the SIPC codec would silently misread a SIT payload, so a
  length check alone could not catch the misuse. `compile_fail` doctests
  check that neither `SipcLink` nor a default codec can be constructed.

The SIPC codec checks the seven-byte minimum, the `u16` wire limit, a
caller-selected allocation bound, and strict packet boundaries.
`SipcFmtTransport` reassembles a packet split across short payload reads,
within that bound. It rejects trailing bytes rather than treating
concatenated packets as one message. Command bytes stay opaque. This crate
implements no SIM, registration, SMS, APN/PDP, call, IMS, or RIL behaviour.
A codec test is not a capability claim.

## Explicit unsupported transport boundary

The S5300 kernel character-device path is not a userspace SIPC5 or SIT byte
stream. The driver adds the selected link header before sending to the CP
link and removes it before queueing data for `read(2)`. Its concrete
character-device names, channel assignments, and I/O types are created from
board DT. The CPIF shared-memory/PCIe rings, packet processor, link state, CP
boot/reset flow, and doorbells are kernel-internal here.

Consequently, `ChannelDevice::transport()` returns an
`UnsupportedDeviceTransport`; its `send` and `receive` methods return the
typed `Error::Unsupported` / `UnsupportedFeature::KernelCharDeviceFraming`
error. This prevents a future caller from accidentally opening a guessed
`/dev` node and sending a link frame the kernel would interpret as payload.

When exact tegu DT evidence identifies an IPC character node, a future target
backend may instead call `DevicePayloadTransport::open` with that explicit
`ChannelDevice`. Its `PayloadTransport` implementation reads and writes the
kernel's payload-only interface and enforces a userspace allocation bound; a
short read is only a character-device chunk, not a complete modem message. It
does not discover paths, expose the CPIF control node, poll/reassemble modem
messages, or interpret modem commands. Opening an IPC node is an active kernel
operation: the pinned driver may run its link `init_comm` hook on `open(2)`.
Callers therefore need exact-target runtime evidence and an explicit bring-up
decision before constructing this transport; the read-only inventory workflow
never opens it. The target-specific channel manifest and a proven modem
protocol are still required before this transport can be wired into telephony.

## Realistic Linux userspace boundary

The pinned driver provides three distinct surfaces, none of which is a
ModemManager modem API:

| Kernel surface | Source-backed role | Safe crate boundary |
| --- | --- | --- |
| `umts_ipc0` / `umts_ipc1` and other `IODEV_IPC` devices | Character-device payload I/O; kernel owns SIT framing and CP state | `PayloadTransport` with an exact caller-supplied node; on tegu (SIT) the payload is opaque and owned by external `sit-proxy` |
| `rmnet0` through `rmnet29` | Kernel raw-IP packet-data netdevs | No implementation here; APN/session/control semantics are not specified by this driver |
| CPIF controls/ioctls and link/PCIe machinery | CP status, crash/GNSS controls, boot/reset, queues, and transport | Unsupported in this crate; never expose as modem commands |

The character-device ioctls visible in the pinned source report CP state,
opened status, crash control, and GNSS image operations. They do not define SIM
access, registration/operator selection, PDP/APN setup, SMS, call control,
audio routing, emergency calling, IMS, or supplementary services. The raw-IP
netdev path likewise does not establish how a userspace telephony service
authenticates, selects an APN, or manages a data session. Those behaviors
must come from separately verified S5300/Google userspace sources or a live
target protocol trace; this crate deliberately supplies neither.

Nothing in this crate maps opaque FMT command bytes to Samsung modem commands
or ModemManager interfaces. Those remaining protocol and device-evidence
tasks are intentionally outside this slice. The `discover`/`discover_at` helpers
are the one runtime observation seam: they read the exact
`samsung,exynos-cp` platform node's `modem_state` sysfs attribute and driver
binding without opening any CPIF character device. They report `ONLINE` as a
kernel-controller state only; they do not imply SIM registration, packet
data, voice, SMS, or IMS capability.
