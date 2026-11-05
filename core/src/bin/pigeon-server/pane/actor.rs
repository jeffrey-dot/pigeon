//! One pane: a PTY, the threads that move its bytes, and the task that owns its terminal
//! state. Everything that touches `PaneState` goes through this task's `Cmd` queue.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

use pigeon_core::session::{PaneMeta, PaneState};
use pigeon_core::term::Palette;

use crate::host::event::{encode, Event};
use crate::host::state::{Exits, HostState, Meta};
use crate::pane::cmd::Cmd;
use crate::pane::pty_io::PtyIo;
use crate::pane::status::StatusBroadcast;

const SCROLLBACK: usize = 10_000;
/// At most this many bytes are fed per batch before the per-batch work (metadata, status,
/// replies, frame) runs, so a flood still yields to commands and frames regularly.
const BATCH_BYTES: usize = 256 * 1024;
/// At most one delta per pane per frame. A burst of output (`cat`, a build log) arrives in
/// many small PTY reads, and one delta each would repaint the whole screen dozens of times
/// per frame and flood an ssh link; a lone keystroke still goes out immediately.
const FRAME: Duration = Duration::from_millis(16);
/// How long an exited child's PTY may stay open (a background job still holding it)
/// before the exit is announced anyway.
const EXIT_GRACE: Duration = Duration::from_millis(500);

/// Pane ids are unique for the whole process, not per session.
static NEXT_PANE: AtomicU32 = AtomicU32::new(1);

type Master = Arc<StdMutex<Box<dyn MasterPty + Send>>>;

/// Spawns a shell in a fresh PTY and starts its actor.
pub fn spawn_pane(
    host: &HostState,
    shell: &str,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Option<(u32, mpsc::Sender<Cmd>)> {
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system().openpty(size).ok()?;
    let mut command = CommandBuilder::new(shell);
    command.env("PI_PROGRAM_STATUS", "1");
    // The starter may be a Linux console or another emulator. Its terminfo would
    // advertise capabilities unrelated to the VT state owned by this host.
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    if let Some(dir) = cwd {
        command.cwd(dir);
    }
    let child = pair.slave.spawn_command(command).ok()?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().ok()?;
    let writer = pair.master.take_writer().ok()?;
    let master: Master = Arc::new(StdMutex::new(pair.master));
    let PtyIo {
        output: pty_rx,
        input: write_tx,
    } = PtyIo::start(reader, writer);

    let pane_id = NEXT_PANE.fetch_add(1, Ordering::Relaxed);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>(64);
    let actor = PaneActor {
        pane: PaneState::new(pane_id, cols, rows, SCROLLBACK),
        child,
        master,
        write_tx,
        unwritten: VecDeque::new(),
        events: host.events.clone(),
        meta: Arc::clone(&host.meta),
        exits: Arc::clone(&host.exits),
        status: StatusBroadcast::default(),
        last_meta: None,
        last_flush: Instant::now() - FRAME,
        dirty: false,
        echo_due: false,
    };
    tokio::spawn(actor.run(cmd_rx, pty_rx, host.current_palette()));
    Some((pane_id, cmd_tx))
}

struct PaneActor {
    pane: PaneState,
    child: Box<dyn Child + Send + Sync>,
    master: Master,
    write_tx: mpsc::Sender<Vec<u8>>,
    /// Input the writer had no room for yet, in order. While it is not empty the actor
    /// takes no commands (see `pty_io::INPUT_QUEUE`), so at most one command's worth waits here
    /// besides query answers.
    unwritten: VecDeque<Vec<u8>>,
    events: broadcast::Sender<Event>,
    meta: Meta,
    exits: Exits,
    status: StatusBroadcast,
    last_meta: Option<(Option<String>, Option<String>)>,
    /// When the last delta left, and whether the screen changed since.
    last_flush: Instant,
    dirty: bool,
    /// Input was written and its echo has not gone out yet: the first output after a
    /// keystroke skips the frame throttle, so fast typing echoes without a 16 ms wait.
    echo_due: bool,
}

impl PaneActor {
    async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<Cmd>,
        mut pty_rx: mpsc::Receiver<Vec<u8>>,
        palette: Option<Palette>,
    ) {
        // Panes are born with the client's palette (or the default dark one), so a program's
        // startup colour query is answered from the first byte.
        self.pane.set_palette(palette);
        let mut ticker = interval(Duration::from_millis(150));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut commands_open = true;
        let mut output_open = true;
        // The child's exit code once reaped, and when: the exit is announced only after
        // the PTY has delivered its last bytes (or a grace period, for a background job
        // that keeps the PTY open), so the final screen includes the final output.
        let mut reaped: Option<(i32, Instant)> = None;
        let mut announced = false;
        loop {
            let sync_deadline = self.pane.sync_deadline().map(Instant::from_std);
            tokio::select! {
                _ = sleep_until(sync_deadline.unwrap_or_else(Instant::now)), if sync_deadline.is_some() => {
                    self.pane.flush_sync();
                    self.after_feed().await;
                }
                _ = sleep_until(self.last_flush + FRAME), if self.dirty => {
                    self.flush_now();
                }
                permit = self.write_tx.clone().reserve_owned(), if !self.unwritten.is_empty() => {
                    match permit {
                        Ok(permit) => {
                            if let Some(chunk) = self.unwritten.pop_front() {
                                permit.send(chunk);
                            }
                        }
                        // The writer is gone with the child: nobody will read this input.
                        Err(_) => self.unwritten.clear(),
                    }
                }
                command = cmd_rx.recv(), if commands_open && self.unwritten.is_empty() => {
                    match command {
                        Some(command) => self.handle(command),
                        None => commands_open = false,
                    }
                }
                _ = ticker.tick(), if !announced => {
                    if reaped.is_none() {
                        if let Ok(Some(status)) = self.child.try_wait() {
                            reaped = Some((status.exit_code() as i32, Instant::now()));
                        }
                    }
                }
                chunk = pty_rx.recv(), if output_open => {
                    match chunk {
                        Some(chunk) => {
                            // Whatever else is already queued goes in the same batch, so
                            // the per-batch work runs once per wake-up, not once per read.
                            self.pane.feed(&chunk);
                            let mut fed = chunk.len();
                            while fed < BATCH_BYTES {
                                let Ok(more) = pty_rx.try_recv() else {
                                    break;
                                };
                                self.pane.feed(&more);
                                fed += more.len();
                            }
                            self.after_feed().await;
                        }
                        None => output_open = false,
                    }
                }
            }
            if let Some((code, at)) = reaped.filter(|_| !announced) {
                if !output_open || at.elapsed() >= EXIT_GRACE {
                    while let Ok(chunk) = pty_rx.try_recv() {
                        self.output(&chunk).await;
                    }
                    if self.dirty {
                        self.flush_now();
                    }
                    self.exited(code);
                    announced = true;
                }
            }
            // Like tmux `remain-on-exit`: an exited pane keeps answering snapshot, scroll
            // and status requests until `close_pane` drops its last sender.
            if !commands_open && announced {
                break;
            }
        }
    }

    fn handle(&mut self, command: Cmd) {
        let pane = &mut self.pane;
        match command {
            Cmd::TextQuery { query, reply } => {
                if !reply.is_closed() {
                    let _ = reply.send(pane.text_query(&query));
                }
            }

            Cmd::Input(bytes) => {
                self.echo_due = true;
                self.write_input(bytes);
            }
            Cmd::Paste(text) => {
                let bytes = pane.paste_bytes(&text);
                self.write_input(bytes);
            }
            Cmd::Resize(cols, rows) => {
                pane.resize(cols, rows);
                if let Ok(master) = self.master.lock() {
                    let _ = master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                // Everything is dirty after a resize; the snapshot each watcher
                // gets covers it, so the damage must not go out a second time.
                let _ = pane.take_damage();
                let _ = self.events.send(Event::Resized {
                    pane_id: pane.pane_id,
                });
            }
            Cmd::Snapshot { history, reply } => {
                let _ = reply.send(pane.snapshot_message(history));
            }
            Cmd::Since { since, reply } => {
                // Pending damage first, so the replay ends at the current screen
                // and the live stream continues right after it.
                if self.dirty {
                    self.flush_now();
                }
                let pane = &mut self.pane;
                let frames = pane
                    .deltas_since(since)
                    .unwrap_or_else(|| vec![pane.snapshot_message(false)]);
                let _ = reply.send(frames);
            }
            Cmd::Scroll { offset, reply } => {
                let _ = reply.send(pane.scroll_message(offset));
            }
            Cmd::Kill => {
                let _ = self.child.kill();
            }
            Cmd::Ack(seq) => {
                pane.status_mut().ack_seq(seq);
                self.status.publish(pane, &self.events);
            }
            Cmd::SetPalette(palette) => {
                pane.set_palette(palette);
                // No flush: the palette only affects query answers, not the screen.
            }
            Cmd::StatusSnapshot { reply } => {
                let message = pigeon_core::proto::status_snapshot_message(
                    pane.pane_id,
                    pane.status().snapshot(),
                );
                let _ = reply.send(message);
            }
            Cmd::Status { reply } => {
                let _ = reply.send(pane.status_message());
            }
            Cmd::LatestStatus { reply } => {
                let pending = pane.status().latest_unacked().is_some();
                let _ = reply.send(pending.then(|| pane.status_message()));
            }
        }
    }

    async fn output(&mut self, chunk: &[u8]) {
        self.pane.feed(chunk);
        self.after_feed().await;
    }

    /// What follows any change to the terminal: metadata, status, query answers, a frame.
    async fn after_feed(&mut self) {
        let pane_id = self.pane.pane_id;
        let cwd = self.pane.cwd().map(str::to_owned);
        let title = self.pane.title();
        if cwd.is_some() || title.is_some() {
            let snapshot = (cwd.clone(), title.clone());
            if self.last_meta.as_ref() != Some(&snapshot) {
                self.last_meta = Some(snapshot);
                self.meta
                    .lock()
                    .await
                    .insert(pane_id, PaneMeta { cwd, title });
                let _ = self.events.send(Event::SessionsChanged);
            }
        }
        self.status.publish(&self.pane, &self.events);
        let replies = self.pane.take_replies();
        if !replies.is_empty() {
            self.write_input(replies);
        }
        if self.echo_due || self.last_flush.elapsed() >= FRAME {
            self.echo_due = false;
            self.flush_now();
        } else {
            self.dirty = true;
        }
    }

    /// The child is gone. The exit code is kept until `close_pane`, so a client that missed
    /// the `Exited` event still learns about it when it resyncs.
    fn exited(&mut self, code: i32) {
        let pane_id = self.pane.pane_id;
        self.pane.status_mut().on_process_exit();
        self.status.publish(&self.pane, &self.events);
        if let Ok(mut exits) = self.exits.lock() {
            exits.insert(pane_id, code);
        }
        let _ = self.events.send(Event::Exited { pane_id, code });
    }

    /// Queues bytes for the PTY without waiting; what does not fit waits in `unwritten`
    /// behind anything already there, so input and query answers keep their order.
    fn write_input(&mut self, bytes: Vec<u8>) {
        if !self.unwritten.is_empty() {
            self.unwritten.push_back(bytes);
            return;
        }
        match self.write_tx.try_send(bytes) {
            Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
            Err(mpsc::error::TrySendError::Full(bytes)) => self.unwritten.push_back(bytes),
        }
    }

    fn flush_now(&mut self) {
        self.dirty = false;
        self.last_flush = Instant::now();
        // Encoded even with nobody watching: the backlog that `attach.since` replays from
        // already keeps the message, and an idle pane produces no deltas.
        if let Some(frame) = self.pane.delta_message().as_ref().and_then(encode) {
            let _ = self.events.send(Event::Delta {
                pane_id: self.pane.pane_id,
                frame,
            });
        }
    }
}
