# ferrix-appd

The application service. Every application is an independent, supervised
process started through an `ferrix_app_runtime_api::Runtime`; `ferrix-appd` owns the catalog, the
single instance per app, surface association, foreground tracking, the home
surface's restart budget, and the `ferrix_app_protocol` Unix socket. A crashing app
is observed, recorded, and published; it never takes the compositor, the
core, `ferrix-appd`, or the home surface with it.

The library depends only on `ferrix-app-api` and `ferrix-app-protocol`.
`ferrix-runtime-native` and `ferrix-runtime-android` are named in `src/main.rs`
alone (composition); everything else works with
any `Box<dyn ferrix_app_runtime_api::Runtime>`, so the whole service is tested on the host
with a channel-driven fake runtime and a fake process tree.

## Layout

| module | role |
|---|---|
| `catalog` | `Catalog::load_dir` over `*.json` descriptors (`AppDescriptor::from_json`; `grants.json` is skipped); rejects duplicate ids and a second `role: home`; lookup, `launchable()`, `autostart()`, `home()` |
| `grants` | `load_grants`: the `GrantsPolicy` from `--grants`/`FERRIX_APP_GRANTS`, else `<first app dir>/grants.json`, else the empty policy (`GrantsSource::Missing`) |
| `service` | `Service`: launch/activate/terminate/home, effective grants at launch, pid association, `identify(pid)`, foreground/background, subscribers, exit handling, home restart |
| `lifecycle` | pure restart policy: `HomeRestartConfig`, `HomeRestartTracker::record_exit(now)` |
| `process_tree` | `ProcessTree` trait, `/proc/<pid>/status`-backed `ProcTree`, bounded `ancestors()` walk |
| `server` | Unix listener, thread per connection, `Inbound` parsing, role gating (`allowed`, `dispatch`), subscription hand-off |
| `records` | `Recorder` with the evidence label and every record constructor; `emit` prints one JSON line |
| `testing` | `FakeRuntime`, `FakeProcessTree`, `channel_sink`, `wait_for` for unit and integration tests |

## Trust rings and role gating

A connection declares its `ClientRole` in `Hello`, which must be the first
message. After that the table in `server::allowed` applies:

| message | roles |
|---|---|
| `Catalog`, `Instances`, `Subscribe` | any |
| `Home` | `compositor`, `home`, `application`, `tool` |
| `Identify` | `service`, `tool` |
| `Launch`, `Activate`, `Terminate` | `home`, `tool` |
| `SurfaceMapped`, `SurfaceUnmapped`, `FocusChanged`, `HomeGesture`, `CloseGesture` | `compositor` |
| `YieldForeground` | `application`, `home` |

A refused request gets `Reply::ProtocolError`; a refused report is dropped
(reports are never replied to) and an `ferrix-appd-request-rejected` record is
emitted. Malformed lines get `ProtocolError` and the connection stays open.
On Linux the peer pid (`SO_PEERCRED`) of a connection is resolved through the
same ancestry walk as surfaces: a launched application that says `Hello` with
`tool`, `service`, or `compositor` is rejected (`peer pid N is application,
not service`). Compositor, tool, and service roles remain bootstrap trust:
those processes are not launched by `ferrix-appd`, so access to the socket
path is their boundary.

## Grants and `Identify`

`apps/grants.json` (`GrantsPolicy`, schema 1, strict) is loaded at startup
beside the catalog: `grants-loaded{apps,path}`, or `grants-missing` when no
file is configured or found (the policy is then **empty** and no application
holds any capability), or `grants-invalid{reason}` followed by
`ferrix-appd-failed` when the file exists but does not parse. At launch the
service computes `granted = declared ∩ granted-by-policy` and
`denied_capabilities = declared \ granted`, hands the effective set to the
runtime (`LaunchContext.granted`), stores it on the instance, and records both
in `app-started`. Trust rings never bypass the file.

`Request::Identify{pid}` (roles `service`, `tool`) resolves a kernel pid to
the running instance it belongs to and answers
`{"reply":"identity","schema_version":1,"app":"org.ferrix.terminal","trust":"untrusted","granted":["shell","background"]}`;
an unknown pid, or one whose instance has crashed or exited, answers
`{"reply":"identity","schema_version":1,"app":null,"trust":null,"granted":[]}`.
Daemons call this for their own peers before the first non-hello request and
enforce capabilities per operation. `appd-identify{pid,app|null,granted_count}`
is recorded once per pid per connection.

## Surface association

The compositor reports the client pid it obtained from the kernel
(`SO_PEERCRED`) with each `SurfaceMapped`/`FocusChanged`. Sandboxed apps are
grandchildren of `ferrix-appd` (`ferrix-appd -> ferrix-sandbox -> init -> app`), so the
service resolves a pid by checking the launched pids first and then walking
`PPid:` from `/proc/<pid>/status` upward (at most 16 hops) until it meets a
pid it launched. Resolved pids are cached until the instance exits, when
every association for that app is dropped.

The xdg `app_id` and title the compositor forwards are untrusted metadata
from the client: they are logged in `app-surface` and compared against the
association, and a disagreement is a warning record (`app-id-mismatch`),
never a re-association.

## Lifecycle

```text
not-running -> starting -> foreground <-> background
                   \                        /
                    -> crashed / terminated <
```

- `launch`: unknown id -> `UnknownApp`; `launchable: false` -> `NotLaunchable`;
  no runtime for the kind (`web`, or Android when Waydroid is absent) ->
  `RuntimeUnsupported`; already
  running -> `activated_existing = true` plus `Notification::Activate{pid}` to
  compositor subscribers; otherwise the runtime spawns, the instance is
  `starting`, and a waiter thread polls the handle until it exits. Activation
  always names that instance's mapped Wayland client pid. If launch or
  activation wins the race with the first surface, the directive is held and
  delivered when `SurfaceMapped` supplies the compositor-visible pid; the
  sandbox supervisor pid is never used as a toplevel identity.
- `SurfaceMapped` for a `starting` instance moves it to `foreground` when the
  compositor last reported focus on that pid or nothing else holds the
  foreground, else `background`. `FocusChanged` swaps foreground/background;
  focus on nothing or on an unknown client sends the foreground app back.
- Exit: a signal or non-zero code is `crashed` (`app-crashed`); code 0 or a
  requested `Terminate` (SIGTERM, then SIGKILL after 2 s) is `terminated`
  (`app-exited`). Either way the pid association is cleared and an
  `AppEvent::Lifecycle` is published. Instances are single per app; a
  relaunch after an exit gets the next instance number.

## Home restart policy

Only the home surface is restarted, and only after an exit that was not
requested through `Terminate`. Exits are counted in a 60 s sliding window:
the first restarts immediately, then after 1 s, 2 s, 4 s; the fifth exit in
the window abandons restarts (`home-restart-abandoned`) until a `Home` or
`Launch` request starts it again, which also resets the budget. Other apps
are never auto-restarted.

## Records

One JSON line per record on stdout, `evidence` first (`QEMU` in the guest,
`mock` with `--nested`):

`ferrix-appd-ready{socket,apps,home}`, `ferrix-appd-failed{reason}`,
`grants-loaded{apps,path}`, `grants-missing`, `grants-invalid{reason}`,
`appd-identify{pid,app,granted_count}`,
`app-launching{app,instance}`,
`app-started{app,instance,pid,granted,denied_capabilities,runtime_diagnostics}`,
`app-launch-failed{app,error,reason}`, `app-surface{app,pid,app_id,title}`,
`app-surface-unknown{pid,app_id}`, `app-id-mismatch{app,pid,reported}`,
`app-foreground{app}`, `app-background{app}`, `app-terminating{app,instance}`,
`app-exited{app,instance,code}`, `app-crashed{app,instance,signal|code}`,
`app-wait-failed{app,instance,reason}`, `home-restarting{attempt,delay_ms}`,
`home-restart-abandoned{exits,window_ms}`, `ferrix-appd-request-rejected{role,message,reason}`,
`ferrix-appd-socket-error{error}`.

## Running

```text
ferrix-appd [--socket PATH] [--app-dir DIR]... [--grants PATH] [--nested] [--no-autostart] [--sandbox PATH]
```

Environment: `FERRIX_APPD_SOCKET`, `FERRIX_APP_DIR`, `FERRIX_APP_GRANTS`,
`FERRIX_SANDBOX_BIN`.
The default descriptor directory is `/usr/share/ferrix/apps` when it exists,
else `apps`; the default grants file is `grants.json` in the first descriptor
directory. The socket is bound before `ferrix-appd-ready` is printed; then the
home surface and every `autostart: true` descriptor are launched (unless
`--no-autostart`), and the socket is served until the process is stopped.
Launched apps inherit `ferrix-appd`'s environment plus `FERRIX_APP_ID`,
`FERRIX_APP_INSTANCE`, and `FERRIX_APPD_SOCKET`.

Tests: `cargo test -p ferrix-appd` runs on macOS and Linux; `tests/socket.rs` drives
a real Unix socket with `ferrix_app_protocol::Client` and the fake runtime,
including a `service`-role client identifying a launched pid. Negative tests
cover: unknown pid → unknown identity; declared-but-not-granted excluded and
reported; `application`/`home`/`compositor` cannot `Identify`; `service`
cannot `Launch`/`Activate`/`Terminate`/`Home` or report; a launched app
cannot `Hello` as `service` or `tool`; identity after a crash is unknown; the
repository `grants.json` parses and grants `shell` to the Terminal only.
