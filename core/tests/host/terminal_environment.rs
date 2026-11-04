//! A pane advertises pigeon's terminal capabilities, not its starter's terminal.

use super::*;

#[test]
fn panes_do_not_inherit_an_unrelated_terminal_type() {
    let _serial = one_at_a_time();
    let program = script("#!/bin/sh\nprintf 'term=<%s> color=<%s>\\n' \"$TERM\" \"$COLORTERM\"\nexec env PS1= /bin/sh -i\n");
    for inherited in [Some("linux"), Some("dumb"), Some("xterm-kitty"), None] {
        let host = start_host_on_with_term(
            &unique_socket(),
            program.to_str().expect("script path"),
            inherited,
        );
        let mut client = Client::connect(&host.socket);
        hello(&mut client);
        let pane = attach_first(&mut client);
        let screen = stable_snapshot(&mut client, pane);
        assert!(
            screen_text(&screen).contains("term=<xterm-256color> color=<truecolor>"),
            "starter TERM={inherited:?}: {}",
            screen_text(&screen),
        );
    }
    std::fs::remove_file(program).expect("remove script");
}

#[test]
fn combining_text_is_preserved_after_a_real_host_reconnect() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello(&mut client);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033[2J\\033[H'\r".to_vec(),
    });
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: "printf 'e\\314\\201\\n'\r".as_bytes().to_vec(),
    });
    let message = client.wait_for(|message| match message {
        Message::Snapshot { text, .. } => text.iter().any(|entry| entry.text == "e\u{301}"),
        _ => false,
    });
    assert!(pigeon_core::api::snapshot_of(&message).is_ok());
    drop(client);
    let mut reconnected = Client::connect(&host.socket);
    hello(&mut reconnected);
    attach(&mut reconnected, pane, None);
    let recovered = snapshot(&mut reconnected);
    let Message::Snapshot { text, .. } = recovered else {
        unreachable!()
    };
    assert!(text.iter().any(|entry| entry.text == "e\u{301}"));
}
