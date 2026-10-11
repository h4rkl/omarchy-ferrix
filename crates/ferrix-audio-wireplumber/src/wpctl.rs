//! Production transport backed by WirePlumber's `wpctl` command.
//!
//! `wpctl list audio sinks` is the machine-readable part of the command's
//! interface.  Sink properties are read with `wpctl inspect` because the list
//! output intentionally exposes only a display name.  The default sink's
//! mixer level comes from `wpctl get-volume`, which prints
//! `Volume: 0.40 [MUTED]` on WirePlumber's cubic (user-facing) scale and
//! prints nothing on stdout for a node without volume.  The transport polls
//! the same snapshot for changes; `wpctl` does not provide a subscription
//! command.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::process::{Command, Output};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Weak};
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{AudioOutputLevel, AudioRoute, Capability, HardwareError};

use super::{RouteEndpoint, TransportEvent, WirePlumberTransport};

const WPCTL: &str = "wpctl";
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct TransportConfig {
    executable: OsString,
    poll_interval: Duration,
}

/// A WirePlumber transport that communicates only through the maintained
/// `wpctl` command-line interface.
#[derive(Clone, Debug)]
pub(super) struct WpctlTransport {
    config: Arc<TransportConfig>,
}

impl WpctlTransport {
    pub(super) fn new() -> Result<Self, HardwareError> {
        let transport = Self::with_config(OsString::from(WPCTL), DEFAULT_POLL_INTERVAL);
        transport
            .run([OsStr::new("status"), OsStr::new("--help")])
            .map_err(audio_failure)?;
        Ok(transport)
    }

    fn with_config(executable: OsString, poll_interval: Duration) -> Self {
        Self {
            config: Arc::new(TransportConfig {
                executable,
                poll_interval,
            }),
        }
    }

    fn run<I, S>(&self, args: I) -> Result<String, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<OsString> = args
            .into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        let output = Command::new(&self.config.executable)
            .args(&args)
            .output()
            .map_err(|error| {
                format!(
                    "failed to start {}: {error}",
                    command_line(&self.config.executable, &args)
                )
            })?;
        check_output(&self.config.executable, &args, output)
    }

    fn endpoints(&self) -> Result<Vec<RouteEndpoint>, String> {
        let output = self.run([OsStr::new("list"), OsStr::new("audio"), OsStr::new("sinks")])?;
        let sinks = parse_sink_list(&output)?;
        let mut endpoints = Vec::new();

        for sink in sinks {
            let inspect = self.run([OsStr::new("inspect"), OsStr::new(&sink.id)])?;
            let properties = parse_properties(&inspect);
            let Some(route) = classify_sink(&sink, &properties) else {
                continue;
            };
            let level = if sink.active {
                parse_volume(&self.run([OsStr::new("get-volume"), OsStr::new(&sink.id)])?)?
            } else {
                None
            };
            endpoints.push(RouteEndpoint {
                id: sink.id,
                route,
                active: sink.active,
                level,
            });
        }

        endpoints.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then(left.route.cmp(&right.route))
                .then(left.active.cmp(&right.active))
        });
        Ok(endpoints)
    }

    fn set_default(&self, id: &str) -> Result<(), String> {
        let Some(id) = parse_node_id(id) else {
            return Err(format!("refusing non-numeric WirePlumber sink ID {id:?}"));
        };
        self.run([OsStr::new("set-default"), OsStr::new(id)])
            .map(|_| ())
    }

    fn set_volume(&self, id: &str, percent: u8) -> Result<(), String> {
        let Some(id) = parse_node_id(id) else {
            return Err(format!("refusing non-numeric WirePlumber sink ID {id:?}"));
        };
        if percent > 100 {
            return Err(format!("refusing volume {percent}% above 100%"));
        }
        let volume = format!("{}.{:02}", percent / 100, percent % 100);
        // `--limit 1.0` keeps WirePlumber from amplifying past 100%.
        self.run([
            OsStr::new("set-volume"),
            OsStr::new("--limit"),
            OsStr::new("1.0"),
            OsStr::new(id),
            OsStr::new(&volume),
        ])
        .map(|_| ())
    }

    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
        let Some(id) = parse_node_id(id) else {
            return Err(format!("refusing non-numeric WirePlumber sink ID {id:?}"));
        };
        let state = if muted { "1" } else { "0" };
        self.run([OsStr::new("set-mute"), OsStr::new(id), OsStr::new(state)])
            .map(|_| ())
    }
}

impl WirePlumberTransport for WpctlTransport {
    fn endpoints(&self) -> Result<Vec<RouteEndpoint>, String> {
        self.endpoints()
    }

    fn set_default(&self, id: &str) -> Result<(), String> {
        self.set_default(id)
    }

    fn set_volume(&self, id: &str, percent: u8) -> Result<(), String> {
        self.set_volume(id, percent)
    }

    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
        self.set_muted(id, muted)
    }

    fn watch(&self, sender: Sender<TransportEvent>) -> Result<(), String> {
        let weak_config: Weak<TransportConfig> = Arc::downgrade(&self.config);
        thread::Builder::new()
            .name("ferrix-audio-wireplumber-watch".to_owned())
            .spawn(move || watch_loop(weak_config, sender))
            .map(|_| ())
            .map_err(|error| format!("failed to start WirePlumber watcher: {error}"))
    }
}

fn watch_loop(weak_config: Weak<TransportConfig>, sender: Sender<TransportEvent>) {
    let mut previous = None;
    while let Some(config) = weak_config.upgrade() {
        let transport = WpctlTransport { config };
        let current = transport.endpoints();
        let poll_interval = transport.config.poll_interval;
        drop(transport);

        if let Ok(current) = current {
            let changed = previous.as_ref() != Some(&current);
            previous = Some(current);
            if changed && sender.send(TransportEvent::Changed).is_err() {
                break;
            }
        }

        thread::sleep(poll_interval);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SinkRow {
    id: String,
    name: String,
    active: bool,
}

fn parse_sink_list(output: &str) -> Result<Vec<SinkRow>, String> {
    let mut rows = Vec::new();
    let mut ids = BTreeSet::new();

    for (line_number, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let has_tabular_fields = fields.len() > 1;
        let tokens: Vec<&str> = if has_tabular_fields {
            fields.clone()
        } else {
            line.split_whitespace().collect()
        };
        let media_index = tokens
            .iter()
            .position(|field| field.eq_ignore_ascii_case("audio/sink"));
        let end_of_name = media_index.unwrap_or(tokens.len());
        let id_index = tokens[..end_of_name]
            .iter()
            .position(|field| parse_node_id(field).is_some());
        let Some(id_index) = id_index else {
            // Headers and informational lines are not sink records.
            continue;
        };
        let Some(id) = parse_node_id(tokens[id_index]).map(str::to_owned) else {
            continue;
        };
        if !ids.insert(id.clone()) {
            return Err(format!("wpctl list returned duplicate sink ID {id}"));
        }

        let active = tokens.iter().any(|field| is_default_marker(field));
        let name = tokens[id_index + 1..end_of_name]
            .iter()
            .filter(|field| !is_default_marker(field))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if name.is_empty() && media_index.is_none() {
            return Err(format!(
                "wpctl list returned sink {id} without a name on line {}",
                line_number + 1
            ));
        }

        rows.push(SinkRow { id, name, active });
    }

    Ok(rows)
}

fn parse_node_id(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = value.strip_prefix('*').unwrap_or(value);
    let value = value.strip_suffix('*').unwrap_or(value);
    let value = value.strip_suffix('.').unwrap_or(value);
    if !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<u64>()
            .is_ok_and(|id| id > 0 && id < u32::MAX as u64)
    {
        Some(value)
    } else {
        None
    }
}

fn is_default_marker(value: &str) -> bool {
    let value = value.trim();
    value == "*"
        || value
            .strip_prefix('*')
            .is_some_and(|rest| parse_node_id(rest).is_some())
        || value
            .strip_suffix('*')
            .is_some_and(|rest| parse_node_id(rest).is_some())
}

/// Parses `wpctl get-volume` output. No `Volume:` line means the node has no
/// mixer volume; a level above 1.0 (software over-amplification) is reported
/// as 100%.
fn parse_volume(output: &str) -> Result<Option<AudioOutputLevel>, String> {
    let Some(line) = output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("Volume:"))
    else {
        return Ok(None);
    };
    let mut fields = line["Volume:".len()..].split_whitespace();
    let volume = fields
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| format!("wpctl get-volume returned an unreadable level: {line:?}"))?;
    let muted = fields.any(|field| field == "[MUTED]");
    Ok(Some(AudioOutputLevel {
        volume_percent: (volume * 100.0).round().min(100.0) as u8,
        muted,
    }))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SinkProperties {
    values: BTreeMap<String, String>,
}

impl SinkProperties {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn values(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

fn parse_properties(output: &str) -> SinkProperties {
    let mut properties = SinkProperties::default();
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches('*').trim();
        if key.is_empty() {
            continue;
        }
        properties
            .values
            .insert(key.to_owned(), parse_property_value(value));
    }
    properties
}

fn parse_property_value(value: &str) -> String {
    let value = value.trim();
    let Some(value) = value.strip_prefix('"') else {
        return value.to_owned();
    };

    let mut parsed = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            parsed.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            break;
        } else {
            parsed.push(character);
        }
    }
    if escaped {
        parsed.push('\\');
    }
    parsed
}

fn classify_sink(sink: &SinkRow, properties: &SinkProperties) -> Option<AudioRoute> {
    const PRIORITY_KEYS: [&str; 11] = [
        "api.bluez5.profile",
        "device.bus",
        "device.profile.name",
        "device.profile.description",
        "port.type",
        "node.name",
        "node.nick",
        "node.description",
        "device.description",
        "device.product.name",
        "factory.name",
    ];

    for key in PRIORITY_KEYS {
        let Some(value) = properties.get(key) else {
            continue;
        };
        if let Some(route) = route_from_property(key, value) {
            return Some(route);
        }
    }

    for (key, value) in properties.values() {
        if let Some(route) = route_from_property(key, value) {
            return Some(route);
        }
    }

    route_from_text(&sink.name)
}

fn route_from_property(key: &str, value: &str) -> Option<AudioRoute> {
    let key = normalize(key);
    let value = normalize(value);
    if key.contains("bluez") || key.contains("bluetooth") {
        return Some(AudioRoute::Bluetooth);
    }
    route_from_text(&value)
}

fn route_from_text(value: &str) -> Option<AudioRoute> {
    let value = normalize(value);
    if has_any(&value, &["bluetooth", "bluez"]) {
        return Some(AudioRoute::Bluetooth);
    }
    if has_any(&value, &["earpiece", "receiver", "handset"]) {
        return Some(AudioRoute::Earpiece);
    }
    if has_any(
        &value,
        &[
            "headphone",
            "headphones",
            "headset",
            "earbud",
            "earbuds",
            "wired",
        ],
    ) || value.contains("line out")
        || value.contains("lineout")
    {
        return Some(AudioRoute::Headphones);
    }
    if has_any(
        &value,
        &["speaker", "speakers", "loudspeaker", "builtin", "internal"],
    ) || value.contains("built in")
    {
        return Some(AudioRoute::Speaker);
    }
    None
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

fn has_any(value: &str, candidates: &[&str]) -> bool {
    value
        .split_whitespace()
        .any(|word| candidates.contains(&word))
}

fn check_output(executable: &OsStr, args: &[OsString], output: Output) -> Result<String, String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        let detail = if detail.is_empty() {
            String::from("no diagnostic output")
        } else {
            format!(": {detail}")
        };
        return Err(format!(
            "{} failed with {}{detail}",
            command_line(executable, args),
            exit_status(&output),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn command_line(executable: &OsStr, args: &[OsString]) -> String {
    let mut command = executable.to_string_lossy().into_owned();
    for argument in args {
        command.push(' ');
        command.push_str(&argument.to_string_lossy());
    }
    command
}

fn exit_status(output: &Output) -> String {
    output.status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| format!("exit status {code}"),
    )
}

fn audio_failure(reason: String) -> HardwareError {
    HardwareError::failed(Capability::Audio, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tabular_sink_rows_and_default_marker() {
        let rows = parse_sink_list(
            "id\tname\tmedia type\tdefault\n42\tPixel Speaker\tAudio/Sink\t*\n43\tWired Headphones\tAudio/Sink\t",
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![
                SinkRow {
                    id: "42".to_owned(),
                    name: "Pixel Speaker".to_owned(),
                    active: true,
                },
                SinkRow {
                    id: "43".to_owned(),
                    name: "Wired Headphones".to_owned(),
                    active: false,
                },
            ]
        );
    }

    #[test]
    fn parses_compact_rows_with_marker_attached_to_id() {
        let rows = parse_sink_list("*42\tbluez_output.AA_BB\tAudio/Sink\tNode").unwrap();
        assert_eq!(rows[0].id, "42");
        assert!(rows[0].active);
    }

    #[test]
    fn rejects_duplicate_sink_ids() {
        let error = parse_sink_list("42\tSpeaker\tAudio/Sink\t*\n42\tHeadphones\tAudio/Sink\t")
            .unwrap_err();
        assert!(error.contains("duplicate sink ID 42"));
    }

    #[test]
    fn parses_inspect_properties_and_unescapes_values() {
        let properties = parse_properties(
            r#"
                node.name = "alsa_output.platform\x2dspeaker"
                node.description = "Pixel \"Speaker\""
                media.class = "Audio/Sink"
            "#,
        );
        assert_eq!(
            properties.get("node.name"),
            Some("alsa_output.platformx2dspeaker")
        );
        assert_eq!(
            properties.get("node.description"),
            Some("Pixel \"Speaker\"")
        );
        assert_eq!(properties.get("media.class"), Some("Audio/Sink"));
    }

    #[test]
    fn classification_prefers_bluetooth_properties() {
        let properties = parse_properties(
            r#"
                node.description = "Bluetooth Headset"
                api.bluez5.profile = "a2dp-sink"
            "#,
        );
        let sink = SinkRow {
            id: "42".to_owned(),
            name: "Headset".to_owned(),
            active: false,
        };
        assert_eq!(
            classify_sink(&sink, &properties),
            Some(AudioRoute::Bluetooth)
        );
    }

    #[test]
    fn unclassifiable_sinks_are_ignored() {
        let sink = SinkRow {
            id: "42".to_owned(),
            name: "Unknown output".to_owned(),
            active: false,
        };
        assert_eq!(classify_sink(&sink, &SinkProperties::default()), None);
    }

    #[test]
    fn parses_get_volume_levels_and_mute() {
        assert_eq!(
            parse_volume("Volume: 0.40\n").unwrap(),
            Some(AudioOutputLevel {
                volume_percent: 40,
                muted: false
            })
        );
        assert_eq!(
            parse_volume("Volume: 0.07 [MUTED]\n").unwrap(),
            Some(AudioOutputLevel {
                volume_percent: 7,
                muted: true
            })
        );
        assert_eq!(
            parse_volume("Volume: 1.50\n")
                .unwrap()
                .unwrap()
                .volume_percent,
            100,
            "over-amplification clamps"
        );
        assert_eq!(parse_volume("").unwrap(), None, "node without volume");
        assert!(parse_volume("Volume: nan\n").is_err());
        assert!(parse_volume("Volume: -0.2\n").is_err());
    }

    /// A fake `wpctl` that records its arguments and answers the read
    /// commands, so the exact command lines are tested without WirePlumber.
    #[cfg(unix)]
    fn fake_wpctl(dir: &std::path::Path) -> (WpctlTransport, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("calls");
        let script = dir.join("wpctl");
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
case "$1" in
  list) printf '51\tPixel Speaker\tAudio/Sink\t*\n52\tWired Headphones\tAudio/Sink\t\n' ;;
  inspect) printf '  node.description = "%s"\n' "$( [ "$2" = 51 ] && echo Speaker || echo Headphones )" ;;
  get-volume) printf 'Volume: 0.35 [MUTED]\n' ;;
esac
"#,
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (
            WpctlTransport::with_config(script.into_os_string(), DEFAULT_POLL_INTERVAL),
            log,
        )
    }

    #[cfg(unix)]
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrix-wpctl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn reads_the_default_sink_level_only() {
        let dir = scratch("read");
        let (transport, log) = fake_wpctl(&dir);
        let endpoints = transport.endpoints().unwrap();
        assert_eq!(
            endpoints,
            vec![
                RouteEndpoint {
                    id: "51".to_owned(),
                    route: AudioRoute::Speaker,
                    active: true,
                    level: Some(AudioOutputLevel {
                        volume_percent: 35,
                        muted: true
                    }),
                },
                RouteEndpoint {
                    id: "52".to_owned(),
                    route: AudioRoute::Headphones,
                    active: false,
                    level: None,
                },
            ]
        );
        let calls = std::fs::read_to_string(&log).unwrap();
        assert_eq!(calls.matches("get-volume").count(), 1);
        assert!(calls.contains("get-volume 51\n"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn level_writes_use_exact_bounded_wpctl_commands() {
        let dir = scratch("write");
        let (transport, log) = fake_wpctl(&dir);
        transport.set_volume("51", 7).unwrap();
        transport.set_volume("51", 100).unwrap();
        transport.set_muted("51", true).unwrap();
        transport.set_muted("51", false).unwrap();
        assert!(transport.set_volume("@DEFAULT_AUDIO_SINK@", 5).is_err());
        assert!(transport.set_muted("51;x", true).is_err());
        assert!(transport.set_volume("51", 101).is_err());
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "set-volume --limit 1.0 51 0.07\n\
             set-volume --limit 1.0 51 1.00\n\
             set-mute 51 1\n\
             set-mute 51 0\n"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn command_start_errors_name_the_command() {
        let transport = WpctlTransport::with_config(
            OsString::from("/definitely/missing/ferrix-wpctl"),
            DEFAULT_POLL_INTERVAL,
        );
        let error = transport.run([OsStr::new("list")]).unwrap_err();
        assert!(error.contains("failed to start /definitely/missing/ferrix-wpctl list"));
    }
}
