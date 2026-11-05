use pigeon_core::api::{self, Message, Snapshot};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::Result;

pub struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
    pub screens: BTreeMap<u32, (u32, Snapshot)>,
    pub wire_bytes: u64,
    pub snapshots: u64,
    pub deltas: u64,
    pub stale_deltas: u64,
    last_ping: Instant,
}

impl Client {
    pub fn connect(socket: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut client = Self {
            stream,
            buffer: Vec::new(),
            screens: BTreeMap::new(),
            wire_bytes: 0,
            snapshots: 0,
            deltas: 0,
            stale_deltas: 0,
            last_ping: Instant::now(),
        };
        client.send(&Message::Hello {
            proto: api::PROTO_VERSION,
            client: "host-bench".into(),
            features: vec!["delta".into()],
        })?;
        client.wait(|message| matches!(message, Message::HelloAck { .. }))?;
        Ok(client)
    }

    pub fn send(&mut self, message: &Message) -> Result<()> {
        self.stream.write_all(&api::encode_frame(message)?)?;
        Ok(())
    }

    pub fn keys(&mut self, pane_id: u32, text: &str) -> Result<()> {
        self.send(&Message::Keys {
            pane_id,
            bytes: text.as_bytes().to_vec(),
        })
    }

    pub fn attach(&mut self, pane_id: u32) -> Result<()> {
        self.send(&Message::Attach {
            id: pane_id,
            pane_id,
            size_policy: Some("locked".into()),
            history: Some(false),
            since: None,
        })?;
        self.wait(
            |message| matches!(message, Message::Snapshot { pane_id: id, .. } if *id == pane_id),
        )?;
        Ok(())
    }

    pub fn wait(&mut self, wanted: impl Fn(&Message) -> bool) -> Result<Message> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let message = self.next(deadline)?;
            if wanted(&message) {
                return Ok(message);
            }
        }
    }

    pub fn next(&mut self, deadline: Instant) -> Result<Message> {
        loop {
            if Instant::now() >= deadline {
                return Err("protocol wait exceeded deadline".into());
            }
            if self.last_ping.elapsed() >= Duration::from_secs(15) {
                self.send(&Message::Ping { ts: 1 })?;
                self.last_ping = Instant::now();
            }
            if let Some((message, used)) = api::decode_frame(&self.buffer)? {
                self.buffer.drain(..used);
                self.apply(&message)?;
                return Ok(message);
            }
            if self.buffer.len() > api::MAX_FRAME + 4 {
                return Err("oversized frame".into());
            }
            self.stream.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut chunk = [0; 65536];
            let count = match self.stream.read(&mut chunk) {
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error.into()),
            };
            if count == 0 {
                return Err("host closed benchmark connection".into());
            }
            self.wire_bytes += count as u64;
            self.buffer.extend_from_slice(&chunk[..count]);
        }
    }

    fn apply(&mut self, message: &Message) -> Result<()> {
        match message {
            Message::Snapshot { seq, .. } => {
                let (pane, screen) = api::snapshot_of(message)?;
                self.screens.insert(pane, (*seq, screen));
                self.snapshots += 1;
            }
            Message::Delta { .. } => {
                let delta = api::delta_of(message)?;
                let (seq, screen) = self
                    .screens
                    .get_mut(&delta.pane_id)
                    .ok_or("delta before snapshot")?;
                // An attach can sample output before its queued broadcast is forwarded.
                if delta.seq <= *seq {
                    self.stale_deltas += 1;
                    return Ok(());
                }
                if delta.seq != seq.checked_add(1).ok_or("sequence overflow")? {
                    return Err(format!(
                        "delta gap for pane {}: {} -> {}",
                        delta.pane_id, seq, delta.seq
                    )
                    .into());
                }
                for (row, cells) in delta.rows {
                    if cells.len() != usize::from(screen.cols) {
                        return Err("delta width mismatch".into());
                    }
                    *screen
                        .screen
                        .get_mut(usize::from(row))
                        .ok_or("delta row outside screen")? = cells;
                }
                if let Some(cursor) = delta.cursor {
                    screen.cursor = cursor;
                }
                if let Some(alt) = delta.alt {
                    screen.alt = alt;
                }
                *seq = delta.seq;
                self.deltas += 1;
            }
            Message::Error { code, detail, .. } => {
                return Err(format!("host {code}: {detail}").into())
            }
            Message::HelloError { reason, .. } => return Err(format!("handshake: {reason}").into()),
            Message::PaneExited { pane_id, code } => {
                return Err(format!("fixture {pane_id} exited: {code}").into())
            }
            _ => {}
        }
        Ok(())
    }

    pub fn has_line(&self, pane: u32, wanted: &str) -> bool {
        self.screens.get(&pane).is_some_and(|(_, screen)| {
            screen.screen.iter().any(|row| {
                let text: String = row.iter().map(|cell| cell.text()).collect();
                text.trim_end() == wanted
            })
        })
    }

    pub fn wait_line(&mut self, pane: u32, wanted: &str) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.has_line(pane, wanted) {
            self.next(deadline)?;
        }
        Ok(())
    }
}
