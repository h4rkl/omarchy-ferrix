//! Unix-socket transport: one thread per connection, line-delimited JSON,
//! peer pid from the kernel on Linux. Malformed lines get a protocol error
//! and the connection stays open; a subscription turns the connection into
//! an event stream until the peer disconnects.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use ferrix_contacts_protocol::{Reply, Request};

use crate::service::{Connection, Outcome, Service};

pub struct Server {
    service: Arc<Service>,
}

impl Server {
    pub fn new(service: Arc<Service>) -> Self {
        Self { service }
    }

    /// Binds `path`, replacing a stale socket file.
    pub fn bind(path: &Path) -> Result<UnixListener, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))?;
        }
        UnixListener::bind(path).map_err(|e| format!("bind {}: {e}", path.display()))
    }

    /// Accepts forever.
    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    if thread::Builder::new()
                        .name("contactsd-conn".to_owned())
                        .spawn(move || server.handle(stream))
                        .is_err()
                    {
                        self.service.recorder().emit(
                            "contactsd-socket-error",
                            [("error", serde_json::Value::from("spawn failed"))],
                        );
                    }
                }
                Err(error) => self.service.recorder().emit(
                    "contactsd-socket-error",
                    [("error", serde_json::Value::from(error.to_string()))],
                ),
            }
        }
    }

    fn handle(&self, stream: UnixStream) {
        let mut connection = Connection::with_pid(peer_pid(&stream));
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let Ok(line) = line else {
                return;
            };
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            let request: Request = match serde_json::from_str(text) {
                Ok(request) => request,
                Err(error) => {
                    let reply = Reply::ProtocolError {
                        reason: format!("malformed request: {error}"),
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                    continue;
                }
            };
            let (outcome, events) = self.service.dispatch(&mut connection, request);
            match outcome {
                Outcome::Reply(reply) => {
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Outcome::Subscribe(reply) => {
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                    let Some(events) = events else {
                        return;
                    };
                    for event in events {
                        if write_json(&mut writer, &event).is_err() {
                            return;
                        }
                    }
                    return;
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .and_then(|credentials| u32::try_from(credentials.pid.as_raw_pid()).ok())
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}

fn write_json(stream: &mut UnixStream, value: &impl serde::Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AllowAll;
    use crate::records::Recorder;
    use crate::store::temp_dir;
    use ferrix_contacts_api::{ContactEvent, ContactInput, NumberInput, NumberLabel};
    use ferrix_contacts_protocol::{Client, ClientKind};
    use std::time::Duration;

    #[test]
    fn socket_round_trip_with_malformed_lines_and_subscription() {
        let dir = temp_dir("socket");
        let (recorder, _) = Recorder::capturing("mock");
        let service = Service::for_tests(&dir, Box::new(AllowAll), recorder, || 5).unwrap();
        let socket = dir.join("contactsd.sock");
        let listener = Server::bind(&socket).unwrap();
        let server = Arc::new(Server::new(Arc::clone(&service)));
        thread::spawn(move || server.serve(listener));

        // A malformed line is answered and does not kill the connection.
        let mut raw = UnixStream::connect(&socket).unwrap();
        raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        raw.write_all(b"{\"request\":\"hello\",\"schema_version\":1,\"client_kind\":\"tool\"}\nnot json\n{\"request\":\"list\",\"bogus\":1}\n{\"request\":\"list\"}\n")
            .unwrap();
        let mut lines = BufReader::new(raw.try_clone().unwrap()).lines();
        assert!(
            lines
                .next()
                .unwrap()
                .unwrap()
                .contains("\"reply\":\"hello\"")
        );
        assert!(lines.next().unwrap().unwrap().contains("protocol-error"));
        assert!(lines.next().unwrap().unwrap().contains("protocol-error"));
        assert!(
            lines
                .next()
                .unwrap()
                .unwrap()
                .contains("\"reply\":\"contacts\"")
        );

        let mut client =
            Client::connect(&socket, ClientKind::Tool, Duration::from_secs(2)).unwrap();
        let subscriber = Client::connect(&socket, ClientKind::Application, Duration::from_secs(2))
            .unwrap()
            .subscribe()
            .unwrap();
        assert!(subscriber.contacts.is_empty());
        let events = subscriber.spawn(Arc::new(|| {}));
        let created = client
            .create(ContactInput {
                display_name: "Test Contact 01".to_owned(),
                given: None,
                family: None,
                numbers: vec![NumberInput {
                    label: NumberLabel::Mobile,
                    raw: "+15550100001".to_owned(),
                }],
            })
            .unwrap();
        let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(event, ContactEvent::ContactChanged { id, .. } if id == created.id));
        assert_eq!(client.list().unwrap().len(), 1);
        assert_eq!(
            client.lookup("(555) 010-0001").unwrap().unwrap().id,
            created.id
        );
        assert!(client.lookup("+15550100077").unwrap().is_none());
        client.delete(&created.id).unwrap();
        assert!(matches!(client.get(&created.id), Err(Ok(_))));
        std::fs::remove_dir_all(&dir).ok();
    }
}
