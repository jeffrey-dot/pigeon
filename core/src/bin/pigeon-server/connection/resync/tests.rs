use super::*;
use crate::host::state::Session;
use crate::pane::cmd::Cmd;
use pigeon_core::{proto, status::StatusStore};
use tokio::{io::AsyncReadExt, sync::mpsc};

#[tokio::test]
async fn resync_replaces_a_lost_clear_with_an_empty_complete_set() {
    let host = HostState::new("/bin/sh".into());
    let (tx, mut rx) = mpsc::channel(4);
    host.sessions
        .lock()
        .await
        .insert(1, Session::new(1, tx, "/bin/sh", "/tmp".into(), 80, 24));
    let mut records = StatusStore::new();
    records.feed(b"\x1b]7501;id=a:state=done\x07\x1b]7501;state=clear\x07");
    let revision = records.revision();
    let pane = tokio::spawn(async move {
        while let Some(command) = rx.recv().await {
            if let Cmd::StatusSnapshot { reply } = command {
                let _ = reply.send(proto::status_snapshot_message(1, records.snapshot()));
            } else {
                panic!("status-only resync must not request a screen or legacy status");
            }
        }
    });
    let state = ClientState {
        greeted: true,
        status_all: true,
        status_snapshot: true,
        ..ClientState::default()
    };
    let (server, mut client) = tokio::io::duplex(4096);
    let (_, writer) = tokio::io::split(server);
    let (outbound, writer_task) = Outbound::start(writer, host.sessions.clone());
    resync(&host, &state, &outbound).await.unwrap();
    let mut messages = Vec::new();
    for _ in 0..2 {
        let mut header = [0; 4];
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.read_exact(&mut header),
        )
        .await
        .unwrap()
        .unwrap();
        let mut payload = vec![0; u32::from_le_bytes(header) as usize];
        client.read_exact(&mut payload).await.unwrap();
        messages.push(proto::decode(&payload).unwrap());
    }
    assert!(matches!(&messages[0], Message::Sessions { items } if items.len() == 1));
    assert!(
        matches!(&messages[1], Message::StatusSnapshot { pane_id: 1, revision: r, records } if *r == revision && records.is_empty())
    );
    outbound.finish(writer_task).await;
    pane.abort();
}
