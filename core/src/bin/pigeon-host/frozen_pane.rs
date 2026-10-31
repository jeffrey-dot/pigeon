//! A pane restored after a host restart: its last screen, no process. It answers the same
//! queries as a live pane (snapshot, scroll, status) so clients treat it like any exited
//! pane; input and resizes go nowhere.

use tokio::sync::mpsc;

use pigeon_core::session::PaneState;

use crate::cmd::Cmd;

pub fn spawn_frozen(mut pane: PaneState) -> mpsc::Sender<Cmd> {
    let (tx, mut rx) = mpsc::channel::<Cmd>(64);
    tokio::spawn(async move {
        while let Some(command) = rx.recv().await {
            match command {
                // Nothing to write to, nothing to resize: the screen stays as it was saved.
                Cmd::Input(_) | Cmd::Paste(_) | Cmd::Resize(..) | Cmd::SetPalette(_) => {}
                Cmd::Ack(seq) => {
                    pane.status_mut().ack_seq(seq);
                }
                // `close_pane` ends the pane; its task goes with the session's last sender.
                Cmd::Kill => {}
                Cmd::Snapshot { history, reply } => {
                    let _ = reply.send(pane.snapshot_message(history));
                }
                Cmd::Since { reply, .. } => {
                    let _ = reply.send(vec![pane.snapshot_message(false)]);
                }
                Cmd::Scroll { offset, reply } => {
                    let _ = reply.send(pane.scroll_message(offset));
                }
                Cmd::Status { reply } => {
                    let _ = reply.send(pane.status_message());
                }
                Cmd::LatestStatus { reply } => {
                    let _ = reply.send(None);
                }
            }
        }
    });
    tx
}
