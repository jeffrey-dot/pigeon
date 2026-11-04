//! Test-only transparent socket recorder. All frame parsing uses the production client API.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pigeon_core::api::{decode_frame, encode_frame, snapshot_of, Message};

fn record(log: &Arc<Mutex<File>>, direction: &str, message: &Message) {
    let summary = match message {
        Message::Hello { features, .. } | Message::HelloAck { features, .. } => {
            format!("hello {}", features.join(","))
        }
        Message::Ping { ts } => format!("ping {ts}"),
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
                    record(&log, direction, &message);
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
