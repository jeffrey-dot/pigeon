//! Screens that survive a host restart (`docs/roadmap.md` P6). Every few seconds, and when a
//! session's set of panes changes, the host writes each session's layout and last screens to
//! `<socket>.state` (`~/.pigeon/state/pigeon.state` for an installed remote host). On start it reads that file back: every saved pane
//! becomes a frozen pane showing its last screen, already exited, so the window shows what
//! was there and the user closes it when done (`pane_exited` with code -1).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use pigeon_core::session::{snapshot_from_message, PaneState, SavedState, SessionState};

use crate::cmd::Cmd;
use crate::frozen_pane::spawn_frozen;
use crate::host_state::HostState;
use crate::pane_actor::NEXT_PANE;

/// How often the screens on disk catch up with the live ones.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// The exit code a restored pane reports: its process belonged to the previous host.
pub const RESTORED_EXIT: i32 = -1;

/// The state file next to the socket and named after it (`pigeon.sock` →
/// `pigeon.state`), so two hosts on two sockets in one directory never share one file.
pub fn state_path(socket: &Path) -> PathBuf {
    socket.with_extension("state")
}

/// Rebuilds the saved sessions as frozen panes. A missing, unreadable, foreign or torn file
/// restores nothing (and is left alone: a newer build may know how to read it).
pub async fn restore(host: &HostState, path: &Path) -> usize {
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    let saved = match SavedState::decode(&bytes) {
        Ok(saved) => saved,
        Err(error) => {
            eprintln!("pigeon-host: not restoring {path:?}: {error:?}");
            return 0;
        }
    };
    let mut restored = 0;
    for session in &saved.sessions {
        let mut renumber = HashMap::new();
        let mut panes = HashMap::new();
        for pane in &session.panes {
            let Some(snapshot) = snapshot_from_message(&pane.screen) else {
                continue;
            };
            let pane_id = NEXT_PANE.fetch_add(1, Ordering::Relaxed);
            let sender = spawn_frozen(PaneState::restored(pane_id, snapshot));
            renumber.insert(pane.pane_id, pane_id);
            panes.insert(pane_id, sender);
            host.meta.lock().await.insert(
                pane_id,
                pigeon_core::session::PaneMeta {
                    cwd: pane.cwd.clone(),
                    title: pane.title.clone(),
                },
            );
            if let Ok(mut exits) = host.exits.lock() {
                exits.insert(pane_id, RESTORED_EXIT);
            }
        }
        let (cols, rows) = session_size(session);
        let Ok(state) = SessionState::restored(session, &renumber, panes, cols, rows) else {
            continue;
        };
        // A session is named after its first pane, like a live one.
        let Some(&session_id) = state.layout.pane_ids().first() else {
            continue;
        };
        host.sessions.lock().await.insert(session_id, state);
        restored += 1;
    }
    restored
}

/// The box a restored session starts with: its first screen's size, until a client votes.
fn session_size(session: &pigeon_core::session::SavedSession) -> (u16, u16) {
    session
        .panes
        .first()
        .and_then(|pane| snapshot_from_message(&pane.screen))
        .map_or((80, 24), |snapshot| (snapshot.cols, snapshot.rows))
}

/// Writes the state file now. Atomic (temp file + rename) and private (0600): a crash
/// mid-write leaves the previous file, never half of one.
pub async fn save(host: &HostState, path: &Path) -> Result<(), String> {
    // Senders are cloned out so no pane is asked for its screen under the sessions lock.
    let panes: Vec<(u32, mpsc::Sender<Cmd>)> = {
        let map = host.sessions.lock().await;
        map.values()
            .flat_map(|session| session.panes.iter())
            .map(|(pane, sender)| (*pane, sender.clone()))
            .collect()
    };
    let mut screens = HashMap::new();
    {
        for (pane_id, sender) in &panes {
            let (tx, rx) = oneshot::channel();
            if sender
                .send(Cmd::Snapshot {
                    history: false,
                    reply: tx,
                })
                .await
                .is_err()
            {
                continue;
            }
            if let Ok(message) = rx.await {
                screens.insert(*pane_id, message);
            }
        }
    }
    let meta = host.meta.lock().await.clone();
    let sessions = {
        let map = host.sessions.lock().await;
        let mut ids: Vec<u32> = map.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| map.get(&id)?.saved(id, &meta, &screens))
            .collect()
    };
    let bytes = SavedState::new(sessions)
        .encode()
        .map_err(|error| format!("encode state: {error:?}"))?;
    write_private(path, &bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("open {temporary:?}: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("write {temporary:?}: {error}"))?;
    std::fs::rename(&temporary, path).map_err(|error| format!("rename to {path:?}: {error}"))
}

/// Saves on a timer for the life of the host.
pub fn start(host: HostState, path: PathBuf) {
    tokio::spawn(async move {
        // `PIGEON_SAVE_MS` shortens the period for the end-to-end tests.
        let every = std::env::var("PIGEON_SAVE_MS")
            .ok()
            .and_then(|ms| ms.parse::<u64>().ok())
            .filter(|ms| *ms >= 50)
            .map_or(SAVE_EVERY, Duration::from_millis);
        let mut ticker = tokio::time::interval(every);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = save(&host, &path).await {
                eprintln!("pigeon-host: {error}");
            }
        }
    });
}
