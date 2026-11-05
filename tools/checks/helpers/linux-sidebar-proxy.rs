//! Test-only transparent socket recorder. All frame parsing uses the production client API.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pigeon_core::api::{Message, decode_frame, encode_frame, snapshot_of};

fn record(log: &Arc<Mutex<File>>, direction: &str, message: &Message) {
    let summary = match message {
        Message::Hello { features, .. } | Message::HelloAck { features, .. } => {
            format!("hello {}", features.join(","))
        }
        Message::Ping { ts } => format!("ping {ts}"),
        Message::TextQuery { id, pane_id, query } => format!("query {id} {pane_id} {query:?}"),
        Message::TextQueryResult {
            id,
            pane_id,
            result,
        } => format!("result {id} {pane_id} {:?}", result.info()),
        Message::Error { id, code, .. } => format!("error {id:?} {code}"),
        Message::Pong { ts } => format!("pong {ts}"),
        Message::Scroll { pane_id, offset } => format!("scroll {pane_id} {offset}"),
        Message::Snapshot {
            pane_id,
            seq,
            cols,
            rows,
            offset,
            history_len,
            alt,
            ..
        } => {
            let text = snapshot_of(message).unwrap().1.screen[0]
                .iter()
                .map(|cell| cell.text())
                .collect::<String>();
            format!(
                "snapshot {pane_id} {seq} {cols} {rows} {} {} {alt} {}",
                offset.map(i64::from).unwrap_or(-1),
                history_len.map(i64::from).unwrap_or(-1),
                text.trim_end()
            )
        }
        Message::Delta { pane_id, seq, .. } => format!("delta {pane_id} {seq}"),
        Message::Keys { pane_id, bytes } => format!("keys {pane_id} {bytes:?}"),
        Message::Paste { pane_id, .. } => format!("paste {pane_id}"),
        Message::ResizePane {
            pane_id,
            cols,
            rows,
        } => format!("resize {pane_id} {cols} {rows}"),
        Message::Attach { pane_id, .. } => format!("attach {pane_id}"),
        Message::Layout {
            session_id,
            focus,
            zoom,
            ..
        } => format!("layout {session_id} {focus:?} {zoom:?}"),
        Message::Sessions { items } => format!(
            "sessions {:?}",
            items
                .iter()
                .map(|s| (s.session_id, &s.panes))
                .collect::<Vec<_>>()
        ),
        Message::Created {
            session_id,
            pane_id,
            ..
        } => format!("created {session_id} {pane_id}"),
        Message::StatusSnapshot {
            pane_id,
            revision,
            records,
        } => format!(
            "status {pane_id} {revision} {:?}",
            records
                .iter()
                .map(|r| (&r.id, r.seq, &r.state))
                .collect::<Vec<_>>()
        ),
        Message::AckStatus { pane_id, seq } => format!("ack {pane_id} {seq}"),
        Message::ClosePane { pane_id, .. } => format!("close {pane_id}"),
        Message::ResizeSplit {
            split_id, ratio, ..
        } => format!("resize_split {split_id} {ratio}"),
        Message::SizeState {
            pane_id,
            clients,
            source,
            ..
        } => format!("size {pane_id} {clients} {source}"),
        _ => return,
    };
    let mut log = log.lock().unwrap();
    writeln!(log, "{direction} {summary}").unwrap();
    log.flush().unwrap();
}

fn forward(mut reader: UnixStream, mut writer: UnixStream, log: Arc<Mutex<File>>, direction: &str) {
    let mut buffer = Vec::new();
    let mut chunk = [0; 65536];
    while let Ok(count) = reader.read(&mut chunk) {
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..count]);
        loop {
            match decode_frame(&buffer) {
                Ok(Some((mut message, used))) => {
                    if std::env::var_os("PIGEON_TEST_NO_SCROLLBACK").is_some() {
                        if let Message::HelloAck { features, .. } = &mut message {
                            features.retain(|feature| feature != "scrollback");
                        }
                    }
                    if std::env::var_os("PIGEON_TEST_NO_SEARCH").is_some() {
                        if let Message::HelloAck { features, .. } = &mut message {
                            features.retain(|feature| {
                                !matches!(feature.as_str(), "text_query" | "text_viewport")
                            });
                        }
                    }
                    if matches!(message, Message::TextQueryResult { .. })
                        && std::env::var_os("PIGEON_TEST_SEARCH_DELAY").is_some()
                    {
                        std::thread::sleep(Duration::from_millis(150));
                    }
                    record(&log, direction, &message);
                    if let Message::Attach {
                        pane_id,
                        since: Some(seq),
                        ..
                    } = &message
                    {
                        let mut log = log.lock().unwrap();
                        writeln!(log, "{direction} resume {pane_id} {seq}").unwrap();
                        log.flush().unwrap();
                    }
                    if matches!(message, Message::Snapshot { .. })
                        && std::env::var_os("PIGEON_TEST_RECOVERY_DELAY").is_some()
                    {
                        std::thread::sleep(Duration::from_millis(350));
                    }
                    // Exercise several GTK gestures before the first answer arrives.
                    if matches!(
                        message,
                        Message::Snapshot {
                            offset: Some(_),
                            ..
                        }
                    ) {
                        std::thread::sleep(Duration::from_millis(80));
                    }
                    if writer.write_all(&encode_frame(&message).unwrap()).is_err() {
                        return;
                    }
                    buffer.drain(..used);
                }
                Ok(None) => break,
                Err(error) => panic!("proxy decode: {error}"),
            }
        }
    }
    let _ = writer.shutdown(std::net::Shutdown::Write);
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--seed") {
        seed(&args[2], &args[3]);
        return;
    }
    let listener = UnixListener::bind(&args[1]).unwrap();
    let log = Arc::new(Mutex::new(File::create(&args[3]).unwrap()));
    for client in listener.incoming() {
        let client = client.unwrap();
        let host = UnixStream::connect(&args[2]).unwrap();
        let reader = client.try_clone().unwrap();
        let writer = host.try_clone().unwrap();
        let copy = log.clone();
        std::thread::spawn(move || forward(reader, writer, copy, "C"));
        let copy = log.clone();
        std::thread::spawn(move || forward(host, client, copy, "S"));
    }
}

fn seed(socket: &str, cwd: &str) {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(
            &encode_frame(&Message::Hello {
                proto: pigeon_core::api::PROTO_VERSION,
                client: "sidebar-test-seed".into(),
                features: vec![],
            })
            .unwrap(),
        )
        .unwrap();
    stream
        .write_all(
            &encode_frame(&Message::NewSession {
                id: 1,
                cwd: Some(cwd.into()),
                shell: None,
                env: None,
            })
            .unwrap(),
        )
        .unwrap();
    let mut buffer = vec![];
    let mut chunk = [0; 65536];
    loop {
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        buffer.extend_from_slice(&chunk[..count]);
        while let Some((message, used)) = decode_frame(&buffer).unwrap() {
            if matches!(message, Message::Created { .. }) {
                return;
            }
            buffer.drain(..used);
        }
    }
}
