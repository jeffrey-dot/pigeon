use super::*;
use crate::host::state::Session;
use pigeon_core::layout::Axis;
use tokio::{io::AsyncReadExt, sync::mpsc};

#[tokio::test]
async fn delayed_layout_event_cannot_restore_old_focus_after_subscription_switch() {
    let host = HostState::new("/bin/sh".into());
    let (tx, _rx) = mpsc::channel(4);
    let mut session = Session::new(1, tx, "/bin/sh", "/tmp".into(), 80, 24);
    session
        .layout
        .split(1, 2, Axis::Vertical, 0.5, 80, 24)
        .unwrap();
    session.layout.focus(2);
    let expected = session.layout_message(1);
    host.sessions.lock().await.insert(1, session);
    let state = ClientState {
        session: Some(1),
        control_since: 1,
        greeted: true,
        ..ClientState::default()
    };
    let stale = Message::Layout {
        session_id: 1,
        tree: proto::Node::Leaf { leaf: 1 },
        zoom: None,
        focus: Some(1),
    };
    let event = Event::Layout {
        session_id: 1,
        sequence: 1,
        frame: crate::host::event::encode(&stale).unwrap(),
    };
    let (server, mut client) = tokio::io::duplex(4096);
    let (_, writer) = tokio::io::split(server);
    let (outbound, writer_task) = Outbound::start(writer, host.sessions.clone());
    forward(event, &host, &state, &outbound).await.unwrap();
    forward(
        Event::Layout {
            session_id: 1,
            sequence: 2,
            frame: crate::host::event::encode(&expected).unwrap(),
        },
        &host,
        &state,
        &outbound,
    )
    .await
    .unwrap();
    let mut header = [0; 4];
    client.read_exact(&mut header).await.unwrap();
    let mut payload = vec![0; u32::from_le_bytes(header) as usize];
    client.read_exact(&mut payload).await.unwrap();
    assert_eq!(proto::decode(&payload).unwrap(), expected);
    outbound.finish(writer_task).await;
}
