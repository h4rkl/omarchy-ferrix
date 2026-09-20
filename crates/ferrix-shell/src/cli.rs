//! Command-line parsing (dependency-free, tested on every host).

use crate::config::DisplayGeometry;

pub const USAGE: &str = "usage: mobile-shell [--nested] [--display WxH] [--socket NAME] [--core PATH] [--appd PATH|--no-appd] [--cmdline PATH|--no-cmdline] [--print-config] [PROFILE]\n\
  PROFILE defaults to /etc/ferrix/profile.toml\n\
  --nested        run in a host window (Linux host with a Wayland/X11 session); implies --no-cmdline and mock evidence\n\
  --display WxH   pin the output geometry (overridden by ferrix.display= on the kernel command line)\n\
  --socket NAME   Wayland socket name under XDG_RUNTIME_DIR (default wayland-0)\n\
  --core PATH     mobile-core state/command socket (default /run/ferrix/core.sock)\n\
  --appd PATH     appd application-service socket (default $FERRIX_APPD_SOCKET or /run/ferrix/appd.sock); connected with retries, never required\n\
  --no-appd       do not link to appd (home gesture then raises the oldest mapped window)\n\
  --cmdline PATH  kernel command line to read ferrix.display= from (default /proc/cmdline)\n\
  --print-config  print the resolved configuration as a JSON record and exit";

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub profile_path: String,
    pub nested: bool,
    pub display: Option<DisplayGeometry>,
    pub socket: Option<String>,
    pub core_socket: String,
    /// `appd` socket to link to; `None` after `--no-appd`.
    pub appd_socket: Option<String>,
    pub cmdline_path: Option<String>,
    pub print_config: bool,
    pub help: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            profile_path: "/etc/ferrix/profile.toml".to_owned(),
            nested: false,
            display: None,
            socket: None,
            core_socket: std::env::var("FERRIX_CORE_SOCKET")
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| mobile_protocol::DEFAULT_SOCKET.to_owned()),
            appd_socket: Some(
                std::env::var(app_api::APPD_SOCKET_ENV)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| app_protocol::DEFAULT_SOCKET.to_owned()),
            ),
            cmdline_path: Some("/proc/cmdline".to_owned()),
            print_config: false,
            help: false,
        }
    }
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.into_iter();
    let mut profile_set = false;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--nested" => {
                out.nested = true;
                out.cmdline_path = None;
            }
            "--no-cmdline" => out.cmdline_path = None,
            "--print-config" => out.print_config = true,
            "--help" | "-h" => out.help = true,
            "--display" => {
                let value = it.next().ok_or("--display requires WxH")?;
                out.display = Some(DisplayGeometry::parse(&value)?);
            }
            "--socket" => {
                let value = it.next().ok_or("--socket requires a name")?;
                if value.is_empty() || value.contains('/') {
                    return Err(format!("invalid socket name {value:?}"));
                }
                out.socket = Some(value);
            }
            "--core" => {
                let value = it.next().ok_or("--core requires a path")?;
                if value.is_empty() {
                    return Err("--core path must not be empty".to_owned());
                }
                out.core_socket = value;
            }
            "--appd" => {
                let value = it.next().ok_or("--appd requires a path")?;
                if value.is_empty() {
                    return Err("--appd path must not be empty".to_owned());
                }
                out.appd_socket = Some(value);
            }
            "--no-appd" => out.appd_socket = None,
            "--cmdline" => {
                let value = it.next().ok_or("--cmdline requires a path")?;
                if !out.nested {
                    out.cmdline_path = Some(value);
                }
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n{USAGE}"));
            }
            _ => {
                if profile_set {
                    return Err(format!("unexpected extra argument {arg:?}\n{USAGE}"));
                }
                out.profile_path = arg;
                profile_set = true;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Args, String> {
        parse(s.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn defaults_target_the_guest() {
        let args = parse_str("").unwrap();
        assert_eq!(args.profile_path, "/etc/ferrix/profile.toml");
        assert!(!args.nested);
        assert_eq!(args.cmdline_path.as_deref(), Some("/proc/cmdline"));
        assert!(!args.core_socket.is_empty());
        assert!(
            args.appd_socket.is_some_and(|s| !s.is_empty()),
            "appd is linked by default"
        );
    }

    #[test]
    fn appd_socket_can_be_set_or_disabled() {
        let args = parse_str("--appd /tmp/appd.sock").unwrap();
        assert_eq!(args.appd_socket.as_deref(), Some("/tmp/appd.sock"));
        let args = parse_str("--no-appd").unwrap();
        assert_eq!(args.appd_socket, None);
        let args = parse_str("--no-appd --appd /tmp/late.sock").unwrap();
        assert_eq!(args.appd_socket.as_deref(), Some("/tmp/late.sock"));
        assert!(parse_str("--appd").is_err());
        assert!(parse(vec!["--appd".to_owned(), String::new()]).is_err());
        assert!(USAGE.contains("--appd PATH") && USAGE.contains("--no-appd"));
    }

    #[test]
    fn nested_route_matches_documented_invocation() {
        let args = parse_str("--nested targets/qemu-aarch64/profile.toml").unwrap();
        assert!(args.nested);
        assert_eq!(args.profile_path, "targets/qemu-aarch64/profile.toml");
        assert_eq!(
            args.cmdline_path, None,
            "nested ignores the host kernel command line"
        );
    }

    #[test]
    fn flags_are_parsed() {
        let args = parse_str(
            "--display 540x1170 --socket wayland-3 --core /tmp/core.sock --cmdline /tmp/cmdline --print-config p.toml",
        )
        .unwrap();
        assert_eq!(
            args.display,
            Some(DisplayGeometry {
                width: 540,
                height: 1170
            })
        );
        assert_eq!(args.socket.as_deref(), Some("wayland-3"));
        assert_eq!(args.core_socket, "/tmp/core.sock");
        assert_eq!(args.cmdline_path.as_deref(), Some("/tmp/cmdline"));
        assert!(args.print_config);
        assert_eq!(args.profile_path, "p.toml");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_str("--display").is_err());
        assert!(parse_str("--display 12").is_err());
        assert!(parse_str("--socket a/b").is_err());
        assert!(parse_str("--core").is_err());
        assert!(parse_str("--bogus").is_err());
        assert!(parse_str("a.toml b.toml").is_err());
    }
}
