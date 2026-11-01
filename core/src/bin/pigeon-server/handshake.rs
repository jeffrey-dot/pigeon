//! `hello` / `hello_ack` (protocol §3.1, §5) and the `sessions` list that follows it.

use std::sync::OnceLock;
use std::time::Duration;

use pigeon_core::proto::{Message, PROTO_VERSION};

use crate::client_state::ClientState;
use crate::host_state::HostState;

/// Capabilities this build offers (protocol §5).
const SERVER_FEATURES: &[&str] = &["delta", "scrollback", "osc7501", "status_all"];

/// Protocol §3.1: a connection that has not said `hello` by then is dropped.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Protocol §1: 45 s without any inbound frame is a dead link (clients ping every 15 s).
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// How long a new connection may take to send `hello` (`PIGEON_HELLO_MS` for the tests).
pub fn hello_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| from_env("PIGEON_HELLO_MS").unwrap_or(HELLO_TIMEOUT))
}

/// How long a greeted connection may stay silent (`PIGEON_IDLE_MS` for the tests).
pub fn idle_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| from_env("PIGEON_IDLE_MS").unwrap_or(IDLE_TIMEOUT))
}

fn from_env(name: &str) -> Option<Duration> {
    std::env::var(name)
        .ok()?
        .parse::<u64>()
        .ok()
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
}

/// The connection counts as greeted only after a successful `hello_ack`; a version
/// mismatch leaves it un-greeted, and the caller closes it after the `hello_error`.
pub fn hello(proto: u16, features: &[String], state: &mut ClientState) -> Message {
    if proto >> 8 != PROTO_VERSION >> 8 {
        return Message::HelloError {
            reason: "version".into(),
            detail: format!("need {PROTO_VERSION:#06x}"),
        };
    }
    state.greeted = true;
    let offered: Vec<String> = SERVER_FEATURES
        .iter()
        .filter(|feature| features.iter().any(|asked| asked == *feature))
        .map(|feature| (*feature).to_owned())
        .collect();
    state.delta_ok = offered.iter().any(|f| f == "delta");
    state.status_all = offered.iter().any(|f| f == "status_all");
    Message::HelloAck {
        proto: PROTO_VERSION,
        server: format!("pigeon-server {}", env!("CARGO_PKG_VERSION")),
        features: offered,
        host_id: hostname(),
        client_id: state.client_id,
        instance: Some(instance().to_owned()),
    }
}

/// The `sessions` reply: one sidebar row per session.
pub async fn sessions(host: &HostState) -> Message {
    let map = host.sessions.lock().await;
    let meta = host.meta.lock().await;
    Message::Sessions {
        items: map
            .iter()
            .map(|(session_id, session)| session.info(*session_id, &meta))
            .collect(),
    }
}

/// This process's identity for `hello_ack.instance`: pid plus start time is unique enough
/// to tell a restarted host from the one a client last saw.
fn instance() -> &'static str {
    static INSTANCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| {
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        format!("{:x}-{}", started, std::process::id())
    })
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "local".to_owned())
}
