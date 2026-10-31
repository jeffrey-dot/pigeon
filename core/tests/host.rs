//! End-to-end: a client attaches, types, disconnects, reconnects — and the shell
//! is still there with the same screen. This is the M0 promise ("关闭窗口进程不死"),
//! proven against the real host binary over the real socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Every host test spawns a real shell (and sometimes vim/htop/less). Running
/// them in parallel starves the machine and makes timing assertions flaky.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use pigeon_core::proto::{self, Message};

struct Host {
    child: Child,
    socket: String,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        // The host saves its screens next to the socket (`persist`).
        let _ = std::fs::remove_file(std::path::Path::new(&self.socket).with_extension("state"));
    }
}

fn start_host() -> Host {
    // An empty prompt keeps the grid still once a command finishes, so the
    // before/after comparison is not racing a redraw.
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    start_host_with(quiet.to_str().expect("utf8"))
}

/// Tests run in parallel: pid + counter + clock keeps socket paths unique.
fn unique_socket() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = format!("pigeon-test-{}-{counter}-{stamp}", std::process::id());
    let socket = format!("/tmp/{unique}.sock");
    let _ = std::fs::remove_file(&socket);
    socket
}

fn start_host_with(program: &str) -> Host {
    start_host_on(&unique_socket(), program)
}

/// A quiet host on a given socket (the state-file tests prepare files next to it).
fn start_host_with_socket(socket: &str) -> Host {
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    start_host_on(socket, quiet.to_str().expect("utf8"))
}

fn start_host_on(socket: &str, program: &str) -> Host {
    let socket = socket.to_owned();
    let child = Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
        .args(["--socket", &socket, "--shell", program])
        .stdout(Stdio::null())
        .stderr(if std::env::var_os("PIGEON_DEBUG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .expect("spawn pigeon-host");
    let host = Host { child, socket };

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if std::path::Path::new(&host.socket).exists() {
            return host;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("host never created its socket");
}

/// A host started the way a remote one is: `--stdio` serves this process's stdio and keeps
/// the socket for later `--proxy` clients (docs/remote.md §2).
fn start_stdio_host() -> (Host, Client) {
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    let socket = unique_socket();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
        .args([
            "--stdio",
            "--socket",
            &socket,
            "--shell",
            quiet.to_str().expect("utf8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if std::env::var_os("PIGEON_DEBUG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .expect("spawn pigeon-host --stdio");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let host = Host {
        child,
        socket: socket.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if std::path::Path::new(&host.socket).exists() {
            return (host, Client::new(Box::new(stdout), Box::new(stdin)));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("host never created its socket");
}

/// Kills a helper process (the proxy) when the test ends.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Client {
    writer: Box<dyn Write + Send>,
    /// Bytes the transport delivered; a thread reads them so both a socket and a child's
    /// pipe can be driven the same way (a pipe has no read timeout to fall back on).
    incoming: std::sync::mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    /// Last snapshot grid, kept so a timeout can show what the pane actually said.
    last_screen: String,
}

impl Client {
    fn new(mut reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Self {
        let (tx, incoming) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            let mut chunk = [0u8; 4096];
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.send(chunk[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            writer,
            incoming,
            buffer: Vec::new(),
            last_screen: String::new(),
        }
    }

    fn connect(socket: &str) -> Self {
        let stream = UnixStream::connect(socket).expect("connect");
        let writer = stream.try_clone().expect("clone the socket for writing");
        Self::new(Box::new(stream), Box::new(writer))
    }

    fn send(&mut self, message: &Message) {
        let frame = proto::encode_frame(message).expect("encode");
        self.writer.write_all(&frame).expect("write");
    }

    /// Reads until a message matching `want` arrives, ignoring the rest.
    fn wait_for(&mut self, want: impl Fn(&Message) -> bool) -> Message {
        // Generous: the suite runs several real shells at once and a loaded machine can
        // stall a redraw for a while (the "flaky" test was always this deadline).
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(found) = self.drain(&want) {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a message; last screen:\n{}",
                self.last_screen
            );
            match self.incoming.recv_timeout(Duration::from_millis(20)) {
                Ok(chunk) => self.buffer.extend_from_slice(&chunk),
                // Nothing yet; the loop deadline decides when to give up.
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("host closed the connection")
                }
            }
        }
    }

    fn drain(&mut self, want: &impl Fn(&Message) -> bool) -> Option<Message> {
        loop {
            match proto::decode_frame(&self.buffer) {
                Ok(Some((message, used))) => {
                    self.buffer.drain(..used);
                    if let Message::Snapshot { .. } = &message {
                        self.last_screen = screen_text(&message);
                    }
                    if want(&message) {
                        return Some(message);
                    }
                }
                Ok(None) => return None,
                Err(error) => panic!("protocol error: {error}"),
            }
        }
    }
}

fn hello(client: &mut Client) {
    client.send(&Message::Hello {
        proto: proto::PROTO_VERSION,
        client: "host-test".into(),
        features: vec!["scrollback".into()],
    });
    let ack = client.wait_for(|m| matches!(m, Message::HelloAck { .. }));
    assert!(matches!(ack, Message::HelloAck { .. }));
}

fn screen_text(message: &Message) -> String {
    let Message::Snapshot { screen, .. } = message else {
        panic!("expected a snapshot, got {:?}", message.name());
    };
    screen
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Drops the only field that legitimately changes between two attaches.
fn normalized(message: &Message) -> Message {
    match message.clone() {
        Message::Snapshot {
            pane_id,
            cols,
            rows,
            alt,
            cursor,
            scroll,
            screen,
            history,
            links,
            ..
        } => Message::Snapshot {
            pane_id,
            seq: 0,
            cols,
            rows,
            alt,
            cursor,
            scroll,
            screen,
            history,
            links,
            history_len: None,
            offset: None,
            modes: None,
        },
        other => other,
    }
}

/// Re-attaches until two consecutive snapshots are identical (all cells,
/// attributes, colours, cursor, alt flag and scroll region), so a live shell
/// cannot race the comparison.
fn stable_snapshot(client: &mut Client, pane_id: u32) -> Message {
    let mut streak = 0;
    let mut previous: Option<Message> = None;
    for _ in 0..30 {
        client.send(&Message::Attach {
            id: 99,
            pane_id,
            size_policy: None,
            history: None,
            since: None,
        });
        let message = normalized(&snapshot(client));
        if previous.as_ref() == Some(&message) {
            streak += 1;
            if streak >= 3 {
                return message;
            }
        } else {
            streak = 0;
            previous = Some(message);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    previous.unwrap_or_else(|| normalized(&snapshot(client)))
}

/// Everything the acceptance criterion names: grid, attributes, colours and
/// widths (inside the cells), cursor, alternate-screen flag and scroll region.
/// Scrollback is excluded: it may legitimately grow between two attaches.
fn comparable(
    message: &Message,
) -> (
    u16,
    u16,
    bool,
    proto::Cursor,
    proto::Scroll,
    Vec<Vec<proto::Cell>>,
) {
    let Message::Snapshot {
        cols,
        rows,
        alt,
        cursor,
        scroll,
        screen,
        ..
    } = normalized(message)
    else {
        panic!("expected a snapshot");
    };
    (cols, rows, alt, cursor, scroll, screen)
}

/// Waits until *this pane's* snapshot contains `needle`.
fn wait_for_pane_text(client: &mut Client, pane: u32, needle: &str) {
    client.wait_for(|message| match message {
        Message::Snapshot { pane_id, .. } if *pane_id == pane => {
            screen_text(message).contains(needle)
        }
        _ => false,
    });
}

/// Waits until some snapshot's grid contains `needle`.
fn wait_for_text(client: &mut Client, needle: &str) {
    client.wait_for(|message| match message {
        Message::Snapshot { .. } => screen_text(message).contains(needle),
        _ => false,
    });
}

fn snapshot(client: &mut Client) -> Message {
    client.wait_for(|m| matches!(m, Message::Snapshot { .. }))
}

#[test]
fn shell_outlives_the_client_and_the_screen_is_restored() {
    let _serial = one_at_a_time();
    let host = start_host();

    // First client: create a session, attach, type a marker.
    let mut first = Client::connect(&host.socket);
    hello(&mut first);
    first.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } = first.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    first.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: None,
        since: None,
    });
    let initial = snapshot(&mut first);
    let Message::Snapshot {
        rows: row_count,
        cols,
        ..
    } = initial
    else {
        unreachable!()
    };
    assert_eq!((row_count, cols), (24, 80), "default grid size");

    first.send(&Message::Keys {
        pane_id,
        bytes: b"echo pigeon-\"\"marker\r".to_vec(),
    });
    // Wait until the shell has echoed the command into the grid.
    wait_for_text(&mut first, "pigeon-marker");
    let before = stable_snapshot(&mut first, pane_id);
    let before_text = screen_text(&before);
    assert!(before_text.contains("pigeon-marker"), "{before_text}");

    drop(first); // the "window closes"
    std::thread::sleep(Duration::from_millis(300));

    // Second client: the pane and its shell are still there, screen unchanged.
    let mut second = Client::connect(&host.socket);
    hello(&mut second);
    second.send(&Message::Sessions { items: Vec::new() });
    let Message::Sessions { items } = second.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(
        items.iter().any(|item| item.panes.contains(&pane_id)),
        "the pane survived the disconnect"
    );

    let after = stable_snapshot(&mut second, pane_id);
    assert_eq!(
        comparable(&after),
        comparable(&before),
        "reconnecting must restore the same grid: cells, attributes, colours, cursor, alt flag"
    );

    // The shell is still alive: the marker command runs again.
    second.send(&Message::Keys {
        pane_id,
        bytes: b"echo still-alive\r".to_vec(),
    });
    wait_for_text(&mut second, "still-alive");
}

#[test]
fn bad_version_is_rejected_with_hello_error() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    client.send(&Message::Hello {
        proto: 0x0200,
        client: "host-test".into(),
        features: Vec::new(),
    });
    let reply = client.wait_for(|m| matches!(m, Message::HelloError { .. }));
    assert!(matches!(reply, Message::HelloError { .. }));
}

#[test]
fn unknown_message_gets_unsupported_and_the_connection_survives() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);

    // A well-formed frame with an unknown `t`, built by hand.
    let payload = ciborium::value::Value::Map(vec![(
        ciborium::value::Value::Text("t".into()),
        ciborium::value::Value::Text("definitely_not_a_message".into()),
    )]);
    let mut body = Vec::new();
    ciborium::into_writer(&payload, &mut body).expect("encode");
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&body);
    client.writer.write_all(&frame).expect("write");

    let reply = client.wait_for(|m| matches!(m, Message::Unsupported { .. }));
    let Message::Unsupported { t_of } = reply else {
        unreachable!()
    };
    assert_eq!(t_of, "definitely_not_a_message");

    // Still usable afterwards.
    client.send(&Message::Ping { ts: 9 });
    let Message::Pong { ts } = client.wait_for(|m| matches!(m, Message::Pong { .. })) else {
        unreachable!()
    };
    assert_eq!(ts, 9);
}

#[test]
fn resize_reaches_the_shell() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    client.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);

    client.send(&Message::ResizePane {
        pane_id,
        cols: 100,
        rows: 30,
    });
    // The shell only reports the new size once its PTY has been resized.
    client.send(&Message::Keys {
        pane_id,
        bytes: b"stty size\r".to_vec(),
    });
    client.wait_for(|message| match message {
        Message::Snapshot { .. } => screen_text(message).contains("30 100"),
        _ => false,
    });
}

/// Attaches `client` to `pane`, optionally asking for a size policy.
fn attach(client: &mut Client, pane: u32, policy: Option<&str>) {
    client.send(&Message::Attach {
        id: 2,
        pane_id: pane,
        size_policy: policy.map(str::to_owned),
        history: None,
        since: None,
    });
}

/// Creates a session and attaches it with the given policy (the first attach decides it).
fn attach_first_with_policy(client: &mut Client, policy: Option<&str>) -> u32 {
    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    attach(client, pane_id, policy);
    let _ = snapshot(client);
    pane_id
}

/// Waits for a `size_state` for `pane` that says exactly this. Announcements from
/// earlier attaches are skipped, so the assertion is about the resize under test.
fn expect_size_state(client: &mut Client, pane: u32, want: (u16, u16, &str, u8)) {
    client.wait_for(|m| match m {
        Message::SizeState {
            pane_id,
            cols,
            rows,
            source,
            clients,
        } => *pane_id == pane && (*cols, *rows, source.as_str(), *clients) == want,
        _ => false,
    });
}

/// `stty size` inside the pane: what the PTY actually got, not what was asked for.
fn pty_size(client: &mut Client, pane: u32) -> String {
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"stty size\r".to_vec(),
    });
    let signal = client.wait_for(|message| match message {
        Message::Snapshot { .. } => screen_text(message).lines().any(|line| {
            let line = line.trim();
            line.split_once(' ').is_some_and(|(rows, cols)| {
                rows.parse::<u16>().is_ok() && cols.parse::<u16>().is_ok()
            })
        }),
        _ => false,
    });
    screen_text(&signal)
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| {
            line.split_once(' ').is_some_and(|(rows, cols)| {
                rows.parse::<u16>().is_ok() && cols.parse::<u16>().is_ok()
            })
        })
        .unwrap_or_default()
        .to_owned()
}

/// `--stdio`: the client on this process's stdin/stdout is a normal client, and the socket
/// the same process binds stays usable — that is how a remote host outlives the window that
/// started it.
#[test]
fn stdio_mode_serves_a_client_and_keeps_the_socket_for_later() {
    let _serial = one_at_a_time();
    let (host, mut client) = start_stdio_host();
    hello(&mut client);
    let pane = attach_first(&mut client);

    // The socket sees the session the stdio client created.
    let mut later = Client::connect(&host.socket);
    hello(&mut later);
    later.send(&Message::Sessions { items: Vec::new() });
    let Message::Sessions { items } = later.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(
        items.iter().any(|session| session.panes.contains(&pane)),
        "the session created over stdio is visible on the socket: {items:?}"
    );
}

/// `--proxy`: the ssh bridge forwards the frames untouched, so the host behind it is
/// indistinguishable from a local one.
#[test]
fn proxy_mode_bridges_stdio_to_a_running_host() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut proxy = Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
        .args(["--proxy", "--socket", &host.socket])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pigeon-host --proxy");
    let stdin = proxy.stdin.take().expect("stdin");
    let stdout = proxy.stdout.take().expect("stdout");
    let _guard = Guard(proxy);
    let mut client = Client::new(Box::new(stdout), Box::new(stdin));

    hello(&mut client);
    let pane = attach_first(&mut client);
    // The command is echoed, so wait for its *result*: 6 * 7 shows up only as output.
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo $((6*7))\r".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "42");
}

/// `--proxy` with no host behind the socket starts one (issue #3): the first `ssh host
/// pigeon-host --proxy` after a reboot must not fail, and the host it starts must outlive
/// the bridge, or closing the window would end the remote sessions.
#[test]
fn proxy_mode_starts_a_host_when_none_is_running() {
    let _serial = one_at_a_time();
    let socket = unique_socket();
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    // Whatever the outcome, the host this test caused must not linger.
    struct Reap(String);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = Command::new("pkill").args(["-f", &self.0]).status();
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(std::path::Path::new(&self.0).with_extension("state"));
        }
    }
    let _reap = Reap(socket.clone());

    let mut proxy = Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
        .args([
            "--proxy",
            "--socket",
            &socket,
            "--shell",
            quiet.to_str().expect("utf8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pigeon-host --proxy");
    let stdin = proxy.stdin.take().expect("stdin");
    let stdout = proxy.stdout.take().expect("stdout");
    let guard = Guard(proxy);
    let mut client = Client::new(Box::new(stdout), Box::new(stdin));
    hello(&mut client);
    let pane = attach_first(&mut client);
    drop(client);
    drop(guard); // the ssh channel closes

    let mut later = Client::connect(&socket);
    hello(&mut later);
    later.send(&Message::Sessions { items: Vec::new() });
    let Message::Sessions { items } = later.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(
        items.iter().any(|session| session.panes.contains(&pane)),
        "the host started by --proxy kept the session: {items:?}"
    );
}

/// Protocol §6 `smallest`: two windows of different sizes get the smaller box, and both
/// are told who won and how many clients are watching.
#[test]
fn size_policy_smallest_shrinks_to_the_smallest_client() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut first = Client::connect(&host.socket);
    hello(&mut first);
    let pane = attach_first_with_policy(&mut first, Some("smallest"));

    first.send(&Message::ResizePane {
        pane_id: pane,
        cols: 100,
        rows: 30,
    });
    expect_size_state(&mut first, pane, (100, 30, "smallest", 1));

    let mut second = Client::connect(&host.socket);
    hello(&mut second);
    attach(&mut second, pane, None);
    let _ = snapshot(&mut second);
    second.send(&Message::ResizePane {
        pane_id: pane,
        cols: 60,
        rows: 20,
    });

    expect_size_state(&mut first, pane, (60, 20, "smallest", 2));
    expect_size_state(&mut second, pane, (60, 20, "smallest", 2));
    assert_eq!(pty_size(&mut first, pane), "20 60");
}

/// Protocol §6 `latest`: whoever typed last owns the size.
#[test]
fn size_policy_latest_gives_the_box_back_to_the_typing_client() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut first = Client::connect(&host.socket);
    hello(&mut first);
    let pane = attach_first_with_policy(&mut first, Some("latest"));

    first.send(&Message::ResizePane {
        pane_id: pane,
        cols: 100,
        rows: 30,
    });
    expect_size_state(&mut first, pane, (100, 30, "latest", 1));

    let mut second = Client::connect(&host.socket);
    hello(&mut second);
    attach(&mut second, pane, None);
    let _ = snapshot(&mut second);
    second.send(&Message::ResizePane {
        pane_id: pane,
        cols: 60,
        rows: 20,
    });
    expect_size_state(&mut first, pane, (60, 20, "latest", 2));

    // Typing in the first window takes the size back.
    first.send(&Message::Keys {
        pane_id: pane,
        bytes: b"\r".to_vec(),
    });
    expect_size_state(&mut first, pane, (100, 30, "latest", 2));
    assert_eq!(pty_size(&mut first, pane), "30 100");
}

/// Protocol §6 `locked`: the second client's resize is reported back as ignored.
#[test]
fn size_policy_locked_ignores_the_newcomer() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut first = Client::connect(&host.socket);
    hello(&mut first);
    let pane = attach_first_with_policy(&mut first, Some("locked"));

    first.send(&Message::ResizePane {
        pane_id: pane,
        cols: 100,
        rows: 30,
    });
    expect_size_state(&mut first, pane, (100, 30, "locked", 1));

    let mut second = Client::connect(&host.socket);
    hello(&mut second);
    // A later `attach` only confirms the policy the first one set.
    attach(&mut second, pane, Some("smallest"));
    let _ = snapshot(&mut second);
    second.send(&Message::ResizePane {
        pane_id: pane,
        cols: 60,
        rows: 20,
    });
    expect_size_state(&mut second, pane, (100, 30, "locked", 2));
    assert_eq!(pty_size(&mut first, pane), "30 100");
}

/// One grid of the protocol as text, for readable assertions.
fn rows_text(rows: &[Vec<proto::Cell>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The client can only scroll back what the snapshot actually carries: the host has to send
/// the history rows, not just the live screen.
#[test]
fn scrollback_history_reaches_the_client_in_the_snapshot() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"seq 1 40\r".to_vec(),
    });
    // Wait for the *output* line `40`, not the echoed command (`wait_for_pane_text` would
    // match "40" inside "seq 1 40").
    client.wait_for(|message| match message {
        Message::Snapshot { screen, .. } => {
            rows_text(screen).lines().any(|line| line.trim() == "40")
        }
        _ => false,
    });
    // History only rides on `attach`: that is what a client sends when the user starts to
    // scroll back (docs/protocol.md §3.3).
    client.send(&Message::Attach {
        id: 9,
        pane_id: pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let message = client.wait_for(|message| match message {
        Message::Snapshot {
            pane_id,
            history: Some(history),
            ..
        } => {
            let text = rows_text(history);
            // The oldest lines scrolled out of the 24-row screen into the history.
            *pane_id == pane
                && text.lines().any(|line| line.trim() == "1")
                && text.lines().any(|line| line.trim() == "2")
        }
        _ => false,
    });
    let Message::Snapshot { screen, .. } = message else {
        unreachable!()
    };
    assert!(
        rows_text(&screen).lines().any(|line| line.trim() == "40"),
        "the newest line stays on the live screen:\n{}",
        rows_text(&screen)
    );
}

/// Ten thousand lines, reachable by `scroll` without the backlog ever crossing the wire.
#[test]
fn scroll_reaches_ten_thousand_lines_of_history() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    // More output than the host keeps: 20000 lines through a 24-row screen.
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"seq 1 20000\r".to_vec(),
    });
    client.wait_for(|message| match message {
        Message::Snapshot { screen, .. } => {
            rows_text(screen).lines().any(|line| line.trim() == "20000")
        }
        _ => false,
    });

    // A live snapshot may still be in flight, so wait for one that actually looks back.
    let wait_top = |client: &mut Client, below: u32| -> (u32, Option<u32>, Option<u32>) {
        let message = client.wait_for(|message| match message {
            Message::Snapshot { screen, .. } => rows_text(screen)
                .lines()
                .find_map(|line| line.trim().parse::<u32>().ok())
                .is_some_and(|top| top < below),
            _ => false,
        });
        let Message::Snapshot {
            screen,
            history_len,
            offset,
            ..
        } = &message
        else {
            unreachable!()
        };
        (
            rows_text(screen)
                .lines()
                .find_map(|line| line.trim().parse::<u32>().ok())
                .unwrap_or_default(),
            *history_len,
            *offset,
        )
    };

    // 5000 lines back from the live screen (line 20000) lands near 15000, not at the top.
    client.send(&Message::Scroll {
        pane_id: pane,
        offset: -5000,
    });
    let (at_15k, total, at) = wait_top(&mut client, 19_950);
    // The indicator needs both numbers: how long the backlog is and where this view sits.
    assert_eq!(
        total,
        Some(10_000),
        "the host reports the whole 10k backlog, not just the visible slice"
    );
    assert_eq!(at, Some(5_000), "and the offset it actually applied");
    // The viewport top moves up by exactly the requested lines: live top ~19977 − 5000.
    assert!(
        (14_970..=14_990).contains(&at_15k),
        "5000 lines back should show line ~14978, got {at_15k}"
    );

    // All the way back: the oldest retained line is ~10000, which is the 10k backlog.
    client.send(&Message::Scroll {
        pane_id: pane,
        offset: -20_000,
    });
    let (oldest, _, clamped) = wait_top(&mut client, 10_100);
    assert_eq!(
        clamped,
        Some(10_000),
        "scrolling past the top clamps to the backlog"
    );
    // 20000 lines fed, 24 still on screen, 10000 kept as backlog: oldest ~9977.
    assert!(
        (9_960..=9_990).contains(&oldest),
        "the backlog is 10000 lines, oldest retained ~9977, got {oldest}"
    );

    // `offset: 0` is the live screen again.
    client.send(&Message::Scroll {
        pane_id: pane,
        offset: 0,
    });
    let live = client.wait_for(|m| matches!(m, Message::Snapshot { .. }));
    let live_text = screen_text(&live);
    assert!(
        live_text.lines().any(|line| line.trim() == "20000"),
        "back to the bottom:\n{live_text}"
    );

    // An unknown pane is an error, not a silent success.
    client.send(&Message::Scroll {
        pane_id: 9999,
        offset: -1,
    });
    let Message::Error { code, .. } = client.wait_for(|m| matches!(m, Message::Error { .. }))
    else {
        unreachable!()
    };
    assert_eq!(code, "no_such_pane");
}

/// The sidebar row names the program, not whatever the shell puts in its window title: fish
/// titles its window `~/dev`, zsh sets the command name. Same rule for every host.
#[test]
fn a_prompt_like_title_is_not_used_as_the_program_name() {
    let _serial = one_at_a_time();
    // A shell whose window title looks like a prompt, then an idle prompt.
    let quiet = script("#!/bin/sh\nprintf '\\033]2;~/dev\\007'\nexec env PS1= /bin/sh -i\n");
    let host = start_host_with(quiet.to_str().expect("utf8"));
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    let message = client.wait_for(|message| match message {
        Message::Sessions { items } => items
            .iter()
            .any(|session| session.active_title.as_deref() == Some("~/dev")),
        _ => false,
    });
    let Message::Sessions { items } = message else {
        unreachable!()
    };
    let session = items
        .iter()
        .find(|session| session.panes.contains(&pane))
        .expect("the session we attached to");
    assert_eq!(
        session.active_title.as_deref(),
        Some("~/dev"),
        "the raw title is kept"
    );
    let expected = quiet.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        session.active_program.as_deref(),
        Some(expected.as_str()),
        "a path is not a program name: fall back to the shell"
    );
}

/// Writes an executable shell script so a pane can run a program with arguments.
fn script(body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::temp_dir().join(format!(
        "pigeon-test-script-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, body).expect("write script");
    let mut permissions = std::fs::metadata(&path).expect("stat").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

fn have_program(name: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Attaches to the first session and returns the pane id.
fn attach_first(client: &mut Client) -> u32 {
    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    client.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(client);
    let _ = wait_for_layout(client, |_| true);
    pane_id
}

#[test]
fn less_pages_forward_when_space_is_pressed() {
    let _serial = one_at_a_time();
    if !have_program("less") {
        eprintln!("less not installed; skipping");
        return;
    }
    let file = std::env::temp_dir().join(format!("pigeon-less-{}.txt", std::process::id()));
    let mut contents = String::new();
    for line in 1..=200 {
        contents.push_str(&format!("line {line}\n"));
    }
    std::fs::write(&file, contents).expect("write demo file");

    let pane = script(&format!("#!/bin/sh\nexec less {}\n", file.display()));
    let host = start_host_with(pane.to_str().expect("utf8"));
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane_id = attach_first(&mut client);

    wait_for_text(&mut client, "line 1");

    // One page down: a line from later in the file must become visible.
    client.send(&Message::Keys {
        pane_id,
        bytes: vec![b' '],
    });
    wait_for_text(&mut client, "line 40");

    client.send(&Message::Keys {
        pane_id,
        bytes: vec![b'q'],
    });
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&pane);
}

#[test]
fn htop_renders_its_panels_and_quits_on_q() {
    let _serial = one_at_a_time();
    if !have_program("htop") {
        eprintln!("htop not installed; skipping");
        return;
    }
    let host = start_host_with("htop");
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane_id = attach_first(&mut client);

    // htop always draws a CPU meter and a task counter.
    wait_for_text(&mut client, "CPU");

    client.send(&Message::Keys {
        pane_id,
        bytes: vec![b'q'],
    });
    let Message::PaneExited { code, .. } =
        client.wait_for(|m| matches!(m, Message::PaneExited { .. }))
    else {
        unreachable!()
    };
    assert_eq!(code, 0, "htop quits cleanly on q");
}

#[test]
fn vim_edits_and_quits() {
    let _serial = one_at_a_time();
    if !have_program("vim") {
        eprintln!("vim not installed; skipping");
        return;
    }
    let file = std::env::temp_dir().join(format!("pigeon-vim-{}.txt", std::process::id()));
    std::fs::write(&file, "hello from vim\n").expect("write file");
    let pane = script(&format!(
        "#!/bin/sh\nexec vim -u NONE -n {}\n",
        file.display()
    ));
    let host = start_host_with(pane.to_str().expect("utf8"));
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane_id = attach_first(&mut client);

    // vim can open with a "Press ENTER" prompt; a real user answers it too.
    client.send(&Message::Keys {
        pane_id,
        bytes: vec![b'\r'],
    });
    wait_for_text(&mut client, "hello from vim");
    client.send(&Message::Keys {
        pane_id,
        bytes: b":q!\r".to_vec(),
    });
    let Message::PaneExited { code, .. } =
        client.wait_for(|m| matches!(m, Message::PaneExited { .. }))
    else {
        unreachable!()
    };
    assert_eq!(code, 0);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&pane);
}

#[test]
fn close_pane_terminates_the_child() {
    let _serial = one_at_a_time();
    let pane = script("#!/bin/sh\nexec sleep 60\n");
    let host = start_host_with(pane.to_str().expect("utf8"));
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane_id = attach_first(&mut client);

    client.send(&Message::ClosePane { id: 9, pane_id });
    let Message::PaneExited {
        pane_id: exited, ..
    } = client.wait_for(|m| matches!(m, Message::PaneExited { .. }))
    else {
        unreachable!()
    };
    assert_eq!(exited, pane_id, "the closed pane reports its exit");

    // And it is gone from the session list.
    client.send(&Message::Sessions { items: Vec::new() });
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(
        !items.iter().any(|item| item.panes.contains(&pane_id)),
        "a closed pane must not be listed"
    );
    let _ = std::fs::remove_file(&pane);
}

// ---------------------------------------------------------------- M1 split

fn splits_of(tree: &proto::Node) -> Vec<(u32, proto::SplitAxis, f32)> {
    let mut out = Vec::new();
    fn walk(node: &proto::Node, out: &mut Vec<(u32, proto::SplitAxis, f32)>) {
        if let proto::Node::Split { split } = node {
            out.push((split.id, split.axis, split.ratio));
            walk(&split.a, out);
            walk(&split.b, out);
        }
    }
    walk(tree, &mut out);
    out
}

fn leaves_of(tree: &proto::Node) -> Vec<u32> {
    let mut out = Vec::new();
    tree.leaves(&mut out);
    out
}

fn tree_of(message: &Message) -> &proto::Node {
    match message {
        Message::Layout { tree, .. } => tree,
        _ => panic!("expected a layout"),
    }
}

/// Waits for a layout that satisfies `want`, skipping stale broadcasts
/// (every attach re-sends the tree).
fn wait_for_layout(client: &mut Client, want: impl Fn(&Message) -> bool) -> Message {
    client.wait_for(|m| matches!(m, Message::Layout { .. }) && want(m))
}

/// Splits `pane` and returns (new pane id, layout message).
fn split(
    client: &mut Client,
    pane: u32,
    axis: proto::SplitAxis,
    ratio: f32,
    expected_leaves: usize,
) -> (u32, Message) {
    client.send(&Message::SplitPane {
        id: 70,
        pane_id: pane,
        axis,
        ratio,
    });
    let layout = wait_for_layout(client, |m| leaves_of(tree_of(m)).len() == expected_leaves);
    let leaves = leaves_of(tree_of(&layout));
    let new_pane = *leaves.last().expect("a leaf");
    (new_pane, layout)
}

#[test]
fn split_makes_three_live_panes() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);

    let (second, layout) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);
    let Message::Layout { tree, focus, .. } = &layout else {
        unreachable!()
    };
    assert_eq!(leaves_of(tree), vec![first, second]);
    assert_eq!(splits_of(tree)[0].1, proto::SplitAxis::Vertical);
    assert_eq!(*focus, Some(second), "the new pane takes focus");

    let (third, layout) = split(&mut client, second, proto::SplitAxis::Horizontal, 0.4, 3);
    let Message::Layout { tree, .. } = &layout else {
        unreachable!()
    };
    assert_eq!(leaves_of(tree), vec![first, second, third]);
    assert_eq!(splits_of(tree).len(), 2, "nested tree with two dividers");
    assert_eq!(splits_of(tree)[1].1, proto::SplitAxis::Horizontal);

    // Every pane has its own live shell: each echoes its own marker.
    for pane in [first, second, third] {
        client.send(&Message::Attach {
            id: 71,
            pane_id: pane,
            size_policy: None,
            history: None,
            since: None,
        });
        let _ = snapshot(&mut client);
        client.send(&Message::Keys {
            pane_id: pane,
            bytes: format!("echo pane-\"\"{pane}\r").into_bytes(),
        });
        wait_for_text(&mut client, &format!("pane-{pane}"));
    }
}

#[test]
fn three_panes_survive_a_disconnect_and_restore_layout_and_screens() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);
    let (third, _) = split(&mut client, second, proto::SplitAxis::Horizontal, 0.5, 3);

    // Put a distinct marker in each pane, then record its stable snapshot.
    let mut before = Vec::new();
    for pane in [first, second, third] {
        client.send(&Message::Attach {
            id: 80,
            pane_id: pane,
            size_policy: None,
            history: None,
            since: None,
        });
        let _ = snapshot(&mut client);
        client.send(&Message::Keys {
            pane_id: pane,
            bytes: format!("echo alive-\"\"{pane}\r").into_bytes(),
        });
        wait_for_text(&mut client, &format!("alive-{pane}"));
        before.push((pane, comparable(&stable_snapshot(&mut client, pane))));
    }

    let layout_before = {
        client.send(&Message::Focus { pane_id: second });
        wait_for_layout(
            &mut client,
            |m| matches!(m, Message::Layout { focus: Some(f), .. } if *f == second),
        )
    };

    drop(client); // "the window closes"
    std::thread::sleep(Duration::from_millis(300));

    let mut after = Client::connect(&host.socket);
    hello(&mut after);
    // The tree comes back with the first attach; the host keeps it per session.
    after.send(&Message::Attach {
        id: 81,
        pane_id: first,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut after);
    let layout_after = after.wait_for(|m| matches!(m, Message::Layout { .. }));

    let Message::Layout {
        tree: tree_before,
        focus: focus_before,
        ..
    } = &layout_before
    else {
        unreachable!()
    };
    let Message::Layout {
        tree: tree_after,
        focus: focus_after,
        ..
    } = &layout_after
    else {
        unreachable!()
    };
    assert_eq!(
        leaves_of(tree_before),
        vec![first, second, third],
        "order survives"
    );
    assert_eq!(leaves_of(tree_after), leaves_of(tree_before));
    assert_eq!(
        splits_of(tree_after),
        splits_of(tree_before),
        "ratios survive"
    );
    assert_eq!(focus_after, focus_before, "focus survives");

    for (pane, screen_before) in before {
        after.send(&Message::Attach {
            id: 82,
            pane_id: pane,
            size_policy: None,
            history: None,
            since: None,
        });
        let now = comparable(&stable_snapshot(&mut after, pane));
        assert_eq!(now, screen_before, "pane {pane} restored cell for cell");
    }
}

#[test]
fn closing_a_pane_merges_it_into_the_sibling() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);

    client.send(&Message::ClosePane {
        id: 90,
        pane_id: second,
    });
    let merged = wait_for_layout(&mut client, |m| leaves_of(tree_of(m)) == vec![first]);
    let tree = tree_of(&merged);
    assert_eq!(leaves_of(tree), vec![first], "the survivor fills the box");
    assert!(splits_of(tree).is_empty(), "no divider left");
}

#[test]
fn resizing_a_split_changes_the_tree_and_the_panes() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (_second, layout) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);
    let Message::Layout { tree, .. } = &layout else {
        unreachable!()
    };
    let split_id = splits_of(tree)[0].0;

    client.send(&Message::ResizeSplit {
        id: 60,
        split_id,
        ratio: 0.7,
    });
    let resized = wait_for_layout(&mut client, |m| {
        splits_of(tree_of(m))
            .first()
            .is_some_and(|(_, _, ratio)| (*ratio - 0.7).abs() < 1e-4)
    });
    let ratio = splits_of(tree_of(&resized))[0].2;
    assert!(
        (ratio - 0.7).abs() < 1e-4,
        "ratio reached the tree: {ratio}"
    );

    // Both panes were re-fitted: the wider one now reports more columns.
    for pane in leaves_of(tree_of(&resized)) {
        client.send(&Message::Attach {
            id: 61,
            pane_id: pane,
            size_policy: None,
            history: None,
            since: None,
        });
        let snapshot = snapshot(&mut client);
        let Message::Snapshot { cols, rows, .. } = snapshot else {
            unreachable!()
        };
        assert!(cols > 0 && rows > 0);
    }
}

#[test]
fn vim_alternate_screen_survives_a_reconnect() {
    let _serial = one_at_a_time();
    let demo = std::env::temp_dir().join(format!("pigeon-vim-alt-{}.txt", std::process::id()));
    std::fs::write(&demo, "hello alt screen\nsecond line\n").expect("demo");

    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (vim_pane, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);

    client.send(&Message::Attach {
        id: 95,
        pane_id: vim_pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);
    client.send(&Message::Keys {
        pane_id: vim_pane,
        bytes: format!("vim -u NONE -n {}\r", demo.display()).into_bytes(),
    });
    // vim can open with a "Press ENTER" prompt; answer it like a user would.
    client.send(&Message::Keys {
        pane_id: vim_pane,
        bytes: vec![b'\r'],
    });

    // Wait until the pane really is on the alternate screen with the buffer drawn.
    let snapshot_with_alt = client.wait_for(|m| match m {
        Message::Snapshot { alt, screen, .. } => {
            *alt && screen.iter().any(|row| {
                row.iter()
                    .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
                    .collect::<String>()
                    .contains("hello alt screen")
            })
        }
        _ => false,
    });
    assert!(matches!(
        snapshot_with_alt,
        Message::Snapshot { alt: true, .. }
    ));
    // vim may still be finishing its first draw; compare a settled screen.
    let before = comparable(&stable_snapshot(&mut client, vim_pane));

    drop(client);
    std::thread::sleep(Duration::from_millis(300));

    let mut after = Client::connect(&host.socket);
    hello(&mut after);
    after.send(&Message::Attach {
        id: 96,
        pane_id: vim_pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let restored = comparable(&stable_snapshot(&mut after, vim_pane));
    assert_eq!(
        restored, before,
        "vim's alternate screen and cursor come back"
    );
    let _ = std::fs::remove_file(&demo);
}

#[test]
fn zooming_a_pane_gives_it_the_whole_window_and_unzoom_restores_the_split() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);

    client.send(&Message::ZoomPane {
        id: 9,
        pane_id: second,
    });
    let zoomed = wait_for_layout(&mut client, |m| {
        matches!(m, Message::Layout { zoom: Some(_), .. })
    });
    let Message::Layout { zoom, focus, .. } = zoomed else {
        unreachable!()
    };
    assert_eq!(zoom, Some(second));
    assert_eq!(focus, Some(second), "zoom focuses what it zooms");
    // The zoomed pane is told the whole box; the client also hides the sibling.
    expect_size_state(&mut client, second, (80, 24, "latest", 1));

    client.send(&Message::Unzoom {
        id: 10,
        pane_id: second,
    });
    let restored = wait_for_layout(&mut client, |m| {
        matches!(m, Message::Layout { zoom: None, .. })
    });
    let Message::Layout { tree, .. } = restored else {
        unreachable!()
    };
    assert_eq!(
        leaves_of(&tree),
        vec![first, second],
        "the tree survived zoom"
    );

    // Zooming an unknown pane is an error, not a silent no-op.
    client.send(&Message::ZoomPane {
        id: 11,
        pane_id: 9999,
    });
    let Message::Error { code, .. } = client.wait_for(|m| matches!(m, Message::Error { .. }))
    else {
        unreachable!()
    };
    assert_eq!(code, "no_such_pane");
}

#[test]
fn two_panes_on_one_connection_both_get_live_output() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);

    // The client watches both panes on the same connection.
    client.send(&Message::Attach {
        id: 120,
        pane_id: second,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);

    client.send(&Message::Keys {
        pane_id: second,
        bytes: b"echo RIGHT-LIVE\r".to_vec(),
    });
    wait_for_pane_text(&mut client, second, "RIGHT-LIVE");

    // Focus moves to the other pane; its output must flow too, without a re-attach.
    client.send(&Message::Focus { pane_id: first });
    let _ = wait_for_layout(
        &mut client,
        |m| matches!(m, Message::Layout { focus: Some(f), .. } if *f == first),
    );
    client.send(&Message::Keys {
        pane_id: first,
        bytes: b"echo LEFT-LIVE\r".to_vec(),
    });
    wait_for_pane_text(&mut client, first, "LEFT-LIVE");

    // Both panes are still live afterwards.
    client.send(&Message::Keys {
        pane_id: second,
        bytes: b"echo RIGHT-AGAIN\r".to_vec(),
    });
    wait_for_pane_text(&mut client, second, "RIGHT-AGAIN");
}

#[test]
fn closing_the_watched_pane_still_delivers_the_new_layout() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);
    // Watch the pane we are about to close: the layout must still arrive.
    client.send(&Message::Attach {
        id: 130,
        pane_id: second,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);
    client.send(&Message::Keys {
        pane_id: second,
        bytes: b"echo BYE\r".to_vec(),
    });
    wait_for_pane_text(&mut client, second, "BYE");

    client.send(&Message::ClosePane {
        id: 131,
        pane_id: second,
    });
    let layout = wait_for_layout(&mut client, |m| leaves_of(tree_of(m)) == vec![first]);
    let Message::Layout { focus, .. } = &layout else {
        unreachable!()
    };
    assert_eq!(*focus, Some(first), "focus falls back to the survivor");
}

#[test]
fn sessions_carry_the_active_pane_and_its_title() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let first = attach_first(&mut client);
    let (second, _) = split(&mut client, first, proto::SplitAxis::Vertical, 0.5, 2);

    client.send(&Message::Sessions { items: Vec::new() });
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    let session = items.first().expect("one session");
    assert_eq!(session.panes, vec![first, second]);
    assert_eq!(
        session.active_pane,
        Some(second),
        "active pane follows focus"
    );
    // No OSC title yet, so the host falls back to whatever program it spawned.
    assert!(
        session
            .active_title
            .as_deref()
            .is_some_and(|t| !t.is_empty()),
        "a fallback label is always present"
    );
}

#[test]
fn status_is_broadcast_once_per_change_and_ack_stops_repeats() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=working\\007'\r".to_vec(),
    });
    client.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "working"));

    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=done:msg=T0s=\\007'\r".to_vec(),
    });
    let done = client.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "done"));
    let Message::Status { seq, msg, .. } = done else {
        unreachable!()
    };
    assert_eq!(msg.as_deref(), Some("OK"), "base64 message decoded");

    client.send(&Message::AckStatus { pane_id: pane, seq });
    // Another feed with no new status must not repeat the acked record.
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: vec![b'\r'],
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        client
            .drain(&|m| matches!(m, Message::Status { seq: other, .. } if *other == seq))
            .is_none(),
        "acked status must not be re-broadcast"
    );
}

#[test]
fn sessions_refresh_when_the_active_pane_title_changes() {
    let _serial = one_at_a_time();
    let script = script("#!/bin/sh\nsleep 1\nprintf '\\033]2;vim demo\\007'\nexec /bin/sh\n");
    let host = start_host_with(script.to_str().expect("utf8"));
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let _pane = attach_first(&mut client);

    // The host pushes a fresh list when a pane announces its title.
    client.send(&Message::Sessions { items: Vec::new() });
    let listed = client.wait_for(|m| match m {
        Message::Sessions { items } => items
            .first()
            .and_then(|item| item.active_title.as_deref())
            .is_some_and(|title| title == "vim demo"),
        _ => false,
    });
    let Message::Sessions { items } = listed else {
        unreachable!()
    };
    assert!(items[0].active_pane.is_some());
    let _ = std::fs::remove_file(&script);
}

#[test]
fn acked_status_is_not_replayed_after_reconnect() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=done:msg=T0s=\\007'\r".to_vec(),
    });
    let done = client.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "done"));
    let Message::Status { seq, .. } = done else {
        unreachable!()
    };
    client.send(&Message::AckStatus { pane_id: pane, seq });
    // The host answers messages in order, so the reply to `sessions` means the ack has been
    // applied before this connection goes away (a fixed sleep raced under load).
    client.send(&Message::Sessions { items: Vec::new() });
    client.wait_for(|m| matches!(m, Message::Sessions { .. }));
    drop(client);

    let mut again = Client::connect(&host.socket);
    hello(&mut again);
    again.send(&Message::Attach {
        id: 140,
        pane_id: pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut again);
    // A second attach replays `latest_status` before its snapshot, so this window is
    // deterministic: if the acked record came back it would arrive before this snapshot.
    again.send(&Message::Attach {
        id: 141,
        pane_id: pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let replayed = std::cell::Cell::new(false);
    again.wait_for(|message| {
        if matches!(message, Message::Status { state, .. } if state == "done") {
            replayed.set(true);
        }
        matches!(message, Message::Snapshot { .. })
    });
    assert!(
        !replayed.get(),
        "an acked record must not come back after a reconnect"
    );
}

#[test]
fn blocked_survives_an_ack_and_names_its_agent() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=blocked:kind=permission:app=claude:msg=UnVuIHRlc3RzPw==\\007'\r"
            .to_vec(),
    });
    let blocked =
        client.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "blocked"));
    let Message::Status {
        seq,
        app,
        msg,
        kind,
        age_ms,
        ..
    } = blocked
    else {
        unreachable!()
    };
    assert_eq!(app.as_deref(), Some("claude"));
    assert_eq!(msg.as_deref(), Some("Run tests?"));
    assert_eq!(kind.as_deref(), Some("permission"));
    assert!(
        age_ms.is_some(),
        "the client needs the state's age to show `Needs you · 2m`"
    );

    // The window acks what it is looking at; the agent is still waiting, so the host must
    // not answer with `idle` on the next output.
    client.send(&Message::AckStatus { pane_id: pane, seq });
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: vec![b'\r'],
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        client
            .drain(&|m| matches!(m, Message::Status { state, .. } if state == "idle"))
            .is_none(),
        "an acked `blocked` must stay visible"
    );
}

#[test]
fn clearing_a_status_broadcasts_idle() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=blocked\\007'\r".to_vec(),
    });
    client.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "blocked"));
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=clear\\007'\r".to_vec(),
    });
    let idle = client.wait_for(
        |m| matches!(m, Message::Status { state, seq, .. } if state == "idle" && *seq == 0),
    );
    assert!(matches!(idle, Message::Status { state, .. } if state == "idle"));
}

/// The one end-to-end case for colour queries: a real host, a real PTY, no TUI and no script
/// file — the shell itself asks `OSC 11;?` and prints the answer it gets back.
#[test]
fn a_pane_asks_for_its_background_and_the_host_answers() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    client.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);
    // Query, read the 25-byte reply, print it with the escapes visible (`cat -v`).
    client.send(&Message::Keys {
        pane_id,
        bytes: b"printf '\\033]11;?\\033\\\\'; IFS= read -r -n 25 -t 5 r; printf 'R[%s]' \"$r\" | cat -v\n"
            .to_vec(),
    });
    // No `appearance` was sent: the host answers with its built-in harp dark theme.
    wait_for_pane_text(&mut client, pane_id, "rgb:0e0e/0f0f/1111");
}

/// The host's `appearance` path: a message reaches panes that already exist, an unusable one
/// keeps the previous palette, and nothing is rebuilt.
#[test]
fn appearance_reaches_existing_panes_and_a_malformed_one_keeps_the_previous() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    client.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: None,
        since: None,
    });
    let _ = snapshot(&mut client);

    // `clear;` keeps only the newest answer on screen, so a stale line cannot satisfy a later
    // assertion.
    fn query() -> Vec<u8> {
        b"clear; printf '\\033]11;?\\033\\\\'; IFS= read -r -n 25 -t 5 r; printf 'R[%s]' \"$r\" | cat -v\n"
            .to_vec()
    }

    // 1) nothing sent yet: the built-in harp dark palette answers.
    client.send(&Message::Keys {
        pane_id,
        bytes: query(),
    });
    wait_for_pane_text(&mut client, pane_id, "rgb:0e0e/0f0f/1111");

    // 2) a light appearance reaches the pane that already exists.
    client.send(&Message::Appearance {
        fg: Some(0x0011_1111),
        bg: Some(0x00ff_ffff),
        ansi: vec![
            0x212121, 0xC30771, 0x10A778, 0xA89C14, 0x008EC4, 0x523C79, 0x20A5BA, 0xE0E0E0,
            0x212121, 0xFB007A, 0x5FD7AF, 0xF3E430, 0x20BBFC, 0x6855DE, 0x4FB8CC, 0xF1F1F1,
        ],
    });
    client.send(&Message::Keys {
        pane_id,
        bytes: query(),
    });
    wait_for_pane_text(&mut client, pane_id, "rgb:ffff/ffff/ffff");

    // 3) an unusable message keeps the previous palette instead of resetting it.
    client.send(&Message::Appearance {
        fg: None,
        bg: None,
        ansi: Vec::new(),
    });
    client.send(&Message::Keys {
        pane_id,
        bytes: query(),
    });
    let latest = client.wait_for(|m| match m {
        Message::Snapshot { .. } => screen_text(m).contains("rgb:"),
        _ => false,
    });
    let text = screen_text(&latest);
    assert!(text.contains("rgb:ffff/ffff/ffff"), "kept palette: {text}");
    assert!(
        !text.contains("rgb:0e0e/0f0f/1111"),
        "a malformed appearance must not reset the palette: {text}"
    );

    // 4) the session and its pane survived: nothing was rebuilt.
    client.send(&Message::Sessions { items: Vec::new() });
    let sessions = client.wait_for(|m| matches!(m, Message::Sessions { .. }));
    match sessions {
        Message::Sessions { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].panes, vec![pane_id]);
        }
        other => panic!("expected sessions, got {}", other.name()),
    }
}

/// What a `delta` client sees: typing arrives as dirty rows with consecutive `seq`, an
/// attach can leave the history out, and a resize is answered with a full snapshot.
#[test]
fn delta_clients_get_dirty_rows_in_sequence_and_a_snapshot_on_resize() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    client.send(&Message::Hello {
        proto: proto::PROTO_VERSION,
        client: "host-test".into(),
        features: vec!["scrollback".into(), "delta".into()],
    });
    let Message::HelloAck { features, .. } =
        client.wait_for(|m| matches!(m, Message::HelloAck { .. }))
    else {
        unreachable!()
    };
    assert!(features.iter().any(|f| f == "delta"));

    client.send(&Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    let Message::Created { pane_id, .. } =
        client.wait_for(|m| matches!(m, Message::Created { .. }))
    else {
        unreachable!()
    };
    client.send(&Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        history: Some(false),
        since: None,
    });
    let Message::Snapshot {
        seq: base, history, ..
    } = client.wait_for(|m| matches!(m, Message::Snapshot { .. }))
    else {
        unreachable!()
    };
    assert!(
        history.is_none(),
        "history: false keeps the backlog off the wire"
    );

    client.send(&Message::Keys {
        pane_id,
        bytes: b"echo pigeon-delta\r".to_vec(),
    });
    // Every frame after the snapshot is a delta numbered base+1, base+2, … until the output
    // line shows up in one of them.
    let mut last = base;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "the output never arrived as a delta"
        );
        let message =
            client.wait_for(|m| matches!(m, Message::Delta { .. } | Message::Snapshot { .. }));
        let Message::Delta { seq, rows, .. } = message else {
            panic!("a delta client got a full snapshot for plain typing");
        };
        assert_eq!(seq, last + 1, "deltas must be consecutive");
        last = seq;
        let text = rows_text(&rows.into_iter().map(|row| row.cells).collect::<Vec<_>>());
        if text.lines().any(|line| line.trim() == "pigeon-delta") {
            break;
        }
    }

    client.send(&Message::ResizePane {
        pane_id,
        cols: 50,
        rows: 12,
    });
    let message = client.wait_for(|m| match m {
        Message::Snapshot { cols, rows, .. } => (*cols, *rows) == (50, 12),
        _ => false,
    });
    let Message::Snapshot { seq, .. } = message else {
        unreachable!()
    };
    assert!(
        seq >= last,
        "the snapshot carries the newest delta it contains"
    );
}

fn hello_delta(client: &mut Client) -> Option<String> {
    client.send(&Message::Hello {
        proto: proto::PROTO_VERSION,
        client: "host-test".into(),
        features: vec!["scrollback".into(), "delta".into()],
    });
    let Message::HelloAck { instance, .. } =
        client.wait_for(|m| matches!(m, Message::HelloAck { .. }))
    else {
        unreachable!()
    };
    instance
}

/// Issue #4: a client that drops and comes back says which `seq` its screen shows; the host
/// replays only the deltas it missed, and when it cannot (a resize, or a `seq` it never
/// sent) it answers with a snapshot as before.
#[test]
fn reconnect_with_since_replays_only_the_missed_deltas() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut first = Client::connect(&host.socket);
    let instance = hello_delta(&mut first);
    assert!(instance.is_some(), "hello_ack names the host instance");
    let pane = attach_first(&mut first);
    first.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo before-\"\"drop\r".to_vec(),
    });
    let seen = loop {
        let Message::Delta { seq, rows, .. } =
            first.wait_for(|m| matches!(m, Message::Delta { .. }))
        else {
            unreachable!()
        };
        let text = rows_text(&rows.into_iter().map(|row| row.cells).collect::<Vec<_>>());
        if text.lines().any(|line| line.trim() == "before-drop") {
            break seq;
        }
    };
    drop(first); // the network goes away

    // Output while nobody watches; another client confirms it reached the screen.
    let mut watcher = Client::connect(&host.socket);
    hello(&mut watcher);
    attach(&mut watcher, pane, None);
    watcher.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo while-\"\"away\r".to_vec(),
    });
    wait_for_pane_text(&mut watcher, pane, "while-away");
    drop(watcher);

    let mut back = Client::connect(&host.socket);
    assert_eq!(hello_delta(&mut back), instance, "same host process");
    back.send(&Message::Attach {
        id: 90,
        pane_id: pane,
        size_policy: None,
        history: Some(false),
        since: Some(seen),
    });
    let mut next = seen + 1;
    loop {
        let message =
            back.wait_for(|m| matches!(m, Message::Delta { .. } | Message::Snapshot { .. }));
        let Message::Delta { seq, rows, .. } = message else {
            panic!("a short drop must not cost a full snapshot");
        };
        assert_eq!(
            seq, next,
            "the replay continues exactly where the client left off"
        );
        next += 1;
        let text = rows_text(&rows.into_iter().map(|row| row.cells).collect::<Vec<_>>());
        if text.lines().any(|line| line.trim() == "while-away") {
            break;
        }
    }

    // A resize empties the backlog: the old rows no longer fit, so a snapshot it is.
    back.send(&Message::ResizePane {
        pane_id: pane,
        cols: 60,
        rows: 20,
    });
    back.wait_for(|m| matches!(m, Message::Snapshot { cols: 60, .. }));
    drop(back);
    let mut late = Client::connect(&host.socket);
    hello_delta(&mut late);
    late.send(&Message::Attach {
        id: 91,
        pane_id: pane,
        size_policy: None,
        history: Some(false),
        since: Some(seen),
    });
    let message = late.wait_for(|m| matches!(m, Message::Delta { .. } | Message::Snapshot { .. }));
    assert!(
        matches!(message, Message::Snapshot { cols: 60, .. }),
        "frames from before the resize cannot be replayed"
    );

    // A `seq` this host never sent (the client talked to a host that has since restarted).
    // A fresh connection: one already watching the pane could see a live delta first.
    drop(late);
    let mut late = Client::connect(&host.socket);
    hello_delta(&mut late);
    late.send(&Message::Attach {
        id: 92,
        pane_id: pane,
        size_policy: None,
        history: Some(false),
        since: Some(u32::MAX - 1),
    });
    let message = late.wait_for(|m| matches!(m, Message::Delta { .. } | Message::Snapshot { .. }));
    assert!(
        matches!(message, Message::Snapshot { .. }),
        "a foreign seq means a snapshot"
    );
}

/// A second host on the same socket (a stray `pigeon-host`, a `--stdio` start racing a
/// running one) must leave the live host and its sessions alone instead of unlinking the
/// socket out from under them.
#[test]
fn a_second_host_does_not_steal_a_live_socket() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    let mut second = Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
        .args(["--socket", &host.socket, "--shell", "/bin/sh"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn second pigeon-host");
    let deadline = Instant::now() + Duration::from_secs(5);
    let exited = loop {
        if let Some(status) = second.try_wait().expect("wait") {
            break Some(status);
        }
        if Instant::now() > deadline {
            let _ = second.kill();
            let _ = second.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        exited.is_some(),
        "the second host must give up, not take over"
    );

    // The first host still answers on the same path, with the same pane.
    let mut again = Client::connect(&host.socket);
    hello(&mut again);
    again.send(&Message::Attach {
        id: 900,
        pane_id: pane,
        size_policy: None,
        history: None,
        since: None,
    });
    let reply = again.wait_for(|m| matches!(m, Message::Snapshot { .. } | Message::Error { .. }));
    assert!(
        matches!(reply, Message::Snapshot { .. }),
        "the live host keeps its panes: {reply:?}"
    );
}

#[test]
fn the_socket_is_private_from_the_moment_it_exists() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = one_at_a_time();
    let host = start_host();
    let mode = std::fs::metadata(&host.socket)
        .expect("socket")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode & 0o077, 0, "group/other must have no access: {mode:o}");
}

fn hello_with(client: &mut Client, features: &[&str]) -> Vec<String> {
    client.send(&Message::Hello {
        proto: proto::PROTO_VERSION,
        client: "host-test".into(),
        features: features.iter().map(|f| (*f).to_owned()).collect(),
    });
    let Message::HelloAck { features, .. } =
        client.wait_for(|m| matches!(m, Message::HelloAck { .. }))
    else {
        unreachable!()
    };
    features
}

/// A sidebar follows every agent without attaching: `status_all` delivers status for panes
/// the connection never attached, both what was pending when it connected and what comes
/// later, and it never joins the session's size vote.
#[test]
fn status_all_follows_agents_without_attaching() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);
    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=working:app=claude\\007'\r".to_vec(),
    });
    worker.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "working"));

    let mut sidebar = Client::connect(&host.socket);
    let offered = hello_with(&mut sidebar, &["status_all"]);
    assert!(offered.iter().any(|f| f == "status_all"));
    let pending = sidebar.wait_for(|m| matches!(m, Message::Status { .. }));
    assert!(
        matches!(&pending, Message::Status { pane_id, state, .. } if *pane_id == pane && state == "working"),
        "the pending status arrives right after the session list"
    );

    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=blocked:kind=question\\007'\r".to_vec(),
    });
    let live =
        sidebar.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "blocked"));
    assert!(matches!(live, Message::Status { pane_id, .. } if pane_id == pane));
    // Status only: no screen for a pane it never attached, and it is not a size voter.
    assert!(sidebar
        .drain(&|m| matches!(m, Message::Snapshot { .. } | Message::Delta { .. }))
        .is_none());
    worker.send(&Message::ResizePane {
        pane_id: pane,
        cols: 90,
        rows: 30,
    });
    expect_size_state(&mut worker, pane, (90, 30, "latest", 1));
}

/// Without the feature a connection keeps the old behaviour: status only for watched panes.
#[test]
fn status_without_the_feature_stays_with_watched_panes() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);

    let mut other = Client::connect(&host.socket);
    let offered = hello_with(&mut other, &["no_such_feature"]);
    assert!(!offered
        .iter()
        .any(|f| f == "status_all" || f == "no_such_feature"));
    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=blocked\\007'\r".to_vec(),
    });
    worker.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "blocked"));
    std::thread::sleep(Duration::from_millis(200));
    assert!(other
        .drain(&|m| matches!(m, Message::Status { .. }))
        .is_none());
}

/// Reads exactly one frame from a raw socket (no reader thread, so the test decides when
/// the socket stops being drained).
fn read_frame(stream: &mut UnixStream) -> Message {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).expect("frame header");
    let length = u32::from_le_bytes(header) as usize;
    let mut frame = header.to_vec();
    frame.resize(4 + length, 0);
    stream.read_exact(&mut frame[4..]).expect("frame payload");
    let (message, _) = proto::decode_frame(&frame)
        .expect("decode")
        .expect("whole frame");
    message
}

/// A window that stops reading falls behind the host's event stream. Once it reads again it
/// must still learn what it missed: here a status set while it was stalled, buried under
/// far more events than the host buffers for one connection, plus the final size.
#[test]
fn a_lagging_client_gets_the_current_state_back() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);

    // The slow window: a raw socket that is read through its attach and then left alone, so
    // the host's writes to it block once the socket buffer fills and its share of the event
    // stream piles up behind them.
    let mut slow = UnixStream::connect(&host.socket).expect("connect");
    for message in [
        Message::Hello {
            proto: proto::PROTO_VERSION,
            client: "host-test-slow".into(),
            features: vec!["delta".into()],
        },
        Message::Attach {
            id: 2,
            pane_id: pane,
            size_policy: None,
            history: Some(false),
            since: None,
        },
    ] {
        slow.write_all(&proto::encode_frame(&message).expect("encode"))
            .expect("write");
    }
    let mut seen_snapshot = false;
    loop {
        match read_frame(&mut slow) {
            Message::Snapshot { .. } => seen_snapshot = true,
            Message::Layout { .. } if seen_snapshot => break,
            _ => {}
        }
    }
    expect_size_state(&mut worker, pane, (80, 24, "latest", 2));

    // Every resize is a layout, a size_state and a fresh snapshot for each watcher: a few
    // hundred of them overflow both the socket buffer and the host's event buffer.
    let churn = |worker: &mut Client| {
        for step in 0..400u16 {
            let (cols, rows) = if step % 2 == 0 { (100, 30) } else { (90, 28) };
            worker.send(&Message::ResizePane {
                pane_id: pane,
                cols,
                rows,
            });
        }
        expect_size_state(worker, pane, (90, 28, "latest", 2));
    };
    churn(&mut worker);
    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]7501;state=working:app=claude\\007'\r".to_vec(),
    });
    worker.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "working"));
    churn(&mut worker);

    let reader = slow.try_clone().expect("clone the socket for reading");
    let mut slow = Client::new(Box::new(reader), Box::new(slow));
    let status =
        slow.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "working"));
    assert!(matches!(status, Message::Status { pane_id, .. } if pane_id == pane));
    expect_size_state(&mut slow, pane, (90, 28, "latest", 2));
    // And the screen is live again: new output reaches the window that lagged.
    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo after-the-lag\r".to_vec(),
    });
    slow.wait_for(|m| match m {
        Message::Delta { rows, .. } => rows
            .iter()
            .any(|row| rows_text(std::slice::from_ref(&row.cells)).contains("after-the-lag")),
        Message::Snapshot { .. } => screen_text(m).contains("after-the-lag"),
        _ => false,
    });
}

/// What a delta-taking window draws: the last snapshot with every later delta applied in
/// `seq` order, exactly as protocol §3.3 tells a client to.
struct Screen {
    seq: u32,
    grid: Vec<Vec<proto::Cell>>,
    cursor: proto::Cursor,
    alt: bool,
}

impl Screen {
    /// Applies one frame for `pane`; a gap in `seq` fails the test (the queue must never
    /// leave one).
    fn apply(screen: &mut Option<Self>, pane: u32, message: &Message) {
        match message {
            Message::Snapshot {
                pane_id,
                seq,
                screen: grid,
                cursor,
                alt,
                ..
            } if *pane_id == pane => {
                *screen = Some(Self {
                    seq: *seq,
                    grid: grid.clone(),
                    cursor: *cursor,
                    alt: *alt,
                });
            }
            Message::Delta {
                pane_id,
                seq,
                rows,
                cursor,
                alt,
                ..
            } if *pane_id == pane => {
                let Some(current) = screen.as_mut() else {
                    panic!("a delta before any snapshot");
                };
                if *seq <= current.seq {
                    return;
                }
                assert_eq!(*seq, current.seq + 1, "deltas must stay contiguous");
                current.seq = *seq;
                for row in rows {
                    current.grid[usize::from(row.y)] = row.cells.clone();
                }
                if let Some(cursor) = cursor {
                    current.cursor = *cursor;
                }
                if let Some(alt) = alt {
                    current.alt = *alt;
                }
            }
            _ => {}
        }
    }

    fn text(&self) -> String {
        rows_text(&self.grid)
    }
}

/// A raw delta client that attaches `pane`, reads through the attach and then stops
/// reading; returns it with the screen it has drawn so far.
fn stalled_delta_client(socket: &str, pane: u32) -> (UnixStream, Option<Screen>) {
    let mut slow = UnixStream::connect(socket).expect("connect");
    for message in [
        Message::Hello {
            proto: proto::PROTO_VERSION,
            client: "host-test-slow".into(),
            features: vec!["delta".into()],
        },
        Message::Attach {
            id: 2,
            pane_id: pane,
            size_policy: None,
            history: Some(false),
            since: None,
        },
    ] {
        slow.write_all(&proto::encode_frame(&message).expect("encode"))
            .expect("write");
    }
    let mut screen = None;
    loop {
        let message = read_frame(&mut slow);
        Screen::apply(&mut screen, pane, &message);
        if matches!(message, Message::Layout { .. }) && screen.is_some() {
            return (slow, screen);
        }
    }
}

/// A window behind a stalled link: the pane floods far more output than any socket buffer
/// holds while the window reads nothing. When it reads again, `seq` has no gap, the status
/// set mid-flood still arrives, and the screen it ends up drawing equals a fresh attach.
#[test]
fn a_stalled_client_catches_up_to_the_same_screen_and_keeps_the_status() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);
    let (slow, mut screen) = stalled_delta_client(&host.socket, pane);

    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"seq 1 60000; printf '\\033]7501;state=working:app=flood\\007'; seq 1 60000; echo flood-done\r"
            .to_vec(),
    });
    worker.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "working"));
    wait_for_text(&mut worker, "flood-done");
    let fresh = stable_snapshot(&mut worker, pane);
    let (_, _, fresh_alt, fresh_cursor, _, fresh_grid) = comparable(&fresh);

    let reader = slow.try_clone().expect("clone the socket for reading");
    let mut slow = Client::new(Box::new(reader), Box::new(slow));
    let mut saw_status = false;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        assert!(
            Instant::now() < deadline,
            "the stalled window never caught up; it shows:\n{}",
            screen.as_ref().map(Screen::text).unwrap_or_default()
        );
        let message = slow.wait_for(|m| {
            matches!(
                m,
                Message::Snapshot { .. } | Message::Delta { .. } | Message::Status { .. }
            )
        });
        if matches!(&message, Message::Status { pane_id, state, .. } if *pane_id == pane && state == "working")
        {
            saw_status = true;
        }
        Screen::apply(&mut screen, pane, &message);
        let caught_up = screen.as_ref().is_some_and(|s| {
            s.grid == fresh_grid && s.cursor == fresh_cursor && s.alt == fresh_alt
        });
        if caught_up && saw_status {
            break;
        }
    }
}

/// Control messages queued behind a stalled socket come out in the order they happened,
/// none missing, with a flood of screen updates interleaved.
#[test]
fn control_messages_keep_their_order_behind_a_stalled_socket() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let first = attach_first(&mut worker);
    let (_second, layout) = split(&mut worker, first, proto::SplitAxis::Vertical, 0.5, 2);
    let split_id = splits_of(tree_of(&layout))[0].0;
    let (slow, _) = stalled_delta_client(&host.socket, first);

    worker.send(&Message::Keys {
        pane_id: first,
        bytes: b"seq 1 40000; printf '\\033]7501;state=working\\007'; seq 1 40000; printf '\\033]7501;state=blocked:kind=question\\007'\r"
            .to_vec(),
    });
    let ratios: Vec<f32> = (0..40).map(|step| 0.2 + 0.01 * step as f32).collect();
    for &ratio in &ratios {
        worker.send(&Message::ResizeSplit {
            id: 80,
            split_id,
            ratio,
        });
    }
    let last = ratios[ratios.len() - 1];
    wait_for_layout(&mut worker, |m| {
        splits_of(tree_of(m))
            .first()
            .is_some_and(|(_, _, ratio)| (*ratio - last).abs() < 1e-4)
    });
    worker.wait_for(|m| matches!(m, Message::Status { state, .. } if state == "blocked"));

    let reader = slow.try_clone().expect("clone the socket for reading");
    let mut slow = Client::new(Box::new(reader), Box::new(slow));
    let mut seen_ratios: Vec<f32> = Vec::new();
    let mut seen_states: Vec<String> = Vec::new();
    loop {
        let message =
            slow.wait_for(|m| matches!(m, Message::Layout { .. } | Message::Status { .. }));
        match &message {
            Message::Layout { tree, .. } => {
                if let Some((_, _, ratio)) = splits_of(tree).first() {
                    seen_ratios.push(*ratio);
                }
            }
            Message::Status { state, .. } => seen_states.push(state.clone()),
            _ => unreachable!(),
        }
        let done = seen_ratios
            .last()
            .is_some_and(|ratio| (ratio - last).abs() < 1e-4)
            && seen_states.last().is_some_and(|state| state == "blocked");
        if done {
            break;
        }
    }
    // Every resize the worker made, in order, after the 0.5 the attach announced.
    let after_attach: Vec<f32> = seen_ratios
        .iter()
        .copied()
        .skip_while(|ratio| (ratio - 0.5).abs() < 1e-4)
        .collect();
    assert_eq!(after_attach.len(), ratios.len(), "{seen_ratios:?}");
    for (seen, want) in after_attach.iter().zip(&ratios) {
        assert!((seen - want).abs() < 1e-4, "{seen_ratios:?}");
    }
    let working = seen_states.iter().position(|state| state == "working");
    let blocked = seen_states.iter().position(|state| state == "blocked");
    assert!(
        matches!((working, blocked), (Some(w), Some(b)) if w < b),
        "{seen_states:?}"
    );
}

/// P6: a host that goes away (crash, reboot, upgrade) comes back showing every session's
/// last screen. The processes are gone, so each restored pane reports `pane_exited` with
/// code -1 and ignores input; the layout and the screens are what they were.
#[test]
fn a_restarted_host_shows_the_last_screens_as_ended_sessions() {
    let _serial = one_at_a_time();
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    let socket = unique_socket();
    let spawn = |socket: &str| {
        Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
            .args(["--socket", socket, "--shell", quiet.to_str().expect("utf8")])
            .env("PIGEON_SAVE_MS", "100")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn pigeon-host")
    };
    let wait_for_socket = |socket: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !std::path::Path::new(socket).exists() {
            assert!(Instant::now() < deadline, "host never created its socket");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let state = std::path::Path::new(&socket).with_extension("state");
    let _ = std::fs::remove_file(&state);

    let mut first = spawn(&socket);
    wait_for_socket(&socket);
    let mut client = Client::connect(&socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::SplitPane {
        id: 3,
        pane_id: pane,
        axis: proto::SplitAxis::Vertical,
        ratio: 0.5,
    });
    client.wait_for(|m| {
        matches!(
            m,
            Message::Layout {
                tree: proto::Node::Split { .. },
                ..
            }
        )
    });
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo before-the-crash\r".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "before-the-crash");
    // Two save periods, so the file has the echoed line.
    std::thread::sleep(Duration::from_millis(400));
    drop(client);
    let _ = first.kill();
    let _ = first.wait();
    assert!(
        state.exists(),
        "the host saved its state next to the socket"
    );
    let _ = std::fs::remove_file(&socket);

    let mut second = spawn(&socket);
    wait_for_socket(&socket);
    let mut client = Client::connect(&socket);
    hello(&mut client);
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert_eq!(items.len(), 1, "the session came back");
    assert_eq!(items[0].panes.len(), 2, "with both halves of the split");
    let restored = items[0].panes[0];
    attach(&mut client, restored, None);
    let screen =
        client.wait_for(|m| matches!(m, Message::Snapshot { pane_id, .. } if *pane_id == restored));
    assert!(
        screen_text(&screen).contains("before-the-crash"),
        "{}",
        screen_text(&screen)
    );
    let exited = client
        .wait_for(|m| matches!(m, Message::PaneExited { pane_id, .. } if *pane_id == restored));
    assert!(matches!(exited, Message::PaneExited { code: -1, .. }));
    let layout = client.wait_for(|m| matches!(m, Message::Layout { .. }));
    assert!(matches!(
        layout,
        Message::Layout {
            tree: proto::Node::Split { .. },
            ..
        }
    ));

    // Closing the restored panes ends the session for good, and the next save forgets it.
    for pane in items[0].panes.clone() {
        client.send(&Message::ClosePane {
            id: 9,
            pane_id: pane,
        });
    }
    std::thread::sleep(Duration::from_millis(400));
    let _ = second.kill();
    let _ = second.wait();
    let _ = std::fs::remove_file(&socket);
    let mut third = spawn(&socket);
    wait_for_socket(&socket);
    let mut client = Client::connect(&socket);
    hello(&mut client);
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(
        items.is_empty(),
        "a closed restored session does not come back again"
    );
    let _ = third.kill();
    let _ = third.wait();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(&state);
}

/// A state file that is not one (garbage, a crash mid-write of another build) restores
/// nothing and the host still serves.
#[test]
fn a_corrupt_state_file_restores_nothing_and_the_host_still_serves() {
    let _serial = one_at_a_time();
    let socket = unique_socket();
    let state = std::path::Path::new(&socket).with_extension("state");
    std::fs::write(&state, b"\xa1\x67version\x01 truncated").expect("write");
    let host = start_host_with_socket(&socket);
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(items.is_empty());
    let _ = attach_first(&mut client);
    let _ = std::fs::remove_file(&state);
}

/// The local host starts like a `tmux` server (no launchd job): `--detach` returns once a
/// host answers on the socket, the host is in its own process group (so the window's exit
/// does not take it along), and a second `--detach` reuses the live host.
#[test]
fn detach_starts_one_host_that_outlives_its_starter() {
    let _serial = one_at_a_time();
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    let socket = unique_socket();
    let log = std::path::Path::new(&socket).with_extension("log");
    struct Reap(String);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = Command::new("pkill")
                .args(["-f", "--", &format!("--socket {}", self.0)])
                .status();
            let path = std::path::Path::new(&self.0);
            for file in [
                path.to_path_buf(),
                path.with_extension("state"),
                path.with_extension("log"),
            ] {
                let _ = std::fs::remove_file(file);
            }
        }
    }
    let _reap = Reap(socket.clone());
    let detach = || {
        Command::new(env!("CARGO_BIN_EXE_pigeon-host"))
            .args([
                "--detach",
                "--socket",
                &socket,
                "--shell",
                quiet.to_str().expect("utf8"),
            ])
            .arg("--log")
            .arg(&log)
            .status()
            .expect("run --detach")
    };
    assert!(
        detach().success(),
        "the starter exits cleanly once the host answers"
    );
    let mut client = Client::connect(&socket);
    hello(&mut client);
    let pane = attach_first(&mut client);

    let pgrep = |pattern: &str| {
        let out = Command::new("pgrep")
            .args(["-f", "--", pattern])
            .output()
            .expect("pgrep");
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse::<i32>().ok())
            .collect::<Vec<_>>()
    };
    let hosts = pgrep(&format!("--socket {socket} --shell"));
    assert_eq!(hosts.len(), 1, "exactly one host: {hosts:?}");
    let pgid = Command::new("ps")
        .args(["-o", "pgid=", "-p", &hosts[0].to_string()])
        .output()
        .expect("ps");
    let pgid: i32 = String::from_utf8_lossy(&pgid.stdout)
        .trim()
        .parse()
        .expect("pgid");
    assert_eq!(pgid, hosts[0], "the host leads its own process group");

    assert!(detach().success());
    assert_eq!(
        pgrep(&format!("--socket {socket} --shell")),
        hosts,
        "the live host was reused"
    );
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo still-here\r".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "still-here");
    assert!(log.exists(), "the host writes its log where it was told");
}

/// "Stop Session Host…": `shutdown` ends every session, removes the socket and the saved
/// screens (nothing comes back on the next start), and the host process exits.
#[test]
fn shutdown_ends_the_host_and_leaves_nothing_to_restore() {
    let _serial = one_at_a_time();
    let mut host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"echo $$ > /dev/null\r".to_vec(),
    });
    client.send(&Message::Shutdown);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = host.child.try_wait().expect("wait") {
            break status;
        }
        assert!(Instant::now() < deadline, "the host did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "a requested shutdown is a clean exit");
    assert!(
        !std::path::Path::new(&host.socket).exists(),
        "socket removed"
    );
    assert!(
        !std::path::Path::new(&host.socket)
            .with_extension("state")
            .exists(),
        "no screens left to restore"
    );
}

/// `shutdown` before `hello` is refused like any other message: a stray connection cannot
/// end the user's sessions without speaking the protocol first.
#[test]
fn shutdown_before_hello_is_a_protocol_error() {
    let _serial = one_at_a_time();
    let mut host = start_host();
    let mut client = Client::connect(&host.socket);
    client.send(&Message::Shutdown);
    let reply = client.wait_for(|m| matches!(m, Message::Error { .. }));
    assert!(matches!(reply, Message::Error { code, .. } if code == "protocol"));
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        host.child.try_wait().expect("wait").is_none(),
        "the host keeps running"
    );
}
