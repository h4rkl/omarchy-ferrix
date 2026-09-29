# ferrix-s5300-cpif

This crate provides bounded, Rust-only SIPC5 and Exynos SIT link-frame codecs
for the Google S5300 CPIF work. The exact tegu DTS selects `PROTOCOL_SIT`, so
the SIT codec is the board-relevant framing slice; SIPC5 remains a generic
source-backed slice for related configurations. It also models the seven-byte
userspace FMT envelope carried inside the SIT link frame and can apply that
envelope to an explicitly supplied payload transport. The crate intentionally
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
- [`modem_prj.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_prj.h): the packed seven-byte userspace FMT header and CP-state ioctl declarations;
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

### Userspace FMT envelope

`FmtPacket` and `FmtCodec` implement the inner packet returned by
`ipc_read(2)` and accepted by `ipc_write(2)`: a little-endian `u16` total
length followed by `msg_seq`, `ack_seq`, `main_cmd`, `sub_cmd`, and `cmd_type`
bytes. The parser enforces the seven-byte minimum, the `u16` wire limit, a
caller-selected allocation bound, and strict packet boundaries. `FmtTransport`
wraps an explicit `PayloadTransport`, reassembles a packet split across short
payload reads within that bound, and still rejects trailing bytes rather than
silently treating concatenated packets as one message. The kernel still owns
SIT framing and packet fragmentation. Command bytes remain opaque:
the open S5300 source does not provide a tegu command vocabulary, so this
crate does not implement SIM, registration, SMS, APN/PDP, calls, IMS, or a
Google RIL replacement.

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
| `umts_ipc0` / `umts_ipc1` and other `IODEV_IPC` devices | Character-device payload I/O; kernel owns SIT framing and CP state | `PayloadTransport` plus opaque `FmtTransport`, with an exact caller-supplied node |
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
