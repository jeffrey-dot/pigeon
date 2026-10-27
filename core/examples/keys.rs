//! Dev helper: attach to a pane and send raw bytes.
//!
//! Synthetic keyboard events need Accessibility permission; this goes through the
//! real protocol instead, so the GUI can be driven while it renders.
//!
//!     cargo run --example keys -- --socket /tmp/p.sock --pane 1 --send 'q'

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use pigeon_core::proto::{self, Message};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut socket = String::new();
    let mut pane_id = 0u32;
    let mut payload = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next().ok_or("--socket needs a value")?,
            "--pane" => pane_id = args.next().ok_or("--pane needs a value")?.parse()?,
            "--send" => payload = args.next().ok_or("--send needs a value")?,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    let mut stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;

    send(
        &mut stream,
        &Message::Hello {
            proto: proto::PROTO_VERSION,
            client: "pigeon-keys".into(),
            features: Vec::new(),
        },
    )?;
    // Drain whatever the host says first (hello_ack + sessions).
    let mut buf = [0u8; 4096];
    let _ = stream.read(&mut buf);

    send(
        &mut stream,
        &Message::Attach {
            id: 1,
            pane_id,
            size_policy: None,
        },
    )?;
    send(
        &mut stream,
        &Message::Keys {
            pane_id,
            bytes: unescape(&payload),
        },
    )?;
    // Give the host a moment to write to the PTY before we hang up.
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}

fn send(stream: &mut UnixStream, message: &Message) -> Result<(), Box<dyn std::error::Error>> {
    let frame = proto::encode_frame(message)?;
    stream.write_all(&frame)?;
    Ok(())
}

fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.extend_from_slice(ch.to_string().as_bytes());
            continue;
        }
        match chars.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('s') => out.push(b' '),
            Some('\\') => out.push(b'\\'),
            Some(other) => {
                out.push(b'\\');
                out.extend_from_slice(other.to_string().as_bytes());
            }
            None => out.push(b'\\'),
        }
    }
    out
}
