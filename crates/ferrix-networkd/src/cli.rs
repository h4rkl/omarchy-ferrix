//! Command-line contract, parsed as a pure function so the typed startup
//! errors (for example `--mock-control` with a production backend) are
//! unit-tested without a process.

use std::fmt;
use std::path::PathBuf;

use ferrix_mock_control::ChannelPath;
use ferrix_network_api::{DEFAULT_NETWORKD_SOCKET, NETWORKD_SOCKET_ENV};

/// virtio-serial port name of the network mock-control channel (ADR-006).
pub const CONTROL_PORT_NAME: &str = "ferrix.network";
/// Default `ferrix-appd` socket inside the guest.
pub const DEFAULT_APPD_SOCKET: &str = "/run/ferrix/ferrix-appd.sock";
/// Environment variable overriding [`DEFAULT_APPD_SOCKET`].
pub const APPD_SOCKET_ENV: &str = "FERRIX_APPD_SOCKET";
/// Nested host default socket.
pub const NESTED_SOCKET: &str = "out/qemu-aarch64/networkd.sock";

pub const USAGE: &str = "usage: ferrix-networkd --backend mock|networkmanager [--profile PATH] [--socket PATH] [--mock-control auto|none|PATH] [--appd PATH|none] [--nested] [--allow-unauthenticated]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Mock,
    NetworkManager,
}

impl BackendKind {
    pub fn name(self) -> &'static str {
        match self {
            BackendKind::Mock => "mock",
            BackendKind::NetworkManager => "networkmanager",
        }
    }
}

/// Where callers are identified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentitySource {
    Appd(PathBuf),
    /// `--appd none`: deny everything...
    Deny,
    /// ...unless `--allow-unauthenticated` (nested development only).
    AllowAll,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub backend: BackendKind,
    pub profile: Option<PathBuf>,
    pub socket: PathBuf,
    pub mock_control: ChannelPath,
    pub identity: IdentitySource,
    pub nested: bool,
}

impl Args {
    /// Evidence label for records: nested mock or the target bootstrap label.
    pub fn evidence(&self) -> String {
        crate::records::runtime_evidence(self.nested)
    }
}

/// Typed startup failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliError {
    Usage(String),
    /// The mock-control channel drives mock state only.
    MockControlWithProductionBackend,
    /// `--allow-unauthenticated` is a nested development switch.
    UnauthenticatedOutsideNested,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(reason) => write!(f, "{reason}\n{USAGE}"),
            CliError::MockControlWithProductionBackend => f.write_str(
                "--mock-control is refused with the networkmanager backend: the channel drives mock state only",
            ),
            CliError::UnauthenticatedOutsideNested => {
                f.write_str("--allow-unauthenticated requires --nested")
            }
        }
    }
}

impl std::error::Error for CliError {}

/// Parses `argv[1..]` with `env` supplying environment lookups.
pub fn parse<I, S>(argv: I, env: &dyn Fn(&str) -> Option<String>) -> Result<Args, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let env = |key: &str| env(key).filter(|v| !v.is_empty());
    let mut backend = None;
    let mut profile = None;
    let mut socket: Option<PathBuf> = env(NETWORKD_SOCKET_ENV).map(PathBuf::from);
    let mut mock_control: Option<ChannelPath> = None;
    let mut appd: Option<String> = env(APPD_SOCKET_ENV);
    let mut nested = false;
    let mut allow_unauthenticated = false;

    let mut it = argv.into_iter();
    let value = |it: &mut I::IntoIter, flag: &str| {
        it.next()
            .map(|s| s.as_ref().to_owned())
            .ok_or_else(|| CliError::Usage(format!("{flag} needs a value")))
    };
    while let Some(arg) = it.next() {
        match arg.as_ref() {
            "--backend" => {
                backend = Some(match value(&mut it, "--backend")?.as_str() {
                    "mock" => BackendKind::Mock,
                    "networkmanager" => BackendKind::NetworkManager,
                    other => {
                        return Err(CliError::Usage(format!(
                            "unknown backend {other:?}; expected mock|networkmanager"
                        )));
                    }
                });
            }
            "--profile" => profile = Some(PathBuf::from(value(&mut it, "--profile")?)),
            "--socket" => socket = Some(PathBuf::from(value(&mut it, "--socket")?)),
            "--mock-control" => {
                mock_control = Some(ChannelPath::parse(
                    &value(&mut it, "--mock-control")?,
                    CONTROL_PORT_NAME,
                ));
            }
            "--appd" => appd = Some(value(&mut it, "--appd")?),
            "--nested" => nested = true,
            "--allow-unauthenticated" => allow_unauthenticated = true,
            "--help" | "-h" => return Err(CliError::Usage("help requested".to_owned())),
            other => return Err(CliError::Usage(format!("unknown argument {other:?}"))),
        }
    }

    let backend = backend.ok_or_else(|| CliError::Usage("--backend is required".to_owned()))?;
    let mock_control = match (backend, mock_control) {
        (BackendKind::NetworkManager, Some(ChannelPath::None))
        | (BackendKind::NetworkManager, None) => ChannelPath::None,
        (BackendKind::NetworkManager, Some(_)) => {
            return Err(CliError::MockControlWithProductionBackend);
        }
        (BackendKind::Mock, Some(channel)) => channel,
        // Guest default: the virtio port; nested default: no channel.
        (BackendKind::Mock, None) if nested => ChannelPath::None,
        (BackendKind::Mock, None) => ChannelPath::parse("auto", CONTROL_PORT_NAME),
    };
    if allow_unauthenticated && !nested {
        return Err(CliError::UnauthenticatedOutsideNested);
    }
    let identity = match appd.as_deref() {
        Some("none") if allow_unauthenticated => IdentitySource::AllowAll,
        Some("none") => IdentitySource::Deny,
        Some(path) => IdentitySource::Appd(PathBuf::from(path)),
        None if allow_unauthenticated => IdentitySource::AllowAll,
        None => IdentitySource::Appd(PathBuf::from(DEFAULT_APPD_SOCKET)),
    };
    let socket = socket.unwrap_or_else(|| {
        PathBuf::from(if nested {
            NESTED_SOCKET
        } else {
            DEFAULT_NETWORKD_SOCKET
        })
    });
    Ok(Args {
        backend,
        profile,
        socket,
        mock_control,
        identity,
        nested,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn parse_args(args: &[&str]) -> Result<Args, CliError> {
        parse(args.iter().copied(), &no_env)
    }

    #[test]
    fn guest_defaults() {
        let args = parse_args(&["--backend", "mock"]).unwrap();
        assert_eq!(args.backend, BackendKind::Mock);
        assert_eq!(args.socket, PathBuf::from("/run/ferrix/networkd.sock"));
        assert_eq!(
            args.mock_control,
            ChannelPath::Auto {
                port_name: "ferrix.network".into()
            }
        );
        assert_eq!(
            args.identity,
            IdentitySource::Appd(PathBuf::from("/run/ferrix/ferrix-appd.sock"))
        );
        assert!(!args.nested);
        assert_eq!(args.evidence(), "QEMU");
    }

    #[test]
    fn nested_defaults_and_explicit_paths() {
        let args = parse_args(&[
            "--backend",
            "mock",
            "--nested",
            "--mock-control",
            "out/qemu-aarch64/network-control.sock",
            "--appd",
            "out/qemu-aarch64/ferrix-appd.sock",
            "--profile",
            "targets/qemu-aarch64/profile.toml",
        ])
        .unwrap();
        assert!(args.nested);
        assert_eq!(args.evidence(), "mock");
        assert_eq!(args.socket, PathBuf::from(NESTED_SOCKET));
        assert_eq!(
            args.mock_control,
            ChannelPath::Explicit(PathBuf::from("out/qemu-aarch64/network-control.sock"))
        );
        assert_eq!(
            args.identity,
            IdentitySource::Appd(PathBuf::from("out/qemu-aarch64/ferrix-appd.sock"))
        );
        assert_eq!(
            args.profile,
            Some(PathBuf::from("targets/qemu-aarch64/profile.toml"))
        );
        let bare = parse_args(&["--backend", "mock", "--nested"]).unwrap();
        assert_eq!(bare.mock_control, ChannelPath::None);
    }

    #[test]
    fn environment_supplies_sockets() {
        let env = |key: &str| match key {
            NETWORKD_SOCKET_ENV => Some("/tmp/n.sock".to_owned()),
            APPD_SOCKET_ENV => Some("/tmp/a.sock".to_owned()),
            _ => None,
        };
        let args = parse(["--backend", "mock"], &env).unwrap();
        assert_eq!(args.socket, PathBuf::from("/tmp/n.sock"));
        assert_eq!(
            args.identity,
            IdentitySource::Appd(PathBuf::from("/tmp/a.sock"))
        );
        // Flags beat the environment.
        let args = parse(["--backend", "mock", "--socket", "/tmp/x.sock"], &env).unwrap();
        assert_eq!(args.socket, PathBuf::from("/tmp/x.sock"));
    }

    #[test]
    fn mock_control_with_the_production_backend_is_a_typed_error() {
        assert_eq!(
            parse_args(&["--backend", "networkmanager", "--mock-control", "auto"]),
            Err(CliError::MockControlWithProductionBackend)
        );
        assert_eq!(
            parse_args(&[
                "--backend",
                "networkmanager",
                "--mock-control",
                "/tmp/c.sock"
            ]),
            Err(CliError::MockControlWithProductionBackend)
        );
        let ok = parse_args(&["--backend", "networkmanager"]).unwrap();
        assert_eq!(ok.mock_control, ChannelPath::None);
        let explicit_none =
            parse_args(&["--backend", "networkmanager", "--mock-control", "none"]).unwrap();
        assert_eq!(explicit_none.mock_control, ChannelPath::None);
    }

    #[test]
    fn identity_switches() {
        assert_eq!(
            parse_args(&["--backend", "mock", "--appd", "none"])
                .unwrap()
                .identity,
            IdentitySource::Deny
        );
        assert_eq!(
            parse_args(&[
                "--backend",
                "mock",
                "--nested",
                "--appd",
                "none",
                "--allow-unauthenticated"
            ])
            .unwrap()
            .identity,
            IdentitySource::AllowAll
        );
        assert_eq!(
            parse_args(&[
                "--backend",
                "mock",
                "--appd",
                "none",
                "--allow-unauthenticated"
            ]),
            Err(CliError::UnauthenticatedOutsideNested)
        );
    }

    #[test]
    fn usage_errors_are_typed() {
        assert!(matches!(parse_args(&[]), Err(CliError::Usage(_))));
        assert!(matches!(
            parse_args(&["--backend"]),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_args(&["--backend", "wifi"]),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_args(&["--backend", "mock", "--bogus"]),
            Err(CliError::Usage(_))
        ));
        assert!(
            CliError::MockControlWithProductionBackend
                .to_string()
                .contains("mock state only")
        );
    }
}
