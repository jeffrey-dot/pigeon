//! Blocking PTY transport with bounded queues; terminal state stays in PaneActor.

use std::io::{Read, Write};
use tokio::sync::mpsc;

/// One PTY read. Big enough that `cat` of a large file takes a few reads per frame, not
/// dozens; the buffer is reused, and each read becomes one owned chunk of exactly its size.
const READ_CHUNK: usize = 64 * 1024;
/// Chunks of input waiting for the PTY. A child that stops reading its input (busy, or
/// stopped with ^Z) fills this; the actor then stops taking commands until it drains, which
/// pushes back through the bounded command queue to the client instead of growing memory.
const INPUT_QUEUE: usize = 64;

pub(super) struct PtyIo {
    pub output: mpsc::Receiver<Vec<u8>>,
    pub input: mpsc::Sender<Vec<u8>>,
}

impl PtyIo {
    pub fn start(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Self {
        Self {
            output: start_reader(reader),
            input: start_writer(writer),
        }
    }
}

/// PTY output, read on a thread because the PTY reader is blocking.
fn start_reader(mut reader: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
    let (pty_tx, pty_rx) = mpsc::channel::<Vec<u8>>(64);
    let debug = crate::debug();
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if debug {
                        eprintln!(
                            "pigeon-server: pty {:?}",
                            String::from_utf8_lossy(&buffer[..count])
                        );
                    }
                    if pty_tx.blocking_send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    pty_rx
}

/// PTY input, written on a thread so a stalled child never blocks the actor.
fn start_writer(mut writer: Box<dyn Write + Send>) -> mpsc::Sender<Vec<u8>> {
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(INPUT_QUEUE);
    std::thread::spawn(move || {
        while let Some(chunk) = write_rx.blocking_recv() {
            if writer.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    write_tx
}
