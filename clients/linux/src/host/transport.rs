//! Bounded socket pumps. Failure closes both directions and is reported to the UI.
use async_channel::{Receiver, Sender};
use pigeon_core::api::{decode_frame, encode_frame, Message};
use std::{
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const OUT_BYTES: usize = 8 * 1024 * 1024;
const IN_BYTES: usize = 16 * 1024 * 1024;
const FRAME_COUNT: usize = 256;

struct Charged<T> {
    value: Option<T>,
    bytes: usize,
    budget: Arc<AtomicUsize>,
}
impl<T> Charged<T> {
    fn new(value: T, bytes: usize, budget: &Arc<AtomicUsize>, cap: usize) -> Option<Self> {
        let mut used = budget.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes).filter(|next| *next <= cap)?;
            match budget.compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(current) => used = current,
            }
        }
        Some(Self {
            value: Some(value),
            bytes,
            budget: Arc::clone(budget),
        })
    }
}
impl<T> Drop for Charged<T> {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct State {
    socket: UnixStream,
    ready: AtomicBool,
    closed: AtomicBool,
    reason: Mutex<Option<String>>,
    out: Sender<Charged<Vec<u8>>>,
    incoming: Sender<Charged<Message>>,
}
impl State {
    fn close(&self, reason: &str) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            if let Ok(mut saved) = self.reason.lock() {
                *saved = Some(reason.into());
            }
            self.out.close();
            self.incoming.close();
            let _ = self.socket.shutdown(Shutdown::Both);
        }
    }
}

#[derive(Clone)]
pub struct Transport {
    state: Arc<State>,
    budget: Arc<AtomicUsize>,
    incoming: Receiver<Charged<Message>>,
}

impl Transport {
    pub fn new(socket: UnixStream) -> Result<Self, String> {
        Self::with_deadlines(socket, Duration::from_secs(5), Duration::from_secs(45))
    }
    fn with_deadlines(
        mut reader: UnixStream,
        hello: Duration,
        idle: Duration,
    ) -> Result<Self, String> {
        let mut writer = reader.try_clone().map_err(|e| e.to_string())?;
        let control = reader.try_clone().map_err(|e| e.to_string())?;
        reader
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| e.to_string())?;
        writer
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let (out, output) = async_channel::bounded::<Charged<Vec<u8>>>(FRAME_COUNT);
        let (incoming, input) = async_channel::bounded::<Charged<Message>>(FRAME_COUNT);
        let state = Arc::new(State {
            socket: control,
            ready: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            reason: Mutex::new(None),
            out,
            incoming,
        });
        let transport = Self {
            state: state.clone(),
            budget: Arc::new(AtomicUsize::new(0)),
            incoming: input,
        };
        let outbound = state.clone();
        thread::spawn(move || {
            while let Ok(frame) = output.recv_blocking() {
                if outbound.closed.load(Ordering::Acquire) {
                    break;
                }
                if frame
                    .value
                    .as_ref()
                    .is_some_and(|bytes| writer.write_all(bytes).is_err())
                {
                    outbound.close("Host write failed or timed out");
                    break;
                }
            }
        });
        let heartbeat = transport.clone();
        thread::spawn(move || {
            let budget = Arc::new(AtomicUsize::new(0));
            let started = Instant::now();
            let mut received = started;
            let mut ping = started;
            let mut buffer = Vec::with_capacity(65536);
            let mut chunk = [0; 16384];
            'read: loop {
                if state.closed.load(Ordering::Acquire) {
                    break;
                }
                let ready = state.ready.load(Ordering::Acquire);
                if (!ready && started.elapsed() >= hello) || (ready && received.elapsed() >= idle) {
                    state.close(if ready {
                        "Host timed out"
                    } else {
                        "Host handshake timed out"
                    });
                    break;
                }
                if ready && ping.elapsed() >= Duration::from_secs(15) {
                    heartbeat.send(Message::Ping {
                        ts: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    });
                    ping = Instant::now();
                }
                match reader.read(&mut chunk) {
                    Ok(0) => {
                        state.close("Host closed the connection");
                        break;
                    }
                    Ok(count) => buffer.extend_from_slice(&chunk[..count]),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue
                    }
                    Err(_) => {
                        state.close("Host read failed");
                        break;
                    }
                }
                loop {
                    match decode_frame(&buffer) {
                        Ok(Some((message, used))) => {
                            received = Instant::now();
                            let Some(packet) = Charged::new(message, used, &budget, IN_BYTES)
                            else {
                                state.close("Host receive queue exceeded its byte limit");
                                break 'read;
                            };
                            buffer.drain(..used);
                            if state.incoming.try_send(packet).is_err() {
                                state.close("Host receive queue exceeded its frame limit");
                                break 'read;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            state.close("Host sent an invalid frame");
                            break 'read;
                        }
                    }
                }
            }
        });
        Ok(transport)
    }

    pub fn ready(&self) {
        self.state.ready.store(true, Ordering::Release);
    }
    pub fn connected(&self) -> bool {
        !self.state.closed.load(Ordering::Acquire)
    }
    pub fn send(&self, message: Message) {
        if self.state.closed.load(Ordering::Acquire) {
            return;
        }
        let Ok(frame) = encode_frame(&message) else {
            self.state.close("Cannot encode outgoing message");
            return;
        };
        let len = frame.len();
        let Some(packet) = Charged::new(frame, len, &self.budget, OUT_BYTES) else {
            self.state.close("Input queue exceeded its byte limit");
            return;
        };
        if self.state.out.try_send(packet).is_err() {
            self.state.close("Input queue exceeded its frame limit");
        }
    }
    pub fn recv(&self) -> Option<Message> {
        let mut packet = self.incoming.recv_blocking().ok()?;
        packet.value.take()
    }
    pub async fn next(&self) -> Option<Message> {
        let mut packet = self.incoming.recv().await.ok()?;
        packet.value.take()
    }
    pub fn reason(&self) -> String {
        self.state
            .reason
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .unwrap_or_else(|| "Disconnected".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charge_is_released_and_oversize_is_refused() {
        let budget = Arc::new(AtomicUsize::new(0));
        let first = Charged::new((), 4, &budget, 8).unwrap();
        assert!(Charged::new((), 5, &budget, 8).is_none());
        drop(first);
        assert_eq!(budget.load(Ordering::Relaxed), 0);
        assert!(Charged::new((), 8, &budget, 8).is_some());
    }
    #[test]
    fn silent_handshake_closes_instead_of_hanging_the_client() {
        let (client, _server) = UnixStream::pair().unwrap();
        let transport =
            Transport::with_deadlines(client, Duration::from_millis(150), Duration::from_secs(2))
                .unwrap();
        assert!(transport.recv().is_none());
        assert!(transport.reason().contains("handshake"));
    }
    #[test]
    fn received_frames_keep_order_and_idle_closes_both_directions() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let transport =
            Transport::with_deadlines(client, Duration::from_secs(1), Duration::from_millis(200))
                .unwrap();
        transport.ready();
        for ts in 1..=3 {
            server
                .write_all(&encode_frame(&Message::Pong { ts }).unwrap())
                .unwrap();
        }
        for ts in 1..=3 {
            assert_eq!(transport.recv(), Some(Message::Pong { ts }));
        }
        assert!(transport.recv().is_none());
        assert_eq!(transport.reason(), "Host timed out");
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).unwrap(), 0);
    }

    #[test]
    fn malformed_frame_and_receive_flood_close_with_explicit_reason() {
        for flood in [false, true] {
            let (client, mut server) = UnixStream::pair().unwrap();
            let transport = Transport::new(client).unwrap();
            transport.ready();
            if flood {
                let mut frames = Vec::new();
                for ts in 0..=FRAME_COUNT {
                    frames.extend(encode_frame(&Message::Pong { ts: ts as u64 }).unwrap());
                }
                server.write_all(&frames).unwrap();
            } else {
                server.write_all(&u32::MAX.to_le_bytes()).unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while !transport.state.closed.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(transport.state.closed.load(Ordering::Acquire));
            assert!(transport.reason().contains(if flood {
                "frame limit"
            } else {
                "invalid frame"
            }));
        }
    }

    #[test]
    fn oversized_outgoing_message_is_not_silently_dropped() {
        let (client, _server) = UnixStream::pair().unwrap();
        let transport = Transport::new(client).unwrap();
        transport.send(Message::Paste {
            pane_id: 1,
            text: "x".repeat(OUT_BYTES + 1),
        });
        assert!(transport.recv().is_none());
        assert_eq!(transport.reason(), "Cannot encode outgoing message");
    }
}
