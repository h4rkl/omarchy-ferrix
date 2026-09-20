# ferrix-networkd

Privileged networking service (ADR-002): owns the public `NetworkState`,
folds backend events, fans deltas out to subscribers over
`ferrix-network-protocol`, and authorizes every request by verified `AppId`
and capability (ADR-004). Backends are composed only in `src/main.rs`
(`--backend mock|networkmanager`); the library works with any
`ferrix_network_adapter_api::NetworkBackend`.

```text
ferrix-networkd --backend mock|networkmanager [--profile PATH] [--socket PATH]
                [--mock-control auto|none|PATH] [--appd PATH|none] [--nested]
                [--allow-unauthenticated]
```

| Flag | Default | Notes |
| --- | --- | --- |
| `--socket` | `/run/ferrix/networkd.sock` (`FERRIX_NETWORKD_SOCKET`); `out/qemu-aarch64/networkd.sock` with `--nested` | bound before `networkd-ready` |
| `--mock-control` | `auto` in the guest (virtio-serial port `ferrix.network`), `none` nested | refused with `--backend networkmanager` (typed `networkd-failed`, exit 2) |
| `--appd` | `FERRIX_APPD_SOCKET` or `/run/ferrix/ferrix-appd.sock` | `none` denies every caller unless `--allow-unauthenticated` (nested only; emits `networkd-unauthenticated`) |
| `--nested` | off | evidence label `mock` instead of `QEMU` |

Capabilities: `network-state` → `state`, `subscribe`; `network-control` →
`set-radio`, `scan`, `connect`, `connect-profile`, `disconnect`, `forget`.
`hello` needs none (the snapshot rides along only with `network-state`).

## Records (stdout, one JSON line, `evidence` first)

`networkd-ready {socket, backend}`, `networkd-failed {status, reason}`,
`network-mock-control {status: ready|none|failed, path, detail}`,
`network-event {kind, <section>}` where `kind` is the delta name and the
section is the typed object (`radio`, `scan`, `connection`, `connectivity`,
`adapter`, reduced `ip`, `access_points`/`profiles` as id+digest summaries,
`snapshot` for `service-restarted`), `networkd-request-denied {app,
capability, request}`, `networkd-request-rejected {request, reason}`,
`network-backend-lost {reason}`, `networkd-identity {source}`. Records never
carry SSID text, hardware addresses, or passphrases.

## Mock-control channel (host only, mock backend only)

Not an application API. The daemon owns the `ferrix_mock_control::Handler`:
backend mutations (`state`, `set-adapter`, `set-radio`, `set-access-points`,
`scan-result`, `set-connect-result`, `set-connectivity`, `disconnect`,
`set-ip`, `tick`, `crash-backend`) are forwarded to the mock adapter; the
test-only **client verbs** `scan`, `connect {ssid, passphrase?}`,
`disconnect-request`, `forget {ssid}`, and `set-radio-enabled {enabled}`
are executed through the same `dispatch` path a protocol client uses,
under the synthetic identity `host.tool.mock-control` holding both network
capabilities. Typed rejections come back as `{"status":"error",...}`. The
ack's `state` is the adapter's evidence-safe summary; the resulting deltas
appear as `network-event` records. An ordered backend barrier is consumed
before every successful ack, so the service state and records cannot trail
into the next scenario step.
