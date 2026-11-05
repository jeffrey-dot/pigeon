use super::*;

#[test]
fn osc8_links_survive_real_pty_host_reconnect() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033]8;;https://example.com/osc8\\007target\\033]8;;\\007\\n'\r".to_vec(),
    });
    let message = client.wait_for(|message| match message {
        Message::Snapshot {
            links: Some(links), ..
        } => links
            .iter()
            .any(|link| link.uri == "https://example.com/osc8"),
        _ => false,
    });
    let first = pigeon_core::api::snapshot_of(&message).unwrap().1;
    assert!(first
        .screen
        .iter()
        .flatten()
        .any(|cell| cell.hyperlink.as_deref() == Some("https://example.com/osc8")));
    drop(client);
    let mut reconnected = Client::connect(&host.socket);
    hello(&mut reconnected);
    attach(&mut reconnected, pane, None);
    let recovered = pigeon_core::api::snapshot_of(&snapshot(&mut reconnected))
        .unwrap()
        .1;
    assert!(recovered
        .screen
        .iter()
        .flatten()
        .any(|cell| cell.hyperlink.as_deref() == Some("https://example.com/osc8")));
}
