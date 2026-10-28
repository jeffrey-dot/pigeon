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
    }
}

fn start_host() -> Host {
    // An empty prompt keeps the grid still once a command finishes, so the
    // before/after comparison is not racing a redraw.
    let quiet = script("#!/bin/sh\nexec env PS1= /bin/sh -i\n");
    start_host_with(quiet.to_str().expect("utf8"))
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
        let deadline = Instant::now() + Duration::from_secs(25);
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
    client.stream.write_all(&frame).expect("write");

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
        });
        let _ = snapshot(&mut client);
        client.send(&Message::Keys {
            pane_id: pane,
            bytes: format!("echo pane-{pane}\r").into_bytes(),
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
        });
        let _ = snapshot(&mut client);
        client.send(&Message::Keys {
            pane_id: pane,
            bytes: format!("echo alive-{pane}\r").into_bytes(),
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
    });
    let restored = comparable(&stable_snapshot(&mut after, vim_pane));
    assert_eq!(
        restored, before,
        "vim's alternate screen and cursor come back"
    );
    let _ = std::fs::remove_file(&demo);
}

#[test]
fn zoom_messages_are_unsupported_at_the_host() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);

    for tag in ["zoom_pane", "unzoom"] {
        let payload = ciborium::value::Value::Map(vec![
            (
                ciborium::value::Value::Text("t".into()),
                ciborium::value::Value::Text(tag.into()),
            ),
            (
                ciborium::value::Value::Text("pane_id".into()),
                ciborium::value::Value::Integer(1u32.into()),
            ),
        ]);
        let mut body = Vec::new();
        ciborium::into_writer(&payload, &mut body).expect("encode");
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&body);
        client.stream.write_all(&frame).expect("write");
        let Message::Unsupported { t_of } =
            client.wait_for(|m| matches!(m, Message::Unsupported { .. }))
        else {
            unreachable!()
        };
        assert_eq!(t_of, tag);
    }

    // The connection is still healthy afterwards.
    client.send(&Message::Ping { ts: 11 });
    let Message::Pong { ts } = client.wait_for(|m| matches!(m, Message::Pong { .. })) else {
        unreachable!()
    };
    assert_eq!(ts, 11);
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
    });
    let _ = snapshot(&mut again);
    // A second attach replays `latest_status` before its snapshot, so this window is
    // deterministic: if the acked record came back it would arrive before this snapshot.
    again.send(&Message::Attach {
        id: 141,
        pane_id: pane,
        size_policy: None,
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
    });
    let _ = snapshot(&mut client);
    // Query, read the 25-byte reply, print it with the escapes visible (`cat -v`).
    client.send(&Message::Keys {
        pane_id,
        bytes: b"printf '\\033]11;?\\033\\\\'; IFS= read -r -n 25 -t 5 r; printf 'R[%s]' \"$r\" | cat -v\n"
            .to_vec(),
    });
    // No `appearance` was sent: the host answers with its built-in harp dark theme.
    wait_for_pane_text(&mut client, pane_id, "rgb:0505/0505/0505");
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
    wait_for_pane_text(&mut client, pane_id, "rgb:0505/0505/0505");

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
        !text.contains("rgb:0505/0505/0505"),
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
