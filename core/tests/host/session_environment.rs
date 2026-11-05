//! Explicit per-session environment never changes the host or another session.
use super::*;
use std::collections::BTreeMap;

fn visible(message: &Message) -> String {
    let (_, snapshot) = pigeon_core::api::snapshot_of(message).unwrap();
    snapshot
        .screen
        .iter()
        .map(|row| row.iter().map(|cell| cell.text()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn create(client: &mut Client, id: u32, env: Option<BTreeMap<String, String>>) -> u32 {
    client.send(&Message::NewSession {
        id,
        cwd: None,
        shell: None,
        env,
    });
    let reply = client.wait_for(|m| matches!(m, Message::Created { id: got, .. } if *got == id));
    let Message::Created { pane_id, .. } = reply else {
        unreachable!()
    };
    attach(client, pane_id, None);
    pane_id
}

#[test]
fn requested_environment_overrides_host_and_stays_with_its_session() {
    let _serial = one_at_a_time();
    let program = script("#!/bin/sh\nprintf '<%s>|<%s>|<%s>|<%s>|<%s>\\n' \"$PIGEON_TEST_VALUE\" \"$PIGEON_TEST_EMPTY\" \"$PIGEON_TEST_KEEP\" \"$TERM\" \"$COLORTERM\"\nexec env PS1= /bin/sh -i\n");
    let host = start_host_env(
        &unique_socket(),
        program.to_str().unwrap(),
        &[
            ("PIGEON_TEST_VALUE", "host"),
            ("PIGEON_TEST_EMPTY", "nonempty"),
            ("PIGEON_TEST_KEEP", "inherited"),
        ],
    );
    let mut client = Client::connect(&host.socket);
    client.send(&Message::Hello {
        proto: proto::PROTO_VERSION,
        client: "env-test".into(),
        features: vec!["session_env".into()],
    });
    let ack = client.wait_for(|m| matches!(m, Message::HelloAck { .. }));
    assert!(pigeon_core::api::offers_feature(&ack, "session_env").unwrap());
    let pane = create(
        &mut client,
        1,
        Some(BTreeMap::from([
            ("PIGEON_TEST_VALUE".into(), "你好=value".into()),
            ("PIGEON_TEST_EMPTY".into(), String::new()),
        ])),
    );
    let screen = visible(&stable_snapshot(&mut client, pane));
    assert!(
        screen.contains("<你好=value>|<>|<inherited>|<xterm-256color>|<truecolor>"),
        "{screen}"
    );
    let (sibling, _) = split(&mut client, pane, proto::SplitAxis::Horizontal, 0.5, 2);
    attach(&mut client, sibling, None);
    let screen = visible(&stable_snapshot(&mut client, sibling));
    assert!(
        screen.contains("<你好=value>|<>|<inherited>|<xterm-256color>|<truecolor>"),
        "{screen}"
    );
    let other = create(&mut client, 2, None);
    let screen = visible(&stable_snapshot(&mut client, other));
    assert!(
        screen.contains("<host>|<nonempty>|<inherited>|<xterm-256color>|<truecolor>"),
        "{screen}"
    );
    std::fs::remove_file(program).unwrap();
}

#[test]
fn invalid_environment_is_rejected_before_spawning_and_connection_survives() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    for (id, name, value) in [
        (1, "BAD=NAME", "x"),
        (2, "OK", "bad\0value"),
        (3, "TERM", "dumb"),
    ] {
        client.send(&Message::NewSession {
            id,
            cwd: None,
            shell: None,
            env: Some(BTreeMap::from([(name.into(), value.into())])),
        });
        let reply = client.wait_for(|m| {
            matches!(m, Message::Error { id: Some(got), .. } if *got == id)
                || matches!(m, Message::Created { id: got, .. } if *got == id)
        });
        assert!(matches!(reply, Message::Error { code, .. } if code == "invalid_arg"));
    }
    client.send(&Message::Sessions { items: vec![] });
    let Message::Sessions { items } = client.wait_for(|m| matches!(m, Message::Sessions { .. }))
    else {
        unreachable!()
    };
    assert!(items.is_empty());
    assert!(create(&mut client, 4, None) > 0);
}
