//! End-to-end: a client attaches, types, disconnects, reconnects — and the shell
//! is still there with the same screen. This is the M0 promise ("关闭窗口进程不死"),
//! proven against the real host binary over the real socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
    }
}

fn start_host() -> Host {
    start_host_with("/bin/sh")
}

fn start_host_with(program: &str) -> Host {
    // Tests run in parallel: pid + counter + clock keeps socket paths unique.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = format!("pigeon-test-{}-{counter}-{stamp}", std::process::id());
    let socket = format!("/tmp/{unique}.sock");
    let _ = std::fs::remove_file(&socket);
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

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
    /// Last snapshot grid, kept so a timeout can show what the pane actually said.
    last_screen: String,
}

impl Client {
    fn connect(socket: &str) -> Self {
        let stream = UnixStream::connect(socket).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .expect("timeout");
        Self {
            stream,
            buffer: Vec::new(),
            last_screen: String::new(),
        }
    }

    fn send(&mut self, message: &Message) {
        let frame = proto::encode_frame(message).expect("encode");
        self.stream.write_all(&frame).expect("write");
    }

    /// Reads until a message matching `want` arrives, ignoring the rest.
    fn wait_for(&mut self, want: impl Fn(&Message) -> bool) -> Message {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = self.drain(&want) {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a message; last screen:\n{}",
                self.last_screen
            );
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => panic!("host closed the connection"),
                Ok(count) => self.buffer.extend_from_slice(&chunk[..count]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Nothing yet; the loop deadline decides when to give up.
                    continue;
                }
                Err(error) => panic!("read failed: {error}"),
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

/// Re-attaches until two consecutive snapshots are identical, so a live shell
/// cannot race the comparison.
fn stable_snapshot(client: &mut Client, pane_id: u32) -> String {
    let mut previous = String::new();
    for _ in 0..25 {
        client.send(&Message::Attach {
            id: 99,
            pane_id,
            size_policy: None,
        });
        let text = screen_text(&snapshot(client));
        if text == previous {
            return text;
        }
        previous = text;
        std::thread::sleep(Duration::from_millis(120));
    }
    previous
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
        bytes: b"echo pigeon-marker\r".to_vec(),
    });
    // Wait until the shell has echoed the command into the grid.
    wait_for_text(&mut first, "pigeon-marker");
    let before_text = stable_snapshot(&mut first, pane_id);
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

    let after_text = stable_snapshot(&mut second, pane_id);
    assert_eq!(
        after_text, before_text,
        "reconnecting must restore the same screen"
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
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);

    // A well-formed frame with an unknown `t`, built by hand.
    let payload = ciborium::value::Value::Map(vec![(
        ciborium::value::Value::Text("t".into()),
        ciborium::value::Value::Text("split_pane".into()),
    )]);
    let mut body = Vec::new();
    ciborium::into_writer(&payload, &mut body).expect("encode");
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&body);
    client.stream.write_all(&frame).expect("write");

    let reply = client.wait_for(|m| matches!(m, Message::Unsupported { .. }));
    let Message::Unsupported { t_of } = reply else {
        unreachable!()
    };
    assert_eq!(t_of, "split_pane");

    // Still usable afterwards.
    client.send(&Message::Ping { ts: 9 });
    let Message::Pong { ts } = client.wait_for(|m| matches!(m, Message::Pong { .. })) else {
        unreachable!()
    };
    assert_eq!(ts, 9);
}

#[test]
fn resize_reaches_the_shell() {
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
    });
    let _ = snapshot(client);
    pane_id
}

#[test]
fn less_pages_forward_when_space_is_pressed() {
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
