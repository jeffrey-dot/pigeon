//! One pane: a PTY, the threads that move its bytes, and the task that owns its terminal
//! state. Everything that touches `PaneState` goes through this task's `Cmd` queue.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

use pigeon_core::session::{PaneMeta, PaneState};
use pigeon_core::term::Palette;

use crate::cmd::Cmd;
use crate::event::{encode, Event};
use crate::host_state::{Exits, HostState, Meta};

const SCROLLBACK: usize = 10_000;
/// At most one delta per pane per frame. A burst of output (`cat`, a build log) arrives in
/// many small PTY reads, and one delta each would repaint the whole screen dozens of times
/// per frame and flood an ssh link; a lone keystroke still goes out immediately.
const FRAME: Duration = Duration::from_millis(16);

/// Pane ids are unique for the whole process, not per session.
pub static NEXT_PANE: AtomicU32 = AtomicU32::new(1);

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
    // A terminal must tell programs what it is; without this htop/vim refuse.
    command.env(
        "TERM",
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_owned()),
    );
    command.env("COLORTERM", "truecolor");
    if let Some(dir) = cwd {
        command.cwd(dir);
    }
    let child = pair.slave.spawn_command(command).ok()?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().ok()?;
    let writer = pair.master.take_writer().ok()?;
    let master: Master = Arc::new(StdMutex::new(pair.master));
    let pty_rx = start_reader(reader);
    let write_tx = start_writer(writer);

    let pane_id = NEXT_PANE.fetch_add(1, Ordering::Relaxed);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>(64);
    let actor = PaneActor {
        pane: PaneState::new(pane_id, cols, rows, SCROLLBACK),
        child,
        master,
        write_tx,
        events: host.events.clone(),
        meta: Arc::clone(&host.meta),
        exits: Arc::clone(&host.exits),
        last_revision: 0,
        last_meta: None,
        last_flush: Instant::now() - FRAME,
        dirty: false,
        echo_due: false,
    };
    tokio::spawn(actor.run(cmd_rx, pty_rx, host.current_palette()));
    Some((pane_id, cmd_tx))
}

/// PTY output, read on a thread because the PTY reader is blocking.
fn start_reader(mut reader: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
    let (pty_tx, pty_rx) = mpsc::channel::<Vec<u8>>(64);
    let debug = std::env::var_os("PIGEON_DEBUG").is_some();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if debug {
                        eprintln!(
                            "pigeon-host: pty {:?}",
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
fn start_writer(mut writer: Box<dyn Write + Send>) -> std::sync::mpsc::Sender<Vec<u8>> {
    let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for chunk in write_rx {
            if writer.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    write_tx
}

struct PaneActor {
    pane: PaneState,
    child: Box<dyn Child + Send + Sync>,
    master: Master,
    write_tx: std::sync::mpsc::Sender<Vec<u8>>,
    events: broadcast::Sender<Event>,
    meta: Meta,
    exits: Exits,
    last_revision: u32,
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
        loop {
            tokio::select! {
                _ = sleep_until(self.last_flush + FRAME), if self.dirty => {
                    self.flush_now();
                }
                command = cmd_rx.recv(), if commands_open => {
                    match command {
                        Some(command) => self.handle(command),
                        None => commands_open = false,
                    }
                }
                _ = ticker.tick() => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        self.exited(status.exit_code() as i32);
                        break;
                    }
                }
                chunk = pty_rx.recv() => {
                    let Some(chunk) = chunk else {
                        let code = self
                            .child
                            .wait()
                            .map(|status| status.exit_code() as i32)
                            .unwrap_or(-1);
                        self.exited(code);
                        break;
                    };
                    self.output(&chunk).await;
                }
            }
        }
    }

    fn handle(&mut self, command: Cmd) {
        let pane = &mut self.pane;
        match command {
            Cmd::Input(bytes) => {
                self.echo_due = true;
                let _ = self.write_tx.send(bytes);
            }
            Cmd::Paste(text) => {
                let _ = self.write_tx.send(pane.paste_bytes(&text));
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
            }
            Cmd::SetPalette(palette) => {
                pane.set_palette(palette);
                // No flush: the palette only affects query answers, not the screen.
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
        let pane_id = self.pane.pane_id;
        self.pane.feed(chunk);
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
        let revision = self.pane.status().revision();
        if revision != self.last_revision {
            self.last_revision = revision;
            if let Some(frame) = encode(&self.pane.status_message()) {
                let _ = self.events.send(Event::Status { pane_id, frame });
            }
        }
        let replies = self.pane.take_replies();
        if !replies.is_empty() {
            let _ = self.write_tx.send(replies);
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
        if let Ok(mut exits) = self.exits.lock() {
            exits.insert(pane_id, code);
        }
        let _ = self.events.send(Event::Exited { pane_id, code });
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
