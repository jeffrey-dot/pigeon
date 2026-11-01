//! The sending half of one connection: an `OutQueue` that the reader side fills and a
//! writer task that drains it into the socket. The reader never waits on the socket, so a
//! window behind a stalled ssh link cannot hold up event intake for that connection; its
//! cost is the queue, which `OutQueue` bounds.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt, WriteHalf};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use pigeon_core::proto::{self, Message};

use crate::host_state::Sessions;
use crate::out_queue::{Frame, OutQueue, Outgoing, OverCap};
use crate::pane_query::snapshot_of;

/// Encoded bytes one connection may have waiting before the host gives up on it. A full
/// snapshot with history is at most one 8 MiB frame, so this leaves room for several plus
/// a long run of control messages; a window this far behind reconnects and re-attaches.
pub const QUEUE_CAP: usize = 64 * 1024 * 1024;

/// How long a closing connection may take to send what is still queued (a final `error`).
const LINGER: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Outbound {
    queue: Arc<Mutex<OutQueue>>,
    ready: Arc<Notify>,
}

impl Outbound {
    /// Starts the writer task for `writer`.
    pub fn start<S>(
        writer: WriteHalf<S>,
        sessions: Sessions,
    ) -> (Self, JoinHandle<Result<(), String>>)
    where
        S: AsyncWrite + Send + 'static,
    {
        let outbound = Self {
            queue: Arc::new(Mutex::new(OutQueue::new(QUEUE_CAP))),
            ready: Arc::new(Notify::new()),
        };
        let task = tokio::spawn(write_loop(outbound.clone(), writer, sessions));
        (outbound, task)
    }

    /// Queues a message that must arrive, in order with the others like it.
    pub fn send(&self, message: &Message) -> Result<(), String> {
        let frame = encode(message)?;
        self.control(frame)
    }

    /// Queues an already encoded control frame.
    pub fn control(&self, frame: Frame) -> Result<(), String> {
        let pushed = self.lock().push_control(frame);
        self.ready.notify_one();
        pushed.map_err(over_cap)
    }

    /// Queues a live delta; it may be folded into a snapshot if the pane has one waiting.
    pub fn delta(&self, pane_id: u32, frame: Frame) -> Result<(), String> {
        let pushed = self.lock().push_delta(pane_id, frame);
        self.ready.notify_one();
        pushed.map_err(over_cap)
    }

    /// Queues a snapshot of the pane, taken when the writer gets to it.
    pub fn snapshot(&self, pane_id: u32) {
        self.lock().push_snapshot(pane_id);
        self.ready.notify_one();
    }

    /// Lets the writer send what is queued, then waits for it to finish (bounded by
    /// `LINGER`, so a client that stopped reading cannot keep the task alive).
    pub async fn finish(&self, task: JoinHandle<Result<(), String>>) {
        self.lock().close();
        self.ready.notify_one();
        let abort = task.abort_handle();
        if tokio::time::timeout(LINGER, task).await.is_err() {
            abort.abort();
        }
    }

    fn lock(&self) -> MutexGuard<'_, OutQueue> {
        // The queue holds no invariant a panicking holder could have broken halfway that
        // matters more than delivering the rest, and nothing here panics anyway.
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The next item, or `None` once the queue is closed and empty.
    async fn next(&self) -> Option<Outgoing> {
        loop {
            {
                let mut queue = self.lock();
                if let Some(item) = queue.pop() {
                    return Some(item);
                }
                if queue.is_closed() {
                    return None;
                }
            }
            // `notify_one` keeps a permit when nobody waits, so a push between the check
            // above and this await is not lost.
            self.ready.notified().await;
        }
    }

    fn is_empty(&self) -> bool {
        self.lock().len() == 0
    }
}

async fn write_loop<S>(
    outbound: Outbound,
    mut writer: WriteHalf<S>,
    sessions: Sessions,
) -> Result<(), String>
where
    S: AsyncWrite,
{
    while let Some(item) = outbound.next().await {
        let frame = match item {
            Outgoing::Frame(frame) => frame,
            // Taken now, so it is the newest screen no matter how long it waited. A pane
            // that is gone meanwhile has nothing to show (its `layout` says so).
            Outgoing::Snapshot(pane_id) => match snapshot_of(&sessions, pane_id, false).await {
                Some(snapshot) => encode(&snapshot)?,
                None => continue,
            },
        };
        writer
            .write_all(&frame)
            .await
            .map_err(|error| error.to_string())?;
        // tokio's stdout is buffered; without a flush a `--stdio` client would wait for a
        // frame still sitting in the writer. Once per burst is enough.
        if outbound.is_empty() {
            writer.flush().await.map_err(|error| error.to_string())?;
        }
    }
    writer.flush().await.map_err(|error| error.to_string())
}

pub fn encode(message: &Message) -> Result<Frame, String> {
    proto::encode_frame(message)
        .map(Frame::from)
        .map_err(|error| format!("encode {}: {error}", message.name()))
}

fn over_cap(error: OverCap) -> String {
    format!(
        "too slow: {} bytes queued (cap {}), disconnecting",
        error.queued, error.cap
    )
}
